//! Manual, numeric-only comparison of two integrity-gated local Whisper models.
#![allow(clippy::expect_used)]

use std::{fs, path::Path};

use flowdictate_asr::{AsrConfig, CanonicalAudio, LocalWhisper, Transcript};
use flowdictate_asr_ipc::{
    CancellationToken, Language, LanguageMode, RuntimeErrorCode, WorkerError,
    WORKER_MODEL_COMPATIBILITY,
};
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
    VerifiedModel,
};
use flowdictate_pipeline::{
    run_verified_benchmark_fixture, BenchmarkTranscript, LanguageConfigurableBackend,
    RecognitionBenchmarkConfig, RecognitionTextPolicy, TranscriptionBackend,
};

const TINY_ID: &str = "asr-whisper-tiny-multilingual-q5_1";
const BASE_ID: &str = "asr-whisper-base-multilingual-q5_1-comparison";

struct DirectTranscript(Transcript);

impl BenchmarkTranscript for DirectTranscript {
    fn benchmark_text(&self) -> &str {
        self.0.text()
    }
}

struct DirectBackend {
    runtime: LocalWhisper,
    language_mode: LanguageMode,
}

impl DirectBackend {
    fn new(
        model: VerifiedModel,
        language_mode: LanguageMode,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let runtime = LocalWhisper::from_verified_model(model, AsrConfig::new(2, language_mode)?)?;
        Ok(Self {
            runtime,
            language_mode,
        })
    }
}

impl TranscriptionBackend for DirectBackend {
    type Transcript = DirectTranscript;

    fn transcribe(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Self::Transcript, WorkerError> {
        if cancellation.is_cancelled() {
            return Err(WorkerError::Cancelled);
        }
        let audio = CanonicalAudio::new(samples).map_err(|_| WorkerError::InvalidAudio)?;
        self.runtime
            .transcribe(audio)
            .map(DirectTranscript)
            .map_err(|_| WorkerError::RuntimeFailed(RuntimeErrorCode::Inference))
    }
}

impl LanguageConfigurableBackend for DirectBackend {
    fn language_mode(&self) -> LanguageMode {
        self.language_mode
    }

    fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError> {
        if language_mode == self.language_mode {
            Ok(false)
        } else {
            Err(WorkerError::InvalidConfig)
        }
    }
}

#[derive(Clone, Copy)]
struct Aggregate {
    source_duration_ms: u64,
    inference_total_us: u128,
    inference_p50_us: u128,
    inference_p95_us: u128,
    corpus_rtf_bp: u128,
    reference_words: u64,
    word_substitutions: u64,
    word_deletions: u64,
    word_insertions: u64,
    word_errors: u64,
    wer_bp: u128,
    reference_characters: u64,
    reference_devanagari_characters: u64,
    reference_ascii_latin_characters: u64,
    hypothesis_characters: u64,
    hypothesis_devanagari_characters: u64,
    hypothesis_ascii_latin_characters: u64,
    character_errors: u64,
    cer_bp: u128,
}

fn verified_model(
    workspace: &Path,
    directory: &str,
    file_name: &str,
    model_id: &str,
) -> Result<VerifiedModel, Box<dyn std::error::Error>> {
    let approved_root = workspace.join("models").join(directory);
    Ok(verify_compiled_model_file(
        &approved_root.join(file_name),
        &approved_root,
        model_id,
        WORKER_MODEL_COMPATIBILITY,
    )?)
}

fn benchmark_model(
    workspace: &Path,
    model: VerifiedModel,
    language_mode: LanguageMode,
) -> Result<Aggregate, Box<dyn std::error::Error>> {
    let benchmark_root = workspace.join("benches");
    let mut manifest_bytes = fs::read(benchmark_root.join("fixtures.csv"))?;
    if language_mode == LanguageMode::Automatic {
        manifest_bytes = String::from_utf8(manifest_bytes)?
            .replace("fixed:hi", "automatic")
            .into_bytes();
    }
    let manifest = parse_benchmark_fixture_manifest(&manifest_bytes)?;
    let hindi_entries = manifest
        .entries()
        .iter()
        .filter(|entry| entry.language_tags().iter().any(|tag| tag == "hi"))
        .collect::<Vec<_>>();
    if hindi_entries.len() != 5 {
        return Err("reviewed Hindi regression set must contain five cases".into());
    }

    let mut backend = DirectBackend::new(model, language_mode)?;
    let recognition_config =
        RecognitionBenchmarkConfig::default().with_text_policy(RecognitionTextPolicy::FleursHindi);
    let mut inference_micros = [0_u128; 5];
    let mut source_duration_ms = 0_u64;
    let mut reference_words = 0_u64;
    let mut word_substitutions = 0_u64;
    let mut word_deletions = 0_u64;
    let mut word_insertions = 0_u64;
    let mut word_errors = 0_u64;
    let mut reference_characters = 0_u64;
    let mut reference_devanagari_characters = 0_u64;
    let mut reference_ascii_latin_characters = 0_u64;
    let mut hypothesis_characters = 0_u64;
    let mut hypothesis_devanagari_characters = 0_u64;
    let mut hypothesis_ascii_latin_characters = 0_u64;
    let mut character_errors = 0_u64;

    for (index, entry) in hindi_entries.into_iter().enumerate() {
        let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
        let summary = run_verified_benchmark_fixture(
            fixture,
            &mut backend,
            &CancellationToken::new(),
            recognition_config,
        )?;
        inference_micros[index] = summary.inference_elapsed.as_micros();
        source_duration_ms += u64::from(summary.source_duration_ms);
        reference_words += u64::try_from(summary.recognition.reference_words)?;
        word_substitutions += u64::from(summary.recognition.word_substitutions);
        word_deletions += u64::from(summary.recognition.word_deletions);
        word_insertions += u64::from(summary.recognition.word_insertions);
        word_errors += u64::from(summary.recognition.word_errors);
        reference_characters += u64::try_from(summary.recognition.reference_characters)?;
        reference_devanagari_characters +=
            u64::try_from(summary.recognition.reference_devanagari_characters)?;
        reference_ascii_latin_characters +=
            u64::try_from(summary.recognition.reference_ascii_latin_characters)?;
        hypothesis_characters += u64::try_from(summary.recognition.hypothesis_characters)?;
        hypothesis_devanagari_characters +=
            u64::try_from(summary.recognition.hypothesis_devanagari_characters)?;
        hypothesis_ascii_latin_characters +=
            u64::try_from(summary.recognition.hypothesis_ascii_latin_characters)?;
        character_errors += u64::from(summary.recognition.character_errors);
    }

    inference_micros.sort_unstable();
    let inference_total_us = inference_micros.iter().sum::<u128>();
    Ok(Aggregate {
        source_duration_ms,
        inference_total_us,
        inference_p50_us: inference_micros[2],
        inference_p95_us: inference_micros[4],
        corpus_rtf_bp: inference_total_us * 10_000 / (u128::from(source_duration_ms) * 1_000),
        reference_words,
        word_substitutions,
        word_deletions,
        word_insertions,
        word_errors,
        wer_bp: u128::from(word_errors) * 10_000 / u128::from(reference_words),
        reference_characters,
        reference_devanagari_characters,
        reference_ascii_latin_characters,
        hypothesis_characters,
        hypothesis_devanagari_characters,
        hypothesis_ascii_latin_characters,
        character_errors,
        cer_bp: u128::from(character_errors) * 10_000 / u128::from(reference_characters),
    })
}

#[allow(clippy::print_stdout)]
fn print_numeric_result(
    model_code: u8,
    language_mode_code: u8,
    artifact_bytes: u64,
    result: Aggregate,
) {
    println!(
        "model_code={} language_mode_code={} artifact_bytes={} fixture_count=5 source_duration_ms={} inference_total_us={} inference_case_p50_us={} inference_case_p95_us={} corpus_rtf_bp={} reference_words={} word_substitutions={} word_deletions={} word_insertions={} word_errors={} wer_bp={} reference_characters={} reference_devanagari_characters={} reference_ascii_latin_characters={} hypothesis_characters={} hypothesis_devanagari_characters={} hypothesis_ascii_latin_characters={} character_errors={} cer_bp={}",
        model_code,
        language_mode_code,
        artifact_bytes,
        result.source_duration_ms,
        result.inference_total_us,
        result.inference_p50_us,
        result.inference_p95_us,
        result.corpus_rtf_bp,
        result.reference_words,
        result.word_substitutions,
        result.word_deletions,
        result.word_insertions,
        result.word_errors,
        result.wer_bp,
        result.reference_characters,
        result.reference_devanagari_characters,
        result.reference_ascii_latin_characters,
        result.hypothesis_characters,
        result.hypothesis_devanagari_characters,
        result.hypothesis_ascii_latin_characters,
        result.character_errors,
        result.cer_bp,
    );
}

#[test]
#[ignore = "requires both separately reviewed local model artifacts"]
fn reviewed_tiny_and_base_models_return_comparable_numeric_hindi_metrics(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root should exist")?;

    let tiny_fixed = benchmark_model(
        workspace,
        verified_model(
            workspace,
            "whisper-tiny-q5_1",
            "ggml-tiny-q5_1.bin",
            TINY_ID,
        )?,
        LanguageMode::Fixed(Language::Hindi),
    )?;
    let tiny_automatic = benchmark_model(
        workspace,
        verified_model(
            workspace,
            "whisper-tiny-q5_1",
            "ggml-tiny-q5_1.bin",
            TINY_ID,
        )?,
        LanguageMode::Automatic,
    )?;
    let base_fixed = benchmark_model(
        workspace,
        verified_model(
            workspace,
            "whisper-base-q5_1",
            "ggml-base-q5_1.bin",
            BASE_ID,
        )?,
        LanguageMode::Fixed(Language::Hindi),
    )?;
    let base_automatic = benchmark_model(
        workspace,
        verified_model(
            workspace,
            "whisper-base-q5_1",
            "ggml-base-q5_1.bin",
            BASE_ID,
        )?,
        LanguageMode::Automatic,
    )?;

    print_numeric_result(1, 1, 32_152_673, tiny_fixed);
    print_numeric_result(1, 2, 32_152_673, tiny_automatic);
    print_numeric_result(2, 1, 59_707_625, base_fixed);
    print_numeric_result(2, 2, 59_707_625, base_automatic);
    assert_eq!(tiny_fixed.source_duration_ms, 34_680);
    for result in [tiny_automatic, base_fixed, base_automatic] {
        assert_eq!(result.source_duration_ms, tiny_fixed.source_duration_ms);
        assert_eq!(result.reference_words, tiny_fixed.reference_words);
        assert_eq!(result.reference_characters, tiny_fixed.reference_characters);
    }
    Ok(())
}
