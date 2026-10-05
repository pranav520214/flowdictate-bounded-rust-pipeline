//! Bounded local IPC and process supervision for `FlowDictate` ASR.
//!
//! The parent side of this crate has no native inference dependency. It sends
//! already-verified model bytes and bounded PCM through anonymous pipes to a
//! dedicated worker, kills that process when a deadline expires, and starts a
//! clean replacement from the retained verified bytes.

use std::{
    error::Error,
    fmt,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use flowdictate_audio::{
    verify_compiled_model_bytes, ModelCompatibility, ModelVerificationError, VerifiedModel,
    VerifiedModelBytes, COMPILED_MODEL_MANIFEST,
};

/// Canonical sample rate used on the IPC boundary.
pub const ASR_SAMPLE_RATE_HZ: usize = 16_000;
/// Longest audio window accepted by either side of the IPC boundary.
pub const MAX_INFERENCE_SECONDS: usize = 30;
/// Maximum number of PCM samples in one request.
pub const MAX_INFERENCE_SAMPLES: usize = ASR_SAMPLE_RATE_HZ * MAX_INFERENCE_SECONDS;
/// Maximum segment timestamp emitted by the worker.
pub const MAX_INFERENCE_MILLISECONDS: u32 = 30_000;
/// Maximum UTF-8 transcript bytes returned for one request.
pub const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;
/// Maximum transcript segments returned for one request.
pub const MAX_TRANSCRIPT_SEGMENTS: usize = 128;
/// Maximum native inference thread count accepted from the parent.
pub const MAX_INFERENCE_THREADS: u8 = 8;
/// Fixed command-line marker required by the version-two worker executable.
pub const WORKER_ARGUMENT: &str = "--flowdictate-asr-worker-v2";
/// Only model identity accepted by the version-two worker protocol.
pub const WORKER_MODEL_ID: &str = "asr-whisper-tiny-multilingual-q5_1";

const STARTUP_MAGIC: [u8; 8] = *b"FDASR002";
const READY_MAGIC: [u8; 8] = *b"FDREADY2";
const STARTUP_ERROR_MAGIC: [u8; 8] = *b"FDERROR2";
const REQUEST_TRANSCRIBE: u8 = 1;
const REQUEST_SHUTDOWN: u8 = 2;
const RESPONSE_TRANSCRIPT: u8 = 0;
const RESPONSE_ERROR: u8 = 1;
const PIPE_BUFFER_BYTES: usize = 64 * 1024;
const MIN_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_TIMEOUT: Duration = Duration::from_secs(120);
const CANCELLATION_POLL: Duration = Duration::from_millis(10);

/// Runtime compatibility repeated by the worker before native model parsing.
pub const WORKER_MODEL_COMPATIBILITY: ModelCompatibility = ModelCompatibility {
    purpose: "asr",
    runtime: "whisper.cpp",
    architecture: "whisper",
    quantization: "q5_1",
};

/// Fixed languages exposed by the reviewed multilingual worker configuration.
///
/// This is a configuration allowlist, not a production-quality claim. Quality
/// remains gated by per-language benchmark and native-speaker review.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    /// English (`en`).
    English,
    /// Hindi (`hi`).
    Hindi,
    /// Punjabi (`pa`).
    Punjabi,
    /// Bengali (`bn`).
    Bengali,
    /// Marathi (`mr`).
    Marathi,
    /// Tamil (`ta`).
    Tamil,
    /// Telugu (`te`).
    Telugu,
    /// Urdu (`ur`).
    Urdu,
    /// Gujarati (`gu`).
    Gujarati,
    /// Kannada (`kn`).
    Kannada,
    /// Malayalam (`ml`).
    Malayalam,
    /// Arabic (`ar`).
    Arabic,
    /// German (`de`).
    German,
    /// French (`fr`).
    French,
    /// Spanish (`es`).
    Spanish,
    /// Italian (`it`).
    Italian,
    /// Portuguese (`pt`).
    Portuguese,
    /// Dutch (`nl`).
    Dutch,
    /// Polish (`pl`).
    Polish,
    /// Turkish (`tr`).
    Turkish,
    /// Indonesian (`id`).
    Indonesian,
    /// Japanese (`ja`).
    Japanese,
    /// Korean (`ko`).
    Korean,
}

impl Language {
    /// Returns the static ISO 639-1 code accepted by `whisper.cpp`.
    #[must_use]
    pub const fn iso_639_1(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Hindi => "hi",
            Self::Punjabi => "pa",
            Self::Bengali => "bn",
            Self::Marathi => "mr",
            Self::Tamil => "ta",
            Self::Telugu => "te",
            Self::Urdu => "ur",
            Self::Gujarati => "gu",
            Self::Kannada => "kn",
            Self::Malayalam => "ml",
            Self::Arabic => "ar",
            Self::German => "de",
            Self::French => "fr",
            Self::Spanish => "es",
            Self::Italian => "it",
            Self::Portuguese => "pt",
            Self::Dutch => "nl",
            Self::Polish => "pl",
            Self::Turkish => "tr",
            Self::Indonesian => "id",
            Self::Japanese => "ja",
            Self::Korean => "ko",
        }
    }

    /// Parses one compiled allowlisted ISO 639-1 language code.
    #[must_use]
    pub const fn from_iso_639_1(code: &str) -> Option<Self> {
        match code.as_bytes() {
            b"en" => Some(Self::English),
            b"hi" => Some(Self::Hindi),
            b"pa" => Some(Self::Punjabi),
            b"bn" => Some(Self::Bengali),
            b"mr" => Some(Self::Marathi),
            b"ta" => Some(Self::Tamil),
            b"te" => Some(Self::Telugu),
            b"ur" => Some(Self::Urdu),
            b"gu" => Some(Self::Gujarati),
            b"kn" => Some(Self::Kannada),
            b"ml" => Some(Self::Malayalam),
            b"ar" => Some(Self::Arabic),
            b"de" => Some(Self::German),
            b"fr" => Some(Self::French),
            b"es" => Some(Self::Spanish),
            b"it" => Some(Self::Italian),
            b"pt" => Some(Self::Portuguese),
            b"nl" => Some(Self::Dutch),
            b"pl" => Some(Self::Polish),
            b"tr" => Some(Self::Turkish),
            b"id" => Some(Self::Indonesian),
            b"ja" => Some(Self::Japanese),
            b"ko" => Some(Self::Korean),
            _ => None,
        }
    }

    const fn wire_code(self) -> u8 {
        match self {
            Self::English => 1,
            Self::Hindi => 2,
            Self::Punjabi => 3,
            Self::Bengali => 4,
            Self::Marathi => 5,
            Self::Tamil => 6,
            Self::Telugu => 7,
            Self::Urdu => 8,
            Self::Gujarati => 9,
            Self::Kannada => 10,
            Self::Malayalam => 11,
            Self::Arabic => 12,
            Self::German => 13,
            Self::French => 14,
            Self::Spanish => 15,
            Self::Italian => 16,
            Self::Portuguese => 17,
            Self::Dutch => 18,
            Self::Polish => 19,
            Self::Turkish => 20,
            Self::Indonesian => 21,
            Self::Japanese => 22,
            Self::Korean => 23,
        }
    }

    const fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::English),
            2 => Some(Self::Hindi),
            3 => Some(Self::Punjabi),
            4 => Some(Self::Bengali),
            5 => Some(Self::Marathi),
            6 => Some(Self::Tamil),
            7 => Some(Self::Telugu),
            8 => Some(Self::Urdu),
            9 => Some(Self::Gujarati),
            10 => Some(Self::Kannada),
            11 => Some(Self::Malayalam),
            12 => Some(Self::Arabic),
            13 => Some(Self::German),
            14 => Some(Self::French),
            15 => Some(Self::Spanish),
            16 => Some(Self::Italian),
            17 => Some(Self::Portuguese),
            18 => Some(Self::Dutch),
            19 => Some(Self::Polish),
            20 => Some(Self::Turkish),
            21 => Some(Self::Indonesian),
            22 => Some(Self::Japanese),
            23 => Some(Self::Korean),
            _ => None,
        }
    }
}

/// Session-stable language selection for local inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageMode {
    /// Detect language independently inside the local multilingual model.
    Automatic,
    /// Force one allowlisted ISO 639-1 language for every inference window.
    Fixed(Language),
}

impl LanguageMode {
    const fn wire_code(self) -> u8 {
        match self {
            Self::Automatic => 0,
            Self::Fixed(language) => language.wire_code(),
        }
    }

    const fn from_wire(value: u8) -> Option<Self> {
        if value == 0 {
            Some(Self::Automatic)
        } else {
            match Language::from_wire(value) {
                Some(language) => Some(Self::Fixed(language)),
                None => None,
            }
        }
    }
}

/// Bounded worker and deadline configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    threads: u8,
    language_mode: LanguageMode,
    startup_timeout: Duration,
    inference_timeout: Duration,
}

impl WorkerConfig {
    /// Validates CPU and wall-clock limits for the supervised worker.
    ///
    /// # Errors
    ///
    /// Rejects zero/excessive threads or deadlines outside 10 ms–120 s.
    pub fn new(
        threads: u8,
        language_mode: LanguageMode,
        startup_timeout: Duration,
        inference_timeout: Duration,
    ) -> Result<Self, WorkerError> {
        if threads == 0
            || threads > MAX_INFERENCE_THREADS
            || startup_timeout < MIN_TIMEOUT
            || startup_timeout > MAX_TIMEOUT
            || inference_timeout < MIN_TIMEOUT
            || inference_timeout > MAX_TIMEOUT
        {
            return Err(WorkerError::InvalidConfig);
        }
        Ok(Self {
            threads,
            language_mode,
            startup_timeout,
            inference_timeout,
        })
    }

    /// Returns the native CPU worker count.
    #[must_use]
    pub const fn threads(self) -> u8 {
        self.threads
    }

    /// Returns the automatic or fixed local inference language policy.
    #[must_use]
    pub const fn language_mode(self) -> LanguageMode {
        self.language_mode
    }

    /// Returns the model-transfer and initialization deadline.
    #[must_use]
    pub const fn startup_timeout(self) -> Duration {
        self.startup_timeout
    }

    /// Returns the per-request wall-clock deadline.
    #[must_use]
    pub const fn inference_timeout(self) -> Duration {
        self.inference_timeout
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            language_mode: LanguageMode::Automatic,
            startup_timeout: Duration::from_secs(15),
            inference_timeout: Duration::from_secs(30),
        }
    }
}

/// Cloneable one-shot cancellation signal for an ASR request or dictation
/// session. It contains no audio or transcript data.
#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates an uncancelled signal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Repeated calls are harmless.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Worker-side startup state reconstructed from the bounded pipe.
pub struct WorkerStartup {
    /// Validated runtime configuration.
    pub config: WorkerRuntimeConfig,
    /// Independently re-hashed model bytes.
    pub model: VerifiedModelBytes,
}

/// Minimal configuration consumed by the native child process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerRuntimeConfig {
    /// Native CPU worker count.
    pub threads: u8,
    /// Automatic or allowlisted fixed language selection.
    pub language_mode: LanguageMode,
}

/// One decoded request accepted by the child process.
pub enum WorkerRequest {
    /// Transcribe bounded canonical PCM.
    Transcribe {
        /// Parent-generated request identity.
        request_id: u64,
        /// Owned mono 16 kHz PCM; the worker clears it after use.
        samples: Vec<f32>,
    },
    /// Exit without processing another request.
    Shutdown,
}

/// Byte offsets and timestamps for one IPC transcript segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerTranscriptSegment {
    /// UTF-8 start offset in the transcript.
    pub byte_start: usize,
    /// UTF-8 exclusive end offset in the transcript.
    pub byte_end: usize,
    /// Start timestamp in milliseconds.
    pub start_ms: u32,
    /// End timestamp in milliseconds.
    pub end_ms: u32,
}

/// Bounded transcript received from the isolated worker.
#[derive(Eq, PartialEq)]
pub struct WorkerTranscript {
    text: Vec<u8>,
    segments: Vec<WorkerTranscriptSegment>,
}

impl WorkerTranscript {
    /// Returns the worker's exact UTF-8 output.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.text).unwrap_or_default()
    }

    /// Returns validated segment metadata.
    #[must_use]
    pub fn segments(&self) -> &[WorkerTranscriptSegment] {
        &self.segments
    }
}

impl Drop for WorkerTranscript {
    fn drop(&mut self) {
        self.text.fill(0);
    }
}

/// Stable payload-free native failure categories sent across IPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RuntimeErrorCode {
    /// Runtime state could not be created.
    StateInit = 1,
    /// Native inference failed.
    Inference = 2,
    /// Native output exceeded a bound.
    OutputBound = 3,
    /// Native output was structurally invalid.
    InvalidOutput = 4,
}

impl RuntimeErrorCode {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::StateInit),
            2 => Some(Self::Inference),
            3 => Some(Self::OutputBound),
            4 => Some(Self::InvalidOutput),
            _ => None,
        }
    }
}

/// Parent-owned supervised native worker.
pub struct AsrWorker {
    worker_path: PathBuf,
    _executable_guard: File,
    model: Arc<[u8]>,
    config: WorkerConfig,
    child: Option<Child>,
    pipes: Option<Pipes>,
    next_request_id: u64,
    generation: u64,
}

impl AsrWorker {
    /// Starts an explicitly selected worker using model bytes read from the
    /// exact handle retained by the model-security gate.
    ///
    /// The executable remains open without write/delete sharing on Windows,
    /// preventing path replacement for the lifetime of the supervisor.
    ///
    /// # Errors
    ///
    /// Returns a payload-free failure for executable, model, spawn, IPC, or
    /// startup deadline failures.
    pub fn spawn(
        worker_executable: &Path,
        verified_model: VerifiedModel,
        config: WorkerConfig,
    ) -> Result<Self, WorkerError> {
        let (worker_path, executable_guard) = open_worker_executable(worker_executable)?;
        let verified_bytes = verified_model
            .into_verified_bytes()
            .map_err(map_model_error)?;
        if verified_bytes.entry().id != WORKER_MODEL_ID {
            return Err(WorkerError::ModelRejected);
        }
        let model: Arc<[u8]> = Arc::from(verified_bytes.into_vec().into_boxed_slice());
        let mut worker = Self {
            worker_path,
            _executable_guard: executable_guard,
            model,
            config,
            child: None,
            pipes: None,
            next_request_id: 1,
            generation: 0,
        };
        worker.launch()?;
        Ok(worker)
    }

    /// Returns the current OS process identifier.
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Returns how many worker generations have reached ready state.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the language policy of the current ready worker generation.
    #[must_use]
    pub const fn language_mode(&self) -> LanguageMode {
        self.config.language_mode
    }

    /// Replaces the ready worker with a clean generation under a new language
    /// policy. Reapplying the current policy is a no-op.
    ///
    /// If startup under the new policy fails, the previous policy is restored
    /// and relaunched before the original payload-free error is returned.
    ///
    /// # Errors
    ///
    /// Returns a worker startup category when the new generation fails but the
    /// previous one is restored, or [`WorkerError::RecoveryFailed`] when no
    /// ready rollback generation can be established.
    pub fn set_language_mode(&mut self, language_mode: LanguageMode) -> Result<bool, WorkerError> {
        if self.config.language_mode == language_mode {
            return Ok(false);
        }
        if !self.terminate() {
            return Err(WorkerError::RecoveryFailed);
        }

        let previous = self.config;
        self.config.language_mode = language_mode;
        match self.launch() {
            Ok(()) => Ok(true),
            Err(primary) => {
                self.config = previous;
                if self.launch().is_ok() {
                    Err(primary)
                } else {
                    Err(WorkerError::RecoveryFailed)
                }
            }
        }
    }

    /// Transcribes one canonical PCM window under the configured wall-clock
    /// deadline.
    ///
    /// A timeout or broken protocol causes immediate process termination. The
    /// supervisor then starts and verifies a clean replacement before returning
    /// the original timeout/protocol error. If recovery fails, it returns
    /// [`WorkerError::RecoveryFailed`].
    ///
    /// # Errors
    ///
    /// Rejects invalid PCM before IPC and returns fixed worker, timeout, IPC, or
    /// recovery categories without audio or transcript payloads.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<WorkerTranscript, WorkerError> {
        self.transcribe_internal(samples, None)
    }

    /// Transcribes one canonical PCM window while observing a one-shot
    /// cancellation token.
    ///
    /// Cancellation before dispatch avoids IPC. Cancellation during native
    /// inference terminates and replaces the worker process before returning.
    ///
    /// # Errors
    ///
    /// Applies the normal transcription errors and returns
    /// [`WorkerError::Cancelled`] when the token is observed.
    pub fn transcribe_with_cancel(
        &mut self,
        samples: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<WorkerTranscript, WorkerError> {
        self.transcribe_internal(samples, Some(cancellation))
    }

    fn transcribe_internal(
        &mut self,
        samples: &[f32],
        cancellation: Option<&CancellationToken>,
    ) -> Result<WorkerTranscript, WorkerError> {
        validate_audio(samples)?;
        if cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Err(WorkerError::Cancelled);
        }
        let deadline = Instant::now()
            .checked_add(self.config.inference_timeout)
            .ok_or(WorkerError::InvalidConfig)?;
        let mut owned = SensitivePcm::new();
        owned
            .samples
            .try_reserve_exact(samples.len())
            .map_err(|_| WorkerError::AllocationFailed)?;
        owned.samples.extend_from_slice(samples);

        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(WorkerError::RequestIdExhausted)?;
        let pipes = self.pipes.take().ok_or(WorkerError::WorkerUnavailable)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let mut pipes = pipes;
            let result = transact(&mut pipes, request_id, &owned.samples);
            drop(owned);
            let _ = sender.send((result, pipes));
        });

        loop {
            if cancellation.is_some_and(CancellationToken::is_cancelled) {
                if !self.terminate() {
                    drop(handle);
                    return Err(WorkerError::RecoveryFailed);
                }
                let _ = handle.join();
                return self.recover(WorkerError::Cancelled);
            }

            let now = Instant::now();
            if now >= deadline {
                if !self.terminate() {
                    drop(handle);
                    return Err(WorkerError::RecoveryFailed);
                }
                let _ = handle.join();
                return self.recover(WorkerError::InferenceTimedOut);
            }

            let wait = deadline
                .saturating_duration_since(now)
                .min(CANCELLATION_POLL);
            match receiver.recv_timeout(wait) {
                Ok((result, pipes)) => {
                    let joined = handle.join().is_ok();
                    if !joined {
                        return self.recover(WorkerError::IpcFailed);
                    }
                    self.pipes = Some(pipes);
                    if cancellation.is_some_and(CancellationToken::is_cancelled) {
                        return Err(WorkerError::Cancelled);
                    }
                    return match result {
                        Ok(WorkerResponse::Transcript(transcript)) => Ok(transcript),
                        Ok(WorkerResponse::Error(code)) => Err(WorkerError::RuntimeFailed(code)),
                        Err(_) => self.recover(WorkerError::IpcFailed),
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if !self.terminate() {
                        drop(handle);
                        return Err(WorkerError::RecoveryFailed);
                    }
                    let _ = handle.join();
                    return self.recover(WorkerError::IpcFailed);
                }
            }
        }
    }

    fn recover<T>(&mut self, primary: WorkerError) -> Result<T, WorkerError> {
        if !self.terminate() {
            return Err(WorkerError::RecoveryFailed);
        }
        if self.launch().is_ok() {
            Err(primary)
        } else {
            Err(WorkerError::RecoveryFailed)
        }
    }

    fn launch(&mut self) -> Result<(), WorkerError> {
        let mut command = Command::new(&self.worker_path);
        command
            .arg(WORKER_ARGUMENT)
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

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|_| WorkerError::SpawnFailed)?;
        let stdin = child.stdin.take().ok_or(WorkerError::SpawnFailed)?;
        let stdout = child.stdout.take().ok_or(WorkerError::SpawnFailed)?;
        let model = Arc::clone(&self.model);
        let runtime_config = WorkerRuntimeConfig {
            threads: self.config.threads,
            language_mode: self.config.language_mode,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let result = parent_handshake(stdin, stdout, &model, runtime_config);
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(self.config.startup_timeout) {
            Ok(Ok(pipes)) => {
                if handle.join().is_err() {
                    let _ = terminate_child(&mut child);
                    return Err(WorkerError::StartupFailed);
                }
                if child
                    .try_wait()
                    .map_err(|_| WorkerError::StartupFailed)?
                    .is_some()
                {
                    return Err(WorkerError::StartupFailed);
                }
                self.child = Some(child);
                self.pipes = Some(pipes);
                self.generation = self.generation.saturating_add(1);
                Ok(())
            }
            Ok(Err(error)) => {
                if terminate_child(&mut child) {
                    let _ = handle.join();
                } else {
                    drop(handle);
                }
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if terminate_child(&mut child) {
                    let _ = handle.join();
                    Err(WorkerError::StartupTimedOut)
                } else {
                    drop(handle);
                    Err(WorkerError::StartupFailed)
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if terminate_child(&mut child) {
                    let _ = handle.join();
                } else {
                    drop(handle);
                }
                Err(WorkerError::StartupFailed)
            }
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

impl Drop for AsrWorker {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

struct Pipes {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

struct SensitivePcm {
    samples: Vec<f32>,
}

impl SensitivePcm {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }
}

impl Drop for SensitivePcm {
    fn drop(&mut self) {
        self.samples.fill(0.0);
    }
}

enum WorkerResponse {
    Transcript(WorkerTranscript),
    Error(RuntimeErrorCode),
}

/// Reads and verifies the bounded startup frame in a child process.
///
/// # Errors
///
/// Rejects malformed framing, invalid configuration, unexpected model size,
/// allocation/I/O failure, or bytes that fail the compiled model manifest.
pub fn worker_read_startup<R: Read>(reader: &mut R) -> Result<WorkerStartup, ProtocolError> {
    let mut magic = [0_u8; 8];
    reader.read_exact(&mut magic).map_err(map_io)?;
    if magic != STARTUP_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    let threads = read_u8(reader)?;
    let language_mode =
        LanguageMode::from_wire(read_u8(reader)?).ok_or(ProtocolError::InvalidConfig)?;
    let reserved = read_u16(reader)?;
    if threads == 0 || threads > MAX_INFERENCE_THREADS || reserved != 0 {
        return Err(ProtocolError::InvalidConfig);
    }
    let model_size = read_u64(reader)?;
    let entry = COMPILED_MODEL_MANIFEST
        .entries
        .iter()
        .find(|entry| entry.id == WORKER_MODEL_ID)
        .ok_or(ProtocolError::ModelRejected)?;
    if model_size != entry.size_bytes {
        return Err(ProtocolError::ModelRejected);
    }
    let size = usize::try_from(model_size).map_err(|_| ProtocolError::ModelRejected)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| ProtocolError::AllocationFailed)?;
    bytes.resize(size, 0);
    reader.read_exact(&mut bytes).map_err(map_io)?;
    let model = verify_compiled_model_bytes(bytes, WORKER_MODEL_ID, WORKER_MODEL_COMPATIBILITY)
        .map_err(|_| ProtocolError::ModelRejected)?;
    Ok(WorkerStartup {
        config: WorkerRuntimeConfig {
            threads,
            language_mode,
        },
        model,
    })
}

/// Writes the fixed ready marker after native initialization succeeds.
///
/// # Errors
///
/// Returns [`ProtocolError::IoFailed`] when the pipe cannot be written.
pub fn worker_write_ready<W: Write>(writer: &mut W) -> Result<(), ProtocolError> {
    writer.write_all(&READY_MAGIC).map_err(map_io)?;
    writer.flush().map_err(map_io)
}

/// Writes a fixed startup-error marker with no diagnostic payload.
pub fn worker_write_startup_error<W: Write>(writer: &mut W) {
    let _ = writer.write_all(&STARTUP_ERROR_MAGIC);
    let _ = writer.flush();
}

/// Reads one bounded request. Clean EOF is treated as worker shutdown.
///
/// # Errors
///
/// Rejects unknown tags, malformed framing, invalid samples, excessive sample
/// counts, or allocation/I/O failures.
pub fn worker_read_request<R: Read>(
    reader: &mut R,
) -> Result<Option<WorkerRequest>, ProtocolError> {
    let mut tag = [0_u8; 1];
    match reader.read(&mut tag) {
        Ok(0) => return Ok(None),
        Ok(1) => {}
        Ok(_) => return Err(ProtocolError::InvalidFrame),
        Err(_) => return Err(ProtocolError::IoFailed),
    }
    match tag[0] {
        REQUEST_SHUTDOWN => Ok(Some(WorkerRequest::Shutdown)),
        REQUEST_TRANSCRIBE => {
            let request_id = read_u64(reader)?;
            let count =
                usize::try_from(read_u32(reader)?).map_err(|_| ProtocolError::InvalidAudio)?;
            if count == 0 || count > MAX_INFERENCE_SAMPLES {
                return Err(ProtocolError::InvalidAudio);
            }
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(count)
                .map_err(|_| ProtocolError::AllocationFailed)?;
            for _ in 0..count {
                let mut bytes = [0_u8; 4];
                if reader.read_exact(&mut bytes).is_err() {
                    samples.fill(0.0);
                    return Err(ProtocolError::IoFailed);
                }
                let sample = f32::from_le_bytes(bytes);
                if !sample.is_finite() || !(-1.0..=1.0).contains(&sample) {
                    samples.fill(0.0);
                    return Err(ProtocolError::InvalidAudio);
                }
                samples.push(sample);
            }
            Ok(Some(WorkerRequest::Transcribe {
                request_id,
                samples,
            }))
        }
        _ => Err(ProtocolError::InvalidFrame),
    }
}

/// Writes a bounded transcript response.
///
/// # Errors
///
/// Rejects transcript/segment bounds before writing or returns an I/O failure.
pub fn worker_write_transcript<W: Write>(
    writer: &mut W,
    request_id: u64,
    text: &str,
    segments: &[WorkerTranscriptSegment],
) -> Result<(), ProtocolError> {
    validate_transcript(text, segments)?;
    writer.write_all(&[RESPONSE_TRANSCRIPT]).map_err(map_io)?;
    write_u64(writer, request_id)?;
    write_u32(
        writer,
        u32::try_from(text.len()).map_err(|_| ProtocolError::InvalidTranscript)?,
    )?;
    write_u16(
        writer,
        u16::try_from(segments.len()).map_err(|_| ProtocolError::InvalidTranscript)?,
    )?;
    writer.write_all(text.as_bytes()).map_err(map_io)?;
    for segment in segments {
        write_u32(
            writer,
            u32::try_from(segment.byte_start).map_err(|_| ProtocolError::InvalidTranscript)?,
        )?;
        write_u32(
            writer,
            u32::try_from(segment.byte_end).map_err(|_| ProtocolError::InvalidTranscript)?,
        )?;
        write_u32(writer, segment.start_ms)?;
        write_u32(writer, segment.end_ms)?;
    }
    writer.flush().map_err(map_io)
}

/// Writes a fixed native-runtime error response.
///
/// # Errors
///
/// Returns an I/O failure if the response cannot be delivered.
pub fn worker_write_runtime_error<W: Write>(
    writer: &mut W,
    request_id: u64,
    code: RuntimeErrorCode,
) -> Result<(), ProtocolError> {
    writer.write_all(&[RESPONSE_ERROR]).map_err(map_io)?;
    write_u64(writer, request_id)?;
    write_u16(writer, code as u16)?;
    writer.flush().map_err(map_io)
}

fn parent_handshake(
    stdin: ChildStdin,
    stdout: ChildStdout,
    model: &[u8],
    config: WorkerRuntimeConfig,
) -> Result<Pipes, WorkerError> {
    let mut stdin = BufWriter::with_capacity(PIPE_BUFFER_BYTES, stdin);
    let mut stdout = BufReader::with_capacity(PIPE_BUFFER_BYTES, stdout);
    stdin
        .write_all(&STARTUP_MAGIC)
        .map_err(|_| WorkerError::IpcFailed)?;
    stdin
        .write_all(&[config.threads])
        .map_err(|_| WorkerError::IpcFailed)?;
    stdin
        .write_all(&[config.language_mode.wire_code()])
        .map_err(|_| WorkerError::IpcFailed)?;
    stdin
        .write_all(&0_u16.to_le_bytes())
        .map_err(|_| WorkerError::IpcFailed)?;
    stdin
        .write_all(
            &u64::try_from(model.len())
                .map_err(|_| WorkerError::ModelRejected)?
                .to_le_bytes(),
        )
        .map_err(|_| WorkerError::IpcFailed)?;
    stdin.write_all(model).map_err(|_| WorkerError::IpcFailed)?;
    stdin.flush().map_err(|_| WorkerError::IpcFailed)?;
    let mut response = [0_u8; 8];
    stdout
        .read_exact(&mut response)
        .map_err(|_| WorkerError::StartupFailed)?;
    if response == READY_MAGIC {
        Ok(Pipes { stdin, stdout })
    } else {
        Err(WorkerError::StartupFailed)
    }
}

fn transact(
    pipes: &mut Pipes,
    request_id: u64,
    samples: &[f32],
) -> Result<WorkerResponse, ProtocolError> {
    pipes
        .stdin
        .write_all(&[REQUEST_TRANSCRIBE])
        .map_err(map_io)?;
    write_u64(&mut pipes.stdin, request_id)?;
    write_u32(
        &mut pipes.stdin,
        u32::try_from(samples.len()).map_err(|_| ProtocolError::InvalidAudio)?,
    )?;
    for sample in samples {
        pipes
            .stdin
            .write_all(&sample.to_le_bytes())
            .map_err(map_io)?;
    }
    pipes.stdin.flush().map_err(map_io)?;
    read_response(&mut pipes.stdout, request_id)
}

fn read_response<R: Read>(
    reader: &mut R,
    expected_request_id: u64,
) -> Result<WorkerResponse, ProtocolError> {
    let tag = read_u8(reader)?;
    let request_id = read_u64(reader)?;
    if request_id != expected_request_id {
        return Err(ProtocolError::RequestMismatch);
    }
    match tag {
        RESPONSE_ERROR => {
            let code =
                RuntimeErrorCode::from_u16(read_u16(reader)?).ok_or(ProtocolError::InvalidFrame)?;
            Ok(WorkerResponse::Error(code))
        }
        RESPONSE_TRANSCRIPT => {
            let text_len =
                usize::try_from(read_u32(reader)?).map_err(|_| ProtocolError::InvalidTranscript)?;
            let segment_count = usize::from(read_u16(reader)?);
            if text_len > MAX_TRANSCRIPT_BYTES || segment_count > MAX_TRANSCRIPT_SEGMENTS {
                return Err(ProtocolError::InvalidTranscript);
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(text_len)
                .map_err(|_| ProtocolError::AllocationFailed)?;
            bytes.resize(text_len, 0);
            reader.read_exact(&mut bytes).map_err(map_io)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| ProtocolError::InvalidTranscript)?;
            let mut segments = Vec::new();
            segments
                .try_reserve_exact(segment_count)
                .map_err(|_| ProtocolError::AllocationFailed)?;
            for _ in 0..segment_count {
                segments.push(WorkerTranscriptSegment {
                    byte_start: usize::try_from(read_u32(reader)?)
                        .map_err(|_| ProtocolError::InvalidTranscript)?,
                    byte_end: usize::try_from(read_u32(reader)?)
                        .map_err(|_| ProtocolError::InvalidTranscript)?,
                    start_ms: read_u32(reader)?,
                    end_ms: read_u32(reader)?,
                });
            }
            validate_transcript(text, &segments)?;
            Ok(WorkerResponse::Transcript(WorkerTranscript {
                text: bytes,
                segments,
            }))
        }
        _ => Err(ProtocolError::InvalidFrame),
    }
}

fn validate_audio(samples: &[f32]) -> Result<(), WorkerError> {
    if samples.is_empty()
        || samples.len() > MAX_INFERENCE_SAMPLES
        || samples
            .iter()
            .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
    {
        return Err(WorkerError::InvalidAudio);
    }
    Ok(())
}

fn validate_transcript(
    text: &str,
    segments: &[WorkerTranscriptSegment],
) -> Result<(), ProtocolError> {
    if text.len() > MAX_TRANSCRIPT_BYTES || segments.len() > MAX_TRANSCRIPT_SEGMENTS {
        return Err(ProtocolError::InvalidTranscript);
    }
    let mut prior_end = 0;
    for segment in segments {
        if segment.byte_start != prior_end
            || segment.byte_end < segment.byte_start
            || segment.byte_end > text.len()
            || !text.is_char_boundary(segment.byte_start)
            || !text.is_char_boundary(segment.byte_end)
            || segment.end_ms < segment.start_ms
            || segment.end_ms > MAX_INFERENCE_MILLISECONDS
        {
            return Err(ProtocolError::InvalidTranscript);
        }
        prior_end = segment.byte_end;
    }
    if prior_end != text.len() || (segments.is_empty() && !text.is_empty()) {
        return Err(ProtocolError::InvalidTranscript);
    }
    Ok(())
}

fn open_worker_executable(path: &Path) -> Result<(PathBuf, File), WorkerError> {
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

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        options.share_mode(FILE_SHARE_READ);
    }
    let guard = options
        .open(&canonical)
        .map_err(|_| WorkerError::InvalidExecutable)?;
    let opened = guard
        .metadata()
        .map_err(|_| WorkerError::InvalidExecutable)?;
    if !opened.is_file() || opened.len() == 0 || is_reparse_point(&opened) {
        return Err(WorkerError::InvalidExecutable);
    }
    Ok((canonical, guard))
}

fn terminate_child(child: &mut Child) -> bool {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return true;
    }
    if child.kill().is_err() {
        return false;
    }
    child.wait().is_ok()
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &Metadata) -> bool {
    false
}

fn map_model_error(_: ModelVerificationError) -> WorkerError {
    WorkerError::ModelRejected
}

fn map_io(_: io::Error) -> ProtocolError {
    ProtocolError::IoFailed
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, ProtocolError> {
    let mut bytes = [0_u8; 1];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(bytes[0])
}

fn read_u16<R: Read>(reader: &mut R) -> Result<u16, ProtocolError> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, ProtocolError> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, ProtocolError> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes).map_err(map_io)?;
    Ok(u64::from_le_bytes(bytes))
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> Result<(), ProtocolError> {
    writer.write_all(&value.to_le_bytes()).map_err(map_io)
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> Result<(), ProtocolError> {
    writer.write_all(&value.to_le_bytes()).map_err(map_io)
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), ProtocolError> {
    writer.write_all(&value.to_le_bytes()).map_err(map_io)
}

/// Payload-free supervisor failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerError {
    /// Worker count or deadline configuration is outside compiled limits.
    InvalidConfig,
    /// PCM is empty, excessive, non-finite, or out of range.
    InvalidAudio,
    /// The selected worker path is not a stable regular executable file.
    InvalidExecutable,
    /// The verified model could not be copied or did not match the worker identity.
    ModelRejected,
    /// Bounded memory could not be reserved.
    AllocationFailed,
    /// The worker process could not be created.
    SpawnFailed,
    /// Model transfer or initialization exceeded the startup deadline.
    StartupTimedOut,
    /// The worker rejected startup or exited before ready.
    StartupFailed,
    /// No live worker/pipes are available.
    WorkerUnavailable,
    /// Request/response framing or transport failed.
    IpcFailed,
    /// Native inference exceeded the parent-enforced wall-clock deadline.
    InferenceTimedOut,
    /// The worker reported a fixed native-runtime failure.
    RuntimeFailed(RuntimeErrorCode),
    /// The failed/timed-out worker was killed but its replacement did not start.
    RecoveryFailed,
    /// The request counter can no longer produce a unique identity.
    RequestIdExhausted,
    /// The caller cancelled before completion; any in-flight worker was replaced.
    Cancelled,
}

impl fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfig => "invalid ASR worker configuration",
            Self::InvalidAudio => "invalid ASR worker audio",
            Self::InvalidExecutable => "invalid ASR worker executable",
            Self::ModelRejected => "ASR worker model was rejected",
            Self::AllocationFailed => "ASR worker memory allocation failed",
            Self::SpawnFailed => "ASR worker could not be started",
            Self::StartupTimedOut => "ASR worker startup timed out",
            Self::StartupFailed => "ASR worker startup failed",
            Self::WorkerUnavailable => "ASR worker is unavailable",
            Self::IpcFailed => "ASR worker IPC failed",
            Self::InferenceTimedOut => "ASR worker inference timed out and was restarted",
            Self::RuntimeFailed(_) => "ASR worker runtime failed",
            Self::RecoveryFailed => "ASR worker recovery failed",
            Self::RequestIdExhausted => "ASR worker request identity exhausted",
            Self::Cancelled => "ASR worker request was cancelled",
        };
        formatter.write_str(message)
    }
}

impl Error for WorkerError {}

/// Payload-free framing failures used inside the child process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// Frame magic does not identify this protocol version.
    InvalidMagic,
    /// A frame tag, reserved value, or response shape is invalid.
    InvalidFrame,
    /// Worker runtime configuration is invalid.
    InvalidConfig,
    /// Model size or digest is not the compiled identity.
    ModelRejected,
    /// PCM framing or sample values are invalid.
    InvalidAudio,
    /// Transcript bytes or metadata violate protocol bounds.
    InvalidTranscript,
    /// Response request identity differs from the outstanding request.
    RequestMismatch,
    /// Bounded memory could not be reserved.
    AllocationFailed,
    /// Anonymous-pipe input/output failed.
    IoFailed,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidMagic => "invalid ASR IPC magic",
            Self::InvalidFrame => "invalid ASR IPC frame",
            Self::InvalidConfig => "invalid ASR IPC configuration",
            Self::ModelRejected => "ASR IPC model rejected",
            Self::InvalidAudio => "invalid ASR IPC audio",
            Self::InvalidTranscript => "invalid ASR IPC transcript",
            Self::RequestMismatch => "ASR IPC request mismatch",
            Self::AllocationFailed => "ASR IPC allocation failed",
            Self::IoFailed => "ASR IPC I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_configuration_is_bounded() {
        assert_eq!(
            WorkerConfig::new(
                0,
                LanguageMode::Automatic,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(WorkerError::InvalidConfig)
        );
        assert!(WorkerConfig::new(
            8,
            LanguageMode::Fixed(Language::English),
            MIN_TIMEOUT,
            MAX_TIMEOUT,
        )
        .is_ok());
        assert_eq!(
            WorkerConfig::new(
                9,
                LanguageMode::Automatic,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(WorkerError::InvalidConfig)
        );
    }

    #[test]
    fn language_modes_have_stable_unique_wire_codes() {
        const LANGUAGES: [Language; 23] = [
            Language::English,
            Language::Hindi,
            Language::Punjabi,
            Language::Bengali,
            Language::Marathi,
            Language::Tamil,
            Language::Telugu,
            Language::Urdu,
            Language::Gujarati,
            Language::Kannada,
            Language::Malayalam,
            Language::Arabic,
            Language::German,
            Language::French,
            Language::Spanish,
            Language::Italian,
            Language::Portuguese,
            Language::Dutch,
            Language::Polish,
            Language::Turkish,
            Language::Indonesian,
            Language::Japanese,
            Language::Korean,
        ];

        assert_eq!(LanguageMode::from_wire(0), Some(LanguageMode::Automatic));
        for (index, language) in LANGUAGES.into_iter().enumerate() {
            let mode = LanguageMode::Fixed(language);
            assert_eq!(LanguageMode::from_wire(mode.wire_code()), Some(mode));
            assert_eq!(usize::from(mode.wire_code()), index + 1);
            assert_eq!(language.iso_639_1().len(), 2);
            assert_eq!(
                Language::from_iso_639_1(language.iso_639_1()),
                Some(language)
            );
        }
        assert_eq!(Language::from_iso_639_1("zz"), None);
        assert_eq!(Language::from_iso_639_1("EN"), None);
        assert_eq!(LanguageMode::from_wire(24), None);
        assert_eq!(LanguageMode::from_wire(u8::MAX), None);
    }

    #[test]
    fn cancellation_token_is_cloneable_and_one_shot() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn malformed_startup_is_rejected_before_allocation() {
        let mut frame = Vec::from(*b"BADMAGIC");
        frame.extend_from_slice(&[2, 1, 0, 0]);
        frame.extend_from_slice(&32_152_673_u64.to_le_bytes());
        assert!(matches!(
            worker_read_startup(&mut frame.as_slice()),
            Err(ProtocolError::InvalidMagic)
        ));
    }

    #[test]
    fn unknown_language_code_is_rejected_before_model_allocation() {
        let mut frame = Vec::from(STARTUP_MAGIC);
        frame.extend_from_slice(&[2, u8::MAX]);
        assert_eq!(
            worker_read_startup(&mut frame.as_slice()).map(|_| ()),
            Err(ProtocolError::InvalidConfig)
        );
    }

    #[test]
    fn request_parser_rejects_oversize_and_nonfinite_audio() {
        let mut oversize = vec![REQUEST_TRANSCRIBE];
        oversize.extend_from_slice(&1_u64.to_le_bytes());
        oversize.extend_from_slice(
            &u32::try_from(MAX_INFERENCE_SAMPLES + 1)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        assert!(matches!(
            worker_read_request(&mut oversize.as_slice()),
            Err(ProtocolError::InvalidAudio)
        ));

        let mut nonfinite = vec![REQUEST_TRANSCRIBE];
        nonfinite.extend_from_slice(&1_u64.to_le_bytes());
        nonfinite.extend_from_slice(&1_u32.to_le_bytes());
        nonfinite.extend_from_slice(&f32::NAN.to_le_bytes());
        assert!(matches!(
            worker_read_request(&mut nonfinite.as_slice()),
            Err(ProtocolError::InvalidAudio)
        ));
    }

    #[test]
    fn transcript_writer_rejects_noncontiguous_offsets() {
        let segments = [WorkerTranscriptSegment {
            byte_start: 1,
            byte_end: 2,
            start_ms: 0,
            end_ms: 10,
        }];
        assert_eq!(
            worker_write_transcript(&mut Vec::new(), 1, "ok", &segments),
            Err(ProtocolError::InvalidTranscript)
        );
    }

    #[test]
    fn transcript_frame_round_trips_unicode_and_timestamps() -> Result<(), Box<dyn Error>> {
        let text = " नमस्ते";
        let segments = [WorkerTranscriptSegment {
            byte_start: 0,
            byte_end: text.len(),
            start_ms: 20,
            end_ms: 740,
        }];
        let mut frame = Vec::new();
        assert!(worker_write_transcript(&mut frame, 41, text, &segments).is_ok());

        let decoded = read_response(&mut frame.as_slice(), 41)?;
        let WorkerResponse::Transcript(transcript) = decoded else {
            return Err("valid transcript frame was rejected".into());
        };
        assert_eq!(transcript.text(), text);
        assert_eq!(transcript.segments(), segments);
        Ok(())
    }
}
