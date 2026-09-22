//! Consent-gated live-session ownership for rolling ASR and consensus commits.

use std::{error::Error, fmt, time::Duration};

use flowdictate_asr_ipc::{CancellationToken, LanguageMode};
use flowdictate_audio::{AudioConsumer, FinalizeReason, PlatformCaptureError};

use crate::{
    CancelReport, CaptureControl, ConsensusCommit, LanguageChangeReport,
    LanguageConfigurableBackend, SessionState, StreamingDictationPipeline, StreamingFinalBoundary,
    StreamingPipelineError, TranscriptHypothesis, TranscriptionBackend,
};

/// Non-sensitive result of one bounded streaming scheduler drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingSessionDrainReport {
    /// Native interleaved samples removed from the bounded ring.
    pub samples_read: usize,
    /// Whether one exact hardware chunk was processed.
    pub chunk_processed: bool,
    /// Whether a discontinuous read was discarded and the generation reset.
    pub discontinuity_observed: bool,
    /// Local rolling inference calls completed by this chunk.
    pub inferences_run: usize,
    /// Sum of rolling-window canonical samples inferred by this drain.
    pub inference_samples: u64,
    /// Wall time spent inside local transcription calls for this drain.
    pub inference_elapsed: Duration,
    /// Whether the bounded borrowed pending hypothesis changed.
    pub hypothesis_updated: bool,
    /// Immutable consensus commits appended to caller-owned storage.
    pub commits_written: usize,
    /// Canonical samples retained across pre-roll and rolling inference.
    pub retained_canonical_samples: usize,
}

/// Non-sensitive result of hotkey release or explicit streaming stop.
#[derive(Debug, Eq, PartialEq)]
pub struct StreamingStopReport {
    /// Native interleaved samples drained after capture paused.
    pub samples_read: usize,
    /// Exact hardware chunks processed during the bounded final drain.
    pub chunks_processed: usize,
    /// Discontinuity epochs observed during the final drain.
    pub discontinuities_observed: usize,
    /// Incomplete native samples discarded rather than padded or retained.
    pub partial_samples_discarded: usize,
    /// Rolling inference calls completed while draining.
    pub inferences_run: usize,
    /// Sum of rolling-window canonical samples inferred while stopping.
    pub inference_samples: u64,
    /// Wall time spent inside local transcription calls while stopping.
    pub inference_elapsed: Duration,
    /// Whether final local inference ran for an active segment.
    pub final_inference_ran: bool,
    /// Immutable consensus commits appended by drain and finalization.
    pub commits_written: usize,
    final_boundary: StreamingFinalBoundary,
}

impl StreamingStopReport {
    /// Consumes this report and returns its one-shot final refinement boundary.
    #[must_use]
    pub fn into_final_boundary(self) -> StreamingFinalBoundary {
        self.final_boundary
    }
}

/// Owns a paused capture, bounded ring consumer, and streaming pipeline.
///
/// The owner does not enumerate or open a microphone. Capture starts only
/// through [`Self::start`], after an explicit consented listening interaction.
/// Pending text is exposed only as a borrowed view and commits are written into
/// caller-owned preallocated storage; there is no internal event queue.
pub struct StreamingLiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    capture: Option<C>,
    consumer: AudioConsumer,
    pipeline: StreamingDictationPipeline<B>,
    native_chunk: Vec<f32>,
    native_fill: usize,
    discontinuity_epoch: u64,
    cancellation: CancellationToken,
    state: SessionState,
    session_commits_written: usize,
}

impl<C, B> StreamingLiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    /// Preallocates a streaming session around an already-paused capture.
    ///
    /// # Errors
    ///
    /// Rejects an invalid fixed chunk shape or bounded allocation failure.
    pub fn new(
        capture: C,
        consumer: AudioConsumer,
        pipeline: StreamingDictationPipeline<B>,
    ) -> Result<Self, StreamingSessionError> {
        let input_samples = pipeline.input_samples_per_chunk();
        if input_samples == 0 {
            return Err(StreamingSessionError::InvalidConfig);
        }
        let mut native_chunk = Vec::new();
        native_chunk
            .try_reserve_exact(input_samples)
            .map_err(|_| StreamingSessionError::AllocationFailed)?;
        native_chunk.resize(input_samples, 0.0);
        let discontinuity_epoch = consumer.discontinuity_epoch();
        Ok(Self {
            capture: Some(capture),
            consumer,
            pipeline,
            native_chunk,
            native_fill: 0,
            discontinuity_epoch,
            cancellation: CancellationToken::new(),
            state: SessionState::Idle,
            session_commits_written: 0,
        })
    }

    /// Returns the payload-free lifecycle state.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Borrows the current uncommitted local display hypothesis.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        self.pipeline.pending_text()
    }

    /// Returns commit capacity required for one scheduler drain.
    #[must_use]
    pub const fn maximum_outputs_per_drain(&self) -> usize {
        self.pipeline.maximum_outputs_per_chunk()
    }

    /// Returns conservative commit capacity for a complete bounded stop.
    #[must_use]
    pub fn maximum_outputs_per_stop(&self) -> usize {
        let chunks = self
            .consumer
            .capacity_samples()
            .saturating_add(self.native_chunk.len().saturating_sub(1))
            / self.native_chunk.len();
        chunks
            .saturating_mul(self.pipeline.maximum_outputs_per_chunk())
            .saturating_add(self.pipeline.maximum_outputs_per_finalize())
    }

    /// Returns a cloneable one-shot cancellation handle while listening.
    #[must_use]
    pub fn cancellation_token(&self) -> Option<CancellationToken> {
        (self.state == SessionState::Listening).then(|| self.cancellation.clone())
    }

    /// Starts the already-authorized capture after erasing stale queued audio.
    ///
    /// # Errors
    ///
    /// Rejects non-idle state or a local capture resume failure.
    pub fn start(&mut self) -> Result<(), StreamingSessionError> {
        if self.state != SessionState::Idle {
            return Err(StreamingSessionError::InvalidState);
        }
        self.pipeline.reset_session();
        self.session_commits_written = 0;
        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.discontinuity_epoch = self.consumer.discontinuity_epoch();
        self.cancellation = CancellationToken::new();
        let resume = self
            .capture
            .as_ref()
            .ok_or(StreamingSessionError::InvalidState)?
            .resume_capture();
        if let Err(error) = resume {
            self.cancellation.cancel();
            self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(StreamingSessionError::Capture(error));
        }
        self.state = SessionState::Listening;
        Ok(())
    }

    /// Drains at most one exact native chunk into rolling local inference.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state, insufficient commit capacity, or a local
    /// streaming/capture failure. Processing failures stop the session closed.
    pub fn drain_once(
        &mut self,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingSessionDrainReport, StreamingSessionError> {
        if self.state != SessionState::Listening {
            return Err(StreamingSessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_drain() {
            return Err(StreamingSessionError::Pipeline(
                StreamingPipelineError::OutputCapacityTooSmall,
            ));
        }
        self.drain_once_inner(outputs)
    }

    /// Pauses capture and finalizes the pending suffix on hotkey release.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or streaming pipeline failure.
    pub fn hotkey_released(
        &mut self,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingStopReport, StreamingSessionError> {
        self.finish(FinalizeReason::HotkeyReleased, outputs)
    }

    /// Pauses capture and finalizes the pending suffix on explicit stop.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or streaming pipeline failure.
    pub fn stop(
        &mut self,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingStopReport, StreamingSessionError> {
        self.finish(FinalizeReason::ExplicitStop, outputs)
    }

    /// Cancels ASR, pauses capture, and erases session/ring/pipeline samples.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state or reports capture pause failure after
    /// volatile audio and hypothesis state have still been erased.
    pub fn cancel(&mut self) -> Result<CancelReport, StreamingSessionError> {
        if self.state != SessionState::Listening {
            return Err(StreamingSessionError::InvalidState);
        }
        self.cancellation.cancel();
        self.pipeline.cancel_pending(&self.cancellation);
        let pause = self
            .capture
            .as_ref()
            .ok_or(StreamingSessionError::InvalidState)?
            .pause_capture();
        if pause.is_err() {
            self.capture.take();
        }
        let scratch = self.native_fill;
        self.erase_native_scratch();
        let queued = self.discard_queued_bounded();
        match pause {
            Ok(()) => self.state = SessionState::Idle,
            Err(error) => {
                self.state = SessionState::Faulted;
                return Err(StreamingSessionError::Capture(error));
            }
        }
        Ok(CancelReport {
            samples_discarded: scratch.saturating_add(queued),
        })
    }

    fn drain_once_inner(
        &mut self,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingSessionDrainReport, StreamingSessionError> {
        if self.cancellation.is_cancelled() {
            return self.fail_pipeline(StreamingPipelineError::Rolling(
                crate::RollingInferenceError::Cancelled,
            ));
        }
        let read = self
            .consumer
            .read(&mut self.native_chunk[self.native_fill..]);
        if read.discontinuity_epoch != self.discontinuity_epoch {
            self.discontinuity_epoch = read.discontinuity_epoch;
            self.pipeline.handle_discontinuity();
            self.erase_native_scratch();
            return Ok(StreamingSessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: true,
                inferences_run: 0,
                inference_samples: 0,
                inference_elapsed: Duration::ZERO,
                hypothesis_updated: false,
                commits_written: 0,
                retained_canonical_samples: 0,
            });
        }
        self.native_fill = self.native_fill.saturating_add(read.samples_read);
        if self.native_fill != self.native_chunk.len() {
            return Ok(StreamingSessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: false,
                inferences_run: 0,
                inference_samples: 0,
                inference_elapsed: Duration::ZERO,
                hypothesis_updated: false,
                commits_written: 0,
                retained_canonical_samples: self.pipeline.retained_samples(),
            });
        }

        let result =
            self.pipeline
                .process_interleaved(&self.native_chunk, &self.cancellation, outputs);
        self.erase_native_scratch();
        match result {
            Ok(report) => {
                self.session_commits_written = self
                    .session_commits_written
                    .saturating_add(report.commits_written);
                Ok(StreamingSessionDrainReport {
                    samples_read: read.samples_read,
                    chunk_processed: true,
                    discontinuity_observed: false,
                    inferences_run: report.inferences_run,
                    inference_samples: report.inference_samples,
                    inference_elapsed: report.inference_elapsed,
                    hypothesis_updated: report.hypothesis_updated,
                    commits_written: report.commits_written,
                    retained_canonical_samples: report.retained_samples,
                })
            }
            Err(error) => self.fail_pipeline(error),
        }
    }

    fn finish(
        &mut self,
        reason: FinalizeReason,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingStopReport, StreamingSessionError> {
        if self.state != SessionState::Listening {
            return Err(StreamingSessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_stop() {
            return Err(StreamingSessionError::Pipeline(
                StreamingPipelineError::OutputCapacityTooSmall,
            ));
        }
        let pause = self
            .capture
            .as_ref()
            .ok_or(StreamingSessionError::InvalidState)?
            .pause_capture();
        if let Err(error) = pause {
            self.cancellation.cancel();
            self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            self.erase_native_scratch();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(StreamingSessionError::Capture(error));
        }

        let initial_outputs = outputs.len();
        let mut samples_read = 0usize;
        let mut chunks_processed = 0usize;
        let mut discontinuities_observed = 0usize;
        let mut inferences_run = 0usize;
        let mut inference_samples = 0u64;
        let mut inference_elapsed = Duration::ZERO;
        let mut budget = self.consumer.capacity_samples();
        while budget > 0 {
            let report = self.drain_once_inner(outputs)?;
            samples_read = samples_read.saturating_add(report.samples_read);
            budget = budget.saturating_sub(report.samples_read);
            chunks_processed = chunks_processed.saturating_add(usize::from(report.chunk_processed));
            discontinuities_observed =
                discontinuities_observed.saturating_add(usize::from(report.discontinuity_observed));
            inferences_run = inferences_run.saturating_add(report.inferences_run);
            inference_samples = inference_samples.saturating_add(report.inference_samples);
            inference_elapsed = inference_elapsed.saturating_add(report.inference_elapsed);
            if report.samples_read == 0 {
                break;
            }
        }
        let partial_samples_discarded = self.native_fill;
        self.erase_native_scratch();
        let finalized = match self.pipeline.finalize(reason, &self.cancellation, outputs) {
            Ok(report) => report,
            Err(error) => return self.fail_pipeline(error),
        };
        self.session_commits_written = self
            .session_commits_written
            .saturating_add(finalized.commits_written);
        self.cancellation.cancel();
        self.state = SessionState::Idle;
        inference_samples = inference_samples
            .saturating_add(u64::try_from(finalized.inference_samples).unwrap_or(u64::MAX));
        inference_elapsed = inference_elapsed.saturating_add(finalized.inference_elapsed);
        Ok(StreamingStopReport {
            samples_read,
            chunks_processed,
            discontinuities_observed,
            partial_samples_discarded,
            inferences_run,
            inference_samples,
            inference_elapsed,
            final_inference_ran: finalized.inference_ran,
            commits_written: outputs.len().saturating_sub(initial_outputs),
            final_boundary: StreamingFinalBoundary::new(self.session_commits_written),
        })
    }

    fn fail_pipeline<T>(
        &mut self,
        error: StreamingPipelineError,
    ) -> Result<T, StreamingSessionError> {
        self.cancellation.cancel();
        self.pipeline.cancel_pending(&self.cancellation);
        let paused = self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.pause_capture().is_ok());
        if !paused {
            self.capture.take();
        }
        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.state = if paused {
            SessionState::Idle
        } else {
            SessionState::Faulted
        };
        Err(StreamingSessionError::Pipeline(error))
    }

    fn discard_queued_bounded(&mut self) -> usize {
        let mut discarded = 0usize;
        let mut budget = self.consumer.capacity_samples();
        while budget > 0 {
            let request = self.native_chunk.len().min(budget);
            let report = self.consumer.read(&mut self.native_chunk[..request]);
            self.native_chunk[..report.samples_read].fill(0.0);
            discarded = discarded.saturating_add(report.samples_read);
            budget = budget.saturating_sub(report.samples_read);
            if report.samples_read == 0 {
                break;
            }
        }
        self.discontinuity_epoch = self.consumer.discontinuity_epoch();
        discarded
    }

    fn erase_native_scratch(&mut self) {
        self.native_chunk.fill(0.0);
        self.native_fill = 0;
    }
}

impl<C, B> StreamingLiveSession<C, B>
where
    C: CaptureControl,
    B: LanguageConfigurableBackend,
    B::Transcript: TranscriptHypothesis,
{
    /// Returns the language policy of the current ready worker generation.
    #[must_use]
    pub fn language_mode(&self) -> LanguageMode {
        self.pipeline.language_mode()
    }

    /// Returns the consensus generation that owns current volatile state.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.pipeline.generation()
    }

    /// Applies a language policy only while capture is idle.
    ///
    /// Scratch and queued audio are erased before a changed worker generation
    /// is requested. Listening/faulted sessions reject the transition without
    /// mutating their current language or volatile state.
    ///
    /// # Errors
    ///
    /// Returns invalid state while non-idle or a payload-free pipeline worker
    /// failure after the idle buffers have been erased.
    pub fn set_language_mode(
        &mut self,
        language_mode: LanguageMode,
    ) -> Result<LanguageChangeReport, StreamingSessionError> {
        if self.state != SessionState::Idle {
            return Err(StreamingSessionError::InvalidState);
        }
        if self.pipeline.language_mode() == language_mode {
            return self
                .pipeline
                .set_language_mode(language_mode)
                .map_err(StreamingSessionError::Pipeline);
        }

        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.pipeline
            .set_language_mode(language_mode)
            .map_err(StreamingSessionError::Pipeline)
    }
}

impl<C, B> Drop for StreamingLiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
    B::Transcript: TranscriptHypothesis,
{
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.pipeline.cancel_pending(&self.cancellation);
        self.erase_native_scratch();
        if let Some(capture) = self.capture.take() {
            let _ = capture.pause_capture();
        }
        let _ = self.discard_queued_bounded();
    }
}

/// Payload-free streaming live-session failures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamingSessionError {
    /// The capture/streaming-pipeline fixed shape is invalid.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// The requested lifecycle transition is invalid.
    InvalidState,
    /// The local capture lifecycle failed.
    Capture(PlatformCaptureError),
    /// Volatile rolling audio/ASR/consensus processing failed.
    Pipeline(StreamingPipelineError),
}

impl fmt::Display for StreamingSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid streaming live session configuration",
            Self::AllocationFailed => "streaming live session allocation failed",
            Self::InvalidState => "invalid streaming live session state",
            Self::Capture(_) => "streaming live capture lifecycle failed",
            Self::Pipeline(_) => "streaming live pipeline failed",
        };
        formatter.write_str(message)
    }
}

impl Error for StreamingSessionError {}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };

    use flowdictate_asr_ipc::{Language, LanguageMode, WorkerError};
    use flowdictate_audio::{
        bounded_audio_ring, AudioFormat, CaptureWrite, VadConfig, VAD_FRAME_SAMPLES,
    };

    use super::*;
    use crate::{
        refine_streaming_final, CleanupConfig, ConsensusConfig, HypothesisSegment,
        RollingInferenceConfig, RollingInferenceError,
    };

    #[derive(Clone, Default)]
    struct FakeCapture {
        resumes: Arc<AtomicUsize>,
        pauses: Arc<AtomicUsize>,
        fail_pause: Arc<AtomicBool>,
        drops: Arc<AtomicUsize>,
    }

    impl CaptureControl for FakeCapture {
        fn resume_capture(&self) -> Result<(), PlatformCaptureError> {
            self.resumes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn pause_capture(&self) -> Result<(), PlatformCaptureError> {
            self.pauses.fetch_add(1, Ordering::Relaxed);
            if self.fail_pause.load(Ordering::Relaxed) {
                Err(PlatformCaptureError::StreamPauseFailed)
            } else {
                Ok(())
            }
        }
    }

    impl Drop for FakeCapture {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

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
        language_mode: LanguageMode,
    }

    impl TranscriptionBackend for Backend {
        type Transcript = Hypothesis;

        fn transcribe(
            &mut self,
            _samples: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Self::Transcript, WorkerError> {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
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
            Ok(true)
        }
    }

    type TestSession = StreamingLiveSession<FakeCapture, Backend>;

    fn session(
        ring_capacity: usize,
        responses: impl IntoIterator<Item = Result<Hypothesis, WorkerError>>,
    ) -> Result<(flowdictate_audio::CaptureProducer, TestSession), StreamingSessionError> {
        let format =
            AudioFormat::new(16_000, 1).map_err(|_| StreamingSessionError::InvalidConfig)?;
        let vad =
            VadConfig::new(0.0, 0.0, 1, 2, 20).map_err(|_| StreamingSessionError::InvalidConfig)?;
        let rolling = RollingInferenceConfig::new(
            VAD_FRAME_SAMPLES * 2,
            VAD_FRAME_SAMPLES,
            VAD_FRAME_SAMPLES * 20,
        )
        .map_err(|error| StreamingSessionError::Pipeline(StreamingPipelineError::Rolling(error)))?;
        let consensus = ConsensusConfig::new(2, 0, 0).map_err(|error| {
            StreamingSessionError::Pipeline(StreamingPipelineError::Rolling(
                RollingInferenceError::Consensus(error),
            ))
        })?;
        let (producer, consumer) =
            bounded_audio_ring(ring_capacity).map_err(|_| StreamingSessionError::InvalidConfig)?;
        let pipeline = StreamingDictationPipeline::new(
            format,
            VAD_FRAME_SAMPLES,
            vad,
            rolling,
            consensus,
            Backend {
                responses: responses.into_iter().collect(),
                language_mode: LanguageMode::Automatic,
            },
        )
        .map_err(StreamingSessionError::Pipeline)?;
        let live = StreamingLiveSession::new(FakeCapture::default(), consumer, pipeline)?;
        Ok((producer, live))
    }

    fn push_and_drain(
        producer: &mut flowdictate_audio::CaptureProducer,
        session: &mut TestSession,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingSessionDrainReport, StreamingSessionError> {
        if !matches!(
            producer.try_push_f32(&[0.0; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ) {
            return Err(StreamingSessionError::InvalidState);
        }
        session.drain_once(outputs)
    }

    fn push_until_hypothesis(
        producer: &mut flowdictate_audio::CaptureProducer,
        session: &mut TestSession,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingSessionDrainReport, StreamingSessionError> {
        for _ in 0..8 {
            let report = push_and_drain(producer, session, outputs)?;
            if report.hypothesis_updated {
                return Ok(report);
            }
        }
        Err(StreamingSessionError::InvalidState)
    }

    fn push_until_commit(
        producer: &mut flowdictate_audio::CaptureProducer,
        session: &mut TestSession,
        outputs: &mut Vec<ConsensusCommit>,
    ) -> Result<StreamingSessionDrainReport, StreamingSessionError> {
        for _ in 0..8 {
            let report = push_and_drain(producer, session, outputs)?;
            if report.commits_written > 0 {
                return Ok(report);
            }
        }
        Err(StreamingSessionError::InvalidState)
    }

    #[test]
    fn live_frames_publish_borrowed_hypothesis_then_immutable_commit(
    ) -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 4,
            [
                Ok(Hypothesis::new("hello", 0, 16)),
                Ok(Hypothesis::new("hello", 0, 16)),
            ],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let partial = push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        assert!(partial.hypothesis_updated);
        assert_eq!(partial.commits_written, 0);
        assert_eq!(session.pending_text(), "hello");
        let committed = push_until_commit(&mut producer, &mut session, &mut outputs)?;
        assert!(committed.hypothesis_updated);
        assert_eq!(committed.commits_written, 1);
        assert_eq!(outputs[0].text(), "hello");
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn start_erases_stale_audio_and_installs_a_fresh_token() -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(VAD_FRAME_SAMPLES * 2, [])?;
        assert!(matches!(
            producer.try_push_f32(&[0.5; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        session.start()?;
        let first = session
            .cancellation_token()
            .ok_or(StreamingSessionError::InvalidState)?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        assert_eq!(session.drain_once(&mut outputs)?.samples_read, 0);
        session.cancel()?;
        assert!(first.is_cancelled());
        session.start()?;
        let second = session
            .cancellation_token()
            .ok_or(StreamingSessionError::InvalidState)?;
        assert!(!second.is_cancelled());
        assert_eq!(session.state(), SessionState::Listening);
        Ok(())
    }

    #[test]
    fn output_capacity_failure_does_not_consume_ring_audio() -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(VAD_FRAME_SAMPLES * 2, [])?;
        session.start()?;
        assert!(matches!(
            producer.try_push_f32(&[0.2; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let mut no_capacity = Vec::new();
        assert_eq!(
            session.drain_once(&mut no_capacity),
            Err(StreamingSessionError::Pipeline(
                StreamingPipelineError::OutputCapacityTooSmall
            ))
        );
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        assert_eq!(
            session.drain_once(&mut outputs)?.samples_read,
            VAD_FRAME_SAMPLES
        );
        Ok(())
    }

    #[test]
    fn discontinuity_erases_pending_hypothesis_and_discards_read(
    ) -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) =
            session(VAD_FRAME_SAMPLES, [Ok(Hypothesis::new("pending", 0, 16))])?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "pending");
        assert!(matches!(
            producer.try_push_f32(&[0.1; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        assert!(matches!(
            producer.try_push_f32(&[0.2; 1]),
            CaptureWrite::Dropped { .. }
        ));
        let report = session.drain_once(&mut outputs)?;
        assert!(report.discontinuity_observed);
        assert!(!report.chunk_processed);
        assert_eq!(session.pending_text(), "");
        assert_eq!(report.retained_canonical_samples, 0);
        Ok(())
    }

    #[test]
    fn hotkey_release_finalizes_pending_suffix_and_returns_idle() -> Result<(), Box<dyn Error>> {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 3,
            [
                Ok(Hypothesis::new("draft", 0, 16)),
                Ok(Hypothesis::new("final", 0, 32)),
            ],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "draft");
        let report = session.hotkey_released(&mut outputs)?;
        assert!(report.final_inference_ran);
        assert!(report.inference_samples > 0);
        assert_eq!(report.commits_written, 1);
        assert_eq!(outputs[0].text(), "final");
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pending_text(), "");
        let refined = refine_streaming_final(
            report.into_final_boundary(),
            outputs,
            CleanupConfig::default(),
        )?;
        assert_eq!(refined.text(), "Final");
        Ok(())
    }

    #[test]
    fn cancellation_erases_pending_and_queued_audio() -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 3,
            [Ok(Hypothesis::new("pending", 0, 16))],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "pending");
        assert!(matches!(
            producer.try_push_f32(&[0.4; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let report = session.cancel()?;
        assert_eq!(report.samples_discarded, VAD_FRAME_SAMPLES);
        assert_eq!(session.pending_text(), "");
        assert_eq!(session.state(), SessionState::Idle);
        Ok(())
    }

    #[test]
    fn language_change_is_idle_only_and_resets_the_streaming_generation(
    ) -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 3,
            [Ok(Hypothesis::new("pending", 0, 16))],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "pending");
        assert_eq!(
            session.set_language_mode(LanguageMode::Fixed(Language::Hindi)),
            Err(StreamingSessionError::InvalidState)
        );
        assert_eq!(session.language_mode(), LanguageMode::Automatic);
        assert_eq!(session.pending_text(), "pending");

        session.cancel()?;
        let previous_generation = session.generation();
        let changed = session.set_language_mode(LanguageMode::Fixed(Language::Hindi))?;
        assert!(changed.changed);
        assert_eq!(changed.generation, previous_generation + 1);
        assert_eq!(
            session.language_mode(),
            LanguageMode::Fixed(Language::Hindi)
        );
        assert_eq!(session.pending_text(), "");

        let unchanged = session.set_language_mode(LanguageMode::Fixed(Language::Hindi))?;
        assert!(!unchanged.changed);
        assert_eq!(unchanged.generation, changed.generation);

        session.start()?;
        assert_eq!(
            session.language_mode(),
            LanguageMode::Fixed(Language::Hindi)
        );
        let report = push_and_drain(&mut producer, &mut session, &mut outputs)?;
        assert!(report.chunk_processed);
        Ok(())
    }

    #[test]
    fn external_cancellation_stops_closed_and_erases_pending() -> Result<(), StreamingSessionError>
    {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 3,
            [Ok(Hypothesis::new("pending", 0, 16))],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        let cancellation = session
            .cancellation_token()
            .ok_or(StreamingSessionError::InvalidState)?;
        cancellation.cancel();
        assert!(matches!(
            session.drain_once(&mut outputs),
            Err(StreamingSessionError::Pipeline(
                StreamingPipelineError::Rolling(RollingInferenceError::Cancelled)
            ))
        ));
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn pause_failure_drops_capture_after_streaming_state_is_erased(
    ) -> Result<(), StreamingSessionError> {
        let (mut producer, mut session) = session(
            VAD_FRAME_SAMPLES * 3,
            [Ok(Hypothesis::new("pending", 0, 16))],
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        push_until_hypothesis(&mut producer, &mut session, &mut outputs)?;
        let capture = session
            .capture
            .as_ref()
            .ok_or(StreamingSessionError::InvalidState)?;
        let fail_pause = Arc::clone(&capture.fail_pause);
        let drops = Arc::clone(&capture.drops);
        fail_pause.store(true, Ordering::Relaxed);

        assert!(matches!(
            session.cancel(),
            Err(StreamingSessionError::Capture(
                PlatformCaptureError::StreamPauseFailed
            ))
        ));
        assert_eq!(session.state(), SessionState::Faulted);
        assert_eq!(session.pending_text(), "");
        assert!(session.capture.is_none());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        Ok(())
    }
}
