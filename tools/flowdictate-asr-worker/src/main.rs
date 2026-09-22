//! Silent child-process entry point for bounded local ASR inference.

use std::{
    ffi::OsStr,
    io::{self, BufReader, BufWriter},
    process::ExitCode,
};

use flowdictate_asr::{AsrConfig, AsrError, CanonicalAudio, LocalWhisper};
use flowdictate_asr_ipc::{
    worker_read_request, worker_read_startup, worker_write_ready, worker_write_runtime_error,
    worker_write_startup_error, worker_write_transcript, RuntimeErrorCode, WorkerRequest,
    WorkerTranscriptSegment, WORKER_ARGUMENT,
};

const PIPE_BUFFER_BYTES: usize = 64 * 1024;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    if arguments.next().is_none()
        || arguments.next().as_deref() != Some(OsStr::new(WORKER_ARGUMENT))
        || arguments.next().is_some()
    {
        return ExitCode::FAILURE;
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::with_capacity(PIPE_BUFFER_BYTES, stdin.lock());
    let mut writer = BufWriter::with_capacity(PIPE_BUFFER_BYTES, stdout.lock());
    if run_worker(&mut reader, &mut writer).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_worker<R: io::Read, W: io::Write>(reader: &mut R, writer: &mut W) -> Result<(), ()> {
    let Ok(startup) = worker_read_startup(reader) else {
        worker_write_startup_error(writer);
        return Err(());
    };
    let config =
        AsrConfig::new(startup.config.threads, startup.config.language_mode).map_err(|_| ())?;
    let Ok(runtime) = LocalWhisper::from_verified_bytes(startup.model, config) else {
        worker_write_startup_error(writer);
        return Err(());
    };
    worker_write_ready(writer).map_err(|_| ())?;

    loop {
        let Some(request) = worker_read_request(reader).map_err(|_| ())? else {
            return Ok(());
        };
        match request {
            WorkerRequest::Shutdown => return Ok(()),
            WorkerRequest::Transcribe {
                request_id,
                mut samples,
            } => {
                let response =
                    CanonicalAudio::new(&samples).and_then(|audio| runtime.transcribe(audio));
                match response {
                    Ok(transcript) => {
                        let mut segments = Vec::new();
                        if segments
                            .try_reserve_exact(transcript.segments().len())
                            .is_err()
                        {
                            samples.fill(0.0);
                            worker_write_runtime_error(
                                writer,
                                request_id,
                                RuntimeErrorCode::OutputBound,
                            )
                            .map_err(|_| ())?;
                            continue;
                        }
                        segments.extend(transcript.segments().iter().map(|segment| {
                            WorkerTranscriptSegment {
                                byte_start: segment.byte_start,
                                byte_end: segment.byte_end,
                                start_ms: segment.start_ms,
                                end_ms: segment.end_ms,
                            }
                        }));
                        samples.fill(0.0);
                        worker_write_transcript(writer, request_id, transcript.text(), &segments)
                            .map_err(|_| ())?;
                    }
                    Err(error) => {
                        samples.fill(0.0);
                        worker_write_runtime_error(writer, request_id, map_runtime_error(error))
                            .map_err(|_| ())?;
                    }
                }
            }
        }
    }
}

fn map_runtime_error(error: AsrError) -> RuntimeErrorCode {
    match error {
        AsrError::StateInitFailed => RuntimeErrorCode::StateInit,
        AsrError::InferenceFailed => RuntimeErrorCode::Inference,
        AsrError::TooManySegments
        | AsrError::TranscriptTooLong
        | AsrError::TranscriptAllocationFailed => RuntimeErrorCode::OutputBound,
        AsrError::InvalidTranscript
        | AsrError::InvalidTimestamp
        | AsrError::InvalidConfig
        | AsrError::IncompatibleVerifiedModel
        | AsrError::ModelSizeUnsupported
        | AsrError::ModelAllocationFailed
        | AsrError::ModelReadFailed
        | AsrError::RuntimeInitFailed
        | AsrError::EmptyAudio
        | AsrError::AudioTooLong
        | AsrError::InvalidSample => RuntimeErrorCode::InvalidOutput,
    }
}
