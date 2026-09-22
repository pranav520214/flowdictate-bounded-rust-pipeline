//! Consent-gated capture ownership for the experimental Nemotron pipeline.

use std::{error::Error, fmt};

use flowdictate_asr_ipc::{CancellationToken, WorkerError};
use flowdictate_audio::{AudioConsumer, FinalizeReason, PlatformCaptureError};

use crate::{
    CancelReport, CaptureControl, ExperimentalNemotronPipeline, NativePipelineError,
    NativeStreamingBackend, NativeStreamingError, SessionState,
};

/// Non-sensitive result of one bounded experimental scheduler drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSessionDrainReport {
    /// Native interleaved samples removed from the bounded ring.
    pub samples_read: usize,
    /// Whether one exact hardware chunk was processed.
    pub chunk_processed: bool,
    /// Whether an unknown capture gap reset the native stream.
    pub discontinuity_observed: bool,
    /// Complete 160 ms native PCM pushes completed.
    pub inferences_run: usize,
    /// Whether the borrowed latest partial changed.
    pub hypothesis_updated: bool,
    /// Final transcripts transferred to caller-owned storage.
    pub finals_written: usize,
    /// Canonical samples retained outside the native recognizer cache.
    pub retained_canonical_samples: usize,
}

/// Non-sensitive result of an experimental stop or hotkey release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSessionStopReport {
    /// Native interleaved samples removed during the final bounded drain.
    pub samples_read: usize,
    /// Exact hardware chunks processed during that drain.
    pub chunks_processed: usize,
    /// Discontinuity epochs observed during the final drain.
    pub discontinuities_observed: usize,
    /// Incomplete native samples overwritten instead of padded.
    pub partial_samples_discarded: usize,
    /// Complete 160 ms native PCM pushes completed during the final drain.
    pub inferences_run: usize,
    /// Whether an active native utterance was explicitly finalized.
    pub finalized: bool,
    /// Final transcripts transferred to caller-owned storage.
    pub finals_written: usize,
}

/// Explicit opt-in owner for paused capture, bounded ring, DSP/VAD, and Nemotron.
///
/// Construction does not enumerate or open a microphone. Listening begins only
/// when [`Self::start`] is called after the product's explicit consent action.
/// The owner keeps no transcript history: partial text is borrowed and finals
/// move directly into caller-owned storage.
pub struct ExperimentalNemotronLiveSession<C, B>
where
    C: CaptureControl,
    B: NativeStreamingBackend,
{
    capture: Option<C>,
    consumer: AudioConsumer,
    pipeline: ExperimentalNemotronPipeline<B>,
    native_chunk: Vec<f32>,
    native_fill: usize,
    discontinuity_epoch: u64,
    cancellation: CancellationToken,
    state: SessionState,
}

impl<C, B> ExperimentalNemotronLiveSession<C, B>
where
    C: CaptureControl,
    B: NativeStreamingBackend,
{
    /// Preallocates a session around an already-paused, consent-controlled capture.
    ///
    /// # Errors
    ///
    /// Rejects invalid fixed shapes or bounded allocation failure.
    pub fn new(
        capture: C,
        consumer: AudioConsumer,
        pipeline: ExperimentalNemotronPipeline<B>,
    ) -> Result<Self, NativeSessionError> {
        let input_samples = pipeline.input_samples_per_chunk();
        if input_samples == 0 {
            return Err(NativeSessionError::InvalidConfig);
        }
        let mut native_chunk = Vec::new();
        native_chunk
            .try_reserve_exact(input_samples)
            .map_err(|_| NativeSessionError::AllocationFailed)?;
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
        })
    }

    /// Returns the payload-free lifecycle state.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Borrows the latest uncommitted native hypothesis.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        self.pipeline.pending_text()
    }

    /// Returns final-output capacity required by one scheduler drain.
    #[must_use]
    pub const fn maximum_outputs_per_drain(&self) -> usize {
        self.pipeline.maximum_outputs_per_chunk()
    }

    /// Returns conservative output capacity for a complete bounded stop.
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

    /// Erases stale queued data and starts the already-authorized capture.
    ///
    /// # Errors
    ///
    /// Rejects non-idle state or a payload-free capture resume failure.
    pub fn start(&mut self) -> Result<(), NativeSessionError> {
        if self.state != SessionState::Idle {
            return Err(NativeSessionError::InvalidState);
        }
        if let Err(error) = self.pipeline.reset_session() {
            self.erase_native_scratch();
            let _ = self.discard_queued_bounded();
            self.capture.take();
            self.state = SessionState::Faulted;
            return Err(NativeSessionError::Pipeline(error));
        }
        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.discontinuity_epoch = self.consumer.discontinuity_epoch();
        self.cancellation = CancellationToken::new();
        let resume = self
            .capture
            .as_ref()
            .ok_or(NativeSessionError::InvalidState)?
            .resume_capture();
        if let Err(error) = resume {
            self.cancellation.cancel();
            let _ = self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(NativeSessionError::Capture(error));
        }
        self.state = SessionState::Listening;
        Ok(())
    }

    /// Drains at most one exact hardware chunk into the experimental path.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state, insufficient output capacity, or a local
    /// capture/native pipeline failure. Processing failures stop closed.
    pub fn drain_once(
        &mut self,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativeSessionDrainReport, NativeSessionError> {
        if self.state != SessionState::Listening {
            return Err(NativeSessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_drain() {
            return Err(NativeSessionError::Pipeline(
                NativePipelineError::OutputCapacityTooSmall,
            ));
        }
        self.drain_once_inner(outputs)
    }

    /// Pauses capture and finalizes native speech on hotkey release.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or native-pipeline failure.
    pub fn hotkey_released(
        &mut self,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativeSessionStopReport, NativeSessionError> {
        self.finish(FinalizeReason::HotkeyReleased, outputs)
    }

    /// Pauses capture and finalizes native speech on explicit stop.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or native-pipeline failure.
    pub fn stop(
        &mut self,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativeSessionStopReport, NativeSessionError> {
        self.finish(FinalizeReason::ExplicitStop, outputs)
    }

    /// Cancels inference, pauses capture, and overwrites volatile buffers.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state or reports pause failure after erasure.
    pub fn cancel(&mut self) -> Result<CancelReport, NativeSessionError> {
        if self.state != SessionState::Listening {
            return Err(NativeSessionError::InvalidState);
        }
        self.cancellation.cancel();
        let recovery = self.pipeline.cancel_pending(&self.cancellation);
        let pause = self
            .capture
            .as_ref()
            .ok_or(NativeSessionError::InvalidState)?
            .pause_capture();
        if pause.is_err() {
            self.capture.take();
        }
        let scratch = self.native_fill;
        self.erase_native_scratch();
        let queued = self.discard_queued_bounded();
        if let Err(error) = pause {
            self.state = SessionState::Faulted;
            return Err(NativeSessionError::Capture(error));
        }
        if let Err(error) = recovery {
            self.capture.take();
            self.state = SessionState::Faulted;
            return Err(NativeSessionError::Pipeline(error));
        }
        self.state = SessionState::Idle;
        Ok(CancelReport {
            samples_discarded: scratch.saturating_add(queued),
        })
    }

    fn drain_once_inner(
        &mut self,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativeSessionDrainReport, NativeSessionError> {
        if self.cancellation.is_cancelled() {
            return self.fail_pipeline(cancelled_pipeline_error());
        }
        let read = self
            .consumer
            .read(&mut self.native_chunk[self.native_fill..]);
        if read.discontinuity_epoch != self.discontinuity_epoch {
            self.discontinuity_epoch = read.discontinuity_epoch;
            if let Err(error) = self.pipeline.handle_discontinuity() {
                self.erase_native_scratch();
                return self.fail_pipeline(error);
            }
            self.erase_native_scratch();
            return Ok(NativeSessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: true,
                inferences_run: 0,
                hypothesis_updated: false,
                finals_written: 0,
                retained_canonical_samples: 0,
            });
        }
        self.native_fill = self.native_fill.saturating_add(read.samples_read);
        if self.native_fill != self.native_chunk.len() {
            return Ok(NativeSessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: false,
                inferences_run: 0,
                hypothesis_updated: false,
                finals_written: 0,
                retained_canonical_samples: self.pipeline.retained_samples(),
            });
        }
        let result =
            self.pipeline
                .process_interleaved(&self.native_chunk, &self.cancellation, outputs);
        self.erase_native_scratch();
        match result {
            Ok(report) => Ok(NativeSessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: true,
                discontinuity_observed: false,
                inferences_run: report.inferences_run,
                hypothesis_updated: report.hypothesis_updated,
                finals_written: report.finals_written,
                retained_canonical_samples: report.retained_samples,
            }),
            Err(error) => self.fail_pipeline(error),
        }
    }

    fn finish(
        &mut self,
        reason: FinalizeReason,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativeSessionStopReport, NativeSessionError> {
        if self.state != SessionState::Listening {
            return Err(NativeSessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_stop() {
            return Err(NativeSessionError::Pipeline(
                NativePipelineError::OutputCapacityTooSmall,
            ));
        }
        let pause = self
            .capture
            .as_ref()
            .ok_or(NativeSessionError::InvalidState)?
            .pause_capture();
        if let Err(error) = pause {
            self.cancellation.cancel();
            let _ = self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            self.erase_native_scratch();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(NativeSessionError::Capture(error));
        }

        let initial_outputs = outputs.len();
        let mut samples_read = 0usize;
        let mut chunks_processed = 0usize;
        let mut discontinuities_observed = 0usize;
        let mut inferences_run = 0usize;
        let mut budget = self.consumer.capacity_samples();
        while budget > 0 {
            let report = self.drain_once_inner(outputs)?;
            samples_read = samples_read.saturating_add(report.samples_read);
            budget = budget.saturating_sub(report.samples_read);
            chunks_processed = chunks_processed.saturating_add(usize::from(report.chunk_processed));
            discontinuities_observed =
                discontinuities_observed.saturating_add(usize::from(report.discontinuity_observed));
            inferences_run = inferences_run.saturating_add(report.inferences_run);
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
        self.cancellation.cancel();
        self.state = SessionState::Idle;
        Ok(NativeSessionStopReport {
            samples_read,
            chunks_processed,
            discontinuities_observed,
            partial_samples_discarded,
            inferences_run,
            finalized: finalized.finalized,
            finals_written: outputs.len().saturating_sub(initial_outputs),
        })
    }

    fn fail_pipeline<T>(&mut self, error: NativePipelineError) -> Result<T, NativeSessionError> {
        self.cancellation.cancel();
        let recovery = self.pipeline.cancel_pending(&self.cancellation);
        let paused = self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.pause_capture().is_ok());
        if !paused {
            self.capture.take();
        }
        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.state = if paused && recovery.is_ok() {
            SessionState::Idle
        } else {
            SessionState::Faulted
        };
        match recovery {
            Ok(()) => Err(NativeSessionError::Pipeline(error)),
            Err(recovery_error) => Err(NativeSessionError::Pipeline(recovery_error)),
        }
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

impl<C, B> Drop for ExperimentalNemotronLiveSession<C, B>
where
    C: CaptureControl,
    B: NativeStreamingBackend,
{
    fn drop(&mut self) {
        self.cancellation.cancel();
        let _ = self.pipeline.cancel_pending(&self.cancellation);
        self.erase_native_scratch();
        if let Some(capture) = self.capture.take() {
            let _ = capture.pause_capture();
        }
        let _ = self.discard_queued_bounded();
    }
}

fn cancelled_pipeline_error() -> NativePipelineError {
    NativePipelineError::Native(NativeStreamingError::Backend(WorkerError::Cancelled))
}

/// Payload-free failures from experimental capture/session ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSessionError {
    /// Capture and pipeline fixed shapes are incompatible.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// The requested lifecycle transition is invalid.
    InvalidState,
    /// The local capture lifecycle failed.
    Capture(PlatformCaptureError),
    /// Local DSP/VAD/native processing failed.
    Pipeline(NativePipelineError),
}

impl fmt::Display for NativeSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfig => "invalid experimental native session configuration",
            Self::AllocationFailed => "experimental native session allocation failed",
            Self::InvalidState => "invalid experimental native session state",
            Self::Capture(_) => "experimental native capture lifecycle failed",
            Self::Pipeline(_) => "experimental native pipeline failed",
        })
    }
}

impl Error for NativeSessionError {}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };

    use flowdictate_audio::{
        bounded_audio_ring, AudioFormat, CaptureWrite, VadConfig, VAD_FRAME_SAMPLES,
    };

    use super::*;
    use crate::NativeStreamingTranscript;

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

    struct Transcript {
        text: Vec<u8>,
        final_result: bool,
    }

    impl Transcript {
        fn new(text: &str, final_result: bool) -> Self {
            Self {
                text: text.as_bytes().to_vec(),
                final_result,
            }
        }
    }

    impl NativeStreamingTranscript for Transcript {
        fn text(&self) -> &str {
            std::str::from_utf8(&self.text).unwrap_or("")
        }

        fn is_final(&self) -> bool {
            self.final_result
        }
    }

    impl Drop for Transcript {
        fn drop(&mut self) {
            self.text.fill(0);
        }
    }

    struct Backend {
        responses: VecDeque<Option<Transcript>>,
        resets: usize,
        fail_reset: bool,
    }

    impl NativeStreamingBackend for Backend {
        type Transcript = Transcript;

        fn push_native(
            &mut self,
            _: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Option<Self::Transcript>, WorkerError> {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            Ok(self.responses.pop_front().flatten())
        }

        fn finish_native(
            &mut self,
            cancellation: &CancellationToken,
        ) -> Result<Option<Self::Transcript>, WorkerError> {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            Ok(self.responses.pop_front().flatten())
        }

        fn reset_native(&mut self) -> Result<(), WorkerError> {
            self.resets += 1;
            if self.fail_reset {
                Err(WorkerError::RecoveryFailed)
            } else {
                Ok(())
            }
        }
    }

    type TestSession = ExperimentalNemotronLiveSession<FakeCapture, Backend>;

    fn session(
        responses: impl IntoIterator<Item = Option<Transcript>>,
    ) -> Result<(flowdictate_audio::CaptureProducer, TestSession), NativeSessionError> {
        session_with_options(responses, false, VAD_FRAME_SAMPLES * 20)
    }

    fn session_with_reset_policy(
        responses: impl IntoIterator<Item = Option<Transcript>>,
        fail_reset: bool,
    ) -> Result<(flowdictate_audio::CaptureProducer, TestSession), NativeSessionError> {
        session_with_options(responses, fail_reset, VAD_FRAME_SAMPLES * 20)
    }

    fn session_with_options(
        responses: impl IntoIterator<Item = Option<Transcript>>,
        fail_reset: bool,
        ring_capacity: usize,
    ) -> Result<(flowdictate_audio::CaptureProducer, TestSession), NativeSessionError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| NativeSessionError::InvalidConfig)?;
        let vad =
            VadConfig::new(0.0, 0.0, 1, 2, 20).map_err(|_| NativeSessionError::InvalidConfig)?;
        let (producer, consumer) =
            bounded_audio_ring(ring_capacity).map_err(|_| NativeSessionError::InvalidConfig)?;
        let pipeline = ExperimentalNemotronPipeline::new(
            format,
            VAD_FRAME_SAMPLES,
            vad,
            Backend {
                responses: responses.into_iter().collect(),
                resets: 0,
                fail_reset,
            },
        )
        .map_err(NativeSessionError::Pipeline)?;
        let live =
            ExperimentalNemotronLiveSession::new(FakeCapture::default(), consumer, pipeline)?;
        Ok((producer, live))
    }

    fn push_and_drain(
        producer: &mut flowdictate_audio::CaptureProducer,
        session: &mut TestSession,
        outputs: &mut Vec<Transcript>,
    ) -> Result<NativeSessionDrainReport, NativeSessionError> {
        assert!(matches!(
            producer.try_push_f32(&[0.2; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        session.drain_once(outputs)
    }

    fn push_until_partial(
        producer: &mut flowdictate_audio::CaptureProducer,
        session: &mut TestSession,
        outputs: &mut Vec<Transcript>,
    ) -> Result<usize, NativeSessionError> {
        let mut inferences = 0usize;
        for _ in 0..20 {
            let report = push_and_drain(producer, session, outputs)?;
            inferences = inferences.saturating_add(report.inferences_run);
            if report.hypothesis_updated {
                return Ok(inferences);
            }
        }
        Err(NativeSessionError::InvalidState)
    }

    #[test]
    fn explicit_session_start_streams_partial_and_transfers_final() -> Result<(), NativeSessionError>
    {
        let (mut producer, mut session) = session([
            Some(Transcript::new("partial", false)),
            None,
            Some(Transcript::new("final", true)),
        ])?;
        assert_eq!(session.state(), SessionState::Idle);
        session.start()?;
        assert_eq!(session.state(), SessionState::Listening);
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let inference_count = push_until_partial(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(inference_count, 1);
        assert_eq!(session.pending_text(), "partial");
        let stopped = session.stop(&mut outputs)?;
        assert!(stopped.finalized);
        assert_eq!(stopped.finals_written, 1);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].text(), "final");
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn cancellation_erases_partial_and_queued_audio() -> Result<(), NativeSessionError> {
        let (mut producer, mut session) = session([Some(Transcript::new("partial", false))])?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let _ = push_until_partial(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "partial");
        assert!(matches!(
            producer.try_push_f32(&[0.3; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let cancelled = session.cancel()?;
        assert!(cancelled.samples_discarded >= VAD_FRAME_SAMPLES);
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn failed_worker_reset_never_resumes_capture() -> Result<(), NativeSessionError> {
        let (_, mut session) =
            session_with_reset_policy(std::iter::empty::<Option<Transcript>>(), true)?;
        let resumes = Arc::clone(
            &session
                .capture
                .as_ref()
                .ok_or(NativeSessionError::InvalidState)?
                .resumes,
        );
        assert!(matches!(
            session.start(),
            Err(NativeSessionError::Pipeline(NativePipelineError::Native(
                NativeStreamingError::Backend(WorkerError::RecoveryFailed)
            )))
        ));
        assert_eq!(resumes.load(Ordering::Relaxed), 0);
        assert_eq!(session.state(), SessionState::Faulted);
        Ok(())
    }

    #[test]
    fn output_capacity_failure_does_not_consume_ring_audio() -> Result<(), NativeSessionError> {
        let (mut producer, mut session) = session(std::iter::empty::<Option<Transcript>>())?;
        session.start()?;
        assert!(matches!(
            producer.try_push_f32(&[0.2; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let mut no_capacity = Vec::new();
        assert_eq!(
            session.drain_once(&mut no_capacity),
            Err(NativeSessionError::Pipeline(
                NativePipelineError::OutputCapacityTooSmall
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
    fn discontinuity_erases_pending_and_discards_affected_read() -> Result<(), NativeSessionError> {
        let (mut producer, mut session) = session_with_options(
            [Some(Transcript::new("partial", false))],
            false,
            VAD_FRAME_SAMPLES,
        )?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let _ = push_until_partial(&mut producer, &mut session, &mut outputs)?;
        assert_eq!(session.pending_text(), "partial");
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
        assert_eq!(report.retained_canonical_samples, 0);
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn external_cancellation_stops_closed_and_erases_pending() -> Result<(), NativeSessionError> {
        let (mut producer, mut session) = session([Some(Transcript::new("partial", false))])?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let _ = push_until_partial(&mut producer, &mut session, &mut outputs)?;
        let cancellation = session
            .cancellation_token()
            .ok_or(NativeSessionError::InvalidState)?;
        cancellation.cancel();
        assert_eq!(
            session.drain_once(&mut outputs),
            Err(NativeSessionError::Pipeline(cancelled_pipeline_error()))
        );
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pending_text(), "");
        Ok(())
    }

    #[test]
    fn pause_failure_drops_capture_after_volatile_erasure() -> Result<(), NativeSessionError> {
        let (mut producer, mut session) = session([Some(Transcript::new("partial", false))])?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let _ = push_until_partial(&mut producer, &mut session, &mut outputs)?;
        let capture = session
            .capture
            .as_ref()
            .ok_or(NativeSessionError::InvalidState)?;
        let fail_pause = Arc::clone(&capture.fail_pause);
        let drops = Arc::clone(&capture.drops);
        fail_pause.store(true, Ordering::Relaxed);
        assert!(matches!(
            session.cancel(),
            Err(NativeSessionError::Capture(
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
