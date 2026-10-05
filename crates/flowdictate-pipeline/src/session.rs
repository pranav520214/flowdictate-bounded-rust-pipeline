//! Bounded live-session ownership over capture, ring draining, and final output.

use std::{error::Error, fmt};

use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{AudioConsumer, CaptureStream, FinalizeReason, PlatformCaptureError};

use crate::{DictationPipeline, PipelineError, PipelineOutput, TranscriptionBackend};

/// Minimal microphone lifecycle capability required by a live session.
///
/// Implementations must begin paused. A session is the sole owner of this
/// capability and never resumes it except through [`LiveSession::start`].
pub trait CaptureControl {
    /// Resumes local microphone callbacks.
    ///
    /// # Errors
    ///
    /// Returns a payload-free local capture failure.
    fn resume_capture(&self) -> Result<(), PlatformCaptureError>;

    /// Pauses local microphone callbacks.
    ///
    /// # Errors
    ///
    /// Returns a payload-free local capture failure.
    fn pause_capture(&self) -> Result<(), PlatformCaptureError>;
}

impl CaptureControl for CaptureStream {
    fn resume_capture(&self) -> Result<(), PlatformCaptureError> {
        self.resume()
    }

    fn pause_capture(&self) -> Result<(), PlatformCaptureError> {
        self.pause()
    }
}

/// Observable, payload-free live-session state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    /// Capture is paused and no audio is retained by the session or pipeline.
    Idle,
    /// An explicit user interaction has started local listening.
    Listening,
    /// A capture lifecycle failure requires the session owner to be rebuilt.
    Faulted,
}

/// Non-sensitive result of one bounded scheduler drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionDrainReport {
    /// Native interleaved samples removed from the ring.
    pub samples_read: usize,
    /// Whether one exact hardware chunk was processed.
    pub chunk_processed: bool,
    /// Whether this read observed and discarded a discontinuous epoch.
    pub discontinuity_observed: bool,
    /// Final transcripts appended by the processed chunk.
    pub transcripts_written: usize,
    /// Canonical samples still held by the volatile VAD pipeline.
    pub retained_canonical_samples: usize,
}

/// Non-sensitive result of hotkey release or explicit stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopReport {
    /// Native interleaved samples drained after capture paused.
    pub samples_read: usize,
    /// Exact hardware chunks processed during the bounded final drain.
    pub chunks_processed: usize,
    /// Discontinuity epochs observed during the final drain.
    pub discontinuities_observed: usize,
    /// Incomplete native samples discarded rather than padded or retained.
    pub partial_samples_discarded: usize,
    /// Final transcripts appended while draining and forcing the boundary.
    pub transcripts_written: usize,
}

/// Non-sensitive result of cancelling a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelReport {
    /// Native interleaved samples erased from scratch and the bounded ring.
    pub samples_discarded: usize,
}

/// Owns one paused/playing capture, its bounded consumer, and volatile pipeline.
///
/// `LiveSession` does not open a microphone, persist audio/transcripts, log
/// payloads, or perform network work. Construction receives an already-built,
/// paused capture. The caller must invoke [`Self::start`] only after the product
/// has obtained the required explicit user consent/listening interaction.
pub struct LiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
{
    capture: Option<C>,
    consumer: AudioConsumer,
    pipeline: DictationPipeline<B>,
    native_chunk: Vec<f32>,
    native_fill: usize,
    discontinuity_epoch: u64,
    cancellation: CancellationToken,
    state: SessionState,
}

impl<C, B> LiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
{
    /// Preallocates a session around an already-paused local capture.
    ///
    /// # Errors
    ///
    /// Rejects an invalid fixed chunk shape or allocation failure.
    pub fn new(
        capture: C,
        consumer: AudioConsumer,
        pipeline: DictationPipeline<B>,
    ) -> Result<Self, SessionError> {
        let input_samples = pipeline.input_samples_per_chunk();
        if input_samples == 0 {
            return Err(SessionError::InvalidConfig);
        }
        let mut native_chunk = Vec::new();
        native_chunk
            .try_reserve_exact(input_samples)
            .map_err(|_| SessionError::AllocationFailed)?;
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

    /// Returns the transcript capacity needed for one scheduler drain.
    #[must_use]
    pub const fn maximum_outputs_per_drain(&self) -> usize {
        self.pipeline.maximum_outputs_per_chunk()
    }

    /// Returns a conservative transcript capacity for a complete stop drain.
    #[must_use]
    pub fn maximum_outputs_per_stop(&self) -> usize {
        let chunks = self
            .consumer
            .capacity_samples()
            .saturating_add(self.native_chunk.len().saturating_sub(1))
            / self.native_chunk.len();
        chunks
            .saturating_add(1)
            .saturating_mul(self.pipeline.maximum_outputs_per_chunk())
            .saturating_add(1)
    }

    /// Returns a cloneable cancellation handle for UI/event-thread use.
    ///
    /// A fresh token is installed on every successful [`Self::start`].
    #[must_use]
    pub fn cancellation_token(&self) -> Option<CancellationToken> {
        (self.state == SessionState::Listening).then(|| self.cancellation.clone())
    }

    /// Starts capture after an explicit consented listening interaction.
    ///
    /// Stale queued audio is erased before callbacks resume. This method must
    /// never be called from background automation or implicit activation.
    ///
    /// # Errors
    ///
    /// Rejects non-idle state or a capture resume failure.
    pub fn start(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Idle {
            return Err(SessionError::InvalidState);
        }
        self.pipeline.handle_discontinuity();
        self.erase_native_scratch();
        let _ = self.discard_queued_bounded();
        self.discontinuity_epoch = self.consumer.discontinuity_epoch();
        self.cancellation = CancellationToken::new();
        let resume = self
            .capture
            .as_ref()
            .ok_or(SessionError::InvalidState)?
            .resume_capture();
        if let Err(error) = resume {
            self.cancellation.cancel();
            self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(SessionError::Capture(error));
        }
        self.state = SessionState::Listening;
        Ok(())
    }

    /// Drains at most enough native samples to process one exact hardware chunk.
    ///
    /// A changed discontinuity epoch discards the entire read and all pending
    /// VAD/utterance state before any audio from the new epoch can be processed.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state, insufficient caller output capacity, or a
    /// pipeline/capture failure. Processing failures stop the session closed.
    pub fn drain_once(
        &mut self,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<SessionDrainReport, SessionError> {
        if self.state != SessionState::Listening {
            return Err(SessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_drain() {
            return Err(SessionError::Pipeline(
                PipelineError::OutputCapacityTooSmall,
            ));
        }
        self.drain_once_inner(outputs)
    }

    /// Pauses capture and finalizes pending speech as a hotkey release.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or pipeline failure.
    pub fn hotkey_released(
        &mut self,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<StopReport, SessionError> {
        self.finish(FinalizeReason::HotkeyReleased, outputs)
    }

    /// Pauses capture and finalizes pending speech as an explicit stop.
    ///
    /// # Errors
    ///
    /// Returns a fixed state, capacity, capture, or pipeline failure.
    pub fn stop(
        &mut self,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<StopReport, SessionError> {
        self.finish(FinalizeReason::ExplicitStop, outputs)
    }

    /// Cancels ASR, pauses capture, and erases every session-owned/queued sample.
    ///
    /// # Errors
    ///
    /// Rejects non-listening state or reports a capture pause failure after the
    /// volatile audio has still been cancelled and erased.
    pub fn cancel(&mut self) -> Result<CancelReport, SessionError> {
        if self.state != SessionState::Listening {
            return Err(SessionError::InvalidState);
        }
        self.cancellation.cancel();
        self.pipeline.cancel_pending(&self.cancellation);
        let pause = self
            .capture
            .as_ref()
            .ok_or(SessionError::InvalidState)?
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
                return Err(SessionError::Capture(error));
            }
        }
        Ok(CancelReport {
            samples_discarded: scratch.saturating_add(queued),
        })
    }

    fn drain_once_inner(
        &mut self,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<SessionDrainReport, SessionError> {
        if self.cancellation.is_cancelled() {
            return self.fail_pipeline(PipelineError::Cancelled);
        }
        let read = self
            .consumer
            .read(&mut self.native_chunk[self.native_fill..]);
        if read.discontinuity_epoch != self.discontinuity_epoch {
            self.discontinuity_epoch = read.discontinuity_epoch;
            self.pipeline.handle_discontinuity();
            self.erase_native_scratch();
            return Ok(SessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: true,
                transcripts_written: 0,
                retained_canonical_samples: 0,
            });
        }
        self.native_fill = self.native_fill.saturating_add(read.samples_read);
        if self.native_fill != self.native_chunk.len() {
            return Ok(SessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: false,
                discontinuity_observed: false,
                transcripts_written: 0,
                retained_canonical_samples: self.pipeline.retained_samples(),
            });
        }

        let result =
            self.pipeline
                .process_interleaved(&self.native_chunk, &self.cancellation, outputs);
        self.erase_native_scratch();
        match result {
            Ok(report) => Ok(SessionDrainReport {
                samples_read: read.samples_read,
                chunk_processed: true,
                discontinuity_observed: false,
                transcripts_written: report.transcripts_written,
                retained_canonical_samples: report.retained_samples,
            }),
            Err(error) => self.fail_pipeline(error),
        }
    }

    fn finish(
        &mut self,
        reason: FinalizeReason,
        outputs: &mut Vec<PipelineOutput<B::Transcript>>,
    ) -> Result<StopReport, SessionError> {
        if self.state != SessionState::Listening {
            return Err(SessionError::InvalidState);
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_stop() {
            return Err(SessionError::Pipeline(
                PipelineError::OutputCapacityTooSmall,
            ));
        }
        let pause = self
            .capture
            .as_ref()
            .ok_or(SessionError::InvalidState)?
            .pause_capture();
        if let Err(error) = pause {
            self.cancellation.cancel();
            self.pipeline.cancel_pending(&self.cancellation);
            self.capture.take();
            self.erase_native_scratch();
            let _ = self.discard_queued_bounded();
            self.state = SessionState::Faulted;
            return Err(SessionError::Capture(error));
        }

        let initial_outputs = outputs.len();
        let mut samples_read = 0usize;
        let mut chunks_processed = 0usize;
        let mut discontinuities_observed = 0usize;
        let mut budget = self.consumer.capacity_samples();
        while budget > 0 {
            let report = self.drain_once_inner(outputs)?;
            samples_read = samples_read.saturating_add(report.samples_read);
            budget = budget.saturating_sub(report.samples_read);
            chunks_processed = chunks_processed.saturating_add(usize::from(report.chunk_processed));
            discontinuities_observed =
                discontinuities_observed.saturating_add(usize::from(report.discontinuity_observed));
            if report.samples_read == 0 {
                break;
            }
        }
        let partial_samples_discarded = self.native_fill;
        self.erase_native_scratch();
        if let Err(error) = self.pipeline.finalize(reason, &self.cancellation, outputs) {
            return self.fail_pipeline(error);
        }
        self.cancellation.cancel();
        self.state = SessionState::Idle;
        Ok(StopReport {
            samples_read,
            chunks_processed,
            discontinuities_observed,
            partial_samples_discarded,
            transcripts_written: outputs.len().saturating_sub(initial_outputs),
        })
    }

    fn fail_pipeline<T>(&mut self, error: PipelineError) -> Result<T, SessionError> {
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
        Err(SessionError::Pipeline(error))
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

impl<C, B> Drop for LiveSession<C, B>
where
    C: CaptureControl,
    B: TranscriptionBackend,
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

/// Payload-free live-session failures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionError {
    /// The capture/pipeline fixed shapes were invalid.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// The requested transition is invalid for the current state.
    InvalidState,
    /// The local microphone lifecycle failed.
    Capture(PlatformCaptureError),
    /// The volatile processing/ASR pipeline failed.
    Pipeline(PipelineError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid live session configuration",
            Self::AllocationFailed => "live session allocation failed",
            Self::InvalidState => "invalid live session state",
            Self::Capture(_) => "live capture lifecycle failed",
            Self::Pipeline(_) => "live dictation pipeline failed",
        };
        formatter.write_str(message)
    }
}

impl Error for SessionError {}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    use flowdictate_asr_ipc::WorkerError;
    use flowdictate_audio::{
        bounded_audio_ring, AudioFormat, CaptureWrite, SegmentEvent, VadConfig, VAD_FRAME_SAMPLES,
    };

    use super::*;

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

    #[derive(Clone, Default)]
    struct FakeBackend {
        calls: Arc<AtomicUsize>,
    }

    impl TranscriptionBackend for FakeBackend {
        type Transcript = usize;

        fn transcribe(
            &mut self,
            samples: &[f32],
            cancellation: &CancellationToken,
        ) -> Result<Self::Transcript, WorkerError> {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(samples.len())
        }
    }

    type TestSession = LiveSession<FakeCapture, FakeBackend>;

    fn test_session(
        ring_capacity: usize,
    ) -> Result<(flowdictate_audio::CaptureProducer, TestSession), SessionError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| SessionError::InvalidConfig)?;
        let vad = VadConfig::new(0.0, 0.0, 1, 2, 20).map_err(|_| SessionError::InvalidConfig)?;
        let (producer, consumer) =
            bounded_audio_ring(ring_capacity).map_err(|_| SessionError::InvalidConfig)?;
        let pipeline =
            DictationPipeline::new(format, VAD_FRAME_SAMPLES, vad, FakeBackend::default())
                .map_err(SessionError::Pipeline)?;
        let session = LiveSession::new(FakeCapture::default(), consumer, pipeline)?;
        Ok((producer, session))
    }

    #[test]
    fn start_erases_preexisting_audio_and_installs_fresh_token() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES * 2)?;
        assert!(matches!(
            producer.try_push_f32(&[0.5; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        session.start()?;
        let first = session
            .cancellation_token()
            .ok_or(SessionError::InvalidState)?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        assert_eq!(session.drain_once(&mut outputs)?.samples_read, 0);
        session.cancel()?;
        assert!(first.is_cancelled());
        session.start()?;
        let second = session
            .cancellation_token()
            .ok_or(SessionError::InvalidState)?;
        assert!(!second.is_cancelled());
        assert_eq!(session.state(), SessionState::Listening);
        Ok(())
    }

    #[test]
    fn drain_processes_at_most_one_exact_chunk() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES * 3)?;
        session.start()?;
        assert!(matches!(
            producer.try_push_f32(&[0.0; VAD_FRAME_SAMPLES * 2]),
            CaptureWrite::Written { .. }
        ));
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        let first = session.drain_once(&mut outputs)?;
        assert_eq!(first.samples_read, VAD_FRAME_SAMPLES);
        assert!(first.chunk_processed);
        let second = session.drain_once(&mut outputs)?;
        assert_eq!(second.samples_read, VAD_FRAME_SAMPLES);
        assert!(second.chunk_processed);
        assert!(outputs.is_empty());
        Ok(())
    }

    #[test]
    fn discontinuity_discards_the_affected_read_and_pipeline_state() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES)?;
        session.start()?;
        session
            .pipeline
            .utterance
            .observe(SegmentEvent::Idle, &[0.2; VAD_FRAME_SAMPLES])
            .map_err(SessionError::Pipeline)?;
        assert!(matches!(
            producer.try_push_f32(&[0.1; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        assert!(matches!(
            producer.try_push_f32(&[0.2; 1]),
            CaptureWrite::Dropped { .. }
        ));
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        let report = session.drain_once(&mut outputs)?;
        assert!(report.discontinuity_observed);
        assert!(!report.chunk_processed);
        assert_eq!(session.pipeline.retained_samples(), 0);
        assert!(outputs.is_empty());
        Ok(())
    }

    #[test]
    fn hotkey_release_forwards_only_the_final_transcript() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES * 2)?;
        session.start()?;
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_stop());
        for _ in 0..4 {
            assert!(matches!(
                producer.try_push_f32(&[0.0; VAD_FRAME_SAMPLES]),
                CaptureWrite::Written { .. }
            ));
            assert!(session.drain_once(&mut outputs)?.chunk_processed);
            if session.pipeline.retained_samples() > 0 {
                break;
            }
        }
        assert!(session.pipeline.retained_samples() > 0);
        assert!(outputs.is_empty());
        let report = session.hotkey_released(&mut outputs)?;
        assert_eq!(report.transcripts_written, 1);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].reason(), FinalizeReason::HotkeyReleased);
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.pipeline.retained_samples(), 0);
        Ok(())
    }

    #[test]
    fn cancel_erases_partial_and_queued_native_audio() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES * 2)?;
        session.start()?;
        assert!(matches!(
            producer.try_push_f32(&[0.3; VAD_FRAME_SAMPLES / 2]),
            CaptureWrite::Written { .. }
        ));
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        let partial = session.drain_once(&mut outputs)?;
        assert!(!partial.chunk_processed);
        assert!(matches!(
            producer.try_push_f32(&[0.4; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let report = session.cancel()?;
        assert_eq!(
            report.samples_discarded,
            VAD_FRAME_SAMPLES + VAD_FRAME_SAMPLES / 2
        );
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.native_fill, 0);
        assert!(session.native_chunk.iter().all(|sample| *sample == 0.0));
        Ok(())
    }

    #[test]
    fn cancelled_external_token_fails_closed_on_next_drain() -> Result<(), SessionError> {
        let (_producer, mut session) = test_session(VAD_FRAME_SAMPLES * 2)?;
        session.start()?;
        let token = session
            .cancellation_token()
            .ok_or(SessionError::InvalidState)?;
        token.cancel();
        let mut outputs = Vec::with_capacity(session.maximum_outputs_per_drain());
        assert!(matches!(
            session.drain_once(&mut outputs),
            Err(SessionError::Pipeline(PipelineError::Cancelled))
        ));
        assert_eq!(session.state(), SessionState::Idle);
        Ok(())
    }

    #[test]
    fn pause_failure_drops_capture_before_returning_faulted() -> Result<(), SessionError> {
        let (mut producer, mut session) = test_session(VAD_FRAME_SAMPLES * 2)?;
        session.start()?;
        assert!(matches!(
            producer.try_push_f32(&[0.4; VAD_FRAME_SAMPLES]),
            CaptureWrite::Written { .. }
        ));
        let capture = session.capture.as_ref().ok_or(SessionError::InvalidState)?;
        let fail_pause = Arc::clone(&capture.fail_pause);
        let drops = Arc::clone(&capture.drops);
        fail_pause.store(true, Ordering::Relaxed);

        assert!(matches!(
            session.cancel(),
            Err(SessionError::Capture(
                PlatformCaptureError::StreamPauseFailed
            ))
        ));
        assert_eq!(session.state(), SessionState::Faulted);
        assert!(session.capture.is_none());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(session.consumer.read(&mut [0.0; 1]).samples_read, 0);
        Ok(())
    }
}
