//! Bounded, offline Whisper inference for `FlowDictate`.
//!
//! This crate is the only production boundary that links the native
//! `whisper.cpp` runtime. It accepts model bytes exclusively through
//! [`VerifiedModel`], never reopens a model path, and exposes no download or
//! network fallback.

use std::{error::Error, fmt};

use flowdictate_audio::{ModelCompatibility, VerifiedModel, VerifiedModelBytes};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub use flowdictate_asr_ipc::{
    Language, LanguageMode, ASR_SAMPLE_RATE_HZ, MAX_INFERENCE_MILLISECONDS, MAX_INFERENCE_SAMPLES,
    MAX_INFERENCE_SECONDS, MAX_INFERENCE_THREADS, MAX_TRANSCRIPT_BYTES, MAX_TRANSCRIPT_SEGMENTS,
};
/// Hard limit for a model accepted by this adapter.
pub const MAX_ADAPTER_MODEL_BYTES: usize = 512 * 1024 * 1024;

const SUPPORTED_MODEL_IDS: [&str; 2] = [
    "asr-whisper-tiny-multilingual-q5_1",
    "asr-whisper-base-multilingual-q5_1-comparison",
];

/// Compatibility tuple required before this adapter accepts a verified model.
pub const WHISPER_COMPATIBILITY: ModelCompatibility = ModelCompatibility {
    purpose: "asr",
    runtime: "whisper.cpp",
    architecture: "whisper",
    quantization: "q5_1",
};

/// Validated ASR configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsrConfig {
    threads: u8,
    language_mode: LanguageMode,
}

impl AsrConfig {
    /// Constructs a bounded CPU-only inference configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AsrError::InvalidConfig`] for zero threads or a value above
    /// [`MAX_INFERENCE_THREADS`].
    pub const fn new(threads: u8, language_mode: LanguageMode) -> Result<Self, AsrError> {
        if threads == 0 || threads > MAX_INFERENCE_THREADS {
            return Err(AsrError::InvalidConfig);
        }
        Ok(Self {
            threads,
            language_mode,
        })
    }

    /// Returns the native inference worker count.
    #[must_use]
    pub const fn threads(self) -> u8 {
        self.threads
    }

    /// Returns the automatic or fixed local inference language policy.
    #[must_use]
    pub const fn language_mode(self) -> LanguageMode {
        self.language_mode
    }
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            language_mode: LanguageMode::Automatic,
        }
    }
}

/// A borrowed mono 16 kHz PCM window that passed all adapter limits.
#[derive(Clone, Copy)]
pub struct CanonicalAudio<'a> {
    samples: &'a [f32],
}

impl<'a> CanonicalAudio<'a> {
    /// Validates a canonical PCM window without copying it.
    ///
    /// # Errors
    ///
    /// Rejects empty, over-30-second, non-finite, or out-of-range input.
    pub fn new(samples: &'a [f32]) -> Result<Self, AsrError> {
        if samples.is_empty() {
            return Err(AsrError::EmptyAudio);
        }
        if samples.len() > MAX_INFERENCE_SAMPLES {
            return Err(AsrError::AudioTooLong);
        }
        if samples
            .iter()
            .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
        {
            return Err(AsrError::InvalidSample);
        }
        Ok(Self { samples })
    }

    /// Returns the validated sample count.
    #[must_use]
    pub const fn len(self) -> usize {
        self.samples.len()
    }

    /// Returns whether the validated window is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.samples.is_empty()
    }

    /// Returns the validated PCM samples.
    #[must_use]
    pub const fn as_slice(self) -> &'a [f32] {
        self.samples
    }
}

/// Byte offsets and timestamps for one transcript segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranscriptSegment {
    /// UTF-8 byte offset where the segment starts in [`Transcript::text`].
    pub byte_start: usize,
    /// UTF-8 byte offset immediately after the segment in [`Transcript::text`].
    pub byte_end: usize,
    /// Segment start in milliseconds from the beginning of the window.
    pub start_ms: u32,
    /// Segment end in milliseconds from the beginning of the window.
    pub end_ms: u32,
}

/// Bounded transcript returned by the local runtime.
#[derive(Eq, PartialEq)]
pub struct Transcript {
    text: Vec<u8>,
    segments: Vec<TranscriptSegment>,
}

impl Transcript {
    /// Returns the exact UTF-8 text emitted by the runtime.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.text).unwrap_or_default()
    }

    /// Returns bounded segment metadata referencing [`Self::text`].
    #[must_use]
    pub fn segments(&self) -> &[TranscriptSegment] {
        &self.segments
    }
}

impl Drop for Transcript {
    fn drop(&mut self) {
        self.text.fill(0);
    }
}

/// CPU-only local Whisper context initialized from a verified file handle.
pub struct LocalWhisper {
    context: WhisperContext,
    config: AsrConfig,
}

impl LocalWhisper {
    /// Loads the native runtime from the exact handle already hashed by the
    /// model-security gate.
    ///
    /// Native runtime logging is disabled before parsing the model. The path
    /// API is deliberately not used.
    ///
    /// # Errors
    ///
    /// Returns a payload-free error on compatibility, allocation, read, or
    /// native initialization failure.
    pub fn from_verified_model(
        verified: VerifiedModel,
        config: AsrConfig,
    ) -> Result<Self, AsrError> {
        let verified = verified
            .into_verified_bytes()
            .map_err(|_| AsrError::ModelReadFailed)?;
        Self::from_verified_bytes(verified, config)
    }

    /// Loads the native runtime from owned bytes that independently passed the
    /// compiled model gate, such as bytes received by an isolated worker.
    ///
    /// # Errors
    ///
    /// Returns a payload-free error on compatibility, size, or native
    /// initialization failure.
    pub fn from_verified_bytes(
        verified: VerifiedModelBytes,
        config: AsrConfig,
    ) -> Result<Self, AsrError> {
        let entry = verified.entry();
        if !SUPPORTED_MODEL_IDS.contains(&entry.id)
            || entry.purpose != WHISPER_COMPATIBILITY.purpose
            || entry.runtime != WHISPER_COMPATIBILITY.runtime
            || entry.architecture != WHISPER_COMPATIBILITY.architecture
            || entry.quantization != WHISPER_COMPATIBILITY.quantization
        {
            return Err(AsrError::IncompatibleVerifiedModel);
        }

        let model_size =
            usize::try_from(entry.size_bytes).map_err(|_| AsrError::ModelSizeUnsupported)?;
        if model_size == 0 || model_size > MAX_ADAPTER_MODEL_BYTES {
            return Err(AsrError::ModelSizeUnsupported);
        }
        let model_bytes = verified.into_vec();

        whisper_rs::install_logging_hooks();
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(false);
        parameters.flash_attn(false);
        let context = WhisperContext::new_from_buffer_with_params(&model_bytes, parameters)
            .map_err(|_| AsrError::RuntimeInitFailed)?;

        Ok(Self { context, config })
    }

    /// Returns the exact bundled `whisper.cpp` semantic version.
    #[must_use]
    pub fn runtime_version() -> &'static str {
        whisper_rs::WHISPER_CPP_VERSION
    }

    /// Transcribes one bounded canonical audio window synchronously.
    ///
    /// No prompt, prior transcript, file, network, or callback is supplied to
    /// the native runtime. Decoding uses greedy temperature-zero sampling.
    ///
    /// # Errors
    ///
    /// Returns a payload-free error if state creation, inference, UTF-8
    /// extraction, timestamp validation, or an output bound fails.
    pub fn transcribe(&self, audio: CanonicalAudio<'_>) -> Result<Transcript, AsrError> {
        let mut state = self
            .context
            .create_state()
            .map_err(|_| AsrError::StateInitFailed)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(i32::from(self.config.threads));
        params.set_n_max_text_ctx(256);
        params.set_offset_ms(0);
        params.set_duration_ms(duration_ms(audio.len()));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(false);
        params.set_single_segment(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(false);
        params.set_max_len(0);
        params.set_max_tokens(256);
        params.set_debug_mode(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        match self.config.language_mode {
            LanguageMode::Automatic => {
                params.set_language(None);
                params.set_detect_language(true);
            }
            LanguageMode::Fixed(language) => {
                params.set_language(Some(language.iso_639_1()));
                params.set_detect_language(false);
            }
        }

        state
            .full(params, audio.as_slice())
            .map_err(|_| AsrError::InferenceFailed)?;
        collect_transcript(&state)
    }
}

fn collect_transcript(state: &whisper_rs::WhisperState) -> Result<Transcript, AsrError> {
    let count =
        usize::try_from(state.full_n_segments()).map_err(|_| AsrError::InvalidTranscript)?;
    if count > MAX_TRANSCRIPT_SEGMENTS {
        return Err(AsrError::TooManySegments);
    }

    let mut text = Vec::new();
    text.try_reserve(MAX_TRANSCRIPT_BYTES)
        .map_err(|_| AsrError::TranscriptAllocationFailed)?;
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(count)
        .map_err(|_| AsrError::TranscriptAllocationFailed)?;

    for segment in state.as_iter() {
        let value = segment.to_str().map_err(|_| AsrError::InvalidTranscript)?;
        let next_len = text
            .len()
            .checked_add(value.len())
            .ok_or(AsrError::TranscriptTooLong)?;
        if next_len > MAX_TRANSCRIPT_BYTES {
            return Err(AsrError::TranscriptTooLong);
        }
        let byte_start = text.len();
        text.extend_from_slice(value.as_bytes());

        let start_ms = timestamp_ms(segment.start_timestamp())?;
        let end_ms = timestamp_ms(segment.end_timestamp())?;
        if end_ms < start_ms || end_ms > MAX_INFERENCE_MILLISECONDS {
            return Err(AsrError::InvalidTimestamp);
        }
        segments.push(TranscriptSegment {
            byte_start,
            byte_end: next_len,
            start_ms,
            end_ms,
        });
    }

    Ok(Transcript { text, segments })
}

fn duration_ms(samples: usize) -> i32 {
    let milliseconds = samples.saturating_mul(1_000) / ASR_SAMPLE_RATE_HZ;
    i32::try_from(milliseconds).unwrap_or(i32::MAX)
}

fn timestamp_ms(centiseconds: i64) -> Result<u32, AsrError> {
    let milliseconds = centiseconds
        .checked_mul(10)
        .ok_or(AsrError::InvalidTimestamp)?;
    u32::try_from(milliseconds).map_err(|_| AsrError::InvalidTimestamp)
}

/// Payload-free failures from the native ASR boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsrError {
    /// The requested worker configuration is outside compiled limits.
    InvalidConfig,
    /// The verified model identity does not belong to this adapter.
    IncompatibleVerifiedModel,
    /// The model is too large for this adapter or this address space.
    ModelSizeUnsupported,
    /// Memory for the verified model could not be reserved.
    ModelAllocationFailed,
    /// The verified handle could not be rewound or read completely.
    ModelReadFailed,
    /// The native runtime rejected the verified model bytes.
    RuntimeInitFailed,
    /// The native runtime could not create isolated inference state.
    StateInitFailed,
    /// A zero-sample inference window is not accepted.
    EmptyAudio,
    /// The inference window exceeds 30 seconds.
    AudioTooLong,
    /// A sample is non-finite or outside normalized PCM range.
    InvalidSample,
    /// Native inference failed without exposing sensitive payloads.
    InferenceFailed,
    /// The native runtime returned more segments than allowed.
    TooManySegments,
    /// The transcript exceeds the compiled UTF-8 byte limit.
    TranscriptTooLong,
    /// Transcript storage could not be reserved.
    TranscriptAllocationFailed,
    /// The native runtime returned invalid UTF-8 or segment metadata.
    InvalidTranscript,
    /// A native segment timestamp was negative, reversed, or out of range.
    InvalidTimestamp,
}

impl fmt::Display for AsrError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid ASR configuration",
            Self::IncompatibleVerifiedModel => {
                "verified model is incompatible with the ASR adapter"
            }
            Self::ModelSizeUnsupported => "verified model size is unsupported",
            Self::ModelAllocationFailed => "verified model memory allocation failed",
            Self::ModelReadFailed => "verified model handle could not be read",
            Self::RuntimeInitFailed => "local ASR runtime initialization failed",
            Self::StateInitFailed => "local ASR state initialization failed",
            Self::EmptyAudio => "ASR audio window is empty",
            Self::AudioTooLong => "ASR audio window exceeds the hard limit",
            Self::InvalidSample => "ASR audio window contains an invalid sample",
            Self::InferenceFailed => "local ASR inference failed",
            Self::TooManySegments => "ASR transcript has too many segments",
            Self::TranscriptTooLong => "ASR transcript exceeds the hard limit",
            Self::TranscriptAllocationFailed => "ASR transcript memory allocation failed",
            Self::InvalidTranscript => "local ASR runtime returned an invalid transcript",
            Self::InvalidTimestamp => "local ASR runtime returned an invalid timestamp",
        };
        formatter.write_str(message)
    }
}

impl Error for AsrError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_is_bounded() {
        assert_eq!(
            AsrConfig::new(0, LanguageMode::Automatic),
            Err(AsrError::InvalidConfig)
        );
        let fixed = AsrConfig::new(8, LanguageMode::Fixed(Language::English));
        assert_eq!(fixed.map(AsrConfig::threads), Ok(8));
        assert_eq!(
            fixed.map(AsrConfig::language_mode),
            Ok(LanguageMode::Fixed(Language::English))
        );
        assert_eq!(
            AsrConfig::new(9, LanguageMode::Automatic),
            Err(AsrError::InvalidConfig)
        );
    }

    #[test]
    fn fixed_languages_expose_only_reviewed_iso_codes() {
        assert_eq!(Language::English.iso_639_1(), "en");
        assert_eq!(Language::Hindi.iso_639_1(), "hi");
        assert_eq!(Language::Korean.iso_639_1(), "ko");
    }

    #[test]
    fn canonical_audio_rejects_invalid_windows() {
        assert!(matches!(
            CanonicalAudio::new(&[]),
            Err(AsrError::EmptyAudio)
        ));
        assert!(matches!(
            CanonicalAudio::new(&vec![0.0; MAX_INFERENCE_SAMPLES + 1]),
            Err(AsrError::AudioTooLong)
        ));
        assert!(matches!(
            CanonicalAudio::new(&[f32::NAN]),
            Err(AsrError::InvalidSample)
        ));
        assert!(matches!(
            CanonicalAudio::new(&[1.01]),
            Err(AsrError::InvalidSample)
        ));
    }

    #[test]
    fn canonical_audio_accepts_exact_boundaries() {
        let samples = [-1.0, 0.0, 1.0];
        let audio = CanonicalAudio::new(&samples).map(CanonicalAudio::len);
        assert_eq!(audio, Ok(3));
    }

    #[test]
    fn timing_conversions_are_bounded() {
        assert_eq!(duration_ms(ASR_SAMPLE_RATE_HZ), 1_000);
        assert_eq!(timestamp_ms(3_000), Ok(30_000));
        assert_eq!(timestamp_ms(-1), Err(AsrError::InvalidTimestamp));
    }
}
