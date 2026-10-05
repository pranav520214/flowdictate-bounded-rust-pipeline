//! Manual process-isolation acceptance tests using the reviewed local model.

use std::{fs, path::PathBuf, thread, time::Duration};

use flowdictate_asr_ipc::{
    AsrWorker, CancellationToken, Language, LanguageMode, WorkerConfig, WorkerError,
    ASR_SAMPLE_RATE_HZ, MAX_INFERENCE_SAMPLES, WORKER_MODEL_COMPATIBILITY, WORKER_MODEL_ID,
};
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, verify_compiled_model_file,
};
use flowdictate_pipeline::{
    run_verified_benchmark_fixture, RecognitionBenchmarkConfig, RecognitionTextPolicy,
};

fn reviewed_model() -> Result<flowdictate_audio::VerifiedModel, Box<dyn std::error::Error>> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace path is unavailable")?
        .to_path_buf();
    let approved_root = workspace.join("models").join("whisper-tiny-q5_1");
    let model_path = approved_root.join("ggml-tiny-q5_1.bin");
    Ok(verify_compiled_model_file(
        &model_path,
        &approved_root,
        WORKER_MODEL_ID,
        WORKER_MODEL_COMPATIBILITY,
    )?)
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
#[allow(clippy::print_stdout)] // The ignored acceptance test emits numeric-only evidence with --nocapture.
fn reviewed_librispeech_fixture_set_returns_numeric_metrics(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace path is unavailable")?
        .to_path_buf();
    let benchmark_root = workspace.join("benches");
    let manifest_bytes = fs::read(benchmark_root.join("fixtures.csv"))?;
    let manifest = parse_benchmark_fixture_manifest(&manifest_bytes)?;
    let english_entries = manifest
        .entries()
        .iter()
        .filter(|entry| entry.language_tags().iter().any(|tag| tag == "en"))
        .collect::<Vec<_>>();
    if english_entries.len() != 6 {
        return Err("reviewed English regression set must contain six cases".into());
    }
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, WorkerConfig::default())?;
    let recognition_config = RecognitionBenchmarkConfig::default()
        .with_text_policy(RecognitionTextPolicy::LibriSpeechEnglish);
    let mut inference_micros = [0_u128; 6];
    let mut total_duration_ms = 0_u64;
    let mut reference_words = 0_u64;
    let mut word_substitutions = 0_u64;
    let mut word_deletions = 0_u64;
    let mut word_insertions = 0_u64;
    let mut word_errors = 0_u64;
    let mut reference_characters = 0_u64;
    let mut character_errors = 0_u64;
    for (index, entry) in english_entries.into_iter().enumerate() {
        let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
        let summary = run_verified_benchmark_fixture(
            fixture,
            &mut worker,
            &CancellationToken::new(),
            recognition_config,
        )?;
        inference_micros[index] = summary.inference_elapsed.as_micros();
        total_duration_ms += u64::from(summary.source_duration_ms);
        reference_words += u64::try_from(summary.recognition.reference_words)?;
        word_substitutions += u64::from(summary.recognition.word_substitutions);
        word_deletions += u64::from(summary.recognition.word_deletions);
        word_insertions += u64::from(summary.recognition.word_insertions);
        word_errors += u64::from(summary.recognition.word_errors);
        reference_characters += u64::try_from(summary.recognition.reference_characters)?;
        character_errors += u64::from(summary.recognition.character_errors);
    }
    inference_micros.sort_unstable();
    let total_inference_micros = inference_micros.iter().sum::<u128>();
    let corpus_rtf_basis_points =
        total_inference_micros * 10_000 / (u128::from(total_duration_ms) * 1_000);
    let word_error_rate_basis_points =
        u128::from(word_errors) * 10_000 / u128::from(reference_words);
    let character_error_rate_basis_points =
        u128::from(character_errors) * 10_000 / u128::from(reference_characters);

    println!(
        "fixture_count=6 source_duration_ms={} inference_total_us={} inference_case_p50_us={} inference_case_p95_us={} corpus_rtf_bp={} reference_words={} word_substitutions={} word_deletions={} word_insertions={} word_errors={} wer_bp={} reference_characters={} character_errors={} cer_bp={}",
        total_duration_ms,
        total_inference_micros,
        inference_micros[2],
        inference_micros[5],
        corpus_rtf_basis_points,
        reference_words,
        word_substitutions,
        word_deletions,
        word_insertions,
        word_errors,
        word_error_rate_basis_points,
        reference_characters,
        character_errors,
        character_error_rate_basis_points
    );
    assert_eq!(total_duration_ms, 36_460);
    Ok(())
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
#[allow(clippy::print_stdout)] // The ignored acceptance test emits numeric-only evidence with --nocapture.
fn reviewed_fleurs_hindi_fixture_returns_numeric_metrics() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace path is unavailable")?
        .to_path_buf();
    let benchmark_root = workspace.join("benches");
    let manifest_bytes = fs::read(benchmark_root.join("fixtures.csv"))?;
    let manifest = parse_benchmark_fixture_manifest(&manifest_bytes)?;
    let hindi_entries = manifest
        .entries()
        .iter()
        .filter(|entry| entry.language_tags().iter().any(|tag| tag == "hi"))
        .collect::<Vec<_>>();
    if hindi_entries.len() != 5 {
        return Err("reviewed Hindi regression set must contain five cases".into());
    }
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, WorkerConfig::default())?;
    let recognition_config =
        RecognitionBenchmarkConfig::default().with_text_policy(RecognitionTextPolicy::FleursHindi);
    let mut inference_micros = [0_u128; 5];
    let mut total_duration_ms = 0_u64;
    let mut reference_words = 0_u64;
    let mut word_substitutions = 0_u64;
    let mut word_deletions = 0_u64;
    let mut word_insertions = 0_u64;
    let mut word_errors = 0_u64;
    let mut reference_characters = 0_u64;
    let mut character_errors = 0_u64;
    for (index, entry) in hindi_entries.into_iter().enumerate() {
        let fixture = load_benchmark_fixture(&benchmark_root.join("fixtures"), entry)?;
        let summary = run_verified_benchmark_fixture(
            fixture,
            &mut worker,
            &CancellationToken::new(),
            recognition_config,
        )?;
        assert_eq!(summary.language_mode_changed, index == 0);
        inference_micros[index] = summary.inference_elapsed.as_micros();
        total_duration_ms += u64::from(summary.source_duration_ms);
        reference_words += u64::try_from(summary.recognition.reference_words)?;
        word_substitutions += u64::from(summary.recognition.word_substitutions);
        word_deletions += u64::from(summary.recognition.word_deletions);
        word_insertions += u64::from(summary.recognition.word_insertions);
        word_errors += u64::from(summary.recognition.word_errors);
        reference_characters += u64::try_from(summary.recognition.reference_characters)?;
        character_errors += u64::from(summary.recognition.character_errors);
    }
    inference_micros.sort_unstable();
    let total_inference_micros = inference_micros.iter().sum::<u128>();
    let corpus_rtf_basis_points =
        total_inference_micros * 10_000 / (u128::from(total_duration_ms) * 1_000);
    let word_error_rate_basis_points =
        u128::from(word_errors) * 10_000 / u128::from(reference_words);
    let character_error_rate_basis_points =
        u128::from(character_errors) * 10_000 / u128::from(reference_characters);

    println!(
        "fixture_count=5 source_duration_ms={} inference_total_us={} inference_case_p50_us={} inference_case_p95_us={} corpus_rtf_bp={} reference_words={} word_substitutions={} word_deletions={} word_insertions={} word_errors={} wer_bp={} reference_characters={} character_errors={} cer_bp={}",
        total_duration_ms,
        total_inference_micros,
        inference_micros[2],
        inference_micros[4],
        corpus_rtf_basis_points,
        reference_words,
        word_substitutions,
        word_deletions,
        word_insertions,
        word_errors,
        word_error_rate_basis_points,
        reference_characters,
        character_errors,
        character_error_rate_basis_points
    );
    assert_eq!(total_duration_ms, 34_680);
    assert_eq!(worker.language_mode(), LanguageMode::Fixed(Language::Hindi));
    Ok(())
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
fn worker_round_trip_transcribes_synthetic_silence() -> Result<(), Box<dyn std::error::Error>> {
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, WorkerConfig::default())?;
    let silence = vec![0.0_f32; ASR_SAMPLE_RATE_HZ];
    let transcript = worker.transcribe(&silence)?;

    assert!(transcript.text().len() <= flowdictate_asr_ipc::MAX_TRANSCRIPT_BYTES);
    assert!(transcript.segments().len() <= flowdictate_asr_ipc::MAX_TRANSCRIPT_SEGMENTS);
    assert_eq!(worker.generation(), 1);
    assert_eq!(worker.language_mode(), LanguageMode::Automatic);
    Ok(())
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
fn deadline_kills_and_restarts_the_native_worker() -> Result<(), Box<dyn std::error::Error>> {
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let config = WorkerConfig::new(
        2,
        LanguageMode::Automatic,
        Duration::from_secs(15),
        Duration::from_millis(10),
    )?;
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, config)?;
    let original_pid = worker.process_id().ok_or("worker process is unavailable")?;
    let silence = vec![0.0_f32; MAX_INFERENCE_SAMPLES];

    let result = worker.transcribe(&silence);

    assert!(matches!(result, Err(WorkerError::InferenceTimedOut)));
    assert_eq!(worker.generation(), 2);
    assert_ne!(worker.process_id(), Some(original_pid));
    Ok(())
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
fn fixed_language_change_starts_a_clean_worker_generation() -> Result<(), Box<dyn std::error::Error>>
{
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, WorkerConfig::default())?;
    let original_pid = worker.process_id().ok_or("worker process is unavailable")?;

    assert!(worker.set_language_mode(LanguageMode::Fixed(Language::English))?);
    assert_eq!(
        worker.language_mode(),
        LanguageMode::Fixed(Language::English)
    );
    assert_eq!(worker.generation(), 2);
    assert_ne!(worker.process_id(), Some(original_pid));

    let replacement_pid = worker.process_id();
    assert!(!worker.set_language_mode(LanguageMode::Fixed(Language::English))?);
    assert_eq!(worker.process_id(), replacement_pid);
    assert_eq!(worker.generation(), 2);

    let silence = vec![0.0_f32; ASR_SAMPLE_RATE_HZ];
    let transcript = worker.transcribe(&silence)?;
    assert!(transcript.text().len() <= flowdictate_asr_ipc::MAX_TRANSCRIPT_BYTES);
    assert!(transcript.segments().len() <= flowdictate_asr_ipc::MAX_TRANSCRIPT_SEGMENTS);
    Ok(())
}

#[test]
#[ignore = "requires the reviewed model and built worker executable"]
fn cancellation_kills_and_restarts_in_flight_native_inference(
) -> Result<(), Box<dyn std::error::Error>> {
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_flowdictate-asr-worker"));
    let mut worker = AsrWorker::spawn(&worker_path, reviewed_model()?, WorkerConfig::default())?;
    let original_pid = worker.process_id().ok_or("worker process is unavailable")?;
    let silence = vec![0.0_f32; MAX_INFERENCE_SAMPLES];
    let cancellation = CancellationToken::new();
    let canceller = cancellation.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        canceller.cancel();
    });

    let result = worker.transcribe_with_cancel(&silence, &cancellation);
    handle.join().map_err(|_| "cancellation thread failed")?;

    assert!(matches!(result, Err(WorkerError::Cancelled)));
    assert_eq!(worker.generation(), 2);
    assert_ne!(worker.process_id(), Some(original_pid));
    Ok(())
}
