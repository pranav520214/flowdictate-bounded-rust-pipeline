//! Experimental DSP/VAD routing into persistent native Nemotron streaming.

use std::{error::Error, fmt};

use flowdictate_asr_ipc::CancellationToken;
use flowdictate_audio::{
    AudioFormat, AudioProcessingError, AudioProcessingReport, AudioProcessor, FinalizeReason,
    SegmentEvent, VadConfig, VAD_FRAME_SAMPLES,
};
use flowdictate_nemotron_ipc::NEMOTRON_MAX_STREAM_SAMPLES;

use crate::streaming::StreamingSpeechGate;
use crate::{
    NativeStreamingBackend, NativeStreamingError, NativeStreamingInference,
    NativeStreamingTranscript, StreamingPipelineError,
};

/// Payload-free activity from one hardware chunk on the experimental path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativePipelineReport {
    /// Local DSP/VAD processing result.
    pub audio: AudioProcessingReport,
    /// Complete 160 ms native PCM pushes completed.
    pub inferences_run: usize,
    /// Whether the borrowed latest partial changed.
    pub hypothesis_updated: bool,
    /// Final transcripts transferred into caller-owned storage.
    pub finals_written: usize,
    /// Canonical PCM retained in confirmation pre-roll or the native chunk.
    pub retained_samples: usize,
}

/// Payload-free activity from a user-driven final boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativePipelineFinalizeReport {
    /// Whether a native utterance was finalized.
    pub finalized: bool,
    /// Unpadded canonical tail samples presented to the native owner.
    pub tail_samples_received: usize,
    /// Final transcripts transferred into caller-owned storage.
    pub finals_written: usize,
}

/// VAD-gated owner of the explicitly experimental cache-aware ASR path.
///
/// This path never reconstructs overlapping PCM windows. It passes each new
/// canonical frame exactly once to a persistent native stream and transfers
/// finalized transcripts directly to caller-owned storage.
pub struct ExperimentalNemotronPipeline<B>
where
    B: NativeStreamingBackend,
{
    audio: AudioProcessor,
    events: Vec<SegmentEvent>,
    canonical_frames: Vec<f32>,
    canonical_tail: [f32; VAD_FRAME_SAMPLES],
    gate: StreamingSpeechGate,
    inference: NativeStreamingInference<B>,
    maximum_outputs_per_chunk: usize,
}

impl<B> ExperimentalNemotronPipeline<B>
where
    B: NativeStreamingBackend,
{
    /// Constructs the explicit experimental path around an initialized backend.
    ///
    /// # Errors
    ///
    /// Rejects incompatible VAD limits or bounded allocation failures.
    pub fn new(
        format: AudioFormat,
        input_chunk_frames: usize,
        vad_config: VadConfig,
        backend: B,
    ) -> Result<Self, NativePipelineError> {
        let start_frames = usize::try_from(vad_config.start_frames())
            .map_err(|_| NativePipelineError::InvalidConfig)?;
        let active_frames = usize::try_from(vad_config.maximum_active_frames())
            .map_err(|_| NativePipelineError::InvalidConfig)?;
        let pre_roll_samples = start_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(NativePipelineError::InvalidConfig)?;
        let active_samples = active_frames
            .checked_mul(VAD_FRAME_SAMPLES)
            .ok_or(NativePipelineError::InvalidConfig)?;
        if active_samples == 0 || active_samples > NEMOTRON_MAX_STREAM_SAMPLES {
            return Err(NativePipelineError::InvalidConfig);
        }

        let audio = AudioProcessor::new(format, input_chunk_frames, vad_config)
            .map_err(NativePipelineError::Audio)?;
        let event_slots = audio.maximum_events_per_chunk();
        let canonical_slots = audio.maximum_canonical_samples_per_chunk();
        let mut events = Vec::new();
        events
            .try_reserve_exact(event_slots)
            .map_err(|_| NativePipelineError::AllocationFailed)?;
        events.resize(event_slots, SegmentEvent::Idle);
        let mut canonical_frames = Vec::new();
        canonical_frames
            .try_reserve_exact(canonical_slots)
            .map_err(|_| NativePipelineError::AllocationFailed)?;
        canonical_frames.resize(canonical_slots, 0.0);
        let gate = StreamingSpeechGate::new(pre_roll_samples).map_err(map_gate_error)?;
        let inference =
            NativeStreamingInference::new(backend).map_err(NativePipelineError::Native)?;

        Ok(Self {
            audio,
            events,
            canonical_frames,
            canonical_tail: [0.0; VAD_FRAME_SAMPLES],
            gate,
            inference,
            maximum_outputs_per_chunk: event_slots,
        })
    }

    /// Returns the exact interleaved hardware sample count accepted per call.
    #[must_use]
    pub const fn input_samples_per_chunk(&self) -> usize {
        self.audio.input_samples_per_chunk()
    }

    /// Returns conservative final-output capacity for one hardware chunk.
    #[must_use]
    pub const fn maximum_outputs_per_chunk(&self) -> usize {
        self.maximum_outputs_per_chunk
    }

    /// Returns final-output capacity required by an explicit stop boundary.
    #[must_use]
    pub const fn maximum_outputs_per_finalize(&self) -> usize {
        1
    }

    /// Borrows the latest bounded partial without creating a transcript queue.
    #[must_use]
    pub fn pending_text(&self) -> &str {
        self.inference
            .pending()
            .map_or("", NativeStreamingTranscript::text)
    }

    /// Returns canonical samples retained outside the native recognizer cache.
    #[must_use]
    pub fn retained_samples(&self) -> usize {
        self.gate
            .retained_samples()
            .saturating_add(self.inference.buffered_samples())
    }

    /// Routes one exact hardware chunk through local DSP/VAD and native ASR.
    ///
    /// # Errors
    ///
    /// Fails closed on capacity, audio, VAD state, cancellation, or native
    /// errors and erases all locally retained audio and partial text.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativePipelineReport, NativePipelineError> {
        if cancellation.is_cancelled() {
            self.discard_pending();
            return Err(NativePipelineError::Native(NativeStreamingError::Backend(
                flowdictate_asr_ipc::WorkerError::Cancelled,
            )));
        }
        if outputs.capacity().saturating_sub(outputs.len()) < self.maximum_outputs_per_chunk {
            return Err(NativePipelineError::OutputCapacityTooSmall);
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
                return Err(NativePipelineError::Audio(error));
            }
        };
        let mut activity = NativeActivity::default();
        let mut result = Ok(());
        for (index, event) in self
            .events
            .iter()
            .copied()
            .take(audio.events_written)
            .enumerate()
        {
            let Some(start) = index.checked_mul(VAD_FRAME_SAMPLES) else {
                result = Err(NativePipelineError::StateViolation);
                break;
            };
            let Some(end) = start.checked_add(VAD_FRAME_SAMPLES) else {
                result = Err(NativePipelineError::StateViolation);
                break;
            };
            let Some(frame) = self.canonical_frames.get(start..end) else {
                result = Err(NativePipelineError::StateViolation);
                break;
            };
            if let Err(error) = Self::consume_event(
                &mut self.gate,
                &mut self.inference,
                event,
                frame,
                cancellation,
                outputs,
                &mut activity,
            ) {
                result = Err(error);
                break;
            }
        }
        self.canonical_frames.fill(0.0);
        match result {
            Ok(()) => Ok(NativePipelineReport {
                audio,
                inferences_run: activity.inferences_run,
                hypothesis_updated: activity.hypothesis_updated,
                finals_written: outputs.len().saturating_sub(initial_outputs),
                retained_samples: self.retained_samples(),
            }),
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.discard_pending();
                Err(error)
            }
        }
    }

    /// Flushes an active utterance at a user stop/release boundary.
    ///
    /// # Errors
    ///
    /// Rejects invalid boundaries, missing output capacity, invalid VAD state,
    /// cancellation, or native finalization failures.
    pub fn finalize(
        &mut self,
        reason: FinalizeReason,
        cancellation: &CancellationToken,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<NativePipelineFinalizeReport, NativePipelineError> {
        if !matches!(
            reason,
            FinalizeReason::HotkeyReleased | FinalizeReason::ExplicitStop
        ) {
            return Err(NativePipelineError::InvalidBoundary);
        }
        if outputs.capacity() == outputs.len() {
            return Err(NativePipelineError::OutputCapacityTooSmall);
        }
        let report = match self.audio.finalize_active(reason, &mut self.canonical_tail) {
            Ok(report) => report,
            Err(error) => {
                self.discard_pending();
                return Err(NativePipelineError::Audio(error));
            }
        };
        if !matches!(report.event, SegmentEvent::Finalized(_)) {
            self.canonical_tail.fill(0.0);
            self.gate.clear();
            return Ok(NativePipelineFinalizeReport {
                finalized: false,
                tail_samples_received: 0,
                finals_written: 0,
            });
        }
        if !self.gate.active {
            self.discard_pending();
            return Err(NativePipelineError::StateViolation);
        }
        let initial_outputs = outputs.len();
        let tail = report.canonical_samples_written;
        let result = if tail == 0 {
            self.finish_native(cancellation, outputs)
        } else {
            let pushed = self
                .inference
                .push(&self.canonical_tail[..tail], cancellation)
                .map_err(NativePipelineError::Native);
            pushed.and_then(|_| self.finish_native(cancellation, outputs))
        };
        self.canonical_tail.fill(0.0);
        self.gate.clear();
        match result {
            Ok(()) => Ok(NativePipelineFinalizeReport {
                finalized: true,
                tail_samples_received: tail,
                finals_written: outputs.len().saturating_sub(initial_outputs),
            }),
            Err(error) => {
                outputs.truncate(initial_outputs);
                self.discard_pending();
                Err(error)
            }
        }
    }

    /// Erases state spanning an unknown capture gap.
    ///
    /// # Errors
    ///
    /// Returns a native recovery failure if a clean worker cannot be established.
    pub fn handle_discontinuity(&mut self) -> Result<(), NativePipelineError> {
        let _ = self.audio.reset_discontinuity();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        self.inference.reset().map_err(NativePipelineError::Native)
    }

    /// Erases every volatile buffer before a new listening session.
    ///
    /// # Errors
    ///
    /// Returns a native recovery failure if a clean worker cannot be established.
    pub fn reset_session(&mut self) -> Result<(), NativePipelineError> {
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        self.inference.reset().map_err(NativePipelineError::Native)
    }

    /// Cancels and erases local audio/partial state.
    ///
    /// # Errors
    ///
    /// Returns a native recovery failure after local buffers are already erased.
    pub fn cancel_pending(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<(), NativePipelineError> {
        cancellation.cancel();
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        self.inference.reset().map_err(NativePipelineError::Native)
    }

    fn consume_event(
        gate: &mut StreamingSpeechGate,
        inference: &mut NativeStreamingInference<B>,
        event: SegmentEvent,
        frame: &[f32],
        cancellation: &CancellationToken,
        outputs: &mut Vec<B::Transcript>,
        activity: &mut NativeActivity,
    ) -> Result<(), NativePipelineError> {
        if !gate.active {
            return match event {
                SegmentEvent::Idle => gate.push_pre_roll(frame).map_err(map_gate_error),
                SegmentEvent::SpeechStarted => {
                    gate.push_pre_roll(frame).map_err(map_gate_error)?;
                    gate.active = true;
                    for offset in (0..gate.pre_roll.len()).step_by(VAD_FRAME_SAMPLES) {
                        let report = inference
                            .push(
                                &gate.pre_roll[offset..offset + VAD_FRAME_SAMPLES],
                                cancellation,
                            )
                            .map_err(NativePipelineError::Native)?;
                        activity.add(report);
                    }
                    gate.clear_pre_roll();
                    Ok(())
                }
                _ => Err(NativePipelineError::StateViolation),
            };
        }
        match event {
            SegmentEvent::SpeechContinued
            | SegmentEvent::SpeechResumed
            | SegmentEvent::ShortPause => {
                let report = inference
                    .push(frame, cancellation)
                    .map_err(NativePipelineError::Native)?;
                activity.add(report);
                Ok(())
            }
            SegmentEvent::Finalized(_) => {
                let report = inference
                    .push(frame, cancellation)
                    .map_err(NativePipelineError::Native)?;
                activity.add(report);
                let final_result = inference
                    .finish(cancellation)
                    .map_err(NativePipelineError::Native)?;
                outputs.push(final_result);
                gate.clear();
                Ok(())
            }
            SegmentEvent::Idle | SegmentEvent::SpeechStarted => {
                Err(NativePipelineError::StateViolation)
            }
        }
    }

    fn finish_native(
        &mut self,
        cancellation: &CancellationToken,
        outputs: &mut Vec<B::Transcript>,
    ) -> Result<(), NativePipelineError> {
        let transcript = self
            .inference
            .finish(cancellation)
            .map_err(NativePipelineError::Native)?;
        outputs.push(transcript);
        Ok(())
    }

    fn discard_pending(&mut self) {
        self.audio.cancel_pending();
        self.gate.clear();
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        let _ = self.inference.reset();
    }
}

impl<B> Drop for ExperimentalNemotronPipeline<B>
where
    B: NativeStreamingBackend,
{
    fn drop(&mut self) {
        self.events.fill(SegmentEvent::Idle);
        self.canonical_frames.fill(0.0);
        self.canonical_tail.fill(0.0);
        self.gate.clear();
    }
}

#[derive(Default)]
struct NativeActivity {
    inferences_run: usize,
    hypothesis_updated: bool,
}

impl NativeActivity {
    fn add(&mut self, report: crate::NativeStreamingReport) {
        self.inferences_run = self
            .inferences_run
            .saturating_add(usize::from(report.inference_ran));
        self.hypothesis_updated |= report.hypothesis_updated;
    }
}

fn map_gate_error(error: StreamingPipelineError) -> NativePipelineError {
    match error {
        StreamingPipelineError::InvalidConfig => NativePipelineError::InvalidConfig,
        StreamingPipelineError::AllocationFailed => NativePipelineError::AllocationFailed,
        _ => NativePipelineError::StateViolation,
    }
}

/// Payload-free failures from experimental native DSP/VAD routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePipelineError {
    /// VAD or stream limits are incompatible.
    InvalidConfig,
    /// A bounded construction allocation failed.
    AllocationFailed,
    /// Caller-owned final transcript storage has insufficient spare capacity.
    OutputCapacityTooSmall,
    /// A forced finalization reason was not a user boundary.
    InvalidBoundary,
    /// Local DSP/VAD processing failed.
    Audio(AudioProcessingError),
    /// VAD events violated the expected speech state machine.
    StateViolation,
    /// Native cache-aware inference failed.
    Native(NativeStreamingError),
}

impl fmt::Display for NativePipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfig => "invalid experimental native pipeline configuration",
            Self::AllocationFailed => "experimental native pipeline allocation failed",
            Self::OutputCapacityTooSmall => "native final output capacity is too small",
            Self::InvalidBoundary => "invalid native finalization boundary",
            Self::Audio(_) => "native pipeline audio processing failed",
            Self::StateViolation => "native pipeline speech state was invalid",
            Self::Native(_) => "native streaming inference failed",
        })
    }
}

impl Error for NativePipelineError {}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use flowdictate_asr_ipc::WorkerError;

    use super::*;

    struct Transcript {
        text: &'static str,
        final_result: bool,
    }

    impl NativeStreamingTranscript for Transcript {
        fn text(&self) -> &str {
            self.text
        }

        fn is_final(&self) -> bool {
            self.final_result
        }
    }

    struct Backend {
        responses: VecDeque<Option<Transcript>>,
        pushes: usize,
        resets: usize,
    }

    impl NativeStreamingBackend for Backend {
        type Transcript = Transcript;

        fn push_native(
            &mut self,
            _: &[f32],
            _: &CancellationToken,
        ) -> Result<Option<Self::Transcript>, WorkerError> {
            self.pushes += 1;
            Ok(self.responses.pop_front().flatten())
        }

        fn finish_native(
            &mut self,
            _: &CancellationToken,
        ) -> Result<Option<Self::Transcript>, WorkerError> {
            Ok(self.responses.pop_front().flatten())
        }

        fn reset_native(&mut self) -> Result<(), WorkerError> {
            self.resets += 1;
            Ok(())
        }
    }

    fn pipeline(
        responses: impl IntoIterator<Item = Option<Transcript>>,
    ) -> Result<ExperimentalNemotronPipeline<Backend>, NativePipelineError> {
        let format = AudioFormat::new(16_000, 1).map_err(|_| NativePipelineError::InvalidConfig)?;
        let vad =
            VadConfig::new(0.0, 0.0, 1, 2, 20).map_err(|_| NativePipelineError::InvalidConfig)?;
        ExperimentalNemotronPipeline::new(
            format,
            VAD_FRAME_SAMPLES,
            vad,
            Backend {
                responses: responses.into_iter().collect(),
                pushes: 0,
                resets: 0,
            },
        )
    }

    #[test]
    fn confirmed_frames_feed_only_new_pcm_and_transfer_final() -> Result<(), NativePipelineError> {
        let mut pipeline = pipeline([
            Some(Transcript {
                text: "partial",
                final_result: false,
            }),
            None,
            Some(Transcript {
                text: "final",
                final_result: true,
            }),
        ])?;
        let cancellation = CancellationToken::new();
        let mut outputs = Vec::with_capacity(4);
        let mut activity = NativeActivity::default();
        ExperimentalNemotronPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.inference,
            SegmentEvent::SpeechStarted,
            &[0.2; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        for _ in 0..9 {
            ExperimentalNemotronPipeline::consume_event(
                &mut pipeline.gate,
                &mut pipeline.inference,
                SegmentEvent::SpeechContinued,
                &[0.2; VAD_FRAME_SAMPLES],
                &cancellation,
                &mut outputs,
                &mut activity,
            )?;
        }
        assert_eq!(activity.inferences_run, 1);
        assert_eq!(pipeline.pending_text(), "partial");
        ExperimentalNemotronPipeline::consume_event(
            &mut pipeline.gate,
            &mut pipeline.inference,
            SegmentEvent::Finalized(FinalizeReason::Silence),
            &[0.0; VAD_FRAME_SAMPLES],
            &cancellation,
            &mut outputs,
            &mut activity,
        )?;
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].text(), "final");
        assert_eq!(pipeline.pending_text(), "");
        Ok(())
    }

    #[test]
    fn cancellation_erases_pre_roll_and_native_partial() -> Result<(), NativePipelineError> {
        let mut pipeline = pipeline(std::iter::empty::<Option<Transcript>>())?;
        pipeline
            .gate
            .push_pre_roll(&[0.1; VAD_FRAME_SAMPLES])
            .map_err(map_gate_error)?;
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let mut outputs = Vec::with_capacity(pipeline.maximum_outputs_per_chunk());
        assert!(matches!(
            pipeline.process_interleaved(&[0.0; VAD_FRAME_SAMPLES], &cancellation, &mut outputs),
            Err(NativePipelineError::Native(NativeStreamingError::Backend(
                WorkerError::Cancelled
            )))
        ));
        assert_eq!(pipeline.retained_samples(), 0);
        Ok(())
    }
}
