//! Consent and payload-free reporting boundary for the local capture probe.

use std::{error::Error, fmt, time::Duration};

use flowdictate_audio::{CaptureHealthSnapshot, CapturePlan, CaptureSampleFormat};

/// Human-readable disclosure shown before any microphone API is opened.
pub const CONSENT_NOTICE: &str = "FlowDictate local microphone probe. Raw microphone audio stays in volatile memory, is not saved, and is not included in output. Type I CONSENT to continue.";

/// Proof that the exact local microphone disclosure was accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsentToken(());

impl ConsentToken {
    /// Accepts only the exact phrase `I CONSENT`, optionally followed by CR/LF.
    ///
    /// # Errors
    ///
    /// Returns [`ConsentError`] for empty, partial, differently cased, or
    /// extended input.
    pub fn parse(input: &[u8]) -> Result<Self, ConsentError> {
        let without_lf = input.strip_suffix(b"\n").unwrap_or(input);
        let phrase = without_lf.strip_suffix(b"\r").unwrap_or(without_lf);
        if phrase == b"I CONSENT" {
            Ok(Self(()))
        } else {
            Err(ConsentError)
        }
    }
}

/// Consent was absent or did not exactly match the disclosed phrase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsentError;

impl fmt::Display for ConsentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("microphone consent denied")
    }
}

impl Error for ConsentError {}

/// A short, compiled-bounded human capture-probe duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeDuration {
    seconds: u64,
}

impl ProbeDuration {
    /// Maximum duration of one human microphone probe.
    pub const MAX_SECONDS: u64 = 30;

    /// Validates a duration between one and thirty seconds, inclusive.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeDurationError`] outside the compiled range.
    pub const fn new(seconds: u64) -> Result<Self, ProbeDurationError> {
        if seconds == 0 || seconds > Self::MAX_SECONDS {
            Err(ProbeDurationError)
        } else {
            Ok(Self { seconds })
        }
    }

    /// Returns the standard-library duration used by the local probe loop.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        Duration::from_secs(self.seconds)
    }

    /// Returns the validated whole-second value.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }
}

/// Probe duration was zero or exceeded the thirty-second hard limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeDurationError;

impl fmt::Display for ProbeDurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid probe duration")
    }
}

impl Error for ProbeDurationError {}

/// Fixed-schema probe result containing formats and payload-free counters only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeReport {
    duration: ProbeDuration,
    plan: CapturePlan,
    health: CaptureHealthSnapshot,
    drained_samples: u64,
    discontinuity_epoch: u64,
}

impl ProbeReport {
    /// Creates a report after capture has paused and volatile audio was drained.
    #[must_use]
    pub const fn new(
        duration: ProbeDuration,
        plan: CapturePlan,
        health: CaptureHealthSnapshot,
        drained_samples: u64,
        discontinuity_epoch: u64,
    ) -> Self {
        Self {
            duration,
            plan,
            health,
            drained_samples,
            discontinuity_epoch,
        }
    }
}

impl fmt::Display for ProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample_format = match self.plan.sample_format() {
            CaptureSampleFormat::F32 => "f32",
            CaptureSampleFormat::I16 => "i16",
            CaptureSampleFormat::U16 => "u16",
        };
        write!(
            formatter,
            "probe_seconds={} sample_rate_hz={} channels={} sample_format={} \
             ring_capacity_samples={} callback_batches={} written_samples={} \
             sanitized_samples={} dropped_batches={} dropped_samples={} stream_errors={} \
             drained_samples={} discontinuity_epoch={}",
            self.duration.seconds(),
            self.plan.audio_format().sample_rate_hz(),
            self.plan.audio_format().channels(),
            sample_format,
            self.plan.ring_capacity_samples(),
            self.health.callback_batches,
            self.health.written_samples,
            self.health.sanitized_samples,
            self.health.dropped_batches,
            self.health.dropped_samples,
            self.health.stream_errors,
            self.drained_samples,
            self.discontinuity_epoch,
        )
    }
}
