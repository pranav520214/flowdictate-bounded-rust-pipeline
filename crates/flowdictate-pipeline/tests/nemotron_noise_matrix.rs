//! Manual numeric-only noise matrix for the reviewed native Nemotron worker.
#![allow(clippy::print_stdout)]

use std::{fs, path::Path, time::Instant};

#[cfg(windows)]
#[path = "support/process_memory.rs"]
mod process_memory;

fn memory_bytes(pid: u32) -> Result<usize, &'static str> {
    #[cfg(windows)]
    {
        process_memory::working_set(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err("memory measurement unsupported")
    }
}

use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
    ModelCompatibility,
};
use flowdictate_nemotron_ipc::{NemotronWorker, NemotronWorkerConfig, NEMOTRON_CHUNK_SAMPLES};
use flowdictate_pipeline::{
    measure_recognition, prepare_verified_benchmark_audio, DeterministicNoiseConfig,
    FixtureAudioPerturbation, RecognitionBenchmarkConfig, RecognitionBenchmarkSummary,
    RecognitionTextPolicy,
};

const FIXTURE_COUNT: usize = 5;
const SOURCE_DURATION_MS: u64 = 34_680;
const NOISE_SEED_BASE: u64 = 0x464c_4f57_4e4f_4953;
const SNR_MATRIX_DB: [u8; 2] = [20, 10];

#[derive(Default)]
struct NumericAggregate {
    reference_words: u64,
    word_errors: u64,
    reference_characters: u64,
    character_errors: u64,
    hypothesis_characters: u64,
    hypothesis_devanagari_characters: u64,
    hypothesis_ascii_latin_characters: u64,
}

impl NumericAggregate {
    fn add(&mut self, summary: RecognitionBenchmarkSummary) -> Result<(), &'static str> {
        self.reference_words = self
            .reference_words
            .checked_add(u64::try_from(summary.reference_words).map_err(|_| "metric overflow")?)
            .ok_or("metric overflow")?;
        self.word_errors = self
            .word_errors
            .checked_add(u64::from(summary.word_errors))
            .ok_or("metric overflow")?;
        self.reference_characters = self
            .reference_characters
            .checked_add(
                u64::try_from(summary.reference_characters).map_err(|_| "metric overflow")?,
            )
            .ok_or("metric overflow")?;
        self.character_errors = self
            .character_errors
            .checked_add(u64::from(summary.character_errors))
            .ok_or("metric overflow")?;
        self.hypothesis_characters = self
            .hypothesis_characters
            .checked_add(
                u64::try_from(summary.hypothesis_characters).map_err(|_| "metric overflow")?,
            )
            .ok_or("metric overflow")?;
        self.hypothesis_devanagari_characters = self
            .hypothesis_devanagari_characters
            .checked_add(
                u64::try_from(summary.hypothesis_devanagari_characters)
                    .map_err(|_| "metric overflow")?,
            )
            .ok_or("metric overflow")?;
        self.hypothesis_ascii_latin_characters = self
            .hypothesis_ascii_latin_characters
            .checked_add(
                u64::try_from(summary.hypothesis_ascii_latin_characters)
                    .map_err(|_| "metric overflow")?,
            )
            .ok_or("metric overflow")?;
        Ok(())
    }
}

fn workspace_root() -> Result<&'static Path, &'static str> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root should exist")
}

fn spawn_worker(workspace: &Path) -> Result<NemotronWorker, Box<dyn std::error::Error>> {
    let model_root = workspace
        .join("models")
        .join("nemotron-3.5-asr-streaming-0.6b-q8_0");
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
    let worker_path = workspace
        .join("target")
        .join("nemotron-worker-build")
        .join("bin")
        .join("flowdictate-nemotron-worker.exe");
    Ok(NemotronWorker::spawn(
        &worker_path,
        lease,
        NemotronWorkerConfig::default(),
    )?)
}

#[test]
#[ignore = "requires the reviewed model and built native Nemotron worker"]
#[allow(clippy::too_many_lines)] // Keep the single-process lifecycle and measurements in order.
fn reviewed_hindi_noise_matrix_returns_numeric_only_metrics(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = workspace_root()?;
    let benchmark_root = workspace.join("benches");
    let manifest =
        parse_benchmark_fixture_manifest(&fs::read(benchmark_root.join("fixtures.csv"))?)?;
    let entries = manifest
        .entries()
        .iter()
        .filter(|entry| entry.language_tags().iter().any(|tag| tag == "hi"))
        .collect::<Vec<_>>();
    if entries.len() != FIXTURE_COUNT {
        return Err("reviewed Hindi fixture count changed".into());
    }

    assert_eq!(
        NemotronWorkerConfig::default(),
        NemotronWorkerConfig::new(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_secs(30)
        )?
    );
    let startup_started = Instant::now();
    let mut worker = spawn_worker(workspace)?;
    let startup_us = startup_started.elapsed().as_micros();
    let pid = worker.process_id().ok_or("worker unavailable")?;
    let before_stream_bytes = memory_bytes(pid)?;
    let mut during_peak_bytes = 0;
    let mut after_destroy = Vec::with_capacity(10);
    let mut first_active_bytes = 0;
    let mut next_active_bytes = 0;
    let mut all_latency_us = Vec::with_capacity(10);
    let mut first_request_us = 0;
    let mut native_calls_us = 0;
    let cancellation = CancellationToken::new();
    let scoring =
        RecognitionBenchmarkConfig::default().with_text_policy(RecognitionTextPolicy::FleursHindi);

    for target_snr_db in SNR_MATRIX_DB {
        let mut aggregate = NumericAggregate::default();
        let mut duration_ms = 0_u64;
        let mut inference_elapsed_us = 0_u128;

        for (index, entry) in entries.iter().enumerate() {
            let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
            duration_ms = duration_ms
                .checked_add(u64::from(fixture.duration_ms()))
                .ok_or("duration overflow")?;
            let seed = NOISE_SEED_BASE ^ (u64::from(target_snr_db) << 32) ^ u64::try_from(index)?;
            let noise = DeterministicNoiseConfig::new(target_snr_db, seed)?;
            let prepared = prepare_verified_benchmark_audio(
                &fixture,
                FixtureAudioPerturbation::DeterministicWhiteNoise(noise),
                &cancellation,
            )?;
            let inference_started = Instant::now();
            let mut latest = None;
            for chunk in prepared.samples().chunks(NEMOTRON_CHUNK_SAMPLES) {
                let call_started = Instant::now();
                if let Some(transcript) = worker.push(chunk, &cancellation)? {
                    latest = Some(transcript);
                }
                let elapsed = call_started.elapsed().as_micros();
                native_calls_us += elapsed;
                if first_request_us == 0 {
                    first_request_us = elapsed;
                }
                let active_bytes = memory_bytes(pid)?;
                during_peak_bytes = during_peak_bytes.max(active_bytes);
                if first_active_bytes == 0 {
                    first_active_bytes = active_bytes;
                }
                if all_latency_us.len() == 1 && next_active_bytes == 0 {
                    next_active_bytes = active_bytes;
                }
            }
            let call_started = Instant::now();
            if let Some(transcript) = worker.finish(&cancellation)? {
                latest = Some(transcript);
            }
            native_calls_us += call_started.elapsed().as_micros();
            let latency_us = inference_started.elapsed().as_micros();
            all_latency_us.push(latency_us);
            after_destroy.push(memory_bytes(pid)?);
            assert_eq!(worker.process_id(), Some(pid));
            assert_eq!(worker.generation(), 1);
            inference_elapsed_us = inference_elapsed_us
                .checked_add(latency_us)
                .ok_or("elapsed time overflow")?;
            let final_transcript = latest.ok_or("native stream returned no final transcript")?;
            if !final_transcript.is_final() {
                return Err("native stream returned a non-final transcript".into());
            }
            aggregate.add(measure_recognition(
                std::str::from_utf8(fixture.expected_transcript())?,
                final_transcript.text(),
                scoring,
            )?)?;
        }

        if duration_ms != SOURCE_DURATION_MS {
            return Err("reviewed Hindi duration changed".into());
        }
        let elapsed_us = inference_elapsed_us;
        let wer_bp =
            u128::from(aggregate.word_errors) * 10_000 / u128::from(aggregate.reference_words);
        let cer_bp = u128::from(aggregate.character_errors) * 10_000
            / u128::from(aggregate.reference_characters);
        let rtf_bp = elapsed_us * 10_000 / (u128::from(duration_ms) * 1_000);
        assert!(aggregate.hypothesis_characters > 0);
        assert_eq!(
            aggregate.hypothesis_characters,
            aggregate.hypothesis_devanagari_characters
        );
        // Existing narrow Hindi acceptance threshold, not a new quality claim.
        assert!(wer_bp <= 2200);
        println!(
            "model_code=3 perturbation_code=1 target_snr_db={} seed_base={} fixture_count={} source_duration_ms={} elapsed_us={} rtf_bp={} reference_words={} word_errors={} wer_bp={} reference_characters={} character_errors={} cer_bp={} hypothesis_characters={} hypothesis_devanagari_characters={} hypothesis_ascii_latin_characters={}",
            target_snr_db,
            NOISE_SEED_BASE,
            FIXTURE_COUNT,
            duration_ms,
            elapsed_us,
            rtf_bp,
            aggregate.reference_words,
            aggregate.word_errors,
            wer_bp,
            aggregate.reference_characters,
            aggregate.character_errors,
            cer_bp,
            aggregate.hypothesis_characters,
            aggregate.hypothesis_devanagari_characters,
            aggregate.hypothesis_ascii_latin_characters,
        );
    }
    let cold_utterance_us = all_latency_us[0];
    let mut warm = all_latency_us[1..].to_vec();
    warm.sort_unstable();
    println!("case_count=10 pass_count=10 fail_count=0 script_validity_bp=10000 worker_generations={} worker_crashes=0 timeout_count=0 request_deadline_ms=30000 startup_with_model_verification_us={} cold_first_request_us={} cold_utterance_us={} warm_p50_us={} warm_p95_us={} warm_max_us={} native_ipc_roundtrip_us={} harness_overhead_us={}",
        worker.generation(), startup_us, first_request_us, cold_utterance_us,
        warm[4], warm[8], warm[8], native_calls_us,
        all_latency_us.iter().sum::<u128>().saturating_sub(native_calls_us));
    println!("before_first_stream_bytes={} first_active_bytes={} next_active_bytes={} during_peak_bytes={} after_first_destroy_bytes={} after_last_destroy_bytes={} idle_min_bytes={} idle_max_bytes={} idle_strictly_increasing={}",
        before_stream_bytes, first_active_bytes, next_active_bytes, during_peak_bytes,
        after_destroy[0], after_destroy[9], after_destroy.iter().min().ok_or("memory sample missing")?,
        after_destroy.iter().max().ok_or("memory sample missing")?,
        u8::from(after_destroy.windows(2).all(|pair| pair[1] > pair[0])));
    Ok(())
}
