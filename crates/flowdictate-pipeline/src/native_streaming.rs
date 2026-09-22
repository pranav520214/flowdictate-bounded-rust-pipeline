//! Direct cache-aware streaming inference without overlapping PCM windows.

use std::{error::Error, fmt};

use flowdictate_asr_ipc::{CancellationToken, WorkerError};
use flowdictate_nemotron_ipc::{
    NemotronTranscript, NemotronWorker, NEMOTRON_CHUNK_SAMPLES, NEMOTRON_MAX_STREAM_SAMPLES,
};

/// Bounded transcript behavior required by the direct streaming owner.
pub trait NativeStreamingTranscript {
    /// Borrows the validated bounded display text.
    fn text(&self) -> &str;

    /// Returns whether this result closes the current utterance.
    fn is_final(&self) -> bool;
}

impl NativeStreamingTranscript for NemotronTranscript {
    fn text(&self) -> &str {
        NemotronTranscript::text(self)
    }

    fn is_final(&self) -> bool {
        NemotronTranscript::is_final(self)
    }
}

/// Narrow process-backed cache-aware streaming capability.
pub trait NativeStreamingBackend {
    /// Validated, bounded transcript result.
    type Transcript: NativeStreamingTranscript;

    /// Pushes one non-empty chunk of canonical PCM.
    ///
    /// # Errors
    ///
    /// Returns a payload-free worker error if validation, cancellation, IPC, or
    /// native inference fails.
    fn push_native(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Option<Self::Transcript>, WorkerError>;

    /// Flushes the unpadded tail and resets the native stream.
    ///
    /// # Errors
    ///
    /// Returns a payload-free worker error if cancellation, IPC, or native
    /// finalization fails.
    fn finish_native(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self::Transcript>, WorkerError>;

    /// Erases native streaming state and establishes a clean generation.
    ///
    /// # Errors
    ///
    /// Returns a payload-free worker error if a clean worker generation cannot
    /// be established.
    fn reset_native(&mut self) -> Result<(), WorkerError>;
}

impl NativeStreamingBackend for NemotronWorker {
    type Transcript = NemotronTranscript;

    fn push_native(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Option<Self::Transcript>, WorkerError> {
        self.push(samples, cancellation)
    }

    fn finish_native(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self::Transcript>, WorkerError> {
        self.finish(cancellation)
    }

    fn reset_native(&mut self) -> Result<(), WorkerError> {
        self.reset()
    }
}

/// Payload-free direct-streaming update metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeStreamingReport {
    /// Whether this push invoked native decoding.
    pub inference_ran: bool,
    /// Whether a new partial result replaced the prior borrowed view.
    pub hypothesis_updated: bool,
    /// Canonical samples still waiting for the next 160 ms push.
    pub buffered_samples: usize,
    /// Canonical samples accepted in this utterance.
    pub stream_samples: usize,
}

/// Failures from canonical chunking, native execution, or finalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeStreamingError {
    /// Input was empty, excessive, non-finite, or outside normalized range.
    InvalidAudio,
    /// A bounded allocation could not be reserved.
    AllocationFailed,
    /// The process-backed native backend failed.
    Backend(WorkerError),
    /// Finishing did not produce a final native hypothesis.
    MissingFinal,
}

impl fmt::Display for NativeStreamingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAudio => "native streaming audio is invalid",
            Self::AllocationFailed => "native streaming allocation failed",
            Self::Backend(_) => "native streaming backend failed",
            Self::MissingFinal => "native streaming backend omitted its final result",
        })
    }
}

impl Error for NativeStreamingError {}

/// Owner that compacts canonical PCM into persistent 160 ms native streaming state.
pub struct NativeStreamingInference<B>
where
    B: NativeStreamingBackend,
{
    backend: B,
    chunk: Vec<f32>,
    latest: Option<B::Transcript>,
    stream_samples: usize,
}

impl<B> NativeStreamingInference<B>
where
    B: NativeStreamingBackend,
{
    /// Preallocates the complete native chunk without retaining transcript history.
    ///
    /// # Errors
    ///
    /// Returns a bounded allocation failure before accepting audio.
    pub fn new(backend: B) -> Result<Self, NativeStreamingError> {
        let mut chunk = Vec::new();
        chunk
            .try_reserve_exact(NEMOTRON_CHUNK_SAMPLES)
            .map_err(|_| NativeStreamingError::AllocationFailed)?;
        Ok(Self {
            backend,
            chunk,
            latest: None,
            stream_samples: 0,
        })
    }

    /// Borrows only the latest bounded native interim result.
    #[must_use]
    pub fn pending(&self) -> Option<&B::Transcript> {
        self.latest.as_ref()
    }

    /// Returns canonical samples waiting for the next complete native push.
    #[must_use]
    pub const fn buffered_samples(&self) -> usize {
        self.chunk.len()
    }

    /// Accepts at most one native chunk and runs no more than one inference.
    ///
    /// # Errors
    ///
    /// Invalid/cancelled/backend-failed input erases buffered and native state.
    pub fn push(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<NativeStreamingReport, NativeStreamingError> {
        if samples.is_empty()
            || samples.len() > NEMOTRON_CHUNK_SAMPLES
            || samples
                .iter()
                .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
            || self.stream_samples.saturating_add(samples.len()) > NEMOTRON_MAX_STREAM_SAMPLES
        {
            self.reset_after_failure();
            return Err(NativeStreamingError::InvalidAudio);
        }
        if cancellation.is_cancelled() {
            self.reset_after_failure();
            return Err(NativeStreamingError::Backend(WorkerError::Cancelled));
        }
        let needed = NEMOTRON_CHUNK_SAMPLES - self.chunk.len();
        if samples.len() > needed {
            self.reset_after_failure();
            return Err(NativeStreamingError::InvalidAudio);
        }
        self.chunk.extend_from_slice(samples);
        self.stream_samples += samples.len();
        let mut inference_ran = false;
        let mut hypothesis_updated = false;
        if self.chunk.len() == NEMOTRON_CHUNK_SAMPLES {
            inference_ran = true;
            let response = self
                .backend
                .push_native(&self.chunk, cancellation)
                .map_err(|error| self.fail_backend(error))?;
            self.chunk.fill(0.0);
            self.chunk.clear();
            if let Some(transcript) = response {
                self.latest = Some(transcript);
                hypothesis_updated = true;
            }
        }
        Ok(NativeStreamingReport {
            inference_ran,
            hypothesis_updated,
            buffered_samples: self.chunk.len(),
            stream_samples: self.stream_samples,
        })
    }

    /// Flushes an unpadded tail and requires one final native result.
    ///
    /// # Errors
    ///
    /// Cancellation/backend failure or a missing/non-final result resets closed.
    pub fn finish(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<B::Transcript, NativeStreamingError> {
        if cancellation.is_cancelled() {
            self.reset_after_failure();
            return Err(NativeStreamingError::Backend(WorkerError::Cancelled));
        }
        if !self.chunk.is_empty() {
            let response = self
                .backend
                .push_native(&self.chunk, cancellation)
                .map_err(|error| self.fail_backend(error))?;
            self.chunk.fill(0.0);
            self.chunk.clear();
            if let Some(transcript) = response {
                self.latest = Some(transcript);
            }
        }
        let final_result = self
            .backend
            .finish_native(cancellation)
            .map_err(|error| self.fail_backend(error))?;
        self.stream_samples = 0;
        self.latest = None;
        match final_result {
            Some(transcript) if transcript.is_final() => Ok(transcript),
            _ => {
                self.reset_after_failure();
                Err(NativeStreamingError::MissingFinal)
            }
        }
    }

    /// Explicitly erases PCM/partial state and creates a clean worker generation.
    ///
    /// # Errors
    ///
    /// Returns the backend's payload-free recovery failure.
    pub fn reset(&mut self) -> Result<(), NativeStreamingError> {
        self.clear_local();
        self.backend
            .reset_native()
            .map_err(NativeStreamingError::Backend)
    }

    fn fail_backend(&mut self, error: WorkerError) -> NativeStreamingError {
        self.reset_after_failure();
        NativeStreamingError::Backend(error)
    }

    fn reset_after_failure(&mut self) {
        self.clear_local();
        let _ = self.backend.reset_native();
    }

    fn clear_local(&mut self) {
        self.chunk.fill(0.0);
        self.chunk.clear();
        self.latest = None;
        self.stream_samples = 0;
    }
}

impl<B> Drop for NativeStreamingInference<B>
where
    B: NativeStreamingBackend,
{
    fn drop(&mut self) {
        self.clear_local();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Transcript(bool);

    impl NativeStreamingTranscript for Transcript {
        fn text(&self) -> &'static str {
            "test"
        }

        fn is_final(&self) -> bool {
            self.0
        }
    }

    struct Backend {
        pushes: usize,
        resets: usize,
        responses: VecDeque<Option<Transcript>>,
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

    fn backend(responses: impl IntoIterator<Item = Option<Transcript>>) -> Backend {
        Backend {
            pushes: 0,
            resets: 0,
            responses: responses.into_iter().collect(),
        }
    }

    #[test]
    fn canonical_frames_compact_into_one_native_chunk_and_final_tail() -> Result<(), Box<dyn Error>>
    {
        let mut inference = NativeStreamingInference::new(backend([
            Some(Transcript(false)),
            None,
            Some(Transcript(true)),
        ]))?;
        let cancellation = CancellationToken::new();
        for index in 0..10 {
            let report = inference.push(&[0.0; 256], &cancellation)?;
            assert_eq!(report.inference_ran, index == 9);
        }
        assert!(inference.pending().is_some());
        inference.push(&[0.0; 128], &cancellation)?;
        let final_result = inference.finish(&cancellation)?;
        assert!(final_result.is_final());
        Ok(())
    }

    #[test]
    fn cancellation_and_invalid_audio_reset_closed() -> Result<(), Box<dyn Error>> {
        let mut inference =
            NativeStreamingInference::new(backend(std::iter::empty::<Option<Transcript>>()))?;
        let cancellation = CancellationToken::new();
        inference.push(&[0.0; 256], &cancellation)?;
        cancellation.cancel();
        assert_eq!(
            inference.push(&[0.0; 256], &cancellation),
            Err(NativeStreamingError::Backend(WorkerError::Cancelled))
        );
        assert_eq!(inference.chunk.len(), 0);
        assert_eq!(inference.stream_samples, 0);
        assert_eq!(
            inference.push(&[f32::NAN], &CancellationToken::new()),
            Err(NativeStreamingError::InvalidAudio)
        );
        Ok(())
    }
}
