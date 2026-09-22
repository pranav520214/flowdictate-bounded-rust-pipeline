//! Bounded capture/VAD-to-process-ASR orchestration for `FlowDictate`.
//!
//! This crate owns volatile utterance assembly. It receives exact canonical
//! VAD frames from `flowdictate-audio`, retains only bounded confirmation
//! pre-roll and one active utterance, and dispatches finalized audio directly
//! to the isolated ASR supervisor. It performs no persistence, logging, UI, or
//! network work.

use std::{error::Error, fmt};

use flowdictate_asr_ipc::{
    AsrWorker, CancellationToken, WorkerError, WorkerTranscript, MAX_INFERENCE_SAMPLES,
};
pub use flowdictate_asr_ipc::{Language, LanguageMode};
use flowdictate_audio::{
    AudioFormat, AudioProcessingError, AudioProcessingReport, AudioProcessor, FinalizeReason,
    SegmentEvent, VadConfig, VAD_FRAME_SAMPLES,
};
pub use flowdictate_refine::{
    CleanupConfig, CleanupError, LocalEditor, LocalEditorAttempt, LocalEditorError,
    LocalEditorGate, LocalEditorOutput, OptionalRefinementError, OptionalRefinementOutput,
    OptionalRefinementPath, OptionalRefinementReport, OutputValidationError, RefinementNeed,
    RefinementOutput, RefinementPath, RefinementSignals,
};

mod backend_selection;
mod benchmark;
mod consensus;
mod final_refinement;
mod fixture_benchmark;
mod native_pipeline;
mod native_session;
mod native_streaming;
mod rolling;
mod session;
mod streaming;
mod streaming_session;

pub use backend_selection::{
    acknowledge_experimental_nemotron, BackendSelectionChange, BackendSelectionError,
    ExperimentalNemotronDisclosure, ExperimentalNemotronOptIn, LocalAsrBackendSelection,
    LocalAsrSelectionPolicy, EXPERIMENTAL_NEMOTRON_DISCLOSURE_VERSION,
    EXPERIMENTAL_NEMOTRON_MEASURED_PEAK_BYTES, EXPERIMENTAL_NEMOTRON_MODEL_BYTES,
};
pub use benchmark::{
    measure_recognition, RecognitionBenchmarkConfig, RecognitionBenchmarkError,
    RecognitionBenchmarkSummary, RecognitionTextPolicy, StreamingBenchmarkConfig,
    StreamingBenchmarkError, StreamingBenchmarkRecorder, StreamingBenchmarkSummary,
    MAX_RECOGNITION_CHARACTERS, MAX_RECOGNITION_EDIT_CELLS, MAX_RECOGNITION_TOKENS,
};
pub use consensus::{
    ConsensusCommit, ConsensusCommitter, ConsensusConfig, ConsensusError, ConsensusReport,
    HypothesisSegment, TranscriptHypothesis,
};
pub use final_refinement::{
    refine_native_final, refine_native_final_with_dictionary,
    refine_native_final_with_optional_editor, refine_native_final_with_spoken_rules,
    refine_streaming_final, refine_streaming_final_with_dictionary,
    refine_streaming_final_with_optional_editor, refine_streaming_final_with_spoken_rules,
    FinalRefinementError, FinalTranscript, StreamingFinalBoundary,
};
pub use fixture_benchmark::{
    prepare_verified_benchmark_audio, run_verified_benchmark_fixture,
    run_verified_benchmark_fixture_with_perturbation, BenchmarkTranscript,
    DeterministicNoiseConfig, FixtureAudioPerturbation, FixtureBenchmarkError,
    FixtureBenchmarkSummary, FixtureNoiseConfigError, PreparedBenchmarkAudio,
};
pub use native_pipeline::{
    ExperimentalNemotronPipeline, NativePipelineError, NativePipelineFinalizeReport,
    NativePipelineReport,
};
pub use native_session::{
    ExperimentalNemotronLiveSession, NativeSessionDrainReport, NativeSessionError,
    NativeSessionStopReport,
};
pub use native_streaming::{
    NativeStreamingBackend, NativeStreamingError, NativeStreamingInference, NativeStreamingReport,
    NativeStreamingTranscript,
};
pub use rolling::{
    RollingInference, RollingInferenceConfig, RollingInferenceError, RollingInferenceReport,
    RollingResetReport,
};
pub use session::{
    CancelReport, CaptureControl, LiveSession, SessionDrainReport, SessionError, SessionState,
    StopReport,
};
pub use streaming::{
    LanguageChangeReport, StreamingDictationPipeline, StreamingFinalizeReport,
    StreamingPipelineError, StreamingPipelineReport,
};
pub use streaming_session::{
    StreamingLiveSession, StreamingSessionDrainReport, StreamingSessionError, StreamingStopReport,
};

/// Narrow synchronous transcription capability used by the coordinator.
///
/// Implementations must treat `samples` as sensitive borrowed data and must
/// observe `cancellation` while work is in progress.
pub trait TranscriptionBackend {
    /// Validated transcript type returned by the backend.
    type Transcript;

    /// Transcribes one bounded canonical utterance.
    ///
    /// # Errors
    ///
    /// Returns a fixed worker failure category without formatting sensitive
    /// inputs or outputs.
    fn transcribe(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Self::Transcript, WorkerError>;
}

/// Local backend capability for session-stable automatic/fixed language
/// selection.
pub trait LanguageConfigurableBackend: TranscriptionBackend {
    /// Returns the policy used by the current ready backend generation.
    fn language_mode(&self) -> LanguageMode;

    /// Starts a clean backend generation under `language_mode`.
    ///
    /// Returns `false` without restarting when the policy is unchanged.
    ///
    /// # Errors
    ///
    /// Returns a payload-free worker category when replacement or rollback
    /// cannot establish the requested ready generation.
    fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError>;
}

impl TranscriptionBackend for AsrWorker {
    type Transcript = WorkerTranscript;

    fn transcribe(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Self::Transcript, WorkerError> {
        self.transcribe_with_cancel(samples, cancellation)
    }
}

impl LanguageConfigurableBackend for AsrWorker {
    fn language_mode(&self) -> LanguageMode {
        AsrWorker::language_mode(self)
    }

    fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError> {
        AsrWorker::set_language_mode(self, language_mode)
    }
}

/// One finalized transcript and its non-sensitive boundary metadata.
pub struct PipelineOutput<T> {
    transcript: T,
    reason: FinalizeReason,
    canonical_samples: usize,
}

impl<T> PipelineOutput<T> {
    /// Returns why the utterance ended.
    #[must_use]
    pub const fn reason(&self) -> FinalizeReason {
        self.reason
    }

    /// Returns the exact canonical sample count sent to ASR.
    #[must_use]
    pub const fn canonical_samples(&self) -> usize {
        self.canonical_samples
    }

    /// Borrows the validated backend transcript.
    #[must_use]
    pub const fn transcript(&self) -> &T {
        &self.transcript
    }

    /// Transfers transcript ownership to the caller.
    #[must_use]
    pub fn into_transcript(self) -> T {
        self.transcript
    }
}

/// Non-sensitive counters from one processed hardware chunk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipelineReport {
    /// Underlying DSP/VAD report.
    pub audio: AudioProcessingReport,
    /// Finalized transcripts appended to the caller output.
    pub transcripts_written: usize,
    /// Canonical samples currently retained for confirmation or active speech.
    pub retained_samples: usize,
}

/// Stateful owner of one bounded volatile utterance and one ASR backend.
pub struct DictationPipeline<B: TranscriptionBackend> {
    audio: AudioProcessor,
    backend: B,
    events: Vec<SegmentEvent>,
    canonical_frames: Vec<f32>,
    canonical_tail: [f32; VAD_FRAME_SAMPLES],
    utterance: SensitiveUtterance,
}

impl<B: TranscriptionBackend> DictationPipeline<B> {
    /// Constructs and preallocates the complete worker-side orchestration seam.
    ///
    /// # Errors
    ///
    /// Rejects audio/VAD configurations whose active window exceeds the ASR
    /// limit, or any required bounded allocation/processor construction failure.
    pub fn new(
        format: AudioFormat,
        input_chunk_frames: usize,
        vad_config: VadConfig,
        backend: B,
    ) -> Result<Self, PipelineError> {
        let start_frames =
            usize::try_from(vad_config.start_frames()).map_err(|_| PipelineError::InvalidConfig)?;
        let maximum_active_frames = usize::try_from(vad_config.maximum_active_frames())
            .map_err(|_| PipelineError::InvalidConfig)?;
        let pre_roll_samples = start_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(PipelineError::InvalidConfig)?;
        let maximum_utterance_samples = maximum_active_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(PipelineError::InvalidConfig)?;
        if maximum_utterance_samples == 0
            || maximum_utterance_samples > MAX_INFERENCE_SAMPLES
            || pre_roll_samples > maximum_utterance_samples
        {
            return Err(PipelineError::InvalidConfig);
        }

        let audio = AudioProcessor::new(format, input_chunk_frames, vad_config)
            .map_err(PipelineError::Audio)?;
        let event_slots = audio.maximum_events_per_chunk();
        let canonical_slots = audio.maximum_canonical_samples_per_chunk();
        let mut events = Vec::new();
        events
            .try_reserve_exact(event_slots)
            .map_err(|_| PipelineError::AllocationFailed)?;
        events.resize(event_slots, SegmentEvent::Idle);
        let mut canonical_frames = Vec::new();
        canonical_frames
            .try_reserve_exact(canonical_slots)
            .map_err(|_| PipelineError::AllocationFailed)?;
        canonical_frames.resize(canonical_slots, 0.0);

        Ok(Self {
            audio,
            backend,
            events,
            canonical_frames,
            canonical_tail: [0.0; VAD_FRAME_SAMPLES],
            utterance: SensitiveUtterance::new(pre_roll_samples, maximum_utterance_samples)?,
        })
    }

    /// Returns the output capacity required to guarantee allocation-free
    /// transcript appends for one input chunk.
    #[must_use]
    pub const fn maximum_outputs_per_chunk(&self) -> usize {
        self.events.len()
    }

    /// Returns the exact interleaved hardware sample count accepted per call.
    #[must_use]
    pub const fn input_samples_per_chunk(&self) -> usize {
        self.audio.input_samples_per_chunk()
    }

    /// Returns the number of volatile canonical samples currently retained.
    #[must_use]
    pub const fn retained_samples(&self) -> usize {
        self.utterance.len()
    }

    /// Processes one exact hardware chunk through DSP, VAD, bounded utterance
    /// assembly, and synchronous isolated ASR dispatch.
    ///
    /// `outputs` must have spare capacity for [`Self::maximum_outputs_per_chunk`]
    /// so this method never reallocates a transcript container. If any event in
    /// the chunk fails, outputs appended by that chunk are dropped and cleared by
    /// their transcript owners.
    ///
    /// # Errors
    ///
    /// Returns fixed audio, state, capacity, cancellation, or ASR failures.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<PipelineReport, PipelineError> {
        if cancellation.is_cancelled() {
            self.discard_pending();
            return Err(PipelineError::Cancelled);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_chunk() {
            return Err(PipelineError::OutputCapacityTooSmall);
        }

        let initial_output_len = outputs.len();
        let audio = match self.audio.process_interleaved_with_frames(
            input,
            &mut self.events,
            &mut self.canonical_frames,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.discard_pending();
                return Err(PipelineError::Audio(error));
            }
        };
        let result = self.consume_events(audio.events_written, cancellation, outputs);
        self.canonical_frames.fill(0.0);
        match result {
            Ok(transcripts_written) => Ok(PipelineReport {
                audio,
                transcripts_written,
                retained_samples: self.utterance.len(),
            }),
            Err(error) => {
                outputs.truncate(initial_output_len);
                self.discard_pending();
                Err(error)
            }
        }
    }

    /// Finalizes an active utterance for a user boundary and dispatches it.
    /// The incomplete canonical VAD tail is included without zero padding.
    ///
    /// # Errors
    ///
    /// Rejects non-user boundary reasons, insufficient output capacity,
    /// cancellation, audio state failure, or ASR failure.
    pub fn finalize(
        &mut self,
        reason: FinalizeReason,
        cancellation: &CancellationToken,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<bool, PipelineError> {
        if !matches!(
            reason,
            FinalizeReason::HotkeyReleased | FinalizeReason::ExplicitStop
        ) {
            return Err(PipelineError::InvalidBoundary);
        }
        if cancellation.is_cancelled() {
            self.discard_pending();
            return Err(PipelineError::Cancelled);
        }
        if outputs.capacity() == outputs.len() {
            return Err(PipelineError::OutputCapacityTooSmall);
        }

        let report = match self.audio.finalize_active(reason, &mut self.canonical_tail) {
            Ok(report) => report,
            Err(error) => {
                self.discard_pending();
                return Err(PipelineError::Audio(error));
            }
        };
        if !matches!(report.event, SegmentEvent::Finalized(_)) {
            self.utterance.clear_sensitive();
            self.canonical_tail.fill(0.0);
            return Ok(false);
        }
        let append_result = self
            .utterance
            .append(&self.canonical_tail[..report.canonical_samples_written]);
        self.canonical_tail.fill(0.0);
        if let Err(error) = append_result {
            self.discard_pending();
            return Err(error);
        }
        let result = self.dispatch(reason, cancellation, outputs);
        if result.is_err() {
            self.discard_pending();
        }
        result.map(|()| true)
    }

    /// Discards an utterance spanning dropped or discontinuous audio.
    pub fn handle_discontinuity(&mut self) {
        let _ = self.audio.reset_discontinuity();
        self.discard_buffers();
    }

    /// Cancels the supplied session/request and erases all pending audio.
    /// A concurrent in-flight ASR call observes the same token and replaces its
    /// native worker before returning.
    pub fn cancel_pending(&mut self, cancellation: &CancellationToken) {
        cancellation.cancel();
        self.discard_pending();
    }

    fn consume_events(
        &mut self,
        count: usize,
        cancellation: &CancellationToken,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<usize, PipelineError> {
        let mut written = 0;
        for index in 0..count {
            if cancellation.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let frame_start = index
                .checked_mul(VAD_FRAME_SAMPLES)
                .ok_or(PipelineError::StateViolation)?;
            let frame_end = frame_start
                .checked_add(VAD_FRAME_SAMPLES)
                .ok_or(PipelineError::StateViolation)?;
            let event = self.events[index];
            let finalized = self
                .utterance
                .observe(event, &self.canonical_frames[frame_start..frame_end])?;
            if let Some(reason) = finalized {
                self.dispatch(reason, cancellation, outputs)?;
                written += 1;
            }
        }
        Ok(written)
    }

    fn dispatch(
        &mut self,
        reason: FinalizeReason,
        cancellation: &CancellationToken,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<(), PipelineError> {
        let canonical_samples = self.utterance.len();
        if canonical_samples == 0 {
            return Err(PipelineError::StateViolation);
        }
        let result = self
            .backend
            .transcribe(self.utterance.samples(), cancellation);
        self.utterance.clear_sensitive();
        match result {
            Ok(transcript) => {
                outputs.push(PipelineOutput {
                    transcript,
                    reason,
                    canonical_samples,
                });
                Ok(())
            }
            Err(WorkerError::Cancelled) => Err(PipelineError::Cancelled),
            Err(error) => Err(PipelineError::Asr(error)),
        }
    }

    fn discard_pending(&mut self) {
        self.audio.cancel_pending();
        self.discard_buffers();
    }

    fn discard_buffers(&mut self) {
        self.utterance.clear_sensitive();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
    }
}

impl<B: TranscriptionBackend> Drop for DictationPipeline<B> {
    fn drop(&mut self) {
        self.discard_buffers();
    }
}

struct SensitiveUtterance {
    samples: Vec<f32>,
    pre_roll_samples: usize,
    hard_limit: usize,
    active: bool,
}

impl SensitiveUtterance {
    fn new(pre_roll_samples: usize, hard_limit: usize) -> Result<Self, PipelineError> {
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(hard_limit)
            .map_err(|_| PipelineError::AllocationFailed)?;
        Ok(Self {
            samples,
            pre_roll_samples,
            hard_limit,
            active: false,
        })
    }

    const fn len(&self) -> usize {
        self.samples.len()
    }

    fn samples(&self) -> &[f32] {
        &self.samples
    }

    fn observe(
        &mut self,
        event: SegmentEvent,
        frame: &[f32],
    ) -> Result<Option<FinalizeReason>, PipelineError> {
        if frame.len() != VAD_FRAME_SAMPLES {
            return Err(PipelineError::StateViolation);
        }
        if self.active {
            return match event {
                SegmentEvent::SpeechContinued
                | SegmentEvent::SpeechResumed
                | SegmentEvent::ShortPause => {
                    self.append(frame)?;
                    Ok(None)
                }
                SegmentEvent::Finalized(reason) => {
                    self.append(frame)?;
                    self.active = false;
                    Ok(Some(reason))
                }
                SegmentEvent::Idle | SegmentEvent::SpeechStarted => {
                    Err(PipelineError::StateViolation)
                }
            };
        }

        match event {
            SegmentEvent::Idle => {
                self.push_pre_roll(frame)?;
                Ok(None)
            }
            SegmentEvent::SpeechStarted => {
                self.push_pre_roll(frame)?;
                self.active = true;
                Ok(None)
            }
            SegmentEvent::SpeechContinued
            | SegmentEvent::SpeechResumed
            | SegmentEvent::ShortPause
            | SegmentEvent::Finalized(_) => Err(PipelineError::StateViolation),
        }
    }

    fn push_pre_roll(&mut self, frame: &[f32]) -> Result<(), PipelineError> {
        if self.pre_roll_samples < frame.len() {
            return Err(PipelineError::InvalidConfig);
        }
        let required = self
            .samples
            .len()
            .checked_add(frame.len())
            .ok_or(PipelineError::UtteranceTooLong)?;
        if required > self.pre_roll_samples {
            let remove = required - self.pre_roll_samples;
            self.samples.copy_within(remove.., 0);
            let retained = self.samples.len() - remove;
            self.samples[retained..].fill(0.0);
            self.samples.truncate(retained);
        }
        self.append(frame)
    }

    fn append(&mut self, samples: &[f32]) -> Result<(), PipelineError> {
        let required = self
            .samples
            .len()
            .checked_add(samples.len())
            .ok_or(PipelineError::UtteranceTooLong)?;
        if required > self.hard_limit {
            return Err(PipelineError::UtteranceTooLong);
        }
        self.samples.extend_from_slice(samples);
        Ok(())
    }

    fn clear_sensitive(&mut self) {
        self.samples.fill(0.0);
        self.samples.clear();
        self.active = false;
    }
}

impl Drop for SensitiveUtterance {
    fn drop(&mut self) {
        self.samples.fill(0.0);
    }
}

/// Payload-free orchestration failures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PipelineError {
    /// The audio/VAD configuration cannot fit the ASR hard limit.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// Caller output capacity cannot cover one bounded input chunk.
    OutputCapacityTooSmall,
    /// A forced boundary was not a user stop/release event.
    InvalidBoundary,
    /// Audio/VAD processing failed.
    Audio(AudioProcessingError),
    /// VAD events violated the expected utterance state machine.
    StateViolation,
    /// Canonical utterance assembly exceeded the compiled limit.
    UtteranceTooLong,
    /// The caller cancelled the volatile session/request.
    Cancelled,
    /// The isolated ASR supervisor failed.
    Asr(WorkerError),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid dictation pipeline configuration",
            Self::AllocationFailed => "dictation pipeline allocation failed",
            Self::OutputCapacityTooSmall => "dictation output capacity is too small",
            Self::InvalidBoundary => "invalid dictation finalization boundary",
            Self::Audio(_) => "dictation audio processing failed",
            Self::StateViolation => "dictation event state was invalid",
            Self::UtteranceTooLong => "dictation utterance exceeded its hard limit",
            Self::Cancelled => "dictation session was cancelled",
            Self::Asr(_) => "dictation ASR failed",
        };
        formatter.write_str(message)
    }
}

impl Error for PipelineError {}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingBackend {
        calls: usize,
        sample_counts: Vec<usize>,
        fail: Option<WorkerError>,
    }

    impl RecordingBackend {
        fn new() -> Self {
            Self {
                calls: 0,
                sample_counts: Vec::new(),
                fail: None,
            }
        }
    }

    impl TranscriptionBackend for RecordingBackend {
        type Transcript = usize;

        fn transcribe(
            &mut self,
            samples: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Self::Transcript, WorkerError> {
            self.calls += 1;
            self.sample_counts.push(samples.len());
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            if let Some(error) = self.fail {
                return Err(error);
            }
            Ok(samples.len())
        }
    }

    fn frame(value: f32) -> [f32; VAD_FRAME_SAMPLES] {
        [value; VAD_FRAME_SAMPLES]
    }

    #[test]
    fn confirmation_pre_roll_is_bounded_and_dispatched_once() -> Result<(), PipelineError> {
        let mut utterance = SensitiveUtterance::new(VAD_FRAME_SAMPLES * 2, VAD_FRAME_SAMPLES * 5)?;
        utterance.observe(SegmentEvent::Idle, &frame(0.1))?;
        utterance.observe(SegmentEvent::Idle, &frame(0.2))?;
        utterance.observe(SegmentEvent::SpeechStarted, &frame(0.3))?;
        assert_eq!(utterance.len(), VAD_FRAME_SAMPLES * 2);
        assert!((utterance.samples()[0] - 0.2).abs() < f32::EPSILON);
        assert!((utterance.samples()[VAD_FRAME_SAMPLES] - 0.3).abs() < f32::EPSILON);
        utterance.observe(SegmentEvent::SpeechContinued, &frame(0.4))?;
        let finalized = utterance.observe(
            SegmentEvent::Finalized(FinalizeReason::Silence),
            &frame(0.0),
        )?;
        assert_eq!(finalized, Some(FinalizeReason::Silence));
        assert_eq!(utterance.len(), VAD_FRAME_SAMPLES * 4);
        Ok(())
    }

    #[test]
    fn cancellation_discards_pre_roll_before_audio_processing() -> Result<(), PipelineError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| PipelineError::InvalidConfig)?;
        let vad = VadConfig::new(0.8, 0.5, 2, 2, 20).map_err(|_| PipelineError::InvalidConfig)?;
        let backend = RecordingBackend::new();
        let mut pipeline = DictationPipeline::new(format, VAD_FRAME_SAMPLES, vad, backend)?;
        pipeline
            .utterance
            .observe(SegmentEvent::Idle, &frame(0.2))?;
        let token = CancellationToken::new();
        token.cancel();
        let mut outputs = Vec::new();
        let result = pipeline.process_interleaved(&frame(0.0), &token, &mut outputs);
        assert!(matches!(result, Err(PipelineError::Cancelled)));
        assert_eq!(pipeline.retained_samples(), 0);
        assert!(outputs.is_empty());
        assert_eq!(pipeline.backend.calls, 0);
        Ok(())
    }

    #[test]
    fn audio_failure_erases_existing_pre_roll() -> Result<(), PipelineError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| PipelineError::InvalidConfig)?;
        let vad = VadConfig::new(0.8, 0.5, 2, 2, 20).map_err(|_| PipelineError::InvalidConfig)?;
        let backend = RecordingBackend::new();
        let mut pipeline = DictationPipeline::new(format, VAD_FRAME_SAMPLES, vad, backend)?;
        pipeline
            .utterance
            .observe(SegmentEvent::Idle, &frame(0.2))?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(pipeline.maximum_outputs_per_chunk());

        let result = pipeline.process_interleaved(&[0.0; 255], &token, &mut outputs);

        assert!(matches!(
            result,
            Err(PipelineError::Audio(
                AudioProcessingError::WrongInputFrameCount
            ))
        ));
        assert_eq!(pipeline.retained_samples(), 0);
        assert!(outputs.is_empty());
        Ok(())
    }

    #[test]
    fn finalized_frame_dispatches_and_clears_owned_audio() -> Result<(), PipelineError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| PipelineError::InvalidConfig)?;
        let vad = VadConfig::new(0.8, 0.5, 2, 2, 20).map_err(|_| PipelineError::InvalidConfig)?;
        let backend = RecordingBackend::new();
        let mut pipeline = DictationPipeline::new(format, VAD_FRAME_SAMPLES, vad, backend)?;
        pipeline
            .utterance
            .observe(SegmentEvent::Idle, &frame(0.2))?;
        pipeline
            .utterance
            .observe(SegmentEvent::SpeechStarted, &frame(0.8))?;
        pipeline
            .utterance
            .observe(SegmentEvent::SpeechContinued, &frame(0.7))?;
        let reason = pipeline
            .utterance
            .observe(
                SegmentEvent::Finalized(FinalizeReason::Silence),
                &frame(0.0),
            )?
            .ok_or(PipelineError::StateViolation)?;
        let token = CancellationToken::new();
        let mut outputs = Vec::with_capacity(1);
        pipeline.dispatch(reason, &token, &mut outputs)?;
        assert_eq!(pipeline.backend.calls, 1);
        assert_eq!(pipeline.backend.sample_counts, [VAD_FRAME_SAMPLES * 4]);
        assert_eq!(pipeline.retained_samples(), 0);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].canonical_samples(), VAD_FRAME_SAMPLES * 4);
        Ok(())
    }

    #[test]
    fn state_violation_and_hard_limit_fail_closed() -> Result<(), PipelineError> {
        let mut utterance = SensitiveUtterance::new(VAD_FRAME_SAMPLES, VAD_FRAME_SAMPLES * 2)?;
        assert!(matches!(
            utterance.observe(SegmentEvent::SpeechContinued, &frame(0.5)),
            Err(PipelineError::StateViolation)
        ));
        utterance.observe(SegmentEvent::SpeechStarted, &frame(0.5))?;
        utterance.observe(SegmentEvent::SpeechContinued, &frame(0.5))?;
        assert!(matches!(
            utterance.observe(SegmentEvent::ShortPause, &frame(0.0)),
            Err(PipelineError::UtteranceTooLong)
        ));
        Ok(())
    }

    #[test]
    fn pipeline_rejects_vad_window_larger_than_asr_limit() {
        let Ok(format) = AudioFormat::new(16_000, 1) else {
            return;
        };
        let Ok(vad) = VadConfig::new(0.8, 0.5, 2, 2, 1_876) else {
            return;
        };
        assert!(matches!(
            DictationPipeline::new(format, VAD_FRAME_SAMPLES, vad, RecordingBackend::new()),
            Err(PipelineError::InvalidConfig)
        ));
    }
}
