//! Manual real-model stability test, using synthetic PCM and numeric output only.
#![cfg(windows)]
#![allow(clippy::print_stdout)]

#[path = "../../flowdictate-pipeline/tests/support/process_memory.rs"]
mod process_memory;
use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{verify_compiled_model_file, ModelCompatibility};
use flowdictate_nemotron_ipc::{NemotronWorker, NemotronWorkerConfig};
use std::{path::Path, time::Instant};

fn iterations(value: Option<&str>) -> Result<usize, &'static str> {
    let count = value
        .unwrap_or("100")
        .parse()
        .map_err(|_| "invalid soak count")?;
    if !(100..=2000).contains(&count) {
        return Err("soak count outside 100..2000");
    }
    Ok(count)
}

#[test]
fn soak_iteration_configuration_is_bounded() {
    assert_eq!(iterations(None), Ok(100));
    assert_eq!(iterations(Some("1000")), Ok(1000));
    for value in ["0", "99", "2001", "-1", "invalid"] {
        assert!(iterations(Some(value)).is_err());
    }
}

#[test]
#[ignore = "requires reviewed local Nemotron model and rebuilt native worker; 100..2000 sessions"]
#[allow(clippy::too_many_lines)]
fn production_worker_native_soak() -> Result<(), Box<dyn std::error::Error>> {
    let count = iterations(
        std::env::var("FLOWDICTATE_NEMOTRON_SOAK_SESSIONS")
            .ok()
            .as_deref(),
    )?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("root")?;
    let model_root = root.join("models/nemotron-3.5-asr-streaming-0.6b-q8_0");
    let verify_started = Instant::now();
    let lease = verify_compiled_model_file(
        &model_root.join("nemotron-3.5-asr-streaming-0.6b.q8_0.gguf"),
        &model_root,
        "asr-nemotron-3.5-streaming-0.6b-q8_0-experimental",
        ModelCompatibility {
            purpose: "asr",
            runtime: "nemo-speech.cpp",
            architecture: "fastconformer-rnnt",
            quantization: "q8_0",
        },
    )?
    .into_immutable_path_lease()?;
    let verify_us = verify_started.elapsed().as_micros();
    let startup = Instant::now();
    let mut worker = NemotronWorker::spawn(
        &root.join("target/nemotron-worker-build/bin/flowdictate-nemotron-worker.exe"),
        lease,
        NemotronWorkerConfig::default(),
    )?;
    let startup_us = startup.elapsed().as_micros();
    let pid = worker.process_id().ok_or("worker absent")?;
    let initial = process_memory::resources(pid)?;
    assert_eq!(worker.statistics()?.created, 0);
    let token = CancellationToken::new();
    let mut latencies = Vec::with_capacity(count);
    let mut samples = Vec::with_capacity(count / 10 + 2);
    let mut roundtrip_us = 0;
    // 3.2 seconds per utterance: alternating low-amplitude tone and silence.
    // Synthetic, non-sensitive stack buffer; no reviewed speech corpus consumed.
    let mut chunk = [0.0_f32; 2560];
    for session in 0..count {
        let started = Instant::now();
        for block in 0..20 {
            for (i, value) in chunk.iter_mut().enumerate() {
                let phase = (i + block * 2560 + session * 17) % 80;
                *value = if block % 4 == 3 {
                    0.0
                } else if phase < 40 {
                    0.02
                } else {
                    -0.02
                };
            }
            let call = Instant::now();
            let response = worker.push(&chunk, &token);
            chunk.fill(0.0);
            drop(response?); // Hypothesis owner wipes on drop; never inspect/log text.
            roundtrip_us += call.elapsed().as_micros();
        }
        let call = Instant::now();
        let final_result = worker.finish(&token)?.ok_or("missing final")?;
        if !final_result.is_final() {
            return Err("non-final result".into());
        }
        drop(final_result);
        roundtrip_us += call.elapsed().as_micros();
        latencies.push(started.elapsed().as_micros());
        let statistics = worker.statistics()?;
        assert_eq!(statistics.created, u64::try_from(session + 1)?);
        assert_eq!(statistics.created, statistics.finished);
        assert_eq!(statistics.created, statistics.destroyed);
        assert_eq!(statistics.active, 0);
        assert_eq!(statistics.maximum, 1);
        assert_eq!(statistics.errors, 0);
        assert_eq!(worker.generation(), 1);
        assert_eq!(worker.process_id(), Some(pid));
        let interval = if count > 100 { 50 } else { 10 };
        if session == 0 || (session + 1) % interval == 0 || session + 1 == count {
            samples.push(process_memory::resources(pid)?);
        }
    }
    let first = *samples.first().ok_or("resource samples absent")?;
    let last = *samples.last().ok_or("resource samples absent")?;
    let minimum = samples.iter().map(|s| s.0).min().ok_or("RSS absent")?;
    let maximum = samples.iter().map(|s| s.0).max().ok_or("RSS absent")?;
    // A material post-warmup increase fails acceptance, not hidden by restarts.
    assert!(maximum.saturating_sub(first.0) <= 64 * 1024 * 1024);
    assert!(samples
        .iter()
        .all(|s| s.1 <= first.1 + 8 && s.2 <= first.2 + 2));
    let shutdown = Instant::now();
    let statistics = worker.shutdown()?;
    let shutdown_us = shutdown.elapsed().as_micros();
    let cold_us = latencies[0];
    let total_us: u128 = latencies.iter().sum();
    let warm = &mut latencies[1..];
    warm.sort_unstable();
    let percentile = |p: usize| warm[(warm.len() * p).div_ceil(100) - 1];
    println!("sessions_requested={count} sessions_completed={count} failures=0 worker_starts=1 worker_clean_exits=1 worker_crashes=0 worker_restarts=0 supervisor_disables=0 timeouts=0 request_deadline_ms=30000 streams_created={} streams_finished={} streams_destroyed={} stream_errors={} active_streams={} max_simultaneous_streams={} stream_cancellations=0 stream_timeouts=0", statistics.created, statistics.finished, statistics.destroyed, statistics.errors, statistics.active, statistics.maximum);
    println!("model_verification_us={verify_us} worker_initialization_us={startup_us} cold_utterance_us={cold_us} warm_p50_us={} warm_p90_us={} warm_p95_us={} warm_p99_us={} warm_max_us={} total_request_roundtrip_us={roundtrip_us} utterance_outer_overhead_us={} shutdown_us={shutdown_us}", percentile(50), percentile(90), percentile(95), percentile(99), percentile(100), total_us.saturating_sub(roundtrip_us));
    println!("resource_samples={} post_model_rss={} stable_rss={} min_post_session_rss={minimum} max_sampled_rss={maximum} final_rss={} stable_delta_bytes={} strictly_increasing_samples={} initial_handles={} peak_handles={} final_handles={} initial_threads={} min_stable_threads={} max_stable_threads={} final_threads={}", samples.len(), initial.0, first.0, last.0, i128::try_from(last.0)? - i128::try_from(first.0)?, u8::from(samples.windows(2).all(|s| s[1].0 > s[0].0)), initial.1, samples.iter().map(|s|s.1).max().ok_or("handles")?, last.1, initial.2, samples.iter().map(|s|s.2).min().ok_or("threads")?, samples.iter().map(|s|s.2).max().ok_or("threads")?, last.2);
    Ok(())
}
