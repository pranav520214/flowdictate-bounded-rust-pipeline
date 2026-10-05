//! Bounded rolling PCM inference feeding monotonic transcript consensus.

use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use flowdictate_asr_ipc::{
    CancellationToken, WorkerError, ASR_SAMPLE_RATE_HZ, MAX_INFERENCE_SAMPLES,
};
use flowdictate_audio::VAD_FRAME_SAMPLES;

use crate::{
    ConsensusCommit, ConsensusCommitter, ConsensusConfig, ConsensusError, TranscriptHypothesis,
    TranscriptionBackend,
};

const SAMPLES_PER_MILLISECOND: u64 = 16;
const _: () = assert!(ASR_SAMPLE_RATE_HZ == 16_000);

/// Validated bounds and cadence for rolling local inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RollingInferenceConfig {
    minimum_window: usize,
    inference_step: usize,
    maximum_window: usize,
}

impl RollingInferenceConfig {
    /// Creates a canonical-frame-aligned rolling policy.
    ///
    /// # Errors
    ///
    /// Rejects zero or non-frame-aligned sizes, a minimum/step larger than the
    /// configured maximum, or a maximum above the 30-second ASR hard limit.
    pub const fn new(
        minimum_window_samples: usize,
        inference_step_samples: usize,
        maximum_window_samples: usize,
    ) -> Result<Self, RollingInferenceError> {
        if minimum_window_samples == 0
            || inference_step_samples == 0
            || maximum_window_samples == 0
            || !minimum_window_samples.is_multiple_of(VAD_FRAME_SAMPLES)
            || !inference_step_samples.is_multiple_of(VAD_FRAME_SAMPLES)
            || !maximum_window_samples.is_multiple_of(VAD_FRAME_SAMPLES)
            || minimum_window_samples > maximum_window_samples
            || inference_step_samples > maximum_window_samples
            || maximum_window_samples > MAX_INFERENCE_SAMPLES
        {
            return Err(RollingInferenceError::InvalidConfig);
        }
        Ok(Self {
            minimum_window: minimum_window_samples,
            inference_step: inference_step_samples,
            maximum_window: maximum_window_samples,
        })
    }

    /// Returns the PCM required before the first partial inference.
    #[must_use]
    pub const fn minimum_window_samples(self) -> usize {
        self.minimum_window
    }

    /// Returns the new-PCM cadence between partial inferences.
    #[must_use]
    pub const fn inference_step_samples(self) -> usize {
        self.inference_step
    }

    /// Returns the hard retained-PCM limit.
    #[must_use]
    pub const fn maximum_window_samples(self) -> usize {
        self.maximum_window
    }
}

impl Default for RollingInferenceConfig {
    fn default() -> Self {
        Self {
            minimum_window: 16_384,
            inference_step: 8_192,
            maximum_window: MAX_INFERENCE_SAMPLES,
        }
    }
}

/// Payload-free result of one canonical PCM push.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RollingInferenceReport {
    /// Canonical samples accepted from this push.
    pub samples_received: usize,
    /// Whether one local inference request ran.
    pub inference_ran: bool,
    /// Canonical samples presented to the backend for this inference.
    pub inference_samples: usize,
    /// Wall time spent inside the local transcription backend.
    pub inference_elapsed: Duration,
    /// Whether consensus appended one immutable text delta.
    pub commit_written: bool,
    /// Canonical samples retained after consensus-driven trimming.
    pub retained_samples: usize,
    /// Absolute canonical samples observed in this rolling session.
    pub observed_samples: u64,
    /// Absolute timestamp before which PCM was eligible for erasure.
    pub discard_audio_before_ms: u64,
}

/// Non-sensitive result of cancellation, discontinuity, or explicit reset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RollingResetReport {
    /// Canonical PCM samples overwritten and discarded.
    pub samples_discarded: usize,
    /// Consensus generation active after the reset.
    pub generation: u64,
}

/// Owns one bounded canonical PCM window, ASR backend, and consensus state.
///
/// The owner performs no capture, filesystem, network, UI, logging, or
/// persistence work. It retains only uncommitted mono 16 kHz PCM and the
/// consensus engine's uncommitted transcript suffix.
pub struct RollingInference<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    config: RollingInferenceConfig,
    backend: B,
    consensus: ConsensusCommitter,
    pcm: Vec<f32>,
    window_start_sample: u64,
    observed_samples: u64,
    samples_since_inference: usize,
}

impl<B> RollingInference<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    /// Preallocates the complete rolling PCM window and consensus state.
    ///
    /// # Errors
    ///
    /// Returns a fixed consensus or bounded-allocation failure.
    pub fn new(
        config: RollingInferenceConfig,
        consensus_config: ConsensusConfig,
        backend: B,
    ) -> Result<Self, RollingInferenceError> {
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(config.maximum_window)
            .map_err(|_| RollingInferenceError::AllocationFailed)?;
        let consensus =
            ConsensusCommitter::new(consensus_config).map_err(RollingInferenceError::Consensus)?;
        Ok(Self {
            config,
            backend,
            consensus,
            pcm,
            window_start_sample: 0,
            observed_samples: 0,
            samples_since_inference: 0,
        })
    }

    /// Returns the exact number of currently retained canonical samples.
    #[must_use]
    pub const fn retained_samples(&self) -> usize {
        self.pcm.len()
    }

    /// Returns the absolute canonical sample count accepted in this session.
    #[must_use]
    pub const fn observed_samples(&self) -> u64 {
        self.observed_samples
    }

    /// Returns the consensus generation that owns current volatile state.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.consensus.generation()
    }

    /// Borrows the current uncommitted display suffix for a local overlay.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        self.consensus.pending_text()
    }

    #[cfg(test)]
    pub(crate) const fn backend_for_test(&self) -> &B {
        &self.backend
    }

    pub(crate) const fn backend(&self) -> &B {
        &self.backend
    }

    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Appends exact canonical frames and runs at most one rolling inference.
    ///
    /// The call is atomic for output-capacity and hard-window-limit failures.
    /// Malformed PCM, cancellation, backend failure, or invalid hypotheses fail
    /// closed and erase all volatile PCM and pending consensus text.
    ///
    /// # Errors
    ///
    /// Rejects empty/misaligned/invalid PCM, insufficient output capacity when
    /// inference is due, the 30-second window boundary, cancellation, backend
    /// failure, invalid consensus output, timestamp overflow, or allocation
    /// failure.
    pub fn push(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<RollingInferenceReport, RollingInferenceError> {
        if cancellation.is_cancelled() {
            self.clear_volatile();
            return Err(RollingInferenceError::Cancelled);
        }
        if samples.is_empty() || !samples.len().is_multiple_of(VAD_FRAME_SAMPLES) {
            self.clear_volatile();
            return Err(RollingInferenceError::InvalidAudio);
        }
        if samples
            .iter()
            .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
        {
            self.clear_volatile();
            return Err(RollingInferenceError::InvalidAudio);
        }
        let projected_window = self
            .pcm
            .len()
            .checked_add(samples.len())
            .ok_or(RollingInferenceError::WindowLimitReached)?;
        if projected_window > self.config.maximum_window {
            return Err(RollingInferenceError::WindowLimitReached);
        }
        let Ok(received) = u64::try_from(samples.len()) else {
            self.clear_volatile();
            return Err(RollingInferenceError::TimeOverflow);
        };
        let Some(projected_observed) = self.observed_samples.checked_add(received) else {
            self.clear_volatile();
            return Err(RollingInferenceError::TimeOverflow);
        };
        let Some(projected_since) = self.samples_since_inference.checked_add(samples.len()) else {
            self.clear_volatile();
            return Err(RollingInferenceError::TimeOverflow);
        };
        let inference_due = projected_window >= self.config.minimum_window
            && projected_since >= self.config.inference_step;
        if inference_due && outputs.capacity() == outputs.len() {
            return Err(RollingInferenceError::OutputCapacityTooSmall);
        }

        self.pcm.extend_from_slice(samples);
        self.observed_samples = projected_observed;
        self.samples_since_inference = projected_since;
        if !inference_due {
            return Ok(self.report(samples.len(), false, false));
        }

        let initial_outputs = outputs.len();
        let inference_samples = self.pcm.len();
        let inference_started = Instant::now();
        let hypothesis = match self.backend.transcribe(&self.pcm, cancellation) {
            Ok(hypothesis) => hypothesis,
            Err(WorkerError::Cancelled) => {
                self.clear_volatile();
                return Err(RollingInferenceError::Cancelled);
            }
            Err(error) => {
                self.clear_volatile();
                return Err(RollingInferenceError::Asr(error));
            }
        };
        let inference_elapsed = inference_started.elapsed();
        let window_start_ms = samples_to_milliseconds(self.window_start_sample);
        let observed_end_ms = samples_to_milliseconds(self.observed_samples);
        let consensus =
            self.consensus
                .observe(window_start_ms, observed_end_ms, &hypothesis, outputs);
        drop(hypothesis);
        let consensus = match consensus {
            Ok(report) => report,
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.clear_volatile();
                return Err(RollingInferenceError::Consensus(error));
            }
        };
        self.samples_since_inference = 0;
        if let Err(error) = self.discard_committed_pcm(consensus.discard_audio_before_ms) {
            outputs.truncate(initial_outputs);
            self.clear_volatile();
            return Err(error);
        }
        Ok(self.inference_report(
            samples.len(),
            consensus.commit_written,
            inference_samples,
            inference_elapsed,
        ))
    }

    /// Runs one final local inference, commits only the remaining suffix, and
    /// erases the complete rolling window and pending consensus state.
    ///
    /// # Errors
    ///
    /// Rejects insufficient output capacity, cancellation, backend/consensus
    /// failure, or timestamp overflow. Every non-capacity failure erases the
    /// volatile state.
    pub fn finalize(
        &mut self,
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<RollingInferenceReport, RollingInferenceError> {
        self.finalize_with_tail(&[], cancellation, outputs)
    }

    /// Appends one unpadded canonical tail, runs the final inference, and
    /// erases the complete segment.
    ///
    /// The tail is limited to fewer than one complete VAD frame because exact
    /// complete frames must enter through [`Self::push`]. Hard-window and
    /// output-capacity failures are atomic and leave the prior state unchanged.
    ///
    /// # Errors
    ///
    /// Rejects an oversized/invalid tail, insufficient output capacity, the
    /// hard window limit, cancellation, backend/consensus failure, or timestamp
    /// overflow. Every non-capacity/non-window failure erases volatile state.
    pub fn finalize_with_tail(
        &mut self,
        tail: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<RollingInferenceReport, RollingInferenceError> {
        if cancellation.is_cancelled() {
            self.clear_volatile();
            return Err(RollingInferenceError::Cancelled);
        }
        if tail.len() >= VAD_FRAME_SAMPLES
            || tail
                .iter()
                .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
        {
            self.clear_volatile();
            return Err(RollingInferenceError::InvalidAudio);
        }
        if self.pcm.is_empty() && tail.is_empty() {
            let report = self.report(0, false, false);
            self.consensus.reset();
            return Ok(report);
        }
        if outputs.capacity() == outputs.len() {
            return Err(RollingInferenceError::OutputCapacityTooSmall);
        }
        let projected_window = self
            .pcm
            .len()
            .checked_add(tail.len())
            .ok_or(RollingInferenceError::WindowLimitReached)?;
        if projected_window > self.config.maximum_window {
            return Err(RollingInferenceError::WindowLimitReached);
        }
        let tail_samples = u64::try_from(tail.len()).map_err(|_| {
            self.clear_volatile();
            RollingInferenceError::TimeOverflow
        })?;
        let projected_observed =
            self.observed_samples
                .checked_add(tail_samples)
                .ok_or_else(|| {
                    self.clear_volatile();
                    RollingInferenceError::TimeOverflow
                })?;
        self.pcm.extend_from_slice(tail);
        self.observed_samples = projected_observed;
        let initial_outputs = outputs.len();
        let inference_samples = self.pcm.len();
        let inference_started = Instant::now();
        let hypothesis = match self.backend.transcribe(&self.pcm, cancellation) {
            Ok(hypothesis) => hypothesis,
            Err(WorkerError::Cancelled) => {
                self.clear_volatile();
                return Err(RollingInferenceError::Cancelled);
            }
            Err(error) => {
                self.clear_volatile();
                return Err(RollingInferenceError::Asr(error));
            }
        };
        let inference_elapsed = inference_started.elapsed();
        let result = self.consensus.finalize(
            samples_to_milliseconds(self.window_start_sample),
            samples_to_milliseconds(self.observed_samples),
            &hypothesis,
            outputs,
        );
        drop(hypothesis);
        let result = match result {
            Ok(report) => report,
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.clear_volatile();
                return Err(RollingInferenceError::Consensus(error));
            }
        };
        let report = RollingInferenceReport {
            samples_received: tail.len(),
            inference_ran: true,
            inference_samples,
            inference_elapsed,
            commit_written: result.commit_written,
            retained_samples: 0,
            observed_samples: self.observed_samples,
            discard_audio_before_ms: result.discard_audio_before_ms,
        };
        self.clear_segment();
        Ok(report)
    }

    /// Erases the affected PCM/hypothesis generation after missing audio.
    pub fn handle_discontinuity(&mut self) -> RollingResetReport {
        self.clear_segment()
    }

    /// Cancels the shared request and erases PCM and pending hypothesis text.
    pub fn cancel(&mut self, cancellation: &CancellationToken) -> RollingResetReport {
        cancellation.cancel();
        self.clear_segment()
    }

    /// Erases all state and restarts the absolute session sample clock.
    pub fn reset_session(&mut self) -> RollingResetReport {
        let report = self.clear_segment();
        self.window_start_sample = 0;
        self.observed_samples = 0;
        report
    }

    fn report(
        &self,
        samples_received: usize,
        inference_ran: bool,
        commit_written: bool,
    ) -> RollingInferenceReport {
        RollingInferenceReport {
            samples_received,
            inference_ran,
            inference_samples: 0,
            inference_elapsed: Duration::ZERO,
            commit_written,
            retained_samples: self.pcm.len(),
            observed_samples: self.observed_samples,
            discard_audio_before_ms: self.consensus.discard_audio_before_ms(),
        }
    }

    fn inference_report(
        &self,
        samples_received: usize,
        commit_written: bool,
        inference_samples: usize,
        inference_elapsed: Duration,
    ) -> RollingInferenceReport {
        RollingInferenceReport {
            samples_received,
            inference_ran: true,
            inference_samples,
            inference_elapsed,
            commit_written,
            retained_samples: self.pcm.len(),
            observed_samples: self.observed_samples,
            discard_audio_before_ms: self.consensus.discard_audio_before_ms(),
        }
    }

    fn discard_committed_pcm(
        &mut self,
        discard_before_ms: u64,
    ) -> Result<(), RollingInferenceError> {
        let discard_before_sample = discard_before_ms
            .checked_mul(SAMPLES_PER_MILLISECOND)
            .ok_or(RollingInferenceError::TimeOverflow)?;
        let available_end = self
            .window_start_sample
            .checked_add(
                u64::try_from(self.pcm.len()).map_err(|_| RollingInferenceError::TimeOverflow)?,
            )
            .ok_or(RollingInferenceError::TimeOverflow)?;
        let target = discard_before_sample.min(available_end);
        let count = target.saturating_sub(self.window_start_sample);
        let count = usize::try_from(count).map_err(|_| RollingInferenceError::TimeOverflow)?;
        if count == 0 {
            return Ok(());
        }
        let retained = self.pcm.len().saturating_sub(count);
        self.pcm.copy_within(count.., 0);
        self.pcm[retained..].fill(0.0);
        self.pcm.truncate(retained);
        self.window_start_sample = target;
        Ok(())
    }

    fn clear_segment(&mut self) -> RollingResetReport {
        let samples_discarded = self.pcm.len();
        self.pcm.fill(0.0);
        self.pcm.clear();
        self.window_start_sample = self.observed_samples;
        self.samples_since_inference = 0;
        self.consensus.reset();
        RollingResetReport {
            samples_discarded,
            generation: self.consensus.generation(),
        }
    }

    fn clear_volatile(&mut self) {
        let _ = self.clear_segment();
    }
}

impl<B> Drop for RollingInference<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    fn drop(&mut self) {
        self.clear_volatile();
    }
}

const fn samples_to_milliseconds(samples: u64) -> u64 {
    samples / SAMPLES_PER_MILLISECOND
}

/// Payload-free rolling inference failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollingInferenceError {
    /// PCM/cadence/window configuration is invalid.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// PCM was empty, misaligned, non-finite, or outside `[-1, 1]`.
    InvalidAudio,
    /// Appending would exceed the configured/compiled window hard limit.
    WindowLimitReached,
    /// A possible inference has no caller-owned commit slot.
    OutputCapacityTooSmall,
    /// The one-shot session/request was cancelled.
    Cancelled,
    /// Absolute sample/time arithmetic exceeded its representation.
    TimeOverflow,
    /// The isolated local ASR backend failed.
    Asr(WorkerError),
    /// The bounded transcript consensus engine rejected the hypothesis.
    Consensus(ConsensusError),
}

impl fmt::Display for RollingInferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid rolling inference configuration",
            Self::AllocationFailed => "rolling inference allocation failed",
            Self::InvalidAudio => "rolling inference audio is invalid",
            Self::WindowLimitReached => "rolling inference window reached its hard limit",
            Self::OutputCapacityTooSmall => "rolling inference output capacity is too small",
            Self::Cancelled => "rolling inference was cancelled",
            Self::TimeOverflow => "rolling inference time exceeded its bound",
            Self::Asr(_) => "rolling local ASR failed",
            Self::Consensus(_) => "rolling transcript consensus failed",
        };
        formatter.write_str(message)
    }
}

impl Error for RollingInferenceError {}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::HypothesisSegment;

    struct OwnedSegment {
        text: Vec<u8>,
        start_ms: u32,
        end_ms: u32,
    }

    struct OwnedHypothesis {
        segments: Vec<OwnedSegment>,
    }

    impl OwnedHypothesis {
        fn empty() -> Self {
            Self {
                segments: Vec::new(),
            }
        }

        fn one(text: &str, start_ms: u32, end_ms: u32) -> Self {
            Self {
                segments: vec![OwnedSegment {
                    text: text.as_bytes().to_vec(),
                    start_ms,
                    end_ms,
                }],
            }
        }
    }

    impl TranscriptHypothesis for OwnedHypothesis {
        fn segment_count(&self) -> usize {
            self.segments.len()
        }

        fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>> {
            let segment = self.segments.get(index)?;
            let text = std::str::from_utf8(&segment.text).ok()?;
            Some(HypothesisSegment {
                text,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
            })
        }
    }

    impl Drop for OwnedHypothesis {
        fn drop(&mut self) {
            for segment in &mut self.segments {
                segment.text.fill(0);
            }
        }
    }

    struct FakeBackend {
        responses: VecDeque<Result<OwnedHypothesis, WorkerError>>,
        window_lengths: Vec<usize>,
    }

    impl FakeBackend {
        fn new(responses: impl IntoIterator<Item = Result<OwnedHypothesis, WorkerError>>) -> Self {
            Self {
                responses: responses.into_iter().collect(),
                window_lengths: Vec::new(),
            }
        }
    }

    impl TranscriptionBackend for FakeBackend {
        type Transcript = OwnedHypothesis;

        fn transcribe(
            &mut self,
            samples: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Self::Transcript, WorkerError> {
            self.window_lengths.push(samples.len());
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            self.responses
                .pop_front()
                .unwrap_or(Ok(OwnedHypothesis::empty()))
        }
    }

    fn config(maximum: usize) -> Result<RollingInferenceConfig, RollingInferenceError> {
        RollingInferenceConfig::new(VAD_FRAME_SAMPLES * 2, VAD_FRAME_SAMPLES, maximum)
    }

    fn consensus() -> Result<ConsensusConfig, ConsensusError> {
        ConsensusConfig::new(2, 0, 0)
    }

    fn rolling(
        responses: impl IntoIterator<Item = Result<OwnedHypothesis, WorkerError>>,
    ) -> Result<RollingInference<FakeBackend>, RollingInferenceError> {
        RollingInference::new(
            config(VAD_FRAME_SAMPLES * 4)?,
            consensus().map_err(RollingInferenceError::Consensus)?,
            FakeBackend::new(responses),
        )
    }

    #[test]
    fn config_enforces_frame_alignment_and_asr_hard_limit() {
        assert_eq!(
            RollingInferenceConfig::new(1, VAD_FRAME_SAMPLES, VAD_FRAME_SAMPLES),
            Err(RollingInferenceError::InvalidConfig)
        );
        assert_eq!(
            RollingInferenceConfig::new(
                VAD_FRAME_SAMPLES,
                VAD_FRAME_SAMPLES,
                MAX_INFERENCE_SAMPLES + VAD_FRAME_SAMPLES
            ),
            Err(RollingInferenceError::InvalidConfig)
        );
    }

    #[test]
    fn cadence_runs_at_most_one_inference_per_push() -> Result<(), RollingInferenceError> {
        let mut rolling = rolling([Ok(OwnedHypothesis::empty()), Ok(OwnedHypothesis::empty())])?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(2);
        let first = rolling.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert!(!first.inference_ran);
        assert_eq!(first.inference_samples, 0);
        assert_eq!(first.inference_elapsed, Duration::ZERO);
        let second = rolling.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert!(second.inference_ran);
        assert_eq!(second.inference_samples, 512);
        let third = rolling.push(&[0.0; VAD_FRAME_SAMPLES * 2], &token, &mut outputs)?;
        assert!(third.inference_ran);
        assert_eq!(third.inference_samples, 1_024);
        assert_eq!(rolling.backend.window_lengths, [512, 1_024]);
        Ok(())
    }

    #[test]
    fn repeated_hypotheses_commit_and_trim_pcm_with_no_overlap() -> Result<(), RollingInferenceError>
    {
        let mut rolling = rolling([
            Ok(OwnedHypothesis::one("hello", 0, 16)),
            Ok(OwnedHypothesis::one("hello", 0, 16)),
        ])?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(2);
        rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        let report = rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert!(report.commit_written);
        assert_eq!(outputs[0].text(), "hello");
        assert_eq!(report.discard_audio_before_ms, 16);
        assert_eq!(report.retained_samples, VAD_FRAME_SAMPLES * 2);
        assert_eq!(
            rolling.window_start_sample,
            u64::try_from(VAD_FRAME_SAMPLES).map_err(|_| RollingInferenceError::TimeOverflow)?
        );
        Ok(())
    }

    #[test]
    fn committed_pcm_trim_preserves_configured_overlap() -> Result<(), RollingInferenceError> {
        let backend = FakeBackend::new([
            Ok(OwnedHypothesis::one("hello", 0, 32)),
            Ok(OwnedHypothesis::one("hello", 0, 32)),
            Ok(OwnedHypothesis::one("hello", 0, 32)),
        ]);
        let mut rolling = RollingInference::new(
            config(VAD_FRAME_SAMPLES * 6)?,
            ConsensusConfig::new(2, 16, 16).map_err(RollingInferenceError::Consensus)?,
            backend,
        )?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(2);
        rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        let report = rolling.push(&[0.1; VAD_FRAME_SAMPLES], &token, &mut outputs)?;

        assert!(report.commit_written);
        assert_eq!(outputs[0].text(), "hello");
        assert_eq!(report.discard_audio_before_ms, 16);
        assert_eq!(report.retained_samples, VAD_FRAME_SAMPLES * 3);
        assert_eq!(rolling.window_start_sample, VAD_FRAME_SAMPLES as u64);
        assert_eq!(rolling.backend.window_lengths, [512, 768, 1_024]);
        Ok(())
    }

    #[test]
    fn finalization_commits_one_unstable_suffix_and_erases_pcm() -> Result<(), RollingInferenceError>
    {
        let mut rolling = rolling([
            Ok(OwnedHypothesis::empty()),
            Ok(OwnedHypothesis::one("final", 0, 32)),
        ])?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(2);
        rolling.push(&[0.2; VAD_FRAME_SAMPLES * 2], &token, &mut outputs)?;
        let report = rolling.finalize_with_tail(&[0.25; 37], &token, &mut outputs)?;
        assert!(report.inference_ran);
        assert!(report.commit_written);
        assert_eq!(report.samples_received, 37);
        assert_eq!(report.observed_samples, 549);
        assert_eq!(outputs[0].text(), "final");
        assert_eq!(rolling.retained_samples(), 0);
        assert_eq!(rolling.pending_text(), "");
        assert_eq!(rolling.backend.window_lengths, [512, 549]);
        Ok(())
    }

    #[test]
    fn hard_window_limit_and_capacity_fail_without_appending() -> Result<(), RollingInferenceError>
    {
        let backend = FakeBackend::new([Ok(OwnedHypothesis::empty())]);
        let mut limited = RollingInference::new(
            config(VAD_FRAME_SAMPLES * 2)?,
            consensus().map_err(RollingInferenceError::Consensus)?,
            backend,
        )?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(1);
        limited.push(&[0.0; VAD_FRAME_SAMPLES * 2], &token, &mut outputs)?;
        assert_eq!(
            limited.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs),
            Err(RollingInferenceError::WindowLimitReached)
        );
        assert_eq!(limited.retained_samples(), VAD_FRAME_SAMPLES * 2);
        assert_eq!(
            limited.finalize_with_tail(&[0.0; 1], &token, &mut outputs),
            Err(RollingInferenceError::WindowLimitReached)
        );
        assert_eq!(limited.retained_samples(), VAD_FRAME_SAMPLES * 2);

        let mut other = rolling([Ok(OwnedHypothesis::empty())])?;
        let mut no_capacity = Vec::new();
        other.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut no_capacity)?;
        assert_eq!(
            other.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut no_capacity),
            Err(RollingInferenceError::OutputCapacityTooSmall)
        );
        assert_eq!(other.retained_samples(), VAD_FRAME_SAMPLES);
        assert_eq!(
            other.finalize_with_tail(&[0.0; 17], &token, &mut no_capacity),
            Err(RollingInferenceError::OutputCapacityTooSmall)
        );
        assert_eq!(other.retained_samples(), VAD_FRAME_SAMPLES);
        assert_eq!(other.observed_samples(), VAD_FRAME_SAMPLES as u64);
        Ok(())
    }

    #[test]
    fn malformed_pcm_backend_error_and_cancellation_erase_state(
    ) -> Result<(), RollingInferenceError> {
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(2);
        let mut invalid = rolling([Ok(OwnedHypothesis::empty())])?;
        invalid.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert_eq!(
            invalid.push(&[f32::NAN; VAD_FRAME_SAMPLES], &token, &mut outputs),
            Err(RollingInferenceError::InvalidAudio)
        );
        assert_eq!(invalid.retained_samples(), 0);

        let mut invalid_tail = rolling([Ok(OwnedHypothesis::empty())])?;
        invalid_tail.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert_eq!(
            invalid_tail.finalize_with_tail(&[f32::NAN], &token, &mut outputs),
            Err(RollingInferenceError::InvalidAudio)
        );
        assert_eq!(invalid_tail.retained_samples(), 0);

        let mut failed = rolling([Err(WorkerError::IpcFailed)])?;
        failed.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert_eq!(
            failed.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs),
            Err(RollingInferenceError::Asr(WorkerError::IpcFailed))
        );
        assert_eq!(failed.retained_samples(), 0);

        let mut invalid_hypothesis = rolling([Ok(OwnedHypothesis::one("bad", 0, 1_000))])?;
        invalid_hypothesis.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        assert_eq!(
            invalid_hypothesis.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs),
            Err(RollingInferenceError::Consensus(
                ConsensusError::InvalidTimestamp
            ))
        );
        assert_eq!(invalid_hypothesis.retained_samples(), 0);

        let mut cancelled = rolling([Ok(OwnedHypothesis::empty())])?;
        cancelled.push(&[0.0; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        let cancelled_token = CancellationToken::new();
        cancelled_token.cancel();
        assert_eq!(
            cancelled.push(&[0.0; VAD_FRAME_SAMPLES], &cancelled_token, &mut outputs),
            Err(RollingInferenceError::Cancelled)
        );
        assert_eq!(cancelled.retained_samples(), 0);
        Ok(())
    }

    #[test]
    fn discontinuity_cancel_and_session_reset_erase_and_advance_generation(
    ) -> Result<(), RollingInferenceError> {
        let mut rolling = rolling([Ok(OwnedHypothesis::empty())])?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(1);
        rolling.push(&[0.3; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        let discontinuity = rolling.handle_discontinuity();
        assert_eq!(discontinuity.samples_discarded, VAD_FRAME_SAMPLES);
        assert_eq!(discontinuity.generation, 1);
        assert_eq!(
            rolling.observed_samples(),
            u64::try_from(VAD_FRAME_SAMPLES).map_err(|_| RollingInferenceError::TimeOverflow)?
        );
        let reset = rolling.reset_session();
        assert_eq!(reset.generation, 2);
        assert_eq!(rolling.observed_samples(), 0);

        rolling.push(&[0.3; VAD_FRAME_SAMPLES], &token, &mut outputs)?;
        let cancelled = rolling.cancel(&token);
        assert!(token.is_cancelled());
        assert_eq!(cancelled.samples_discarded, VAD_FRAME_SAMPLES);
        assert_eq!(cancelled.generation, 3);
        assert_eq!(rolling.retained_samples(), 0);
        Ok(())
    }
}
