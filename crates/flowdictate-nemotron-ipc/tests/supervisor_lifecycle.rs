//! Model-free tests of the actual production process supervisor on Windows.
#![cfg(windows)]
#![allow(clippy::expect_used)]

use flowdictate_asr_ipc::{CancellationToken, WorkerError};
use flowdictate_audio::{
    verify_model_file, ModelCompatibility, ModelManifest, ModelManifestEntry,
    COMPILED_MODEL_MANIFEST,
};
use flowdictate_nemotron_ipc::{NemotronWorker, NemotronWorkerConfig};
use std::{fs, path::PathBuf, process::Command, sync::OnceLock, time::Duration};

fn worker() -> NemotronWorker {
    worker_with_timeout(Duration::from_millis(100))
}

fn worker_with_timeout(timeout: Duration) -> NemotronWorker {
    static FILES: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    let (executable, model) = FILES.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("flowdictate-nemotron-test-{}", std::process::id()));
        fs::create_dir(&root).expect("unique test directory");
        let executable = root.join("worker.exe");
        let status = Command::new("rustc")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/worker.rs"))
            .args(["--edition=2021", "-o"])
            .arg(&executable)
            .status()
            .expect("local rustc");
        assert!(status.success());
        let model = root.join("fixture.bin");
        fs::write(&model, b"abc").expect("non-sensitive model stub");
        (executable, model)
    });
    let entry = ModelManifestEntry {
        file_name: "fixture.bin",
        size_bytes: 3,
        sha256: [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ],
        ..*COMPILED_MODEL_MANIFEST
            .entries
            .iter()
            .find(|entry| entry.runtime == "nemo-speech.cpp")
            .expect("native identity")
    };
    let lease = verify_model_file(
        model,
        model.parent().expect("test root"),
        entry.id,
        ModelCompatibility {
            purpose: entry.purpose,
            runtime: entry.runtime,
            architecture: entry.architecture,
            quantization: entry.quantization,
        },
        ModelManifest {
            schema_version: 1,
            entries: &[entry],
        },
    )
    .expect("stub integrity")
    .into_immutable_path_lease()
    .expect("Windows lease");
    NemotronWorker::spawn(
        executable,
        lease,
        NemotronWorkerConfig::new(Duration::from_secs(5), timeout).expect("bounded config"),
    )
    .expect("fake worker startup")
}

fn clean_utterance(worker: &mut NemotronWorker) {
    let token = CancellationToken::new();
    worker.push(&[0.0; 160], &token).expect("clean push");
    let result = worker.finish(&token).expect("clean finish").expect("final");
    assert!(result.is_final());
    assert_eq!(result.audio_processed_ms(), 10);
}

#[test]
fn repeated_utterances_reuse_one_process_with_fresh_state() {
    let mut worker = worker();
    let pid = worker.process_id();
    for _ in 0..100 {
        clean_utterance(&mut worker);
    }
    assert_eq!(worker.generation(), 1);
    assert_eq!(worker.process_id(), pid);
}

#[test]
fn timeout_kills_and_restarts_before_reuse() {
    let mut worker = worker();
    let pid = worker.process_id();
    assert!(matches!(
        worker.push(&[1.0], &CancellationToken::new()),
        Err(WorkerError::InferenceTimedOut)
    ));
    assert_eq!(worker.generation(), 2);
    assert_ne!(worker.process_id(), pid);
    clean_utterance(&mut worker);
}

#[test]
fn cancelled_active_stream_is_destroyed_before_next_utterance() {
    let mut worker = worker();
    let token = CancellationToken::new();
    worker.push(&[0.0; 160], &token).expect("active stream");
    token.cancel();
    assert!(matches!(worker.finish(&token), Err(WorkerError::Cancelled)));
    assert_eq!(worker.generation(), 2);
    clean_utterance(&mut worker);
}

#[test]
fn cancellation_during_decode_restarts_cleanly() {
    let mut worker = worker();
    let token = CancellationToken::new();
    let signal = token.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        signal.cancel();
    });
    assert!(matches!(
        worker.push(&[1.0], &token),
        Err(WorkerError::Cancelled)
    ));
    handle.join().expect("cancellation thread");
    assert_eq!(worker.generation(), 2);
    clean_utterance(&mut worker);
}

#[test]
fn decoder_error_and_disconnect_restart_cleanly() {
    let mut worker = worker();
    assert!(matches!(
        worker.push(&[-1.0], &CancellationToken::new()),
        Err(WorkerError::RuntimeFailed(_))
    ));
    clean_utterance(&mut worker);
    assert!(matches!(
        worker.push(&[0.5], &CancellationToken::new()),
        Err(WorkerError::IpcFailed)
    ));
    assert_eq!(worker.generation(), 3);
    clean_utterance(&mut worker);
}

#[test]
fn explicit_reset_discards_active_stream() {
    let mut worker = worker();
    worker
        .push(&[0.0; 160], &CancellationToken::new())
        .expect("active stream");
    worker.reset().expect("reset");
    assert_eq!(worker.generation(), 2);
    clean_utterance(&mut worker);
}

#[test]
fn malformed_result_and_nonfinal_finish_restart_cleanly() {
    let mut worker = worker();
    let token = CancellationToken::new();
    assert!(matches!(
        worker.push(&[0.25], &token),
        Err(WorkerError::IpcFailed)
    ));
    clean_utterance(&mut worker);
    worker.push(&[-0.5], &token).expect("partial decode");
    assert!(matches!(worker.finish(&token), Err(WorkerError::IpcFailed)));
    assert_eq!(worker.generation(), 3);
    clean_utterance(&mut worker);
}

#[test]
fn idle_cancellation_does_not_restart_worker() {
    let mut worker = worker();
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(worker.finish(&token), Err(WorkerError::Cancelled)));
    assert_eq!(worker.generation(), 1);
    clean_utterance(&mut worker);
}

#[test]
fn repeated_cancellation_stress_then_clean_shutdown() {
    let mut worker = worker();
    for _ in 0..30 {
        for _ in 0..2 {
            let token = CancellationToken::new();
            worker.push(&[0.0; 160], &token).expect("active");
            token.cancel();
            assert!(matches!(worker.finish(&token), Err(WorkerError::Cancelled)));
            assert_eq!(worker.statistics().expect("fresh statistics").active, 0);
        }
        clean_utterance(&mut worker);
    }
    assert_eq!(worker.generation(), 61);
    let final_stats = worker.shutdown().expect("clean bounded shutdown");
    assert_eq!(final_stats.created, final_stats.destroyed);
}

#[test]
fn repeated_timeout_recovery_stays_bounded() {
    let mut worker = worker();
    for generation in 2..=11 {
        assert!(matches!(
            worker.push(&[1.0], &CancellationToken::new()),
            Err(WorkerError::InferenceTimedOut)
        ));
        assert_eq!(worker.generation(), generation);
        clean_utterance(&mut worker);
    }
    worker.shutdown().expect("clean shutdown");
}

fn kill_owned_worker(pid: u32) {
    let output = Command::new("taskkill.exe")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .expect("terminate owned test child");
    assert!(
        output.status.success(),
        "test-child termination failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn killed_idle_and_inflight_worker_recover_without_stale_results() {
    let mut worker = worker_with_timeout(Duration::from_secs(5));
    kill_owned_worker(worker.process_id().expect("owned child"));
    assert!(matches!(
        worker.push(&[0.0; 160], &CancellationToken::new()),
        Err(WorkerError::IpcFailed)
    ));
    clean_utterance(&mut worker);
    let pid = worker.process_id().expect("replacement");
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        kill_owned_worker(pid);
    });
    assert!(matches!(
        worker.push(&[1.0], &CancellationToken::new()),
        Err(WorkerError::IpcFailed)
    ));
    killer.join().expect("killer joined");
    assert_eq!(worker.generation(), 3);
    clean_utterance(&mut worker);
    worker.shutdown().expect("clean shutdown");
}

#[test]
fn stale_response_id_cannot_complete_replacement_session() {
    let mut worker = worker();
    for generation in 2..=21 {
        assert!(matches!(
            worker.push(&[0.75], &CancellationToken::new()),
            Err(WorkerError::IpcFailed)
        ));
        assert_eq!(worker.generation(), generation);
        clean_utterance(&mut worker);
    }
    worker.shutdown().expect("clean shutdown");
}

#[test]
fn shutdown_rejects_active_stream_and_bounds_unresponsive_exit() {
    let mut active = worker();
    active
        .push(&[0.0; 160], &CancellationToken::new())
        .expect("active");
    assert!(matches!(active.shutdown(), Err(WorkerError::IpcFailed)));
    let mut stalled = worker();
    let token = CancellationToken::new();
    stalled.push(&[-0.25], &token).expect("fixture push");
    stalled.finish(&token).expect("fixture finish");
    let started = std::time::Instant::now();
    assert!(matches!(
        stalled.shutdown(),
        Err(WorkerError::InferenceTimedOut)
    ));
    assert!(started.elapsed() < Duration::from_secs(8));
}
