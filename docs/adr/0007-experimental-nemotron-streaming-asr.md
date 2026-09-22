# ADR 0007: Gate an experimental native Nemotron streaming ASR adapter

- Status: Accepted for experimental development; prohibited as production default
- Date: 2026-09-01
- Owners: FlowDictate

## Context

The current production seam uses bounded overlapping Whisper inference followed
by consensus. On five reviewed FLEURS Hindi cases, Whisper tiny/base failed the
provisional 22% WER gate and emitted no Devanagari hypothesis characters.

NVIDIA Nemotron 3.5 ASR Streaming 0.6B is a cache-aware FastConformer-RNNT with
native 80–1120 ms streaming chunks. Its pinned Q8 GGUF and the pinned
NeMo-Speech.cpp CPU runtime were evaluated locally with the existing reviewed
Hindi fixtures. The 160 ms streaming probe achieved 11.90% WER, 3.15% CER,
0.337 steady-state corpus RTF, and exclusively Devanagari hypothesis
characters. Peak process working set was 976,093,184 bytes.

The model is much larger than the current artifacts: 741,548,352 bytes versus
32,152,673 bytes for Whisper tiny and 59,707,625 bytes for Whisper base. The
runtime's stable C API accepts a model path, so it reopens the artifact after
FlowDictate's verified-file check. The experimental Windows boundary now holds
the exact verification handle with read-only sharing while NeMo reopens the
canonical path, preventing write/delete replacement. Other platforms still
lack an equivalent proven lease and fail closed.

## Decision

Develop Nemotron only as an opt-in experimental adapter until every gate below
passes. Keep the production isolated Whisper worker and its model identity
unchanged.

The experimental data path is:

1. existing capture, canonical DSP, VAD, cancellation, and hard bounds;
2. a dedicated process-isolated Nemotron worker;
3. one persistent native streaming recognizer per active session;
4. 160 ms canonical PCM pushes producing bounded interim/final results;
5. a small monotonic finalization gate that reuses the existing borrowed
   pending and immutable commit interfaces;
6. immediate state erasure and worker replacement on cancellation, deadline,
   discontinuity, malformed output, or native failure.

The dedicated C++ C-ABI worker, Rust process supervisor, and compact 160 ms
owner are now implemented on Windows. A reviewed Hindi fixture passes the full
process boundary. The owner remains separate from `StreamingLiveSession`, so
this evidence does not change production selection or close the gates below.

Production selection is prohibited until:

- non-Windows model loading consumes already-verified bytes/a handle or gains
  an equivalent tested immutable-path protocol; Windows uses the implemented
  retained-handle lease;
- transcript, word, timestamp, language-code, allocation, and work bounds are
  enforced before results enter the pipeline;
- the worker is demonstrably network-free and cannot trigger CLI model pulls;
- redistribution includes OpenMDW 1.1, Apache-2.0, and all required third-party
  notices after legal review;
- baseline hardware meets approved cold/warm latency and memory budgets;
- broader Hindi/Hinglish, conversational, accent, noise, and native-speaker
  acceptance evidence passes;
- live partial/final behavior passes the existing monotonicity and focus-safe
  human acceptance checks.

## Consequences

Positive:

- Native streaming state can replace repeated overlapping full-window decode
  and most consensus work.
- The initial reviewed Hindi evidence passes the provisional quality gate and
  produces native Devanagari script.
- CPU inference is faster than real time on the current development machine.

Negative:

- The model adds about 742 MB and measured process memory is about 976 MB.
- NeMo-Speech.cpp 0.1.0 is young and adds a C++/GGML/SentencePiece boundary.
- Windows CPU builds currently pull Abseil and Protobuf beneath SentencePiece,
  increasing build and notice complexity.
- Path-only loading requires a platform-proven lease; only Windows has one now.

## Rejected alternatives

- Replace Whisper immediately: rejected because integrity, baseline hardware,
  broader quality, licensing, and human acceptance gates remain open.
- Keep escalating Whisper size: rejected by the five-case base comparison,
  which remained above 100% WER and produced no Devanagari characters.
- Use the NeMo CLI in production: rejected because it exposes model download
  behavior and a broader argument/file surface than the required C ABI.

## Verification evidence

- Model revision: `1c8deaecc64b91f034d73e08dd8b64625eb3395d`
- Model SHA-256: `a5c435f294eea8f88ce68dd27b8c3bfea7f777cb2fbba04fcd30eaa555f429ae`
- Runtime revision: `4f9676226f667d14608487df744f375db87127f8`
- vcpkg baseline: `9e593bb18ea69cc5095e012465dcd675a822ed0d`
- Numeric probe: `crates/flowdictate-asr/tests/nemotron_streaming_probe.rs`
- Model record: `models/nemotron-3.5-asr-streaming-0.6b-q8_0/MODEL_ORIGIN.md`
- Benchmark record: `docs/HINDI_BENCHMARK_RESULTS_2026-09-01.md`
- Windows immutable-path mutation test:
  `crates/flowdictate-audio/tests/model_security.rs`
- Native worker: `native/nemotron-worker/main.cpp`
- Process supervisor: `crates/flowdictate-nemotron-ipc/src/lib.rs`
- Compact streaming owner:
  `crates/flowdictate-pipeline/src/native_streaming.rs`
- Versioned application selection policy:
  `crates/flowdictate-pipeline/src/backend_selection.rs`
- Real-fixture process test:
  `crates/flowdictate-nemotron-ipc/tests/native_process.rs`
