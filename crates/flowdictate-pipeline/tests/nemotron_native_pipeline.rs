//! Manual reviewed-fixture acceptance for the experimental native DSP/VAD path.
#![allow(clippy::expect_used)]

use std::{fs, path::Path};

use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
    AudioFormat, FinalizeReason, ModelCompatibility, VadConfig, VAD_FRAME_SAMPLES,
};
use flowdictate_nemotron_ipc::{NemotronWorker, NemotronWorkerConfig};
use flowdictate_pipeline::ExperimentalNemotronPipeline;

#[test]
#[ignore = "requires the reviewed model and built native Nemotron worker"]
fn reviewed_hindi_fixture_crosses_native_dsp_vad_pipeline() -> Result<(), Box<dyn std::error::Error>>
{
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
    let worker = NemotronWorker::spawn(&worker_path, lease, NemotronWorkerConfig::default())?;

    let benchmark_root = workspace.join("benches");
    let manifest =
        parse_benchmark_fixture_manifest(&fs::read(benchmark_root.join("fixtures.csv"))?)?;
    let entry = manifest
        .entries()
        .iter()
        .find(|entry| entry.fixture_id() == "fleurs-hi-in-dev-1624-386806656898138188")
        .ok_or("reviewed Hindi fixture should exist")?;
    let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;

    let format = AudioFormat::new(16_000, 1)?;
    let vad = VadConfig::new(0.0, 0.0, 1, 2, 1_000)?;
    let mut pipeline = ExperimentalNemotronPipeline::new(format, VAD_FRAME_SAMPLES, vad, worker)?;
    let cancellation = CancellationToken::new();
    let mut finals = Vec::with_capacity(8);
    let mut chunks = fixture.samples().chunks_exact(VAD_FRAME_SAMPLES);
    for chunk in &mut chunks {
        let _ = pipeline.process_interleaved(chunk, &cancellation, &mut finals)?;
    }
    let _ = pipeline.finalize(FinalizeReason::ExplicitStop, &cancellation, &mut finals)?;
    let final_transcript = finals
        .last()
        .ok_or("native pipeline should return a final result")?;
    assert!(final_transcript.is_final());
    let devanagari = final_transcript
        .text()
        .chars()
        .filter(|character| ('\u{0900}'..='\u{097f}').contains(character))
        .count();
    assert!(devanagari > 0);
    Ok(())
}
