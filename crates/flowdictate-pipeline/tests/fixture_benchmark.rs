//! End-to-end synthetic evidence for verified fixture benchmark execution.
#![allow(clippy::expect_used)]

use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use flowdictate_asr_ipc::{CancellationToken, Language, LanguageMode, WorkerError};
use flowdictate_audio::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, VerifiedBenchmarkFixture,
};
use flowdictate_pipeline::{
    run_verified_benchmark_fixture, run_verified_benchmark_fixture_with_perturbation,
    BenchmarkTranscript, DeterministicNoiseConfig, FixtureAudioPerturbation, FixtureBenchmarkError,
    FixtureNoiseConfigError, LanguageConfigurableBackend, RecognitionBenchmarkConfig,
    TranscriptionBackend,
};
use sha2::{Digest, Sha256};

const HEADER: &str = "fixture_id,relative_audio_path,audio_sha256,expected_transcript_path,transcript_sha256,license_spdx,source_url,source_revision,redistributable,language_mode,language_tags,duration_ms,sample_rate_hz,channels,speech_style,acoustic_condition,language_mix,accent_evidence,voice_rights,classification,review_status";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flowdictate-case-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test root should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestTranscript(Vec<u8>);

impl BenchmarkTranscript for TestTranscript {
    fn benchmark_text(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl Drop for TestTranscript {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

struct FakeBackend {
    mode: LanguageMode,
    output: Vec<u8>,
    calls: usize,
    observed_samples: usize,
    observed_fingerprint: u64,
    failure: Option<WorkerError>,
}

impl FakeBackend {
    fn new(mode: LanguageMode, output: &str) -> Self {
        Self {
            mode,
            output: output.as_bytes().to_vec(),
            calls: 0,
            observed_samples: 0,
            observed_fingerprint: 0,
            failure: None,
        }
    }
}

impl Drop for FakeBackend {
    fn drop(&mut self) {
        self.output.fill(0);
    }
}

impl TranscriptionBackend for FakeBackend {
    type Transcript = TestTranscript;

    fn transcribe(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Self::Transcript, WorkerError> {
        self.calls = self.calls.saturating_add(1);
        self.observed_samples = samples.len();
        self.observed_fingerprint = samples.iter().fold(0xcbf2_9ce4_8422_2325, |hash, sample| {
            hash.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(sample.to_bits())
        });
        if cancellation.is_cancelled() {
            return Err(WorkerError::Cancelled);
        }
        if let Some(error) = self.failure {
            return Err(error);
        }
        Ok(TestTranscript(std::mem::take(&mut self.output)))
    }
}

impl LanguageConfigurableBackend for FakeBackend {
    fn language_mode(&self) -> LanguageMode {
        self.mode
    }

    fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError> {
        if self.mode == language_mode {
            return Ok(false);
        }
        self.mode = language_mode;
        Ok(true)
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String should succeed");
            output
        })
}

fn pcm16_wave(sample_rate_hz: u32, channels: u16) -> Vec<u8> {
    let frames = sample_rate_hz / 10;
    let mut data = Vec::new();
    for frame in 0..frames {
        for channel in 0..channels {
            let sample = if (frame + u32::from(channel)) % 2 == 0 {
                4_096_i16
            } else {
                -4_096_i16
            };
            data.extend_from_slice(&sample.to_le_bytes());
        }
    }
    let block_align = channels * 2;
    let byte_rate = sample_rate_hz * u32::from(block_align);
    let riff_size = 4_u32 + 8 + 16 + 8 + u32::try_from(data.len()).expect("test data fits u32");
    let mut wave = Vec::new();
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&riff_size.to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&channels.to_le_bytes());
    wave.extend_from_slice(&sample_rate_hz.to_le_bytes());
    wave.extend_from_slice(&byte_rate.to_le_bytes());
    wave.extend_from_slice(&block_align.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(
        &u32::try_from(data.len())
            .expect("test data fits u32")
            .to_le_bytes(),
    );
    wave.extend_from_slice(&data);
    wave
}

fn verified_fixture(
    label: &str,
    sample_rate_hz: u32,
    channels: u16,
    language_mode: &str,
    language_tags: &str,
    reference: &str,
) -> VerifiedBenchmarkFixture {
    let root = TestRoot::new(label);
    let audio = pcm16_wave(sample_rate_hz, channels);
    fs::write(root.path().join("case.wav"), &audio).expect("audio should be writable");
    fs::write(root.path().join("case.txt"), reference).expect("reference should be writable");
    let row = format!(
        "case-1,case.wav,{},case.txt,{},CC0-1.0,https://example.invalid/source,revision-1,true,{language_mode},{language_tags},100,{sample_rate_hz},{channels},synthetic,clean,monolingual,not-applicable,synthetic-no-person,synthetic-public,approved",
        digest_hex(&audio),
        digest_hex(reference.as_bytes())
    );
    let manifest = format!("{HEADER}\n{row}\n");
    let parsed = parse_benchmark_fixture_manifest(manifest.as_bytes())
        .expect("synthetic manifest should parse");
    load_benchmark_fixture(root.path(), &parsed.entries()[0])
        .expect("synthetic fixture should verify")
}

#[test]
fn verified_fixture_runs_through_language_audio_backend_and_scoring() {
    let fixture = verified_fixture("exact", 16_000, 1, "fixed:en", "en", "hello world");
    let mut backend = FakeBackend::new(LanguageMode::Automatic, "hello world");
    let summary = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::default(),
    )
    .expect("verified fixture should run");

    assert!(summary.language_mode_changed);
    assert_eq!(backend.mode, LanguageMode::Fixed(Language::English));
    assert_eq!(backend.calls, 1);
    assert_eq!(backend.observed_samples, 1_600);
    assert_eq!(summary.canonical_samples, 1_600);
    assert_eq!(summary.perturbation, FixtureAudioPerturbation::None);
    assert_eq!(summary.recognition.word_errors, 0);
    assert_eq!(summary.recognition.character_errors, 0);
}

#[test]
fn deterministic_noise_is_volatile_reproducible_and_reported() {
    let config = DeterministicNoiseConfig::new(20, 0x5eed).expect("noise config should be valid");
    let perturbation = FixtureAudioPerturbation::DeterministicWhiteNoise(config);
    let mut fingerprints = Vec::new();

    for label in ["noise-a", "noise-b"] {
        let fixture = verified_fixture(label, 16_000, 1, "fixed:en", "en", "hello world");
        let mut backend = FakeBackend::new(LanguageMode::Automatic, "hello world");
        let summary = run_verified_benchmark_fixture_with_perturbation(
            fixture,
            &mut backend,
            &CancellationToken::new(),
            perturbation,
            RecognitionBenchmarkConfig::default(),
        )
        .expect("deterministic noise scenario should run");

        assert_eq!(summary.perturbation, perturbation);
        assert_eq!(config.signal_to_noise_db(), 20);
        assert_eq!(config.seed(), 0x5eed);
        fingerprints.push(backend.observed_fingerprint);
    }
    assert_eq!(fingerprints[0], fingerprints[1]);

    let fixture = verified_fixture("noise-none", 16_000, 1, "fixed:en", "en", "hello world");
    let mut backend = FakeBackend::new(LanguageMode::Automatic, "hello world");
    run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::default(),
    )
    .expect("unmodified scenario should run");
    assert_ne!(fingerprints[0], backend.observed_fingerprint);
}

#[test]
fn noise_configuration_fails_closed() {
    for (snr_db, seed) in [(5, 1), (20, 0), (u8::MAX, u64::MAX)] {
        assert_eq!(
            DeterministicNoiseConfig::new(snr_db, seed),
            Err(FixtureNoiseConfigError::InvalidConfig)
        );
    }
}

#[test]
fn stereo_noncanonical_rate_is_downmixed_and_resampled_before_scoring() {
    let fixture = verified_fixture("resample", 8_000, 2, "automatic", "en", "hello world");
    let mut backend = FakeBackend::new(LanguageMode::Automatic, "hello there");
    let summary = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::default(),
    )
    .expect("verified fixture should resample");

    assert!(!summary.language_mode_changed);
    assert_eq!(summary.source_sample_rate_hz, 8_000);
    assert_eq!(summary.source_channels, 2);
    assert_eq!(summary.source_duration_ms, 100);
    assert!((1_500..=1_700).contains(&summary.canonical_samples));
    assert_eq!(backend.observed_samples, summary.canonical_samples);
    assert_eq!(summary.recognition.word_substitutions, 1);
    assert_eq!(
        summary.recognition.word_error_rate_basis_points,
        Some(5_000)
    );
}

#[test]
fn pre_cancelled_case_does_not_change_language_or_call_backend() {
    let fixture = verified_fixture("cancel", 16_000, 1, "fixed:en", "en", "reference");
    let mut backend = FakeBackend::new(LanguageMode::Automatic, "reference");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &cancellation,
        RecognitionBenchmarkConfig::default(),
    );
    assert!(matches!(result, Err(FixtureBenchmarkError::Cancelled)));
    assert_eq!(backend.mode, LanguageMode::Automatic);
    assert_eq!(backend.calls, 0);
}

#[test]
fn unsupported_fixed_language_fails_before_backend_work() {
    let fixture = verified_fixture("language", 16_000, 1, "fixed:zz", "zz", "reference");
    let mut backend = FakeBackend::new(LanguageMode::Automatic, "reference");
    let result = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::default(),
    );
    assert!(matches!(
        result,
        Err(FixtureBenchmarkError::UnsupportedLanguage)
    ));
    assert_eq!(backend.calls, 0);
}

#[test]
fn backend_and_scoring_failures_remain_payload_free() {
    let marker = "fixture-private-marker";
    let fixture = verified_fixture("backend-error", 16_000, 1, "automatic", "en", marker);
    let mut backend = FakeBackend::new(LanguageMode::Automatic, marker);
    backend.failure = Some(WorkerError::InferenceTimedOut);
    let backend_error = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::default(),
    )
    .expect_err("backend failure should be returned");
    assert!(!backend_error.to_string().contains(marker));

    let fixture = verified_fixture("score-error", 16_000, 1, "automatic", "en", marker);
    let mut backend = FakeBackend::new(LanguageMode::Automatic, marker);
    let scoring_error = run_verified_benchmark_fixture(
        fixture,
        &mut backend,
        &CancellationToken::new(),
        RecognitionBenchmarkConfig::new(1, 1, 1).expect("test scoring config should be valid"),
    )
    .expect_err("scoring failure should be returned");
    assert!(matches!(
        scoring_error,
        FixtureBenchmarkError::Recognition(_)
    ));
    assert!(!scoring_error.to_string().contains(marker));
}
