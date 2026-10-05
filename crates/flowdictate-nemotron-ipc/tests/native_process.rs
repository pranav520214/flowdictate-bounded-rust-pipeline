//! Manual real-model acceptance for the isolated Nemotron streaming worker.
#![allow(clippy::expect_used)]

use std::{fs, path::Path};

use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
    ModelCompatibility,
};
use flowdictate_nemotron_ipc::{NemotronWorker, NemotronWorkerConfig, NEMOTRON_CHUNK_SAMPLES};

#[test]
#[ignore = "requires the reviewed model and built native Nemotron worker"]
fn reviewed_hindi_fixture_crosses_the_bounded_native_streaming_process(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root should exist")?;
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
    let mut worker = NemotronWorker::spawn(&worker_path, lease, NemotronWorkerConfig::default())?;
    assert_eq!(worker.generation(), 1);
    assert!(worker.process_id().is_some());

    let benchmark_root = workspace.join("benches");
    let manifest =
        parse_benchmark_fixture_manifest(&fs::read(benchmark_root.join("fixtures.csv"))?)?;
    let entry = manifest
        .entries()
        .iter()
        .find(|entry| entry.fixture_id() == "fleurs-hi-in-dev-1624-386806656898138188")
        .ok_or("reviewed Hindi fixture should exist")?;
    let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
    assert_eq!(fixture.format().sample_rate_hz(), 16_000);
    assert_eq!(fixture.format().channels(), 1);

    let cancellation = CancellationToken::new();
    let mut latest = None;
    for chunk in fixture.samples().chunks(NEMOTRON_CHUNK_SAMPLES) {
        if let Some(transcript) = worker.push(chunk, &cancellation)? {
            latest = Some(transcript);
        }
    }
    if let Some(transcript) = worker.finish(&cancellation)? {
        latest = Some(transcript);
    }
    let final_transcript = latest.ok_or("native stream should return a final result")?;
    assert!(final_transcript.is_final());
    assert!(final_transcript.audio_processed_ms() <= fixture.duration_ms());
    let devanagari = final_transcript
        .text()
        .chars()
        .filter(|character| ('\u{0900}'..='\u{097f}').contains(character))
        .count();
    assert!(devanagari > 0);
    Ok(())
}
