//! Manual, numeric-only Hindi probe for the pinned local Nemotron runtime.
#![allow(clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    str,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
    ModelCompatibility, VerifiedModelPathLease,
};
use flowdictate_pipeline::{
    measure_recognition, RecognitionBenchmarkConfig, RecognitionTextPolicy,
};

const MODEL_BYTES: u64 = 741_548_352;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024;

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct SensitiveBytes(Vec<u8>);

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Default)]
struct Aggregate {
    reference_words: u64,
    word_substitutions: u64,
    word_deletions: u64,
    word_insertions: u64,
    word_errors: u64,
    reference_characters: u64,
    reference_devanagari_characters: u64,
    reference_ascii_latin_characters: u64,
    hypothesis_characters: u64,
    hypothesis_devanagari_characters: u64,
    hypothesis_ascii_latin_characters: u64,
    character_errors: u64,
}

fn verified_model_lease(
    model: &Path,
    approved_root: &Path,
) -> Result<VerifiedModelPathLease, Box<dyn std::error::Error>> {
    Ok(verify_compiled_model_file(
        model,
        approved_root,
        "asr-nemotron-3.5-streaming-0.6b-q8_0-experimental",
        ModelCompatibility {
            purpose: "asr",
            runtime: "nemo-speech.cpp",
            architecture: "fastconformer-rnnt",
            quantization: "q8_0",
        },
    )?
    .into_immutable_path_lease()?)
}

fn add_summary(
    aggregate: &mut Aggregate,
    summary: flowdictate_pipeline::RecognitionBenchmarkSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    aggregate.reference_words += u64::try_from(summary.reference_words)?;
    aggregate.word_substitutions += u64::from(summary.word_substitutions);
    aggregate.word_deletions += u64::from(summary.word_deletions);
    aggregate.word_insertions += u64::from(summary.word_insertions);
    aggregate.word_errors += u64::from(summary.word_errors);
    aggregate.reference_characters += u64::try_from(summary.reference_characters)?;
    aggregate.reference_devanagari_characters +=
        u64::try_from(summary.reference_devanagari_characters)?;
    aggregate.reference_ascii_latin_characters +=
        u64::try_from(summary.reference_ascii_latin_characters)?;
    aggregate.hypothesis_characters += u64::try_from(summary.hypothesis_characters)?;
    aggregate.hypothesis_devanagari_characters +=
        u64::try_from(summary.hypothesis_devanagari_characters)?;
    aggregate.hypothesis_ascii_latin_characters +=
        u64::try_from(summary.hypothesis_ascii_latin_characters)?;
    aggregate.character_errors += u64::from(summary.character_errors);
    Ok(())
}

#[test]
#[ignore = "requires the separately reviewed Nemotron model and pinned native runtime"]
#[allow(clippy::print_stdout)]
#[allow(clippy::too_many_lines)]
fn reviewed_nemotron_streaming_returns_numeric_hindi_metrics(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root should exist")?;
    let approved_model_root = workspace
        .join("models")
        .join("nemotron-3.5-asr-streaming-0.6b-q8_0");
    let model = approved_model_root.join("nemotron-3.5-asr-streaming-0.6b.q8_0.gguf");
    let cli = workspace
        .join(".tools")
        .join("nemo-speech-cpp")
        .join("build-cpu-asr")
        .join("bin")
        .join("nemo-speech.exe");
    let model_lease = verified_model_lease(&model, &approved_model_root)?;
    if !fs::symlink_metadata(&cli)?.file_type().is_file() {
        return Err("pinned Nemotron runtime is unavailable".into());
    }

    let benchmark_root = workspace.join("benches");
    let manifest_bytes = fs::read(benchmark_root.join("fixtures.csv"))?;
    let manifest = parse_benchmark_fixture_manifest(&manifest_bytes)?;
    let entries = manifest
        .entries()
        .iter()
        .filter(|entry| entry.language_tags().iter().any(|tag| tag == "hi"))
        .collect::<Vec<_>>();
    if entries.len() != 5 {
        return Err("reviewed Hindi regression set must contain five cases".into());
    }

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let probe = ProbeDirectory(workspace.join("target").join(format!(
        "nemotron-streaming-probe-{}-{nonce}",
        std::process::id()
    )));
    let input = probe.0.join("input");
    let output = probe.0.join("output");
    fs::create_dir_all(&input)?;
    fs::create_dir_all(&output)?;

    for entry in &entries {
        let _verified = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
        let file_name = format!("{}.wav", entry.fixture_id());
        fs::copy(
            benchmark_root.join("fixtures").join(&file_name),
            input.join(file_name),
        )?;
    }

    let started = Instant::now();
    let process = Command::new(&cli)
        .arg("--quiet")
        .arg("transcribe")
        .arg(&input)
        .arg("--model")
        .arg(model_lease.canonical_path())
        .arg("--language")
        .arg("hi-IN")
        .arg("--device")
        .arg("cpu")
        .arg("--concurrency")
        .arg("1")
        .arg("--format")
        .arg("text")
        .arg("--output-dir")
        .arg(&output)
        .arg("--stream")
        .arg("--force")
        .output()?;
    let elapsed = started.elapsed();
    if !process.status.success() || !process.stdout.is_empty() {
        return Err("Nemotron streaming probe failed without exposing runtime output".into());
    }

    let config =
        RecognitionBenchmarkConfig::default().with_text_policy(RecognitionTextPolicy::FleursHindi);
    let mut aggregate = Aggregate::default();
    let mut source_duration_ms = 0_u64;
    for entry in entries {
        let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
        source_duration_ms += u64::from(fixture.duration_ms());
        let transcript_path = output.join(format!("{}.txt", entry.fixture_id()));
        if fs::metadata(&transcript_path)?.len() > MAX_TRANSCRIPT_BYTES {
            return Err("Nemotron transcript exceeded the probe bound".into());
        }
        let hypothesis = SensitiveBytes(fs::read(transcript_path)?);
        let expected = str::from_utf8(fixture.expected_transcript())?;
        let hypothesis_text = str::from_utf8(&hypothesis.0)?;
        add_summary(
            &mut aggregate,
            measure_recognition(expected, hypothesis_text, config)?,
        )?;
    }

    let elapsed_us = elapsed.as_micros();
    let wer_bp = u128::from(aggregate.word_errors) * 10_000 / u128::from(aggregate.reference_words);
    let cer_bp = u128::from(aggregate.character_errors) * 10_000
        / u128::from(aggregate.reference_characters);
    let corpus_rtf_bp = elapsed_us * 10_000 / (u128::from(source_duration_ms) * 1_000);
    println!(
        "model_code=3 language_mode_code=1 stream_chunk_ms=160 artifact_bytes={} fixture_count=5 source_duration_ms={} elapsed_total_us={} corpus_rtf_including_load_bp={} reference_words={} word_substitutions={} word_deletions={} word_insertions={} word_errors={} wer_bp={} reference_characters={} reference_devanagari_characters={} reference_ascii_latin_characters={} hypothesis_characters={} hypothesis_devanagari_characters={} hypothesis_ascii_latin_characters={} character_errors={} cer_bp={}",
        MODEL_BYTES,
        source_duration_ms,
        elapsed_us,
        corpus_rtf_bp,
        aggregate.reference_words,
        aggregate.word_substitutions,
        aggregate.word_deletions,
        aggregate.word_insertions,
        aggregate.word_errors,
        wer_bp,
        aggregate.reference_characters,
        aggregate.reference_devanagari_characters,
        aggregate.reference_ascii_latin_characters,
        aggregate.hypothesis_characters,
        aggregate.hypothesis_devanagari_characters,
        aggregate.hypothesis_ascii_latin_characters,
        aggregate.character_errors,
        cer_bp,
    );
    assert_eq!(source_duration_ms, 34_680);
    Ok(())
}
