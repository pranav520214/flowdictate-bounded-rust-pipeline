# Local ASR Runtime Boundary

Status: **Pinned CPU adapter and process-isolated deadline boundary implemented**  
Updated: 2026-08-27

## Adopted stack

| Component | Exact version | Role |
|---|---:|---|
| `flowdictate-asr-ipc` | 0.1.0 | Bounded parent/worker protocol and supervisor |
| `flowdictate-asr` | 0.1.0 | FlowDictate-owned validation and output boundary |
| `flowdictate-asr-worker` | 0.1.0 | Silent child process containing native inference |
| `whisper-rs` | 0.16.0 | Safe Rust wrapper, default features disabled |
| `whisper-rs-sys` | 0.15.0 | Native build and generated C bindings |
| `whisper.cpp` | 1.8.3 | Bundled CPU inference implementation |

No CUDA, Vulkan, HIP, SYCL, OpenBLAS, OpenMP, Metal, Core ML, Rust logging backend, model downloader, HTTP client, or FFmpeg feature is enabled.

## Data flow

```text
compiled manifest + approved root
  -> exact path/type/size/SHA-256/compatibility gate
  -> same already-hashed read handle
  -> exact-size bounded parent memory buffer
  -> bounded startup pipe transfer
  -> independent child SHA-256/compatibility verification
  -> whisper.cpp in-memory model loader inside child
  -> bounded request ID + mono 16 kHz PCM frame
  -> bounded UTF-8 transcript + segment offsets/timestamps
  -> parent validation or child kill/restart at deadline
```

The supervisor cannot be constructed from an arbitrary model path or byte slice. It requires `flowdictate_audio::VerifiedModel`, rewinds the retained same file handle, reads exactly the manifest length into bounded memory, and starts an explicitly selected regular non-reparse worker executable. On Windows, both the verified model and worker executable are opened without write/delete sharing during use. The child independently checks the compiled model identity, size, SHA-256, runtime, architecture, and quantization before calling the native in-memory loader; neither parent nor child reopens a model path.

## Runtime policy

- CPU only; 1–8 threads, default 2.
- Input is non-empty normalized finite `f32`, mono 16 kHz, at most 480,000 samples (30 seconds).
- Greedy `best_of=1`, temperature and temperature fallback set to zero.
- No initial prompt, prior context, translation, runtime VAD, debug output, progress output, or realtime output.
- Automatic language detection is enabled by default for the reviewed
  multilingual model. Callers may instead select one of 23 compiled ISO 639-1
  languages; no arbitrary language string is accepted.
- The version-two startup protocol carries one compact language code. Unknown
  codes and version-mismatched worker binaries fail closed before model
  allocation or native parsing.
- Fixed mode passes its static code to `whisper.cpp` and disables detection.
  Automatic mode passes no fixed code and enables local detection.
- Output is at most 128 segments and 65,536 UTF-8 bytes. Segment byte offsets refer into a single owned transcript string.
- Timestamps must be non-negative, ordered, and no later than 30 seconds.
- Errors contain fixed categories only; they do not format model paths, audio, transcripts, or native error payloads.
- Startup and inference deadlines are independently configurable from 10 ms through 120 seconds; defaults are 15 and 30 seconds.
- A cloneable one-shot cancellation token is polled every 10 ms while inference is in flight.
- Parent PCM copies, child request PCM, and owned transcript byte buffers are overwritten when their owners drop.
- The worker receives only a fixed hidden argument, inherits only `SystemRoot`, has null stderr, and uses stdout exclusively for the binary protocol.

## Wall-clock termination

The adapter deliberately does not install native callbacks. Review of the pinned wrapper did not establish its safe abort-callback ownership/type-erasure path as a trustworthy security boundary. A detached Rust thread would also not terminate a stuck C++ call and could leak model/audio state.

The native adapter therefore runs in a child process. Pipe I/O occurs on a helper thread while the owner enforces elapsed startup/inference deadlines and observes cancellation. On timeout or in-flight cancellation, the parent kills and reaps the child, joins the I/O helper only after termination is confirmed, and launches a clean replacement from the retained verified model bytes. If termination cannot be confirmed, the supervisor returns `RecoveryFailed` instead of blocking indefinitely or claiming recovery.

Malformed tags, lengths, request IDs, sample values, UTF-8, offsets, timestamps, or response counts fail closed before an unbounded allocation or transcript release. Protocol and pipe failures also force replacement. A fixed native runtime error is returned without recycling an otherwise healthy worker.

## Process-isolation validation rubric

- [x] Untrusted IPC tags, lengths, and non-finite/out-of-range samples are rejected before native inference or unbounded allocation.
- [x] The child independently re-hashes model bytes obtained from the parent's already-verified handle before native parsing; no model path is sent.
- [x] A real-model release test forces a 10 ms inference deadline, receives `InferenceTimedOut`, and observes both a new process ID and generation 2 after recovery.
- [x] A second real-model release test cancels an in-flight maximum-length request, receives `Cancelled`, and observes a new process ID and generation 2.
- [x] A real-model release test changes automatic mode to fixed English,
  observes a distinct process ID and generation 2, successfully transcribes,
  then proves reapplying English does not restart the worker.
- [x] The complete automatic/fixed wire-code allowlist round-trips, and unknown
  codes fail before model allocation.
- [x] Transcript responses enforce request identity, valid UTF-8, contiguous byte offsets, ordered bounded timestamps, 64 KiB text, and 128 segments.
- [x] Direct no-argument worker execution exits 1 with no output; release import inspection finds no named networking DLL; acceptance tests leave no worker process running.

## Windows build prerequisite

`whisper-rs-sys` generates Windows ABI bindings at build time and compiles bundled C++ with CMake/MSVC. The crate's bundled fallback bindings were rejected because a Windows compile showed Linux ABI layout assertions. This host used Visual Studio 2022 Community 17.14.37, CMake 3.29.2, and workspace-local libclang 21.1.8 from the public NuGet `libclang.runtime.win-x64` package.

The local tool is under ignored `.tools` and is not a product dependency or shipped runtime. Its package SHA-256 is `1296aa72d506a3511e3f509f4966365133af9c935d301a63ec2242bd8c3180ce`; the extracted `libclang.dll` SHA-256 is `d76c1552176563458e4b26819658734393de4afb616f81dbb2bdc65b52716067`.

## Verified evidence

- Ordinary workspace suite: 98 integration/unit tests passed; six
  reviewed-model/process tests are ignored by default.
- Reviewed-model ASR smoke: production integrity gate, same-handle runtime initialization, and one second of synthetic-silence inference passed in debug and optimized release builds.
- Process acceptance: one-second synthetic-silence round trip, forced deadline
  kill/restart, explicit in-flight cancellation/restart, and automatic-to-fixed
  language replacement passed against the real worker in optimized release.
- Release worker imports Windows core/runtime DLLs only; no named Winsock, DNSAPI, WinHTTP, WinINet, or URLMon DLL.
- RustSec audit reports no finding across the 136-entry lockfile.
- Cargo-deny passes advisories, bans, licenses, and sources with an explicit build-only duplicate exception for `shlex` 1.3.0.

This evidence establishes the implemented Windows parent deadline and recovery path under the acceptance fixture. It does not establish speech accuracy, live microphone integration, adversarial native-parser safety, packaged runtime socket behavior, executable-signing authenticity, OS sandboxing, low-end latency/memory, or cross-platform process semantics.
