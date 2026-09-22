//! Bounded parent-side supervision for the native Nemotron streaming worker.

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    str,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use flowdictate_asr_ipc::{CancellationToken, RuntimeErrorCode, WorkerError};
use flowdictate_audio::VerifiedModelPathLease;

/// Exact canonical samples in one native 160 ms push.
pub const NEMOTRON_CHUNK_SAMPLES: usize = 2_560;
/// Maximum samples retained by one native stream.
pub const NEMOTRON_MAX_STREAM_SAMPLES: usize = 480_000;
/// Maximum accepted UTF-8 hypothesis bytes.
pub const NEMOTRON_MAX_TRANSCRIPT_BYTES: usize = 65_536;

const MODEL_ID: &str = "asr-nemotron-3.5-streaming-0.6b-q8_0-experimental";
const MAGIC: &[u8; 8] = b"FDNEMO01";
const VERSION: u16 = 1;
const MAXIMUM_PATH_BYTES: usize = 32 * 1024;
const POLL: Duration = Duration::from_millis(10);
const MIN_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_TIMEOUT: Duration = Duration::from_secs(120);

/// Parent-enforced native worker deadlines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NemotronWorkerConfig {
    startup_timeout: Duration,
    inference_timeout: Duration,
}

impl NemotronWorkerConfig {
    /// Validates startup and per-push deadlines.
    ///
    /// # Errors
    ///
    /// Rejects deadlines outside 10 ms through 120 seconds.
    pub fn new(
        startup_timeout: Duration,
        inference_timeout: Duration,
    ) -> Result<Self, WorkerError> {
        if !(MIN_TIMEOUT..=MAX_TIMEOUT).contains(&startup_timeout)
            || !(MIN_TIMEOUT..=MAX_TIMEOUT).contains(&inference_timeout)
        {
            return Err(WorkerError::InvalidConfig);
        }
        Ok(Self {
            startup_timeout,
            inference_timeout,
        })
    }
}

impl Default for NemotronWorkerConfig {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(30),
            inference_timeout: Duration::from_secs(30),
        }
    }
}

/// One bounded native interim or final hypothesis.
pub struct NemotronTranscript {
    text: Vec<u8>,
    final_result: bool,
    audio_processed_ms: u32,
}

impl NemotronTranscript {
    /// Borrows validated UTF-8 hypothesis text.
    #[must_use]
    pub fn text(&self) -> &str {
        str::from_utf8(&self.text).unwrap_or("")
    }

    /// Returns whether the native stream finalized this hypothesis.
    #[must_use]
    pub const fn is_final(&self) -> bool {
        self.final_result
    }

    /// Returns bounded audio progress relative to the current utterance.
    #[must_use]
    pub const fn audio_processed_ms(&self) -> u32 {
        self.audio_processed_ms
    }
}

impl Drop for NemotronTranscript {
    fn drop(&mut self) {
        self.text.fill(0);
    }
}

struct Pipes {
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

enum Operation {
    Push(SensitivePcm),
    Finish,
    Statistics,
}

/// Numeric native lifecycle counters for the current worker generation only.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeStreamStatistics {
    /// Non-null native handles acquired.
    pub created: u64,
    /// Successful native finish calls.
    pub finished: u64,
    /// Completed native destruction calls.
    pub destroyed: u64,
    /// Numeric runtime-error responses attempted.
    pub errors: u64,
    /// Currently owned streams.
    pub active: u64,
    /// Highest simultaneous stream ownership.
    pub maximum: u64,
}

enum Reply {
    Transcript(Option<NemotronTranscript>),
    Statistics(NativeStreamStatistics),
}

struct SensitivePcm(Vec<f32>);

impl Drop for SensitivePcm {
    fn drop(&mut self) {
        self.0.fill(0.0);
    }
}

/// Process-isolated owner of one persistent Nemotron recognizer and stream.
pub struct NemotronWorker {
    worker_path: PathBuf,
    _executable_guard: File,
    _model_lease: VerifiedModelPathLease,
    model_path: Vec<u8>,
    config: NemotronWorkerConfig,
    child: Option<Child>,
    pipes: Option<Pipes>,
    next_request_id: u64,
    stream_samples: usize,
    generation: u64,
}

impl NemotronWorker {
    /// Starts the reviewed native worker while retaining the immutable model lease.
    ///
    /// # Errors
    ///
    /// Returns payload-free executable, model, spawn, framing, or deadline failures.
    pub fn spawn(
        worker_executable: &Path,
        model_lease: VerifiedModelPathLease,
        config: NemotronWorkerConfig,
    ) -> Result<Self, WorkerError> {
        if model_lease.entry().id != MODEL_ID {
            return Err(WorkerError::ModelRejected);
        }
        let path = model_lease
            .canonical_path()
            .to_str()
            .ok_or(WorkerError::ModelRejected)?
            .as_bytes();
        if path.is_empty() || path.len() > MAXIMUM_PATH_BYTES || path.contains(&0) {
            return Err(WorkerError::ModelRejected);
        }
        let mut model_path = Vec::new();
        model_path
            .try_reserve_exact(path.len())
            .map_err(|_| WorkerError::AllocationFailed)?;
        model_path.extend_from_slice(path);
        let (worker_path, executable_guard) = open_executable(worker_executable)?;
        let mut worker = Self {
            worker_path,
            _executable_guard: executable_guard,
            _model_lease: model_lease,
            model_path,
            config,
            child: None,
            pipes: None,
            next_request_id: 1,
            stream_samples: 0,
            generation: 0,
        };
        worker.launch()?;
        Ok(worker)
    }

    /// Returns the ready worker generation count.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the current child process identifier.
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Discards the current native stream and starts a clean worker generation.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerError::RecoveryFailed`] when the old child cannot be
    /// terminated or the replacement cannot reach ready state.
    pub fn reset(&mut self) -> Result<(), WorkerError> {
        self.stream_samples = 0;
        if !self.terminate() || self.launch().is_err() {
            Err(WorkerError::RecoveryFailed)
        } else {
            Ok(())
        }
    }

    /// Pushes at most one native 160 ms canonical PCM chunk.
    ///
    /// # Errors
    ///
    /// Invalid audio fails before IPC. Cancellation, timeout, and broken framing
    /// kill the child and establish a clean replacement generation.
    pub fn push(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Option<NemotronTranscript>, WorkerError> {
        if samples.is_empty()
            || samples.len() > NEMOTRON_CHUNK_SAMPLES
            || self.stream_samples.saturating_add(samples.len()) > NEMOTRON_MAX_STREAM_SAMPLES
            || samples
                .iter()
                .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
        {
            return Err(WorkerError::InvalidAudio);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(samples.len())
            .map_err(|_| WorkerError::AllocationFailed)?;
        owned.extend_from_slice(samples);
        let response = self.transact_text(Operation::Push(SensitivePcm(owned)), cancellation)?;
        self.stream_samples += samples.len();
        Ok(response)
    }

    /// Drains and destroys the current stream; the next push creates a fresh one.
    ///
    /// # Errors
    ///
    /// Applies the same cancellation, deadline, IPC, and recovery policy as [`Self::push`].
    pub fn finish(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<Option<NemotronTranscript>, WorkerError> {
        let response = self.transact_text(Operation::Finish, cancellation)?;
        if response.as_ref().is_some_and(|text| !text.is_final())
            || (self.stream_samples > 0 && response.is_none())
        {
            return self.recover(WorkerError::IpcFailed);
        }
        self.stream_samples = 0;
        Ok(response)
    }

    /// Queries numeric native counters through the bounded request channel.
    ///
    /// # Errors
    /// Uses the same deadline and recovery policy as audio requests.
    pub fn statistics(&mut self) -> Result<NativeStreamStatistics, WorkerError> {
        match self.transact(Operation::Statistics, &CancellationToken::new())? {
            Reply::Statistics(statistics) => Ok(statistics),
            Reply::Transcript(_) => self.recover(WorkerError::IpcFailed),
        }
    }

    /// Closes an idle worker's pipes and waits at most five seconds for clean EOF exit.
    /// The preceding statistics query uses the normal request/recovery deadline;
    /// after that query succeeds, no replacement is started. The model lease
    /// remains held until exit.
    ///
    /// # Errors
    /// Rejects active/unbalanced streams, abnormal exit, or shutdown timeout;
    /// ownership cleanup forcibly terminates any remaining child on error.
    pub fn shutdown(mut self) -> Result<NativeStreamStatistics, WorkerError> {
        let statistics = self.statistics()?;
        if statistics.active != 0 || statistics.created != statistics.destroyed {
            return Err(WorkerError::IpcFailed);
        }
        self.pipes.take(); // EOF is the native adapter's ordinary clean exit path.
        let started = Instant::now();
        loop {
            let child = self.child.as_mut().ok_or(WorkerError::WorkerUnavailable)?;
            if let Some(status) = child.try_wait().map_err(|_| WorkerError::IpcFailed)? {
                self.child.take();
                return if status.success() {
                    Ok(statistics)
                } else {
                    Err(WorkerError::IpcFailed)
                };
            }
            if started.elapsed() >= Duration::from_secs(5) {
                return Err(WorkerError::InferenceTimedOut);
            }
            thread::sleep(POLL);
        }
    }

    fn transact_text(
        &mut self,
        operation: Operation,
        cancellation: &CancellationToken,
    ) -> Result<Option<NemotronTranscript>, WorkerError> {
        match self.transact(operation, cancellation)? {
            Reply::Transcript(text) => Ok(text),
            Reply::Statistics(_) => self.recover(WorkerError::IpcFailed),
        }
    }

    fn transact(
        &mut self,
        operation: Operation,
        cancellation: &CancellationToken,
    ) -> Result<Reply, WorkerError> {
        if cancellation.is_cancelled() {
            if self.stream_samples > 0 {
                return self.recover(WorkerError::Cancelled);
            }
            return Err(WorkerError::Cancelled);
        }
        let request_id = self.next_request_id;
        self.next_request_id = request_id
            .checked_add(1)
            .ok_or(WorkerError::RequestIdExhausted)?;
        let pipes = self.pipes.take().ok_or(WorkerError::WorkerUnavailable)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let mut pipes = pipes;
            let result = exchange(&mut pipes, request_id, operation);
            let _ = sender.send((result, pipes));
        });
        let deadline = Instant::now()
            .checked_add(self.config.inference_timeout)
            .ok_or(WorkerError::InvalidConfig)?;
        loop {
            if cancellation.is_cancelled() {
                return self.interrupt(handle, WorkerError::Cancelled);
            }
            let now = Instant::now();
            if now >= deadline {
                return self.interrupt(handle, WorkerError::InferenceTimedOut);
            }
            match receiver.recv_timeout(deadline.saturating_duration_since(now).min(POLL)) {
                Ok((result, pipes)) => {
                    if handle.join().is_err() {
                        return self.recover(WorkerError::IpcFailed);
                    }
                    return match result {
                        Ok(response) => {
                            self.pipes = Some(pipes);
                            if cancellation.is_cancelled() {
                                self.recover(WorkerError::Cancelled)
                            } else {
                                Ok(response)
                            }
                        }
                        Err(ProtocolFailure::Runtime(code)) => {
                            drop(pipes);
                            self.recover(WorkerError::RuntimeFailed(code))
                        }
                        Err(ProtocolFailure::Framing) => {
                            drop(pipes);
                            self.recover(WorkerError::IpcFailed)
                        }
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return self.interrupt(handle, WorkerError::IpcFailed);
                }
            }
        }
    }

    fn interrupt<T>(
        &mut self,
        handle: thread::JoinHandle<()>,
        error: WorkerError,
    ) -> Result<T, WorkerError> {
        if !self.terminate() {
            drop(handle);
            return Err(WorkerError::RecoveryFailed);
        }
        let _ = handle.join();
        self.recover(error)
    }

    fn recover<T>(&mut self, error: WorkerError) -> Result<T, WorkerError> {
        self.stream_samples = 0;
        if !self.terminate() || self.launch().is_err() {
            Err(WorkerError::RecoveryFailed)
        } else {
            Err(error)
        }
    }

    fn launch(&mut self) -> Result<(), WorkerError> {
        let mut command = Command::new(&self.worker_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_clear();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|_| WorkerError::SpawnFailed)?;
        let input = child.stdin.take().ok_or(WorkerError::SpawnFailed)?;
        let output = child.stdout.take().ok_or(WorkerError::SpawnFailed)?;
        let path = self.model_path.clone();
        let handle = thread::spawn(move || startup(input, output, &path));
        let started = Instant::now();
        loop {
            if started.elapsed() >= self.config.startup_timeout {
                let _ = terminate_child(&mut child);
                drop(handle);
                return Err(WorkerError::StartupTimedOut);
            }
            if handle.is_finished() {
                return if let Ok(Ok(pipes)) = handle.join() {
                    self.child = Some(child);
                    self.pipes = Some(pipes);
                    self.generation = self.generation.saturating_add(1);
                    Ok(())
                } else {
                    let _ = terminate_child(&mut child);
                    Err(WorkerError::StartupFailed)
                };
            }
            thread::sleep(POLL);
        }
    }

    fn terminate(&mut self) -> bool {
        self.pipes.take();
        if let Some(mut child) = self.child.take() {
            terminate_child(&mut child)
        } else {
            true
        }
    }
}

impl Drop for NemotronWorker {
    fn drop(&mut self) {
        if let Some(pipes) = self.pipes.as_mut() {
            let _ = pipes.input.write_all(&[3]);
            let _ = pipes.input.flush();
        }
        let _ = self.terminate();
        self.model_path.fill(0);
    }
}

#[derive(Clone, Copy)]
enum ProtocolFailure {
    Framing,
    Runtime(RuntimeErrorCode),
}

fn startup(input: ChildStdin, output: ChildStdout, path: &[u8]) -> Result<Pipes, ProtocolFailure> {
    let mut pipes = Pipes {
        input: BufWriter::with_capacity(64 * 1024, input),
        output: BufReader::with_capacity(64 * 1024, output),
    };
    pipes.input.write_all(MAGIC).map_err(map_io)?;
    pipes
        .input
        .write_all(&VERSION.to_le_bytes())
        .map_err(map_io)?;
    write_u32(&mut pipes.input, path.len()).map_err(|_| ProtocolFailure::Framing)?;
    pipes.input.write_all(path).map_err(map_io)?;
    pipes.input.flush().map_err(map_io)?;
    if read_u8(&mut pipes.output).map_err(|_| ProtocolFailure::Framing)? != 0x80 {
        return Err(ProtocolFailure::Framing);
    }
    Ok(pipes)
}

fn exchange(
    pipes: &mut Pipes,
    request_id: u64,
    operation: Operation,
) -> Result<Reply, ProtocolFailure> {
    let statistics = matches!(&operation, Operation::Statistics);
    match operation {
        Operation::Push(samples) => {
            pipes.input.write_all(&[1]).map_err(map_io)?;
            write_u64(&mut pipes.input, request_id)?;
            write_u32(&mut pipes.input, samples.0.len())?;
            for sample in &samples.0 {
                pipes
                    .input
                    .write_all(&sample.to_le_bytes())
                    .map_err(map_io)?;
            }
        }
        Operation::Finish => {
            pipes.input.write_all(&[2]).map_err(map_io)?;
            write_u64(&mut pipes.input, request_id)?;
        }
        Operation::Statistics => {
            pipes.input.write_all(&[4]).map_err(map_io)?;
            write_u64(&mut pipes.input, request_id)?;
        }
    }
    pipes.input.flush().map_err(map_io)?;
    if statistics {
        read_statistics(&mut pipes.output, request_id).map(Reply::Statistics)
    } else {
        read_response(&mut pipes.output, request_id).map(Reply::Transcript)
    }
}

fn read_statistics<R: Read>(
    reader: &mut R,
    request_id: u64,
) -> Result<NativeStreamStatistics, ProtocolFailure> {
    if read_u8(reader)? != 0x85 || read_u64(reader)? != request_id {
        return Err(ProtocolFailure::Framing);
    }
    let result = NativeStreamStatistics {
        created: read_u64(reader)?,
        finished: read_u64(reader)?,
        destroyed: read_u64(reader)?,
        errors: read_u64(reader)?,
        active: read_u64(reader)?,
        maximum: read_u64(reader)?,
    };
    if result.created.checked_sub(result.destroyed) != Some(result.active)
        || result.finished > result.created
        || result.active > result.maximum
        || result.maximum > 1
        || (result.created > 0 && result.maximum != 1)
    {
        return Err(ProtocolFailure::Framing);
    }
    Ok(result)
}

fn read_response<R: Read>(
    reader: &mut R,
    request_id: u64,
) -> Result<Option<NemotronTranscript>, ProtocolFailure> {
    let tag = read_u8(reader)?;
    if read_u64(reader)? != request_id {
        return Err(ProtocolFailure::Framing);
    }
    match tag {
        0x81 => Ok(None),
        0x82 => {
            let final_result = match read_u8(reader)? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolFailure::Framing),
            };
            let audio_processed_ms = read_u32(reader)?;
            let length =
                usize::try_from(read_u32(reader)?).map_err(|_| ProtocolFailure::Framing)?;
            if audio_processed_ms > 30_000 || length > NEMOTRON_MAX_TRANSCRIPT_BYTES {
                return Err(ProtocolFailure::Framing);
            }
            let mut text = Vec::new();
            text.try_reserve_exact(length)
                .map_err(|_| ProtocolFailure::Framing)?;
            text.resize(length, 0);
            if reader.read_exact(&mut text).is_err() {
                text.fill(0);
                return Err(ProtocolFailure::Framing);
            }
            if str::from_utf8(&text).is_err() || text.iter().any(u8::is_ascii_control) {
                text.fill(0);
                return Err(ProtocolFailure::Framing);
            }
            Ok(Some(NemotronTranscript {
                text,
                final_result,
                audio_processed_ms,
            }))
        }
        0x83 => Err(ProtocolFailure::Runtime(match read_u8(reader)? {
            2 => RuntimeErrorCode::OutputBound,
            3 => RuntimeErrorCode::Inference,
            _ => RuntimeErrorCode::InvalidOutput,
        })),
        _ => Err(ProtocolFailure::Framing),
    }
}

fn open_executable(path: &Path) -> Result<(PathBuf, File), WorkerError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| WorkerError::InvalidExecutable)?;
    if !metadata.is_file() || is_reparse_point(&metadata) {
        return Err(WorkerError::InvalidExecutable);
    }
    let canonical = fs::canonicalize(path).map_err(|_| WorkerError::InvalidExecutable)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let guard = options
        .open(&canonical)
        .map_err(|_| WorkerError::InvalidExecutable)?;
    Ok((canonical, guard))
}

fn terminate_child(child: &mut Child) -> bool {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return true;
    }
    child.kill().is_ok() && child.wait().is_ok()
}

fn write_u32<W: Write>(writer: &mut W, value: usize) -> Result<(), ProtocolFailure> {
    let value = u32::try_from(value).map_err(|_| ProtocolFailure::Framing)?;
    writer.write_all(&value.to_le_bytes()).map_err(map_io)
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), ProtocolFailure> {
    writer.write_all(&value.to_le_bytes()).map_err(map_io)
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, ProtocolFailure> {
    let mut bytes = [0; 1];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(bytes[0])
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, ProtocolFailure> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, ProtocolFailure> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(u64::from_le_bytes(bytes))
}

fn map_io(_: io::Error) -> ProtocolFailure {
    ProtocolFailure::Framing
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x0400 != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::read_statistics;

    #[test]
    fn statistics_reject_truncation_stale_ids_and_unbalanced_counts() {
        let frame = |counts: [u64; 6]| {
            let mut bytes = vec![0x85];
            bytes.extend_from_slice(&7_u64.to_le_bytes());
            for count in counts {
                bytes.extend_from_slice(&count.to_le_bytes());
            }
            bytes
        };
        let valid = frame([100, 100, 100, 0, 0, 1]);
        assert!(read_statistics(&mut valid.as_slice(), 7).is_ok());
        assert!(read_statistics(&mut valid.as_slice(), 8).is_err());
        for length in 0..valid.len() {
            assert!(read_statistics(&mut &valid[..length], 7).is_err());
        }
        for counts in [
            [1, 1, 2, 0, 0, 1],
            [1, 2, 1, 0, 0, 1],
            [1, 1, 1, 0, 1, 1],
            [2, 0, 0, 0, 2, 2],
            [1, 1, 1, 0, 0, 0],
        ] {
            assert!(read_statistics(&mut frame(counts).as_slice(), 7).is_err());
        }
    }
}
