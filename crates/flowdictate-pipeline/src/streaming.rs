//! Canonical VAD-frame routing into bounded rolling inference and consensus.

use std::{error::Error, fmt, time::Duration};

use flowdictate_asr_ipc::{CancellationToken, LanguageMode, WorkerError, MAX_INFERENCE_SAMPLES};
use flowdictate_audio::{
    AudioFormat, AudioProcessingError, AudioProcessingReport, AudioProcessor, FinalizeReason,
    SegmentEvent, VadConfig, VAD_FRAME_SAMPLES,
};

use crate::{
    ConsensusCommit, ConsensusConfig, LanguageConfigurableBackend, RollingInference,
    RollingInferenceConfig, RollingInferenceError, RollingInferenceReport, TranscriptHypothesis,
    TranscriptionBackend,
};

/// Non-sensitive counters from one hardware chunk routed to rolling ASR.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamingPipelineReport {
    /// Underlying local DSP/VAD report.
    pub audio: AudioProcessingReport,
    /// Local partial/final inference calls completed by this chunk.
    pub inferences_run: usize,
    /// Sum of rolling-window canonical samples inferred by this chunk.
    pub inference_samples: u64,
    /// Wall time spent inside local transcription calls for this chunk.
    pub inference_elapsed: Duration,
    /// Immutable consensus deltas appended to caller-owned storage.
    pub commits_written: usize,
    /// Whether a fresh hypothesis is available through `pending_text`.
    pub hypothesis_updated: bool,
    /// Canonical samples retained across confirmation pre-roll and rolling ASR.
    pub retained_samples: usize,
}

/// Non-sensitive result of a user-driven streaming boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingFinalizeReport {
    /// Whether final local inference ran.
    pub inference_ran: bool,
    /// Canonical samples presented to the final local inference.
    pub inference_samples: usize,
    /// Wall time spent inside the final local transcription call.
    pub inference_elapsed: Duration,
    /// Immutable consensus deltas appended to caller-owned storage.
    pub commits_written: usize,
    /// Unpadded canonical tail samples included in final inference.
    pub tail_samples_received: usize,
}

/// Non-sensitive result of applying a session-stable language policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageChangeReport {
    /// Whether a clean backend generation was started.
    pub changed: bool,
    /// Consensus generation active after the operation.
    pub generation: u64,
}

/// Owns canonical DSP/VAD, bounded speech pre-roll, rolling ASR, and consensus.
///
/// Complete canonical frames are routed to rolling inference only while speech
/// is active. Confirmation frames are held in a fixed-capacity pre-roll and
/// transferred on `SpeechStarted`; silence before confirmation never reaches
/// ASR. This owner performs no capture, filesystem, network, UI, logging, or
/// persistence work.
pub struct StreamingDictationPipeline<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    audio: AudioProcessor,
    events: Vec<SegmentEvent>,
    canonical_frames: Vec<f32>,
    canonical_tail: [f32; VAD_FRAME_SAMPLES],
    gate: StreamingSpeechGate,
    rolling: RollingInference<B>,
    maximum_outputs_per_chunk: usize,
}

impl<B> StreamingDictationPipeline<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    /// Constructs and preallocates the complete live rolling-ASR seam.
    ///
    /// # Errors
    ///
    /// Rejects incompatible VAD/rolling limits or any bounded construction or
    /// allocation failure.
    pub fn new(
        format: AudioFormat,
        input_chunk_frames: usize,
        vad_config: VadConfig,
        rolling_config: RollingInferenceConfig,
        consensus_config: ConsensusConfig,
        backend: B,
    ) -> Result<Self, StreamingPipelineError> {
        let start_frames = usize::try_from(vad_config.start_frames())
            .map_err(|_| StreamingPipelineError::InvalidConfig)?;
        let maximum_active_frames = usize::try_from(vad_config.maximum_active_frames())
            .map_err(|_| StreamingPipelineError::InvalidConfig)?;
        let pre_roll_samples = start_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(StreamingPipelineError::InvalidConfig)?;
        let maximum_active_samples = maximum_active_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(StreamingPipelineError::InvalidConfig)?;
        if maximum_active_samples == 0
            || maximum_active_samples > MAX_INFERENCE_SAMPLES
            || maximum_active_samples > rolling_config.maximum_window_samples()
            || pre_roll_samples > maximum_active_samples
        {
            return Err(StreamingPipelineError::InvalidConfig);
        }

        let audio = AudioProcessor::new(format, input_chunk_frames, vad_config)
            .map_err(StreamingPipelineError::Audio)?;
        let event_slots = audio.maximum_events_per_chunk();
        let canonical_slots = audio.maximum_canonical_samples_per_chunk();
        let maximum_outputs_per_chunk = event_slots
            .checked_mul(2)
            .ok_or(StreamingPipelineError::InvalidConfig)?;

        let mut events = Vec::new();
        events
            .try_reserve_exact(event_slots)
            .map_err(|_| StreamingPipelineError::AllocationFailed)?;
        events.resize(event_slots, SegmentEvent::Idle);
        let mut canonical_frames = Vec::new();
        canonical_frames
            .try_reserve_exact(canonical_slots)
            .map_err(|_| StreamingPipelineError::AllocationFailed)?;
        canonical_frames.resize(canonical_slots, 0.0);

        Ok(Self {
            audio,
            events,
            canonical_frames,
            canonical_tail: [0.0; VAD_FRAME_SAMPLES],
            gate: StreamingSpeechGate::new(pre_roll_samples)?,
            rolling: RollingInference::new(rolling_config, consensus_config, backend)
                .map_err(StreamingPipelineError::Rolling)?,
            maximum_outputs_per_chunk,
        })
    }

    /// Returns the commit capacity required for one hardware chunk.
    #[must_use]
    pub const fn maximum_outputs_per_chunk(&self) -> usize {
        self.maximum_outputs_per_chunk
    }

    /// Returns the one-slot capacity required for a forced final boundary.
    #[must_use]
    pub const fn maximum_outputs_per_finalize(&self) -> usize {
        1
    }

    /// Returns the exact interleaved hardware sample count accepted per call.
    #[must_use]
    pub const fn input_samples_per_chunk(&self) -> usize {
        self.audio.input_samples_per_chunk()
    }

    /// Returns canonical samples retained in pre-roll and rolling inference.
    #[must_use]
    pub fn retained_samples(&self) -> usize {
        self.gate
            .retained_samples()
            .saturating_add(self.rolling.retained_samples())
    }

    /// Borrows the bounded uncommitted display hypothesis.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        self.rolling.pending_text()
    }

    /// Processes one exact native chunk and routes active canonical frames.
    ///
    /// `outputs` must have the complete spare capacity reported by
    /// [`Self::maximum_outputs_per_chunk`]. Any processing failure rolls back
    /// commits appended by this chunk and erases all volatile audio/text state.
    ///
    /// # Errors
    ///
    /// Returns fixed capacity, audio, state, cancellation, or rolling failures.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingPipelineReport, StreamingPipelineError> {
        if cancellation.is_cancelled() {
            self.discard_pending();
            return Err(StreamingPipelineError::Rolling(
                RollingInferenceError::Cancelled,
            ));
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_chunk {
            return Err(StreamingPipelineError::OutputCapacityTooSmall);
        }

        let initial_outputs = outputs.len();
        let audio = match self.audio.process_interleaved_with_frames(
            input,
            &mut self.events,
            &mut self.canonical_frames,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.discard_pending();
                return Err(StreamingPipelineError::Audio(error));
            }
        };
        let result = self.consume_events(audio.events_written, cancellation, outputs);
        self.canonical_frames.fill(0.0);
        match result {
            Ok(activity) => Ok(StreamingPipelineReport {
                audio,
                inferences_run: activity.inferences_run,
                inference_samples: activity.inference_samples,
                inference_elapsed: activity.inference_elapsed,
                commits_written: outputs.len().saturating_sub(initial_outputs),
                hypothesis_updated: activity.inferences_run > 0,
                retained_samples: self.retained_samples(),
            }),
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.discard_pending();
                Err(error)
            }
        }
    }

    /// Includes the unpadded canonical tail, performs final inference, and
    /// erases the segment at a user-driven boundary.
    ///
    /// # Errors
    ///
    /// Rejects non-user reasons, capacity/cancellation, invalid audio state, or
    /// a rolling inference/consensus failure.
    pub fn finalize(
        &mut self,
        reason: FinalizeReason,
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingFinalizeReport, StreamingPipelineError> {
        if !matches!(
            reason,
            FinalizeReason::HotkeyReleased | FinalizeReason::ExplicitStop
        ) {
            return Err(StreamingPipelineError::InvalidBoundary);
        }
        if cancellation.is_cancelled() {
            self.discard_pending();
            return Err(StreamingPipelineError::Rolling(
                RollingInferenceError::Cancelled,
            ));
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_finalize() {
            return Err(StreamingPipelineError::OutputCapacityTooSmall);
        }

        let report = match self.audio.finalize_active(reason, &mut self.canonical_tail) {
            Ok(report) => report,
            Err(error) => {
                self.discard_pending();
                return Err(StreamingPipelineError::Audio(error));
            }
        };
        if !matches!(report.event, SegmentEvent::Finalized(_)) {
            self.canonical_tail.fill(0.0);
            self.gate.clear();
            let _ = self.rolling.handle_discontinuity();
            return Ok(StreamingFinalizeReport {
                inference_ran: false,
                inference_samples: 0,
                inference_elapsed: Duration::ZERO,
                commits_written: 0,
                tail_samples_received: 0,
            });
        }
        if !self.gate.active {
            self.discard_pending();
            return Err(StreamingPipelineError::StateViolation);
        }

        let initial_outputs = outputs.len();
        let tail_samples = report.canonical_samples_written;
        let result = self.rolling.finalize_with_tail(
            &self.canonical_tail[..tail_samples],
            cancellation,
            outputs,
        );
        self.canonical_tail.fill(0.0);
        match result {
            Ok(rolling) => {
                self.gate.clear();
                Ok(StreamingFinalizeReport {
                    inference_ran: rolling.inference_ran,
                    inference_samples: rolling.inference_samples,
                    inference_elapsed: rolling.inference_elapsed,
                    commits_written: outputs.len().saturating_sub(initial_outputs),
                    tail_samples_received: rolling.samples_received,
                })
            }
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.discard_pending();
                Err(StreamingPipelineError::Rolling(error))
            }
        }
    }

    /// Erases state spanning an unknown audio gap and advances generation.
    pub fn handle_discontinuity(&mut self) {
        let _ = self.audio.reset_discontinuity();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        let _ = self.rolling.handle_discontinuity();
    }

    /// Erases all state and restarts the rolling session sample clock.
    pub fn reset_session(&mut self) {
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        let _ = self.rolling.reset_session();
    }

    /// Cancels local ASR and erases every retained canonical/text buffer.
    pub fn cancel_pending(&mut self, cancellation: &CancellationToken) {
        cancellation.cancel();
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        let _ = self.rolling.cancel(cancellation);
    }

    fn consume_events(
        &mut self,
        count: usize,
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingActivity, StreamingPipelineError> {
        let mut activity = StreamingActivity::default();
        let events = &self.events;
        let canonical_frames = &self.canonical_frames;
        let gate = &mut self.gate;
        let rolling = &mut self.rolling;
        for (index, event) in events.iter().copied().take(count).enumerate() {
            if cancellation.is_cancelled() {
                return Err(StreamingPipelineError::Rolling(
                    RollingInferenceError::Cancelled,
                ));
            }
            let frame_start = index
                .checked_mul(VAD_FRAME_SAMPLES)
                .ok_or(StreamingPipelineError::StateViolation)?;
            let frame_end = frame_start
                .checked_add(VAD_FRAME_SAMPLES)
                .ok_or(StreamingPipelineError::StateViolation)?;
            let frame = &canonical_frames[frame_start..frame_end];
            Self::consume_event(
                gate,
                rolling,
                event,
                frame,
                cancellation,
                outputs,
                &mut activity,
            )?;
        }
        Ok(activity)
    }

    fn consume_event(
        gate: &mut StreamingSpeechGate,
        rolling: &mut RollingInference<B>,
        event: SegmentEvent,
        frame: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<ConsensusCommit>,
        activity: &mut StreamingActivity,
    ) -> Result<(), StreamingPipelineError> {
        if frame.len() != VAD_FRAME_SAMPLES {
            return Err(StreamingPipelineError::StateViolation);
        }
        if !gate.active {
            return match event {
                SegmentEvent::Idle => gate.push_pre_roll(frame),
                SegmentEvent::SpeechStarted => {
                    gate.push_pre_roll(frame)?;
                    gate.active = true;
                    let report = rolling
                        .push(&gate.pre_roll, cancellation, outputs)
                        .map_err(StreamingPipelineError::Rolling)?;
                    activity.add(report);
                    gate.clear_pre_roll();
                    Ok(())
                }
                SegmentEvent::SpeechContinued
                | SegmentEvent::SpeechResumed
                | SegmentEvent::ShortPause
                | SegmentEvent::Finalized(_) => Err(StreamingPipelineError::StateViolation),
            };
        }

        match event {
            SegmentEvent::SpeechContinued
            | SegmentEvent::SpeechResumed
            | SegmentEvent::ShortPause => {
                let report = rolling
                    .push(frame, cancellation, outputs)
                    .map_err(StreamingPipelineError::Rolling)?;
                activity.add(report);
                Ok(())
            }
            SegmentEvent::Finalized(_) => {
                let partial = rolling
                    .push(frame, cancellation, outputs)
                    .map_err(StreamingPipelineError::Rolling)?;
                activity.add(partial);
                let final_report = rolling
                    .finalize(cancellation, outputs)
                    .map_err(StreamingPipelineError::Rolling)?;
                activity.add(final_report);
                gate.clear();
                Ok(())
            }
            SegmentEvent::Idle | SegmentEvent::SpeechStarted => {
                Err(StreamingPipelineError::StateViolation)
            }
        }
    }

    fn discard_pending(&mut self) {
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        let _ = self.rolling.handle_discontinuity();
    }
}

impl<B> StreamingDictationPipeline<B>
where
    B: LanguageConfigurableBackend,
    B::Transcript: TranscriptHypothesis,
{
    /// Returns the language policy of the current ready backend generation.
    #[must_use]
    pub fn language_mode(&self) -> LanguageMode {
        self.rolling.backend().language_mode()
    }

    /// Returns the consensus generation that owns current volatile state.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.rolling.generation()
    }

    /// Erases every volatile audio/text buffer before starting a clean backend
    /// generation under a changed language policy.
    ///
    /// Reapplying the current policy is a no-op and preserves the generation.
    ///
    /// # Errors
    ///
    /// Returns a payload-free worker configuration failure after volatile state
    /// has already been erased.
    pub fn set_language_mode(
        &mut self,
        language_mode: LanguageMode,
    ) -> Result<LanguageChangeReport, StreamingPipelineError> {
        if self.language_mode() == language_mode {
            return Ok(LanguageChangeReport {
                changed: false,
                generation: self.generation(),
            });
        }

        self.reset_session();
        let changed = self
            .rolling
            .backend_mut()
            .set_language_mode(language_mode)
            .map_err(StreamingPipelineError::LanguageConfiguration)?;
        Ok(LanguageChangeReport {
            changed,
            generation: self.generation(),
        })
    }
}

impl<B> Drop for StreamingDictationPipeline<B>
where
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    fn drop(&mut self) {
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
    }
}

#[derive(Default)]
struct StreamingActivity {
    inferences_run: usize,
    inference_samples: u64,
    inference_elapsed: Duration,
}

impl StreamingActivity {
    fn add(&mut self, report: RollingInferenceReport) {
        self.inferences_run = self
            .inferences_run
            .saturating_add(usize::from(report.inference_ran));
        self.inference_samples = self
            .inference_samples
            .saturating_add(u64::try_from(report.inference_samples).unwrap_or(u64::MAX));
        self.inference_elapsed = self
            .inference_elapsed
            .saturating_add(report.inference_elapsed);
    }
}

pub(crate) struct StreamingSpeechGate {
    pub(crate) pre_roll: Vec<f32>,
    pre_roll_limit: usize,
    pub(crate) active: bool,
}

impl StreamingSpeechGate {
    pub(crate) fn new(pre_roll_limit: usize) -> Result<Self, StreamingPipelineError> {
        if pre_roll_limit == 0 || !pre_roll_limit.is_multiple_of(VAD_FRAME_SAMPLES) {
            return Err(StreamingPipelineError::InvalidConfig);
        }
        let mut pre_roll = Vec::new();
        pre_roll
            .try_reserve_exact(pre_roll_limit)
            .map_err(|_| StreamingPipelineError::AllocationFailed)?;
        Ok(Self {
            pre_roll,
            pre_roll_limit,
            active: false,
        })
    }

    pub(crate) const fn retained_samples(&self) -> usize {
        self.pre_roll.len()
    }

    pub(crate) fn push_pre_roll(&mut self, frame: &[f32]) -> Result<(), StreamingPipelineError> {
        if frame.len() != VAD_FRAME_SAMPLES || self.active {
            return Err(StreamingPipelineError::StateViolation);
        }
        let required = self
            .pre_roll
            .len()
            .checked_add(frame.len())
            .ok_or(StreamingPipelineError::StateViolation)?;
        if required > self.pre_roll_limit {
            let remove = required - self.pre_roll_limit;
            self.pre_roll.copy_within(remove.., 0);
            let retained = self.pre_roll.len().saturating_sub(remove);
            self.pre_roll[retained..].fill(0.0);
            self.pre_roll.truncate(retained);
        }
        self.pre_roll.extend_from_slice(frame);
        Ok(())
    }

    pub(crate) fn clear_pre_roll(&mut self) {
        self.pre_roll.fill(0.0);
        self.pre_roll.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.clear_pre_roll();
        self.active = false;
    }
}

impl Drop for StreamingSpeechGate {
    fn drop(&mut self) {
        self.pre_roll.fill(0.0);
    }
}

/// Payload-free streaming pipeline failures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamingPipelineError {
    /// VAD, frame, or rolling limits are incompatible.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// Caller commit storage cannot cover one bounded operation.
    OutputCapacityTooSmall,
    /// A forced boundary was not a user stop/release event.
    InvalidBoundary,
    /// Local DSP/VAD processing failed.
    Audio(AudioProcessingError),
    /// VAD events violated the expected speech state machine.
    StateViolation,
    /// Rolling local ASR or consensus failed.
    Rolling(RollingInferenceError),
    /// A clean backend language generation could not be established.
    LanguageConfiguration(WorkerError),
}

impl fmt::Display for StreamingPipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid streaming pipeline configuration",
            Self::AllocationFailed => "streaming pipeline allocation failed",
            Self::OutputCapacityTooSmall => "streaming output capacity is too small",
            Self::InvalidBoundary => "invalid streaming finalization boundary",
            Self::Audio(_) => "streaming audio processing failed",
            Self::StateViolation => "streaming speech state was invalid",
            Self::Rolling(_) => "rolling local ASR failed",
            Self::LanguageConfiguration(_) => "streaming language configuration failed",
        };
        formatter.write_str(message)
    }
}

impl Error for StreamingPipelineError {}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use flowdictate_asr_ipc::{Language, LanguageMode, WorkerError};

    use super::*;
    use crate::HypothesisSegment;

    struct Hypothesis {
        text: Vec<u8>,
        start_ms: u32,
        end_ms: u32,
    }

    impl Hypothesis {
        fn new(text: &str, start_ms: u32, end_ms: u32) -> Self {
            Self {
                text: text.as_bytes().to_vec(),
                start_ms,
                end_ms,
            }
        }
    }

    impl TranscriptHypothesis for Hypothesis {
        fn segment_count(&self) -> usize {
            usize::from(!self.text.is_empty())
        }

        fn segment(&self, index: usize) -> Option<HypothesisSegment<'_>> {
            if index != 0 || self.text.is_empty() {
                return None;
            }
            Some(HypothesisSegment {
                text: std::str::from_utf8(&self.text).ok()?,
                start_ms: self.start_ms,
                end_ms: self.end_ms,
            })
        }
    }

    impl Drop for Hypothesis {
        fn drop(&mut self) {
            self.text.fill(0);
        }
    }

    struct Backend {
        responses: VecDeque<Result<Hypothesis, WorkerError>>,
        windows: Vec<usize>,
        language_mode: LanguageMode,
        language_changes: usize,
    }

    impl TranscriptionBackend for Backend {
        type Transcript = Hypothesis;

        fn transcribe(
            &mut self,
            samples: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Self::Transcript, WorkerError> {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            self.windows.push(samples.len());
            self.responses
                .pop_front()
                .unwrap_or_else(|| Ok(Hypothesis::new("", 0, 0)))
        }
    }

    impl LanguageConfigurableBackend for Backend {
        fn language_mode(&self) -> LanguageMode {
            self.language_mode
        }

        fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError> {
            if self.language_mode == language_mode {
                return Ok(false);
            }
            self.language_mode = language_mode;
            self.language_changes = self.language_changes.saturating_add(1);
            Ok(true)
        }
    }

    fn pipeline(
        responses: impl IntoIterator<Item = Result<Hypothesis, WorkerError>>,
    ) -> Result<StreamingDictationPipeline<Backend>, StreamingPipelineError> {
        let format =
            AudioFormat::new(16_000, 1).map_err(|_| StreamingPipelineError::InvalidConfig)?;
        let vad = VadConfig::new(0.0, 0.0, 1, 2, 20)
            .map_err(|_| StreamingPipelineError::InvalidConfig)?;
        let rolling = RollingInferenceConfig::new(
            VAD_FRAME_SAMPLES * 2,
            VAD_FRAME_SAMPLES,
            VAD_FRAME_SAMPLES * 20,
        )
        .map_err(StreamingPipelineError::Rolling)?;
        let consensus = ConsensusConfig::new(2, 0, 0).map_err(|error| {
            StreamingPipelineError::Rolling(RollingInferenceError::Consensus(error))
        })?;
        StreamingDictationPipeline::new(
            format,
            VAD_FRAME_SAMPLES,
            vad,
            rolling,
            consensus,
            Backend {
                responses: responses.into_iter().collect(),
                windows: Vec::new(),
                language_mode: LanguageMode::Automatic,
                language_changes: 0,
            },
        )
    }

    #[test]
    fn active_frames_feed_rolling_consensus_and_emit_one_commit(
    ) -> Result<(), StreamingPipelineError> {
        let mut pipeline = pipeline([
            Ok(Hypothesis::new("hello", 0, 16)),
            Ok(Hypothesis::new("hello", 0, 16)),
        ])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::with_capacity(8);
        let mut activity = StreamingActivity::default();
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechStarted,
            &[0.2; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechContinued,
            &[0.3; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechContinued,
            &[0.4; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;

        assert_eq!(activity.inferences_run, 2);
        assert_eq!(activity.inference_samples, 1_280);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].text(), "hello");
        assert_eq!(pipeline.rolling.backend_for_test().windows, [512, 768]);
        assert!(pipeline.gate.pre_roll.is_empty());
        Ok(())
    }

    #[test]
    fn idle_pre_roll_is_bounded_and_never_inferred() -> Result<(), StreamingPipelineError> {
        let mut pipeline = pipeline([])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::with_capacity(8);
        let mut activity = StreamingActivity::default();
        for value in [0.1, 0.2, 0.3] {
            StreamingDictationPipeline::consume_event(
                &mut pipeline.gate,
                &mut pipeline.rolling,
                SegmentEvent::Idle,
                &[value; VAD_FRAME_SAMPLES],
                &cancellation,
                &mut outputs,
                &mut activity,
            )?;
        }
        assert_eq!(pipeline.gate.pre_roll.len(), VAD_FRAME_SAMPLES);
        assert!((pipeline.gate.pre_roll[0] - 0.3).abs() < f32::EPSILON);
        assert!(pipeline.rolling.backend_for_test().windows.is_empty());
        Ok(())
    }

    #[test]
    fn discontinuity_and_cancel_erase_pcm_and_pending_text() -> Result<(), StreamingPipelineError> {
        let mut pipeline = pipeline([Ok(Hypothesis::new("pending", 0, 16))])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::with_capacity(8);
        let mut activity = StreamingActivity::default();
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechStarted,
            &[0.2; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechContinued,
            &[0.3; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        assert_eq!(pipeline.pending_text(), "pending");
        pipeline.handle_discontinuity();
        assert_eq!(pipeline.pending_text(), "");
        assert_eq!(pipeline.retained_samples(), 0);

        pipeline.gate.push_pre_roll(&[0.4; VAD_FRAME_SAMPLES])?;
        pipeline.cancel_pending(&cancellation);
        assert!(cancellation.is_cancelled());
        assert_eq!(pipeline.retained_samples(), 0);
        Ok(())
    }

    #[test]
    fn language_change_erases_pending_state_and_advances_generation_once(
    ) -> Result<(), StreamingPipelineError> {
        let mut pipeline = pipeline([Ok(Hypothesis::new("pending", 0, 16))])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::with_capacity(8);
        let mut activity = StreamingActivity::default();
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechStarted,
            &[0.2; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        StreamingDictationPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.rolling,
            SegmentEvent::SpeechContinued,
            &[0.3; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        assert_eq!(pipeline.pending_text(), "pending");
        let previous_generation = pipeline.generation();

        let changed = pipeline.set_language_mode(LanguageMode::Fixed(Language::Hindi))?;
        assert!(changed.changed);
        assert_eq!(changed.generation, previous_generation + 1);
        assert_eq!(pipeline.pending_text(), "");
        assert_eq!(pipeline.retained_samples(), 0);
        assert_eq!(
            pipeline.language_mode(),
            LanguageMode::Fixed(Language::Hindi)
        );
        assert_eq!(pipeline.rolling.backend_for_test().language_changes, 1);

        let unchanged = pipeline.set_language_mode(LanguageMode::Fixed(Language::Hindi))?;
        assert!(!unchanged.changed);
        assert_eq!(unchanged.generation, changed.generation);
        assert_eq!(pipeline.rolling.backend_for_test().language_changes, 1);
        Ok(())
    }

    #[test]
    fn output_capacity_failure_does_not_consume_native_input() -> Result<(), StreamingPipelineError>
    {
        let mut pipeline = pipeline([])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::new();
        assert_eq!(
            pipeline.process_interleaved(&[0.0; VAD_FRAME_SAMPLES], &cancellation, &mut outputs),
            Err(StreamingPipelineError::OutputCapacityTooSmall)
        );
        assert_eq!(pipeline.retained_samples(), 0);
        Ok(())
    }
}
