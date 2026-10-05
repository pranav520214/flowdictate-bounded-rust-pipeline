//! Secure, bounded audio primitives for `FlowDictate`.

mod cpal_backend;
mod fixture_intake;
mod model_security;

pub use cpal_backend::{
    build_default_capture, enumerate_microphones, CaptureHealth, CaptureHealthSnapshot,
    CaptureStream, MicrophoneInfo, PlatformCaptureError,
};
pub use fixture_intake::{
    load_benchmark_fixture, parse_benchmark_fixture_manifest, BenchmarkFixtureEntry,
    BenchmarkFixtureManifest, FixtureAccentEvidence, FixtureAcousticCondition,
    FixtureClassification, FixtureIntakeError, FixtureLanguageMix, FixtureLanguageMode,
    FixtureSpeechStyle, FixtureVoiceRights, VerifiedBenchmarkFixture, MAX_FIXTURE_AUDIO_BYTES,
    MAX_FIXTURE_DURATION_MS, MAX_FIXTURE_MANIFEST_BYTES, MAX_FIXTURE_MANIFEST_ENTRIES,
    MAX_FIXTURE_SAMPLE_RATE_HZ, MAX_FIXTURE_TRANSCRIPT_BYTES,
};
pub use model_security::{
    verify_compiled_model_bytes, verify_compiled_model_file, verify_model_file, ModelCompatibility,
    ModelManifest, ModelManifestEntry, ModelVerificationError, VerifiedModel, VerifiedModelBytes,
    VerifiedModelPathLease, COMPILED_MODEL_MANIFEST, MAX_MODEL_BYTES,
    MODEL_MANIFEST_SCHEMA_VERSION,
};

use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use rtrb::{Consumer, Producer, RingBuffer};
use rubato::{
    audioadapter_buffers::direct::InterleavedSlice, Async, FixedAsync, Resampler,
    SincInterpolationParameters,
};

use earshot::Detector;

/// Lowest capture rate accepted by the v1 audio boundary.
pub const MIN_SAMPLE_RATE_HZ: u32 = 8_000;
/// Highest capture rate accepted by the v1 audio boundary.
pub const MAX_SAMPLE_RATE_HZ: u32 = 192_000;
/// Highest channel count accepted by the v1 audio boundary.
pub const MAX_CHANNELS: u16 = 2;

/// A capture format that has passed `FlowDictate`'s hard input limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioFormat {
    sample_rate_hz: u32,
    channels: u16,
}

impl AudioFormat {
    /// Validates a hardware capture shape before any buffer is allocated.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError`] when the rate is outside 8–192 kHz or the
    /// stream is not mono/stereo.
    pub const fn new(sample_rate_hz: u32, channels: u16) -> Result<Self, FormatError> {
        if sample_rate_hz < MIN_SAMPLE_RATE_HZ || sample_rate_hz > MAX_SAMPLE_RATE_HZ {
            return Err(FormatError::UnsupportedSampleRate);
        }
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(FormatError::UnsupportedChannelCount);
        }
        Ok(Self {
            sample_rate_hz,
            channels,
        })
    }

    /// Returns the negotiated sample rate.
    #[must_use]
    pub const fn sample_rate_hz(self) -> u32 {
        self.sample_rate_hz
    }

    /// Returns the negotiated interleaved channel count.
    #[must_use]
    pub const fn channels(self) -> u16 {
        self.channels
    }

    /// Returns the interleaved sample capacity required for `duration`.
    ///
    /// The result saturates at [`usize::MAX`] instead of wrapping.
    #[must_use]
    pub fn samples_for_duration(self, duration: Duration) -> usize {
        let samples = duration
            .as_nanos()
            .saturating_mul(u128::from(self.sample_rate_hz))
            .saturating_mul(u128::from(self.channels))
            / 1_000_000_000;
        usize::try_from(samples).unwrap_or(usize::MAX)
    }
}

/// Safe, payload-free format validation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    /// The sample rate is outside the compiled safe range.
    UnsupportedSampleRate,
    /// The stream is not mono or stereo.
    UnsupportedChannelCount,
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSampleRate => formatter.write_str("unsupported sample rate"),
            Self::UnsupportedChannelCount => formatter.write_str("unsupported channel count"),
        }
    }
}

impl Error for FormatError {}

/// Converts validated mono/stereo interleaved input into mono samples.
///
/// Processing occurs in caller-provided storage and leaves any unused output
/// suffix unchanged.
///
/// # Errors
///
/// Returns [`NormalizationError`] when input does not contain complete frames
/// or the destination cannot hold every frame.
pub fn downmix_interleaved_to_mono(
    input: &[f32],
    format: AudioFormat,
    output: &mut [f32],
) -> Result<NormalizationReport, NormalizationError> {
    let channels = usize::from(format.channels());
    if !input.len().is_multiple_of(channels) {
        return Err(NormalizationError::MisalignedInput);
    }
    let frames = input.len() / channels;
    if output.len() < frames {
        return Err(NormalizationError::OutputTooSmall);
    }

    let mut sanitized_samples = 0;
    for (frame, destination) in input.chunks_exact(channels).zip(output.iter_mut()) {
        let (left, left_sanitized) = sanitize_f32_with_flag(frame[0]);
        sanitized_samples += usize::from(left_sanitized);
        let sample = if channels == 1 {
            left
        } else {
            let (right, right_sanitized) = sanitize_f32_with_flag(frame[1]);
            sanitized_samples += usize::from(right_sanitized);
            (left + right) * 0.5
        };
        *destination = sample;
    }

    Ok(NormalizationReport {
        frames_written: frames,
        sanitized_samples,
    })
}

/// Worker-side channel conversion failures with no audio payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizationError {
    /// The interleaved input ends with an incomplete frame.
    MisalignedInput,
    /// The caller-provided output cannot hold every complete frame.
    OutputTooSmall,
}

impl fmt::Display for NormalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MisalignedInput => formatter.write_str("interleaved input is misaligned"),
            Self::OutputTooSmall => formatter.write_str("normalization output is too small"),
        }
    }
}

impl Error for NormalizationError {}

/// Observable worker-side normalization result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizationReport {
    /// Mono frames written to the output prefix.
    pub frames_written: usize,
    /// Non-finite or out-of-range source samples replaced/clamped.
    pub sanitized_samples: usize,
}

/// A fixed-input, mono worker-side resampler targeting 16 kHz ASR PCM.
pub struct MonoResampler {
    inner: Async<f32>,
}

impl MonoResampler {
    /// Creates all resampler state before audio processing begins.
    ///
    /// # Errors
    ///
    /// Rejects rates outside `FlowDictate`'s capture limits, zero-sized chunks,
    /// and backend construction failures.
    pub fn new(input_rate_hz: u32, chunk_frames: usize) -> Result<Self, ResampleBoundaryError> {
        AudioFormat::new(input_rate_hz, 1)
            .map_err(|_| ResampleBoundaryError::UnsupportedInputFormat)?;
        if chunk_frames == 0 {
            return Err(ResampleBoundaryError::ZeroChunkSize);
        }
        let ratio = f64::from(16_000_u32) / f64::from(input_rate_hz);
        let parameters = SincInterpolationParameters::default();
        let inner =
            Async::<f32>::new_sinc(ratio, 1.01, &parameters, chunk_frames, 1, FixedAsync::Input)
                .map_err(|_| ResampleBoundaryError::ConstructionFailed)?;
        Ok(Self { inner })
    }

    /// Returns the exact mono input frame count required by the next call.
    #[must_use]
    pub fn required_input_frames(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Returns the exact output frame capacity required by the next call.
    #[must_use]
    pub fn required_output_frames(&self) -> usize {
        self.inner.output_frames_next()
    }

    /// Returns the maximum output frame capacity required by this instance.
    #[must_use]
    pub fn maximum_output_frames(&self) -> usize {
        self.inner.output_frames_max()
    }

    /// Clears interpolation history after an audio discontinuity.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Resamples one exact input chunk into caller-owned output storage.
    ///
    /// # Errors
    ///
    /// Rejects wrong input length, insufficient output, adapter construction
    /// failure, or a resampler processing failure.
    pub fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<ResampleReport, ResampleBoundaryError> {
        let required_input = self.required_input_frames();
        if input.len() != required_input {
            return Err(ResampleBoundaryError::WrongInputFrameCount);
        }
        let required_output = self.required_output_frames();
        if output.len() < required_output {
            return Err(ResampleBoundaryError::OutputTooSmall);
        }
        if input.iter().any(|sample| !sample.is_finite()) {
            return Err(ResampleBoundaryError::InvalidSample);
        }

        let input_adapter = InterleavedSlice::new(input, 1, input.len())
            .map_err(|_| ResampleBoundaryError::AdapterRejected)?;
        let output_capacity = output.len();
        let mut output_adapter = InterleavedSlice::new_mut(output, 1, output_capacity)
            .map_err(|_| ResampleBoundaryError::AdapterRejected)?;
        let (consumed_frames, produced_frames) = self
            .inner
            .process_into_buffer(&input_adapter, &mut output_adapter, None)
            .map_err(|_| ResampleBoundaryError::ProcessingFailed)?;
        Ok(ResampleReport {
            consumed_frames,
            produced_frames,
        })
    }
}

/// Payload-free failures at the resampler boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResampleBoundaryError {
    /// The input rate is outside the compiled safe range.
    UnsupportedInputFormat,
    /// A zero-sized worker chunk is invalid.
    ZeroChunkSize,
    /// Rubato could not construct the requested resampler.
    ConstructionFailed,
    /// The caller did not provide the exact required input frame count.
    WrongInputFrameCount,
    /// The caller-provided output cannot hold the next bounded chunk.
    OutputTooSmall,
    /// A non-finite value reached the worker resampling boundary.
    InvalidSample,
    /// Caller buffers could not be represented by the mono adapter.
    AdapterRejected,
    /// Resampling failed without exposing audio in the error.
    ProcessingFailed,
}

impl fmt::Display for ResampleBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnsupportedInputFormat => "unsupported resampler input format",
            Self::ZeroChunkSize => "resampler chunk size must be non-zero",
            Self::ConstructionFailed => "resampler construction failed",
            Self::WrongInputFrameCount => "wrong resampler input frame count",
            Self::OutputTooSmall => "resampler output is too small",
            Self::InvalidSample => "resampler input contains an invalid sample",
            Self::AdapterRejected => "resampler buffer adapter rejected the input",
            Self::ProcessingFailed => "resampler processing failed",
        };
        formatter.write_str(message)
    }
}

impl Error for ResampleBoundaryError {}

/// Observable result from one bounded resampling call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResampleReport {
    /// Mono frames consumed from the input.
    pub consumed_frames: usize,
    /// Mono 16 kHz frames written to the output prefix.
    pub produced_frames: usize,
}

/// Canonical VAD frame length: 16 ms at 16 kHz.
pub const VAD_FRAME_SAMPLES: usize = 256;

/// Thin local adapter around the embedded, pure-Rust Earshot detector.
pub struct EarshotVad {
    detector: Detector,
}

impl EarshotVad {
    /// Creates an independent detector for one audio stream.
    #[must_use]
    pub fn new() -> Self {
        Self {
            detector: Detector::default(),
        }
    }

    /// Scores exactly one canonical signed-PCM frame.
    ///
    /// # Errors
    ///
    /// Rejects frames that are not exactly 256 samples.
    pub fn score_i16(&mut self, frame: &[i16]) -> Result<f32, VadError> {
        if frame.len() != VAD_FRAME_SAMPLES {
            return Err(VadError::WrongFrameLength);
        }
        Ok(self.detector.predict_i16(frame).clamp(0.0, 1.0))
    }

    /// Clears detector state after an audio discontinuity or stream change.
    pub fn reset(&mut self) {
        self.detector.reset();
    }
}

impl Default for EarshotVad {
    fn default() -> Self {
        Self::new()
    }
}

/// VAD adapter failures that contain no audio payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VadError {
    /// Earshot accepts only a 16 ms mono 16 kHz frame.
    WrongFrameLength,
}

impl fmt::Display for VadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongFrameLength => formatter.write_str("wrong VAD frame length"),
        }
    }
}

impl Error for VadError {}

/// Validated policy for converting VAD scores into bounded utterances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VadConfig {
    start_threshold: f32,
    continue_threshold: f32,
    start_frames: u32,
    final_silence_frames: u32,
    maximum_active_frames: u32,
}

impl VadConfig {
    /// Validates score thresholds and non-zero hard limits.
    ///
    /// # Errors
    ///
    /// Rejects non-finite/out-of-range thresholds, a continuation threshold
    /// above the start threshold, zero counters, or an active limit not longer
    /// than speech-start confirmation.
    pub fn new(
        start_threshold: f32,
        continue_threshold: f32,
        start_frames: u32,
        final_silence_frames: u32,
        maximum_active_frames: u32,
    ) -> Result<Self, VadConfigError> {
        let valid_threshold = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        if !valid_threshold(start_threshold)
            || !valid_threshold(continue_threshold)
            || continue_threshold > start_threshold
        {
            return Err(VadConfigError::InvalidThreshold);
        }
        if start_frames == 0 || final_silence_frames == 0 || maximum_active_frames <= start_frames {
            return Err(VadConfigError::InvalidFrameLimit);
        }
        Ok(Self {
            start_threshold,
            continue_threshold,
            start_frames,
            final_silence_frames,
            maximum_active_frames,
        })
    }

    /// Returns the confirmation-frame count retained as bounded speech pre-roll.
    #[must_use]
    pub const fn start_frames(self) -> u32 {
        self.start_frames
    }

    /// Returns the hard limit for complete active 16 ms frames.
    #[must_use]
    pub const fn maximum_active_frames(self) -> u32 {
        self.maximum_active_frames
    }
}

/// VAD policy construction failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VadConfigError {
    /// A score threshold is invalid or internally inconsistent.
    InvalidThreshold,
    /// A frame count is zero or the hard limit is too small.
    InvalidFrameLimit,
}

impl fmt::Display for VadConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidThreshold => formatter.write_str("invalid VAD threshold"),
            Self::InvalidFrameLimit => formatter.write_str("invalid VAD frame limit"),
        }
    }
}

impl Error for VadConfigError {}

/// Current high-level utterance state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentState {
    /// No active utterance.
    Silence,
    /// Confirmed speech is active.
    Speech,
    /// A short quiet period inside an active utterance.
    MaybePause,
}

/// Why an active utterance was bounded and finalized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalizeReason {
    /// The configured consecutive-silence boundary was reached.
    Silence,
    /// The compiled/configured active-frame hard limit was reached.
    MaximumDuration,
    /// The user released the hold-to-talk hotkey.
    HotkeyReleased,
    /// The user explicitly stopped capture.
    ExplicitStop,
    /// Audio was dropped or sequence continuity was otherwise lost.
    Discontinuity,
}

/// Observable state-machine event for one score or forced boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentEvent {
    /// No active utterance and no transition.
    Idle,
    /// Speech crossed the configured start confirmation boundary.
    SpeechStarted,
    /// Confirmed speech continued.
    SpeechContinued,
    /// Speech resumed before final-silence confirmation.
    SpeechResumed,
    /// A non-final short pause is in progress.
    ShortPause,
    /// The active utterance ended for the supplied reason.
    Finalized(FinalizeReason),
}

/// Bounded score-to-utterance state machine independent of a VAD model.
pub struct UtteranceSegmenter {
    config: VadConfig,
    state: SegmentState,
    consecutive_start_frames: u32,
    consecutive_silence_frames: u32,
    active_frames: u32,
}

impl UtteranceSegmenter {
    /// Creates an idle segmenter from an already validated policy.
    #[must_use]
    pub const fn new(config: VadConfig) -> Self {
        Self {
            config,
            state: SegmentState::Silence,
            consecutive_start_frames: 0,
            consecutive_silence_frames: 0,
            active_frames: 0,
        }
    }

    /// Returns the current utterance state.
    #[must_use]
    pub const fn state(&self) -> SegmentState {
        self.state
    }

    /// Applies one bounded VAD score.
    pub fn observe(&mut self, score: f32) -> SegmentEvent {
        let score = if score.is_finite() {
            score.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.state == SegmentState::Silence {
            if score >= self.config.start_threshold {
                self.consecutive_start_frames = self.consecutive_start_frames.saturating_add(1);
                if self.consecutive_start_frames >= self.config.start_frames {
                    self.state = SegmentState::Speech;
                    self.active_frames = self.consecutive_start_frames;
                    self.consecutive_start_frames = 0;
                    return SegmentEvent::SpeechStarted;
                }
            } else {
                self.consecutive_start_frames = 0;
            }
            return SegmentEvent::Idle;
        }

        self.active_frames = self.active_frames.saturating_add(1);
        if self.active_frames >= self.config.maximum_active_frames {
            return self.finish(FinalizeReason::MaximumDuration);
        }

        if score >= self.config.continue_threshold {
            self.consecutive_silence_frames = 0;
            let event = if self.state == SegmentState::MaybePause {
                SegmentEvent::SpeechResumed
            } else {
                SegmentEvent::SpeechContinued
            };
            self.state = SegmentState::Speech;
            return event;
        }

        self.state = SegmentState::MaybePause;
        self.consecutive_silence_frames = self.consecutive_silence_frames.saturating_add(1);
        if self.consecutive_silence_frames >= self.config.final_silence_frames {
            self.finish(FinalizeReason::Silence)
        } else {
            SegmentEvent::ShortPause
        }
    }

    /// Finalizes an active utterance for an external bounded-stop reason.
    pub fn force_finalize(&mut self, reason: FinalizeReason) -> SegmentEvent {
        if self.state == SegmentState::Silence {
            self.consecutive_start_frames = 0;
            self.consecutive_silence_frames = 0;
            self.active_frames = 0;
            SegmentEvent::Idle
        } else {
            self.finish(reason)
        }
    }

    fn finish(&mut self, reason: FinalizeReason) -> SegmentEvent {
        self.state = SegmentState::Silence;
        self.consecutive_start_frames = 0;
        self.consecutive_silence_frames = 0;
        self.active_frames = 0;
        SegmentEvent::Finalized(reason)
    }
}

/// Explicit audio encoding/container identities at architectural boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioCodec {
    /// Canonical in-memory floating-point PCM.
    PcmF32,
    /// Signed 16-bit PCM, used only at explicit device/file boundaries.
    PcmI16,
    /// RIFF/WAVE containing an approved PCM encoding.
    WavPcm,
    /// Lossless FLAC container, deferred beyond Milestone 1.
    Flac,
    /// Ogg Opus, deferred beyond Milestone 1.
    Opus,
    /// MP3 is excluded from the trusted audio surface.
    Mp3,
    /// AAC is excluded from the trusted audio surface.
    Aac,
    /// Video/general multimedia containers are excluded.
    VideoContainer,
}

/// Why audio would cross a codec boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecPurpose {
    /// Volatile PCM consumed by local VAD/ASR.
    LiveInference,
    /// Synthetic or license-reviewed local regression fixture.
    RegressionFixture,
    /// Explicit user-authorized diagnostic export.
    DiagnosticExport,
    /// Explicit lossless local dataset/export.
    LosslessDataset,
    /// Explicit compact local recording.
    AuthorizedRecording,
}

/// Approved data route after codec policy validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecRoute {
    /// The audio remains uncompressed in bounded volatile memory.
    VolatilePcm,
    /// The file is explicit and must pass the bounded file/parser gate.
    ExplicitBoundedFile,
}

/// Milestone-specific codec allowlist.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodecPolicy;

impl CodecPolicy {
    /// Returns the Milestone 1 policy: live PCM plus bounded WAV fixtures only.
    #[must_use]
    pub const fn milestone_one() -> Self {
        Self
    }

    /// Validates that a codec is approved for a particular data purpose.
    ///
    /// # Errors
    ///
    /// Rejects excluded broad-media codecs, deferred features, and any
    /// compressed/container format offered to the live inference path.
    pub const fn validate(
        self,
        purpose: CodecPurpose,
        codec: AudioCodec,
    ) -> Result<CodecRoute, CodecPolicyError> {
        if matches!(
            codec,
            AudioCodec::Mp3 | AudioCodec::Aac | AudioCodec::VideoContainer
        ) {
            return Err(CodecPolicyError::ExcludedCodec);
        }
        match (purpose, codec) {
            (CodecPurpose::LiveInference, AudioCodec::PcmF32) => Ok(CodecRoute::VolatilePcm),
            (CodecPurpose::RegressionFixture, AudioCodec::WavPcm) => {
                Ok(CodecRoute::ExplicitBoundedFile)
            }
            (CodecPurpose::LiveInference | CodecPurpose::RegressionFixture, _) => {
                Err(CodecPolicyError::UnsupportedForPurpose)
            }
            (
                CodecPurpose::DiagnosticExport
                | CodecPurpose::LosslessDataset
                | CodecPurpose::AuthorizedRecording,
                _,
            ) => Err(CodecPolicyError::FeatureDisabled),
        }
    }
}

/// Codec-policy failures that contain no audio, filenames, or metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecPolicyError {
    /// The codec is not valid for the requested purpose.
    UnsupportedForPurpose,
    /// The feature is deliberately absent from this milestone.
    FeatureDisabled,
    /// The codec/container is excluded from `FlowDictate`'s trusted surface.
    ExcludedCodec,
}

impl fmt::Display for CodecPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnsupportedForPurpose => "codec is unsupported for this purpose",
            Self::FeatureDisabled => "codec feature is disabled",
            Self::ExcludedCodec => "codec is excluded",
        };
        formatter.write_str(message)
    }
}

impl Error for CodecPolicyError {}

/// Preallocated worker-side audio pipeline. It owns no microphone stream and
/// performs no disk, database, network, or UI work.
pub struct AudioProcessor {
    format: AudioFormat,
    input_chunk_frames: usize,
    mono: Vec<f32>,
    resampled: Vec<f32>,
    resampler: MonoResampler,
    vad: EarshotVad,
    segmenter: UtteranceSegmenter,
    vad_frame: [i16; VAD_FRAME_SAMPLES],
    canonical_vad_frame: [f32; VAD_FRAME_SAMPLES],
    vad_fill: usize,
    maximum_events_per_chunk: usize,
}

impl AudioProcessor {
    /// Preallocates all worker buffers for a fixed hardware chunk shape.
    ///
    /// # Errors
    ///
    /// Rejects zero/overflowing chunk sizes and resampler construction failure.
    pub fn new(
        format: AudioFormat,
        input_chunk_frames: usize,
        vad_config: VadConfig,
    ) -> Result<Self, AudioProcessingError> {
        if input_chunk_frames == 0
            || input_chunk_frames
                .checked_mul(usize::from(format.channels()))
                .is_none()
        {
            return Err(AudioProcessingError::InvalidChunkSize);
        }
        let resampler = MonoResampler::new(format.sample_rate_hz(), input_chunk_frames)
            .map_err(|_| AudioProcessingError::ResamplerFailure)?;
        let maximum_output_frames = resampler.maximum_output_frames();
        let maximum_events_per_chunk =
            maximum_output_frames.saturating_add(VAD_FRAME_SAMPLES - 1) / VAD_FRAME_SAMPLES + 1;
        Ok(Self {
            format,
            input_chunk_frames,
            mono: vec![0.0; input_chunk_frames],
            resampled: vec![0.0; maximum_output_frames],
            resampler,
            vad: EarshotVad::new(),
            segmenter: UtteranceSegmenter::new(vad_config),
            vad_frame: [0; VAD_FRAME_SAMPLES],
            canonical_vad_frame: [0.0; VAD_FRAME_SAMPLES],
            vad_fill: 0,
            maximum_events_per_chunk,
        })
    }

    /// Returns the event slots sufficient for any one configured input chunk.
    #[must_use]
    pub const fn maximum_events_per_chunk(&self) -> usize {
        self.maximum_events_per_chunk
    }

    /// Returns the caller-owned canonical sample slots sufficient for one chunk.
    #[must_use]
    pub const fn maximum_canonical_samples_per_chunk(&self) -> usize {
        self.maximum_events_per_chunk
            .saturating_mul(VAD_FRAME_SAMPLES)
    }

    /// Returns the exact interleaved hardware sample count accepted per call.
    #[must_use]
    pub const fn input_samples_per_chunk(&self) -> usize {
        self.input_chunk_frames
            .saturating_mul(self.format.channels() as usize)
    }

    /// Processes one exact interleaved hardware chunk into local VAD events.
    ///
    /// The provided event suffix beyond `events_written` remains unchanged.
    ///
    /// # Errors
    ///
    /// Rejects wrong input length, insufficient event storage, invalid samples,
    /// or an internal bounded DSP/VAD failure.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        events: &mut [SegmentEvent],
    ) -> Result<AudioProcessingReport, AudioProcessingError> {
        self.process_interleaved_internal(input, events, None)
    }

    /// Processes one hardware chunk and writes each completed canonical VAD
    /// frame contiguously beside its event.
    ///
    /// Frame `n` occupies `canonical_frames[n * 256..(n + 1) * 256]` and
    /// corresponds exactly to `events[n]`. The unused output suffix is not
    /// modified.
    ///
    /// # Errors
    ///
    /// Applies the normal processing checks and rejects canonical output that
    /// cannot hold every possible event frame for the configured chunk.
    pub fn process_interleaved_with_frames(
        &mut self,
        input: &[f32],
        events: &mut [SegmentEvent],
        canonical_frames: &mut [f32],
    ) -> Result<AudioProcessingReport, AudioProcessingError> {
        if canonical_frames.len() < self.maximum_canonical_samples_per_chunk() {
            return Err(AudioProcessingError::CanonicalOutputTooSmall);
        }
        self.process_interleaved_internal(input, events, Some(canonical_frames))
    }

    fn process_interleaved_internal(
        &mut self,
        input: &[f32],
        events: &mut [SegmentEvent],
        mut canonical_frames: Option<&mut [f32]>,
    ) -> Result<AudioProcessingReport, AudioProcessingError> {
        let expected_samples = self
            .input_chunk_frames
            .checked_mul(usize::from(self.format.channels()))
            .ok_or(AudioProcessingError::InvalidChunkSize)?;
        if input.len() != expected_samples {
            return Err(AudioProcessingError::WrongInputFrameCount);
        }
        if events.len() < self.maximum_events_per_chunk {
            return Err(AudioProcessingError::EventOutputTooSmall);
        }

        let normalization = downmix_interleaved_to_mono(input, self.format, &mut self.mono)
            .map_err(|_| AudioProcessingError::NormalizationFailure)?;
        let (peak_level, rms_level) = levels(&self.mono[..normalization.frames_written]);
        let resampled = self
            .resampler
            .process(
                &self.mono[..normalization.frames_written],
                &mut self.resampled,
            )
            .map_err(|_| AudioProcessingError::ResamplerFailure)?;

        let mut events_written: usize = 0;
        let mut vad_frames_processed: usize = 0;
        for sample in self.resampled[..resampled.produced_frames].iter().copied() {
            self.vad_frame[self.vad_fill] = f32_to_i16(sample);
            self.canonical_vad_frame[self.vad_fill] = sample;
            self.vad_fill += 1;
            if self.vad_fill == VAD_FRAME_SAMPLES {
                let score = self
                    .vad
                    .score_i16(&self.vad_frame)
                    .map_err(|_| AudioProcessingError::VadFailure)?;
                if let Some(output) = canonical_frames.as_deref_mut() {
                    let frame_start = events_written.saturating_mul(VAD_FRAME_SAMPLES);
                    let frame_end = frame_start.saturating_add(VAD_FRAME_SAMPLES);
                    output[frame_start..frame_end].copy_from_slice(&self.canonical_vad_frame);
                }
                events[events_written] = self.segmenter.observe(score);
                events_written += 1;
                vad_frames_processed += 1;
                self.vad_frame.fill(0);
                self.canonical_vad_frame.fill(0.0);
                self.vad_fill = 0;
            }
        }

        Ok(AudioProcessingReport {
            input_frames: normalization.frames_written,
            output_frames: resampled.produced_frames,
            vad_frames_processed,
            events_written,
            sanitized_samples: normalization.sanitized_samples,
            peak_level,
            rms_level,
        })
    }

    /// Clears DSP/VAD history and bounds the active segment after dropped audio.
    pub fn reset_discontinuity(&mut self) -> SegmentEvent {
        self.resampler.reset();
        self.vad.reset();
        self.vad_frame.fill(0);
        self.canonical_vad_frame.fill(0.0);
        self.vad_fill = 0;
        self.segmenter.force_finalize(FinalizeReason::Discontinuity)
    }

    /// Finalizes an active utterance and returns its last incomplete canonical
    /// frame without padding. All DSP/VAD state is reset for the next session.
    ///
    /// # Errors
    ///
    /// Rejects tail storage shorter than one complete canonical VAD frame.
    pub fn finalize_active(
        &mut self,
        reason: FinalizeReason,
        canonical_tail: &mut [f32],
    ) -> Result<AudioFinalizeReport, AudioProcessingError> {
        if canonical_tail.len() < VAD_FRAME_SAMPLES {
            return Err(AudioProcessingError::CanonicalOutputTooSmall);
        }
        let event = self.segmenter.force_finalize(reason);
        let canonical_samples_written = if matches!(event, SegmentEvent::Finalized(_)) {
            canonical_tail[..self.vad_fill]
                .copy_from_slice(&self.canonical_vad_frame[..self.vad_fill]);
            self.vad_fill
        } else {
            0
        };
        self.resampler.reset();
        self.vad.reset();
        self.vad_frame.fill(0);
        self.canonical_vad_frame.fill(0.0);
        self.vad_fill = 0;
        Ok(AudioFinalizeReport {
            event,
            canonical_samples_written,
        })
    }

    /// Discards all partial speech and DSP/VAD history without emitting a
    /// finalization event.
    pub fn cancel_pending(&mut self) {
        let mut discarded_tail = [0.0; VAD_FRAME_SAMPLES];
        let _ = self.finalize_active(FinalizeReason::ExplicitStop, &mut discarded_tail);
        discarded_tail.fill(0.0);
    }
}

impl Drop for AudioProcessor {
    fn drop(&mut self) {
        self.mono.fill(0.0);
        self.resampled.fill(0.0);
        self.vad_frame.fill(0);
        self.canonical_vad_frame.fill(0.0);
    }
}

/// Payload-free worker pipeline errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioProcessingError {
    /// The fixed worker chunk is zero or overflows sample capacity.
    InvalidChunkSize,
    /// The input does not contain the configured complete interleaved chunk.
    WrongInputFrameCount,
    /// Caller event storage cannot hold the maximum bounded output.
    EventOutputTooSmall,
    /// Caller canonical-frame storage cannot hold the maximum bounded output.
    CanonicalOutputTooSmall,
    /// Channel conversion failed.
    NormalizationFailure,
    /// Resampler initialization or processing failed.
    ResamplerFailure,
    /// The local VAD rejected a canonical frame.
    VadFailure,
}

impl fmt::Display for AudioProcessingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidChunkSize => "invalid audio worker chunk size",
            Self::WrongInputFrameCount => "wrong audio worker input frame count",
            Self::EventOutputTooSmall => "audio worker event output is too small",
            Self::CanonicalOutputTooSmall => "canonical audio output is too small",
            Self::NormalizationFailure => "audio worker normalization failed",
            Self::ResamplerFailure => "audio worker resampling failed",
            Self::VadFailure => "audio worker VAD failed",
        };
        formatter.write_str(message)
    }
}

impl Error for AudioProcessingError {}

/// Non-sensitive report from one worker chunk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioProcessingReport {
    /// Complete hardware frames consumed.
    pub input_frames: usize,
    /// Canonical 16 kHz mono frames produced.
    pub output_frames: usize,
    /// Complete 16 ms VAD frames evaluated.
    pub vad_frames_processed: usize,
    /// Segment events written to the caller's output prefix.
    pub events_written: usize,
    /// Source samples sanitized during downmixing.
    pub sanitized_samples: usize,
    /// Actual peak magnitude for the worker chunk.
    pub peak_level: f32,
    /// Actual RMS magnitude for the worker chunk.
    pub rms_level: f32,
}

/// Result of an explicit utterance boundary and canonical tail drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioFinalizeReport {
    /// Finalization transition, or idle when no speech was active.
    pub event: SegmentEvent,
    /// Unpadded canonical samples copied into the caller's tail prefix.
    pub canonical_samples_written: usize,
}

fn levels(samples: &[f32]) -> (f32, f32) {
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f32;
    let mut sample_count = 0.0_f32;
    for sample in samples.iter().copied() {
        peak = peak.max(sample.abs());
        sum_squares += sample * sample;
        sample_count += 1.0;
    }
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_squares / sample_count).sqrt()
    };
    (peak, rms)
}

#[allow(clippy::cast_possible_truncation)]
fn f32_to_i16(sample: f32) -> i16 {
    // The clamp and rounding make the conversion intentionally bounded to i16.
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Default and maximum callback-to-worker ring retention.
pub const DEFAULT_RING_DURATION: Duration = Duration::from_secs(2);

/// Callback sample formats accepted by the initial CPAL adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSampleFormat {
    /// Native 32-bit floating-point PCM.
    F32,
    /// Native signed 16-bit PCM.
    I16,
    /// Native unsigned 16-bit PCM.
    U16,
}

/// Fully bounded allocation plan created before a capture stream starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturePlan {
    audio_format: AudioFormat,
    sample_format: CaptureSampleFormat,
    ring_capacity_samples: usize,
}

impl CapturePlan {
    /// Creates a capture plan with no more than two seconds of PCM retention.
    ///
    /// # Errors
    ///
    /// Rejects zero retention or any duration above the compiled hard cap.
    pub fn new(
        audio_format: AudioFormat,
        sample_format: CaptureSampleFormat,
        ring_duration: Duration,
    ) -> Result<Self, CapturePlanError> {
        if ring_duration.is_zero() || ring_duration > DEFAULT_RING_DURATION {
            return Err(CapturePlanError::InvalidRingDuration);
        }
        Ok(Self {
            audio_format,
            sample_format,
            ring_capacity_samples: audio_format.samples_for_duration(ring_duration),
        })
    }

    /// Returns the validated hardware audio shape.
    #[must_use]
    pub const fn audio_format(self) -> AudioFormat {
        self.audio_format
    }

    /// Returns the native callback sample representation.
    #[must_use]
    pub const fn sample_format(self) -> CaptureSampleFormat {
        self.sample_format
    }

    /// Returns the fixed ring capacity in interleaved samples.
    #[must_use]
    pub const fn ring_capacity_samples(self) -> usize {
        self.ring_capacity_samples
    }
}

/// Capture planning failures that contain no device metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePlanError {
    /// Ring retention is zero or exceeds the two-second hard cap.
    InvalidRingDuration,
}

impl fmt::Display for CapturePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRingDuration => formatter.write_str("invalid capture ring duration"),
        }
    }
}

impl Error for CapturePlanError {}

/// Creates a bounded, single-producer/single-consumer audio ring.
///
/// The allocation happens here, before real-time capture begins. Producer and
/// consumer operations never grow the ring.
///
/// # Errors
///
/// Returns [`BufferError::ZeroCapacity`] rather than constructing an unusable
/// ring.
pub fn bounded_audio_ring(
    capacity_samples: usize,
) -> Result<(CaptureProducer, AudioConsumer), BufferError> {
    if capacity_samples == 0 {
        return Err(BufferError::ZeroCapacity);
    }
    let (producer, consumer) = RingBuffer::new(capacity_samples);
    let discontinuity_epoch = Arc::new(AtomicU64::new(0));
    Ok((
        CaptureProducer {
            inner: producer,
            discontinuity_epoch: Arc::clone(&discontinuity_epoch),
        },
        AudioConsumer {
            inner: consumer,
            discontinuity_epoch,
            capacity_samples,
        },
    ))
}

/// Construction failures for bounded audio storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferError {
    /// A zero-capacity ring cannot preserve bounded handoff semantics.
    ZeroCapacity,
}

impl fmt::Display for BufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("audio ring capacity must be non-zero"),
        }
    }
}

impl Error for BufferError {}

/// Producer owned exclusively by the real-time capture callback.
pub struct CaptureProducer {
    inner: Producer<f32>,
    discontinuity_epoch: Arc<AtomicU64>,
}

impl CaptureProducer {
    /// Writes an entire floating-point callback batch or drops the entire batch.
    ///
    /// Non-finite samples become zero and finite samples are clamped to
    /// `[-1.0, 1.0]`. The ring never grows and this method performs no
    /// application allocation.
    pub fn try_push_f32(&mut self, input: &[f32]) -> CaptureWrite {
        self.try_push_converted(input, sanitize_f32_with_flag)
    }

    /// Writes an entire signed 16-bit callback batch or drops the entire batch.
    pub fn try_push_i16(&mut self, input: &[i16]) -> CaptureWrite {
        self.try_push_converted(input, |source| (f32::from(source) / 32_768.0, false))
    }

    /// Writes an entire unsigned 16-bit callback batch or drops the entire batch.
    pub fn try_push_u16(&mut self, input: &[u16]) -> CaptureWrite {
        self.try_push_converted(input, |source| {
            ((f32::from(source) - 32_768.0) / 32_768.0, false)
        })
    }

    fn try_push_converted<T, Convert>(&mut self, input: &[T], mut convert: Convert) -> CaptureWrite
    where
        T: Copy,
        Convert: FnMut(T) -> (f32, bool),
    {
        if input.len() > self.inner.slots() {
            return CaptureWrite::Dropped {
                samples: input.len(),
                discontinuity_epoch: increment_epoch(&self.discontinuity_epoch),
            };
        }

        let Ok(mut chunk) = self.inner.write_chunk(input.len()) else {
            return CaptureWrite::Dropped {
                samples: input.len(),
                discontinuity_epoch: increment_epoch(&self.discontinuity_epoch),
            };
        };
        let (first, second) = chunk.as_mut_slices();
        let mut sanitized_samples = 0;
        for (destination, source) in first.iter_mut().chain(second).zip(input.iter().copied()) {
            let (converted, sanitized) = convert(source);
            sanitized_samples += usize::from(sanitized);
            *destination = converted;
        }
        chunk.commit_all();

        CaptureWrite::Written {
            samples: input.len(),
            sanitized_samples,
            discontinuity_epoch: self.discontinuity_epoch.load(Ordering::Acquire),
        }
    }
}

/// Result of one non-blocking callback write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureWrite {
    /// The complete callback batch was committed to the ring.
    Written {
        /// Number of interleaved samples committed.
        samples: usize,
        /// Number of values replaced or clamped.
        sanitized_samples: usize,
        /// Current discontinuity epoch.
        discontinuity_epoch: u64,
    },
    /// The complete callback batch was dropped because capacity was insufficient.
    Dropped {
        /// Number of interleaved samples dropped.
        samples: usize,
        /// Incremented discontinuity epoch observed by the worker.
        discontinuity_epoch: u64,
    },
}

/// Consumer owned exclusively by the audio processing worker.
pub struct AudioConsumer {
    inner: Consumer<f32>,
    discontinuity_epoch: Arc<AtomicU64>,
    capacity_samples: usize,
}

impl AudioConsumer {
    /// Returns the fixed maximum number of interleaved samples in the ring.
    #[must_use]
    pub const fn capacity_samples(&self) -> usize {
        self.capacity_samples
    }

    /// Returns the latest producer-side discontinuity epoch without consuming audio.
    #[must_use]
    pub fn discontinuity_epoch(&self) -> u64 {
        self.discontinuity_epoch.load(Ordering::Acquire)
    }

    /// Copies as many queued samples as fit into the caller-provided buffer.
    ///
    /// The unused suffix of `output` is left unchanged.
    pub fn read(&mut self, output: &mut [f32]) -> ReadReport {
        let samples_read = {
            let (popped, _) = self.inner.pop_partial_slice(output);
            popped.len()
        };
        ReadReport {
            samples_read,
            discontinuity_epoch: self.discontinuity_epoch.load(Ordering::Acquire),
        }
    }
}

/// Observable result from one worker-side read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadReport {
    /// Number of interleaved samples copied into the output prefix.
    pub samples_read: usize,
    /// Latest callback discontinuity epoch.
    pub discontinuity_epoch: u64,
}

fn increment_epoch(epoch: &AtomicU64) -> u64 {
    epoch
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            Some(value.saturating_add(1))
        })
        .map_or(u64::MAX, |previous| previous.saturating_add(1))
}

fn sanitize_f32_with_flag(sample: f32) -> (f32, bool) {
    if !sample.is_finite() {
        (0.0, true)
    } else if sample < -1.0 {
        (-1.0, true)
    } else if sample > 1.0 {
        (1.0, true)
    } else {
        (sample, false)
    }
}
