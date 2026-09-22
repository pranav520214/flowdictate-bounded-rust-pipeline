# Experimental Nemotron Native Runtime

Status: implemented and real-model tested on Windows x86-64; not selected by
the production live session.

## Boundary

`flowdictate-nemotron-ipc` supervises a dedicated native worker that calls the
pinned NeMo-Speech C ABI directly. The worker has no downloader, model search,
microphone, file output, logging, telemetry, HTTP, or RPC surface. Its model is
opened only after the parent transfers a verified immutable Windows path lease.
The worker uses the pinned runtime's low-latency RNNT geometry and destroys a
finished stream before allocating its replacement, preventing overlap of the
large per-stream native contexts across utterances.

The binary protocol is private, versioned, and bounded:

- startup: fixed magic/version and at most 32 KiB of UTF-8 model path;
- input: at most 2,560 normalized finite float32 samples per request;
- utterance: at most 480,000 samples, or 30 seconds at 16 kHz;
- output: at most 65,536 bytes of validated UTF-8 transcript text;
- commands: push, finish, shutdown, and numeric lifecycle statistics;
- responses: ready, no-result, result, payload-free error categories, or a
  fixed six-u64 lifecycle snapshot with matching request ID.

The parent clears the child environment except for `SystemRoot`, hides the
console, enforces request IDs and deadlines, and kills/relaunches the process
after cancellation or transport failure. Audio copies, transcript bytes, and
model-path bytes are overwritten when their owners are cleared or dropped.

## Compact streaming owner

`NativeStreamingInference` receives canonical mono 16 kHz PCM and combines ten
256-sample VAD frames into one 2,560-sample/160 ms native push. It holds the
native recognizer and stream across pushes, so it does not rebuild overlapping
PCM windows. It retains only the latest bounded partial, flushes an unpadded
tail, and requires a true final result. Invalid audio, cancellation, native
failure, or a missing final result erases local state and requests a clean
worker generation.

`ExperimentalNemotronPipeline` now connects this owner to the production
canonical DSP/VAD boundary. Confirmation pre-roll and each subsequent active
frame enter the persistent stream exactly once; finalized transcripts transfer
directly into caller-owned storage, while the latest partial remains borrowed.
Cancellation, discontinuity, invalid VAD state, and native failure erase the
bridge state. `ExperimentalNemotronLiveSession` now owns an already-paused
capture, bounded ring, fresh per-listen cancellation token, one-chunk drains,
bounded stop work, and caller-owned final output. It never resumes capture
unless a clean native generation is ready. This remains a library-only,
explicitly named experimental path; no app/UI selects it and production still
uses the existing Whisper worker.

## Application selection boundary

The [experimental ASR selection contract](EXPERIMENTAL_BACKEND_SELECTION.md)
now makes production Whisper the immutable default and exposes versioned,
fixed disclosure facts for a future non-technical UI. Declined or stale
acknowledgements cannot create the opaque opt-in required to select Nemotron;
unsupported platforms fail closed; withdrawal returns immediately to Whisper.
The policy is volatile and owns no capture capability, so experimental model
acceptance never grants the separate consent required to listen. No app/UI yet
renders or composes this boundary.

## Verified evidence

- The release-style MSVC build passes `/W4 /WX /permissive- /EHsc /utf-8`.
- Strict Rust Clippy passes across every target and feature with warnings denied.
- A reviewed 6.18-second FLEURS Hindi fixture crosses the model-integrity gate,
  Rust supervisor, isolated C++ worker, NeMo C ABI, and returns a final
  Devanagari hypothesis without printing transcript content.
- The complete offline workspace suite passes 223 ordinary tests; 16 explicit
  model/hardware acceptance tests remain ignored by default.
- The reviewed Hindi fixture also crosses canonical DSP, VAD confirmation,
  frame-by-frame native routing, explicit finalization, and caller-owned final
  transfer without printing transcript content.
- A continuously reused worker passes ten deterministic-noise fixture runs
  under the normal 30-second request deadline: 20/10 dB WER is 9.52%/20.23%,
  CER is 1.89%/8.83%, inference RTF is 0.3138/0.3104, and every hypothesis
  character is Devanagari. No transcript or augmented audio is emitted.
- The noise test first exposed timeout/IPC failure on the second utterance. A
  clean-process diagnostic localized the issue to stream reuse; explicit
  destroy-before-create ordering fixed the one-process matrix.
- Session tests cover explicit start, borrowed partials, caller-owned finals,
  bounded local/external cancellation cleanup, capacity atomicity,
  discontinuity reset, pause-failure capture destruction, and a failed worker
  reset proving capture is never resumed from an unready native generation.
- Import inspection found no WinHTTP, WinINet, Winsock, DNSAPI, or URLMon import
  in the worker executable or its five staged runtime DLLs.
- AddressSanitizer execution is not claimed: this Visual Studio installation is
  missing `clang_rt.asan_dynamic_runtime_thunk-x86_64.lib`. MSVC does not offer
  an equivalent UndefinedBehaviorSanitizer configuration here.

## Build

The [100-session soak report](NEMOTRON_SOAK_2026-09-05.md) records the latest
numeric lifecycle counters, deadline-bound statistics query, observed idle EOF
shutdown, 22 model-free boundary cases and repeated supervisor failure tests.
One real-model worker completed 100 synthetic utterances without restarts or
timeouts and with matched native stream counts. It also records exact ASan/
UBSan link blockers and the unresolved abrupt-parent Job Object containment gap.

The [2026-09-05 lifecycle regression report](NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
supersedes the earlier lifecycle/timing snapshot above: idle now owns no stream,
finish drains then destroys before acknowledging, and the next push creates the
replacement. Five native model-free ownership tests and eight production
supervisor tests pass. The final single-process ten-case matrix passed under
30 seconds per request, with identical accuracy and measured utterance-path
RTF 0.3342/0.3299. The report separates native-call/IPC time from outer-loop
overhead and documents coarse working-set and sanitizer limits.

The worker requires the separately pinned NeMo-Speech source/build directories:

```text
cmake -S native/nemotron-worker -B target/nemotron-worker-build -G Ninja
cmake --build target/nemotron-worker-build --parallel
```

The configured build records the exact source/build paths and copies only the
five required runtime DLLs beside the worker executable. No network access is
needed to rebuild or run after the reviewed sources, dependencies, and model
are present.
