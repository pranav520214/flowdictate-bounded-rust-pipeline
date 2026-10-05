//! End-to-end execution of one verified local benchmark fixture.

use std::{
    error::Error,
    fmt, str,
    time::{Duration, Instant},
};

use flowdictate_asr_ipc::{
    CancellationToken, Language, LanguageMode, WorkerError, WorkerTranscript, MAX_INFERENCE_SAMPLES,
};
use flowdictate_audio::{
    downmix_interleaved_to_mono, FixtureLanguageMode, MonoResampler, VerifiedBenchmarkFixture,
};

use crate::{
    benchmark::real_time_factor_basis_points, measure_recognition, LanguageConfigurableBackend,
    RecognitionBenchmarkConfig, RecognitionBenchmarkError, RecognitionBenchmarkSummary,
};

/// Transcript view accepted by the fixture benchmark scorer.
///
/// Implementations own their sensitive text and must erase it on drop where
/// practical. The runner borrows the view only long enough to reduce it to
/// numeric metrics.
pub trait BenchmarkTranscript {
    /// Returns the exact final hypothesis produced by the local backend.
    fn benchmark_text(&self) -> &str;
}

impl BenchmarkTranscript for WorkerTranscript {
    fn benchmark_text(&self) -> &str {
        self.text()
    }
}

/// Validated deterministic white-noise scenario for a reviewed fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeterministicNoiseConfig {
    signal_to_noise_db: u8,
    seed: u64,
}

impl DeterministicNoiseConfig {
    /// Creates a reproducible noise scenario with a fixed target SNR.
    ///
    /// # Errors
    ///
    /// Accepts only 0, 10, 20, 30, or 40 dB and rejects a zero PRNG seed.
    pub const fn new(signal_to_noise_db: u8, seed: u64) -> Result<Self, FixtureNoiseConfigError> {
        if !matches!(signal_to_noise_db, 0 | 10 | 20 | 30 | 40) || seed == 0 {
            return Err(FixtureNoiseConfigError::InvalidConfig);
        }
        Ok(Self {
            signal_to_noise_db,
            seed,
        })
    }

    /// Returns the target signal-to-noise ratio in decibels.
    #[must_use]
    pub const fn signal_to_noise_db(self) -> u8 {
        self.signal_to_noise_db
    }

    /// Returns the fixed non-sensitive reproducibility seed.
    #[must_use]
    pub const fn seed(self) -> u64 {
        self.seed
    }
}

/// Noise-scenario configuration failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureNoiseConfigError {
    /// The SNR or seed is outside the fixed reproducible allowlist.
    InvalidConfig,
}

impl fmt::Display for FixtureNoiseConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fixture noise configuration is invalid")
    }
}

impl Error for FixtureNoiseConfigError {}

/// Volatile audio perturbation applied after canonicalization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FixtureAudioPerturbation {
    /// Present the verified canonical PCM unchanged.
    #[default]
    None,
    /// Mix deterministic white noise at the validated target SNR.
    DeterministicWhiteNoise(DeterministicNoiseConfig),
}

/// Bounded canonical fixture PCM prepared for one local benchmark run.
///
/// The owner is intentionally not cloneable or debuggable and overwrites its
/// samples on drop.
pub struct PreparedBenchmarkAudio {
    samples: Vec<f32>,
    perturbation: FixtureAudioPerturbation,
}

impl PreparedBenchmarkAudio {
    /// Borrows the mono 16 kHz normalized samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Returns the perturbation applied to the borrowed samples.
    #[must_use]
    pub const fn perturbation(&self) -> FixtureAudioPerturbation {
        self.perturbation
    }
}

impl Drop for PreparedBenchmarkAudio {
    fn drop(&mut self) {
        self.samples.fill(0.0);
    }
}

/// Numeric-only result from one verified fixture/backend execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixtureBenchmarkSummary {
    /// Whether applying the reviewed fixture language policy restarted the backend.
    pub language_mode_changed: bool,
    /// Decoded source sample rate from the verified WAV.
    pub source_sample_rate_hz: u32,
    /// Decoded source channel count from the verified WAV.
    pub source_channels: u16,
    /// Reviewed/verified source duration.
    pub source_duration_ms: u32,
    /// Mono 16 kHz samples presented to the backend.
    pub canonical_samples: usize,
    /// Volatile perturbation applied to canonical PCM.
    pub perturbation: FixtureAudioPerturbation,
    /// Time spent preparing canonical PCM outside the inference measurement.
    pub canonicalization_elapsed: Duration,
    /// Time spent only inside the local transcription backend call.
    pub inference_elapsed: Duration,
    /// Local inference real-time factor in basis points (`10_000 == 1.0`).
    pub real_time_factor_basis_points: u64,
    /// Numeric-only WER/CER and edit accounting.
    pub recognition: RecognitionBenchmarkSummary,
}

/// Payload-free end-to-end fixture benchmark failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureBenchmarkError {
    /// The fixture requested a fixed language outside the compiled allowlist.
    UnsupportedLanguage,
    /// A verified transcript could not be represented as UTF-8.
    InvalidTranscript,
    /// Bounded canonical audio memory could not be reserved.
    AllocationFailed,
    /// Decoded interleaved audio failed the mono normalization boundary.
    NormalizationFailed,
    /// Mono audio failed the bounded production resampling boundary.
    ResamplingFailed,
    /// Canonical PCM was empty or exceeded the 30-second ASR hard limit.
    CanonicalAudioLimit,
    /// A noise scenario was requested for audio with no measurable signal.
    NoiseRequiresSignal,
    /// Cancellation was observed before or after canonicalization/backend work.
    Cancelled,
    /// The local transcription backend failed.
    Backend(WorkerError),
    /// Numeric recognition scoring failed.
    Recognition(RecognitionBenchmarkError),
    /// A timing/sample calculation exceeded its numeric representation.
    ArithmeticOverflow,
}

impl fmt::Display for FixtureBenchmarkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedLanguage => "fixture benchmark language is unsupported",
            Self::InvalidTranscript => "fixture benchmark transcript is invalid",
            Self::AllocationFailed => "fixture benchmark allocation failed",
            Self::NormalizationFailed => "fixture benchmark normalization failed",
            Self::ResamplingFailed => "fixture benchmark resampling failed",
            Self::CanonicalAudioLimit => "fixture benchmark canonical audio is invalid",
            Self::NoiseRequiresSignal => "fixture benchmark noise requires a non-silent signal",
            Self::Cancelled => "fixture benchmark was cancelled",
            Self::Backend(_) => "fixture benchmark backend failed",
            Self::Recognition(_) => "fixture benchmark recognition scoring failed",
            Self::ArithmeticOverflow => "fixture benchmark metric arithmetic overflowed",
        })
    }
}

impl Error for FixtureBenchmarkError {}

/// Runs one already verified local fixture through canonicalization, the
/// configured local backend, and numeric recognition scoring.
///
/// Language switching and canonicalization are deliberately outside the
/// inference timer. The fixture, canonical PCM, expected transcript, and final
/// hypothesis are dropped before return; only numeric fields leave this API.
///
/// # Errors
///
/// Returns a payload-free fixed category for language, allocation, DSP,
/// cancellation, backend, scorer, or arithmetic failures.
pub fn run_verified_benchmark_fixture<B>(
    fixture: VerifiedBenchmarkFixture,
    backend: &mut B,
    cancellation: &CancellationToken,
    recognition_config: RecognitionBenchmarkConfig,
) -> Result<FixtureBenchmarkSummary, FixtureBenchmarkError>
where
    B: LanguageConfigurableBackend,
    B::Transcript: BenchmarkTranscript,
{
    run_verified_benchmark_fixture_with_perturbation(
        fixture,
        backend,
        cancellation,
        FixtureAudioPerturbation::None,
        recognition_config,
    )
}

/// Runs one verified fixture after an optional deterministic, volatile audio
/// perturbation.
///
/// Noise is generated from a fixed local PRNG, mixed only into the bounded
/// canonical buffer, and erased with that buffer. No augmented audio is saved.
///
/// # Errors
///
/// Returns the same payload-free failures as
/// [`run_verified_benchmark_fixture`], plus
/// [`FixtureBenchmarkError::NoiseRequiresSignal`] for silent source audio.
pub fn run_verified_benchmark_fixture_with_perturbation<B>(
    fixture: VerifiedBenchmarkFixture,
    backend: &mut B,
    cancellation: &CancellationToken,
    perturbation: FixtureAudioPerturbation,
    recognition_config: RecognitionBenchmarkConfig,
) -> Result<FixtureBenchmarkSummary, FixtureBenchmarkError>
where
    B: LanguageConfigurableBackend,
    B::Transcript: BenchmarkTranscript,
{
    if cancellation.is_cancelled() {
        return Err(FixtureBenchmarkError::Cancelled);
    }
    let language_mode = reviewed_language_mode(fixture.language_mode())?;
    let language_mode_changed = backend
        .set_language_mode(language_mode)
        .map_err(FixtureBenchmarkError::Backend)?;
    if cancellation.is_cancelled() {
        return Err(FixtureBenchmarkError::Cancelled);
    }

    let format = fixture.format();
    let source_duration_ms = fixture.duration_ms();
    let canonicalization_started = Instant::now();
    let canonical = prepare_verified_benchmark_audio(&fixture, perturbation, cancellation)?;
    let canonicalization_elapsed = canonicalization_started.elapsed();
    if cancellation.is_cancelled() {
        return Err(FixtureBenchmarkError::Cancelled);
    }

    let inference_started = Instant::now();
    let transcript = backend
        .transcribe(canonical.samples(), cancellation)
        .map_err(FixtureBenchmarkError::Backend)?;
    let inference_elapsed = inference_started.elapsed();
    if cancellation.is_cancelled() {
        return Err(FixtureBenchmarkError::Cancelled);
    }

    let expected = str::from_utf8(fixture.expected_transcript())
        .map_err(|_| FixtureBenchmarkError::InvalidTranscript)?;
    let recognition =
        measure_recognition(expected, transcript.benchmark_text(), recognition_config)
            .map_err(FixtureBenchmarkError::Recognition)?;
    let inference_micros = u64::try_from(inference_elapsed.as_micros())
        .map_err(|_| FixtureBenchmarkError::ArithmeticOverflow)?;
    let canonical_sample_count = canonical.samples().len();
    let sample_count = u64::try_from(canonical_sample_count)
        .map_err(|_| FixtureBenchmarkError::ArithmeticOverflow)?;
    let real_time_factor_basis_points =
        real_time_factor_basis_points(sample_count, inference_micros)
            .map_err(|_| FixtureBenchmarkError::ArithmeticOverflow)?;

    let summary = FixtureBenchmarkSummary {
        language_mode_changed,
        source_sample_rate_hz: format.sample_rate_hz(),
        source_channels: format.channels(),
        source_duration_ms,
        canonical_samples: canonical_sample_count,
        perturbation,
        canonicalization_elapsed,
        inference_elapsed,
        real_time_factor_basis_points,
        recognition,
    };
    drop(transcript);
    drop(canonical);
    drop(fixture);
    Ok(summary)
}

/// Canonicalizes and optionally perturbs one verified fixture entirely in
/// bounded volatile memory.
///
/// This seam lets native streaming acceptance tests reuse the exact same noise
/// policy without writing an augmented WAV.
///
/// # Errors
///
/// Returns a payload-free normalization, resampling, audio-bound,
/// perturbation, or cancellation failure.
pub fn prepare_verified_benchmark_audio(
    fixture: &VerifiedBenchmarkFixture,
    perturbation: FixtureAudioPerturbation,
    cancellation: &CancellationToken,
) -> Result<PreparedBenchmarkAudio, FixtureBenchmarkError> {
    if cancellation.is_cancelled() {
        return Err(FixtureBenchmarkError::Cancelled);
    }
    let mut canonical = canonicalize_fixture(fixture)?;
    apply_perturbation(&mut canonical.samples, perturbation, cancellation)?;
    canonical.perturbation = perturbation;
    Ok(canonical)
}

fn apply_perturbation(
    samples: &mut [f32],
    perturbation: FixtureAudioPerturbation,
    cancellation: &CancellationToken,
) -> Result<(), FixtureBenchmarkError> {
    let FixtureAudioPerturbation::DeterministicWhiteNoise(config) = perturbation else {
        return Ok(());
    };
    let mut signal_sum_squares = 0.0_f32;
    let mut noise_sum_squares = 0.0_f32;
    let mut sample_count = 0.0_f32;
    let mut noise = DeterministicNoise::new(config.seed());
    for (index, sample) in samples.iter().copied().enumerate() {
        if index.is_multiple_of(4_096) && cancellation.is_cancelled() {
            return Err(FixtureBenchmarkError::Cancelled);
        }
        let noise_sample = noise.next_sample();
        signal_sum_squares += sample * sample;
        noise_sum_squares += noise_sample * noise_sample;
        sample_count += 1.0;
    }
    if sample_count == 0.0 {
        return Err(FixtureBenchmarkError::CanonicalAudioLimit);
    }
    let signal_rms = (signal_sum_squares / sample_count).sqrt();
    let noise_rms = (noise_sum_squares / sample_count).sqrt();
    if signal_rms <= f32::EPSILON || noise_rms <= f32::EPSILON {
        return Err(FixtureBenchmarkError::NoiseRequiresSignal);
    }
    let amplitude_ratio = match config.signal_to_noise_db() {
        0 => 1.0,
        10 => 3.162_277_7,
        20 => 10.0,
        30 => 31.622_776,
        40 => 100.0,
        _ => return Err(FixtureBenchmarkError::ArithmeticOverflow),
    };
    let noise_scale = signal_rms / (amplitude_ratio * noise_rms);
    noise = DeterministicNoise::new(config.seed());
    for index in 0..samples.len() {
        if index.is_multiple_of(4_096) && cancellation.is_cancelled() {
            samples.fill(0.0);
            return Err(FixtureBenchmarkError::Cancelled);
        }
        samples[index] = (samples[index] + noise.next_sample() * noise_scale).clamp(-1.0, 1.0);
    }
    Ok(())
}

struct DeterministicNoise(u64);

impl DeterministicNoise {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_sample(&mut self) -> f32 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        let bytes = state.wrapping_mul(2_685_821_657_736_338_717).to_le_bytes();
        f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0
    }
}

fn reviewed_language_mode(
    fixture_mode: &FixtureLanguageMode,
) -> Result<LanguageMode, FixtureBenchmarkError> {
    match fixture_mode {
        FixtureLanguageMode::Automatic => Ok(LanguageMode::Automatic),
        FixtureLanguageMode::Fixed(code) => Language::from_iso_639_1(code)
            .map(LanguageMode::Fixed)
            .ok_or(FixtureBenchmarkError::UnsupportedLanguage),
    }
}

fn canonicalize_fixture(
    fixture: &VerifiedBenchmarkFixture,
) -> Result<PreparedBenchmarkAudio, FixtureBenchmarkError> {
    let format = fixture.format();
    let channels = usize::from(format.channels());
    if fixture.samples().is_empty() || !fixture.samples().len().is_multiple_of(channels) {
        return Err(FixtureBenchmarkError::NormalizationFailed);
    }
    let source_frames = fixture.samples().len() / channels;
    let mut mono = bounded_samples(source_frames)?;
    let report = downmix_interleaved_to_mono(fixture.samples(), format, &mut mono.samples)
        .map_err(|_| FixtureBenchmarkError::NormalizationFailed)?;
    if report.frames_written != source_frames || report.sanitized_samples != 0 {
        return Err(FixtureBenchmarkError::NormalizationFailed);
    }
    if format.sample_rate_hz() == 16_000 {
        validate_canonical_length(mono.samples.len())?;
        return Ok(mono);
    }

    let mut resampler = MonoResampler::new(format.sample_rate_hz(), source_frames)
        .map_err(|_| FixtureBenchmarkError::ResamplingFailed)?;
    if resampler.required_input_frames() != source_frames {
        return Err(FixtureBenchmarkError::ResamplingFailed);
    }
    let output_capacity = resampler.maximum_output_frames();
    let mut canonical = bounded_samples(output_capacity)?;
    let resample_report = resampler
        .process(&mono.samples, &mut canonical.samples)
        .map_err(|_| FixtureBenchmarkError::ResamplingFailed)?;
    if resample_report.consumed_frames != source_frames
        || resample_report.produced_frames > output_capacity
    {
        return Err(FixtureBenchmarkError::ResamplingFailed);
    }
    canonical.samples.truncate(resample_report.produced_frames);
    validate_canonical_length(canonical.samples.len())?;
    Ok(canonical)
}

fn bounded_samples(length: usize) -> Result<PreparedBenchmarkAudio, FixtureBenchmarkError> {
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(length)
        .map_err(|_| FixtureBenchmarkError::AllocationFailed)?;
    samples.resize(length, 0.0);
    Ok(PreparedBenchmarkAudio {
        samples,
        perturbation: FixtureAudioPerturbation::None,
    })
}

fn validate_canonical_length(length: usize) -> Result<(), FixtureBenchmarkError> {
    if length == 0 || length > MAX_INFERENCE_SAMPLES {
        return Err(FixtureBenchmarkError::CanonicalAudioLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_noise_hits_target_snr_without_clipping() -> Result<(), FixtureNoiseConfigError>
    {
        let original = (0..16_000)
            .map(|index| if index % 2 == 0 { 0.25_f32 } else { -0.25 })
            .collect::<Vec<_>>();
        let mut mixed = original.clone();
        let config = DeterministicNoiseConfig::new(20, 0x5eed)?;
        let cancellation = CancellationToken::new();

        let result = apply_perturbation(
            &mut mixed,
            FixtureAudioPerturbation::DeterministicWhiteNoise(config),
            &cancellation,
        );

        assert!(result.is_ok());
        let mut signal_energy = 0.0_f32;
        let mut noise_energy = 0.0_f32;
        for (clean, perturbed) in original.iter().zip(&mixed) {
            signal_energy += clean * clean;
            let noise = perturbed - clean;
            noise_energy += noise * noise;
            assert!((-1.0..=1.0).contains(perturbed));
        }
        let measured_ratio = (signal_energy / noise_energy).sqrt();
        assert!((measured_ratio - 10.0).abs() < 0.001);
        Ok(())
    }
}
