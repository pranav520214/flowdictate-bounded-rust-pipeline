# Milestone 1 Audio Security Report

Status: **Audio, bounded session/utterance orchestration, model gate, and process-isolated ASR implemented; platform composition remains open**  
Updated: 2026-08-27

## Outcome

The implementation slice now contains four narrow Rust libraries and two development/runtime tools with no UI, persistence, network client, telemetry, updater, cloud fallback, or broad media parser. The audio crate converts native microphone PCM into event-aligned canonical VAD frames. The live-session owner gates a paused capture behind explicit start, performs bounded ring draining and epoch resets, and owns hotkey stop/cancel cleanup. The volatile pipeline owns bounded confirmation pre-roll and one utterance, dispatches finalized audio directly to the ASR child, and erases it on completion, error, discontinuity, or cancellation. The IPC supervisor independently verifies model bytes, validates responses, and enforces wall-clock deadline/cancellation kill-and-restart recovery.

## Changed files

```text
.gitignore
Cargo.toml
Cargo.lock
README.md
crates/flowdictate-asr/Cargo.toml
crates/flowdictate-asr/src/lib.rs
crates/flowdictate-asr/tests/reviewed_model_smoke.rs
crates/flowdictate-asr/flowdictate-asr.cdx.json
crates/flowdictate-asr-ipc/Cargo.toml
crates/flowdictate-asr-ipc/src/lib.rs
crates/flowdictate-asr-ipc/flowdictate-asr-ipc.cdx.json
crates/flowdictate-audio/Cargo.toml
crates/flowdictate-audio/src/lib.rs
crates/flowdictate-audio/src/cpal_backend.rs
crates/flowdictate-audio/src/model_security.rs
crates/flowdictate-audio/tests/audio_format.rs
crates/flowdictate-audio/tests/audio_processor.rs
crates/flowdictate-audio/tests/audio_processor_allocation.rs
crates/flowdictate-audio/tests/bounded_handoff.rs
crates/flowdictate-audio/tests/callback_allocation.rs
crates/flowdictate-audio/tests/capture_plan.rs
crates/flowdictate-audio/tests/codec_policy.rs
crates/flowdictate-audio/tests/normalization.rs
crates/flowdictate-audio/tests/resampling.rs
crates/flowdictate-audio/tests/synthetic_pipeline.rs
crates/flowdictate-audio/tests/vad.rs
crates/flowdictate-audio/tests/vad_stress.rs
crates/flowdictate-audio/tests/model_security.rs
crates/flowdictate-audio/flowdictate-audio.cdx.json
crates/flowdictate-pipeline/Cargo.toml
crates/flowdictate-pipeline/src/lib.rs
crates/flowdictate-pipeline/src/session.rs
crates/flowdictate-pipeline/flowdictate-pipeline.cdx.json
deny.toml
docs/DEPENDENCY_AUDIT_2026-08-26.md
docs/AUDIO_PIPELINE.md
docs/ASR_RUNTIME.md
docs/DEPENDENCIES.md
docs/MODEL_SECURITY.md
docs/PIPELINE_RUNTIME.md
docs/SESSION_RUNTIME.md
docs/MILESTONE_1_REPORT.md
docs/SECURITY_TESTING.md
models/whisper-tiny-q5_1/MODEL_ORIGIN.md
tools/flowdictate-capture-probe/Cargo.toml
tools/flowdictate-capture-probe/src/lib.rs
tools/flowdictate-capture-probe/src/main.rs
tools/flowdictate-capture-probe/tests/probe_contract.rs
tools/flowdictate-capture-probe/flowdictate-capture-probe.cdx.json
tools/flowdictate-asr-worker/Cargo.toml
tools/flowdictate-asr-worker/src/main.rs
tools/flowdictate-asr-worker/tests/process_boundary.rs
tools/flowdictate-asr-worker/flowdictate-asr-worker.cdx.json
```

There is no Git repository metadata in the workspace, so no commit hash or Git diff can be reported. The paths above are the local review surface.

## Security-relevant implementation

- Capture accepts only validated 8–192 kHz mono/stereo PCM and the CPAL adapter prefers reviewed rates, falling back only within that bounded rate range and reviewed sample types. The local host advertised 192 kHz only; the consented probe reached CPAL stream construction but that profile was rejected there, so no successful stream started and no audio report was emitted.
- The default stream is built paused. Listening requires an explicit `resume` call; there is no fallback source.
- The callback performs numeric conversion, one bounded ring write attempt, and atomic counter updates only.
- Ring capacity is capped at two seconds of negotiated interleaved PCM. A full ring drops the complete incoming batch and increments a discontinuity epoch.
- Worker buffers and DSP/VAD state are allocated during construction. Downmixing sanitizes non-finite/out-of-range values, resampling runs outside the callback, and RMS/peak values come from the real bounded waveform.
- Earshot receives only exact 256-sample mono 16 kHz frames. The utterance state machine has validated thresholds, final-silence boundaries, and a hard active-frame limit.
- Every completed VAD event is paired with its exact 256 canonical samples. The pipeline retains only bounded start-confirmation pre-roll and one preallocated utterance that must fit the 480,000-sample ASR limit.
- Silence and maximum duration dispatch automatically; hotkey release/explicit stop include the unpadded incomplete canonical tail. Discontinuity discards the affected utterance rather than joining speech across missing audio.
- Discontinuities reset interpolation/detector history and finalize active segmentation so speech is never joined across missing audio.
- Codec policy permits only volatile `f32` PCM for live inference. WAV is limited to future bounded regression fixtures; FLAC/Opus exports are disabled and MP3/AAC/video containers are excluded.
- The model gate validates only compiled release identities, approved-root containment, exact filename, regular-file/reparse-point policy, exact size, streaming SHA-256, and adapter compatibility. It returns the already-hashed read handle. The parent rereads that same handle into an exact-size buffer, and the child independently re-verifies the bytes against the compiled manifest before native parsing; no model path crosses IPC or is reopened.
- The separate ASR crate exact-pins `whisper-rs` 0.16.0 with default features disabled; its resolved native package bundles `whisper.cpp` 1.8.3. Model loading uses only the same verified handle via the in-memory API, with CPU/GPU policy fixed to CPU-only and native logs suppressed.
- Canonical inference input is non-empty finite mono 16 kHz `f32`, limited to 480,000 samples/30 seconds. Configuration permits 1–8 threads. Greedy temperature-zero decoding has no prompt, prior context, translation, internal VAD, debug output, or runtime callback.
- Transcript extraction is capped at 128 segments and 65,536 UTF-8 bytes; timestamps must be non-negative, ordered, and within the input-window limit. Adapter errors contain fixed categories only.
- Native inference is confined to `flowdictate-asr-worker` behind a dependency-light binary protocol. Startup/request tags and sizes, samples, request IDs, transcript UTF-8, contiguous offsets, timestamps, byte count, and segment count all fail closed.
- The parent enforces separate bounded startup/inference deadlines. On timeout or protocol failure it kills and reaps the child and starts a clean generation; failure to confirm termination returns `RecoveryFailed` instead of blocking. A release test forces a 10 ms timeout and proves replacement using a changed process ID and generation 2.
- A cloneable one-shot token cancels before dispatch without IPC or polls every 10 ms during inference. The real-model release test cancels in flight and proves child replacement using a changed process ID and generation 2.
- Parent and child PCM buffers and owned transcript byte buffers are overwritten on drop. The worker has a fixed hidden argument, cleared environment except `SystemRoot`, null stderr, binary-only stdout, and no direct model-path or network capability in application code.
- Production code denies unsafe Rust, panics, `unwrap`, `expect`, debug/print macros, and undocumented public API warnings through workspace lint policy. Test-only allocator shims contain the only local unsafe allowance.
- The human probe requires the exact `I CONSENT` phrase before opening CPAL, caps the run at 30 seconds, drains/zeroes caller-owned PCM storage, and prints only a fixed numeric report.

## Verification

Passed locally on Windows x86-64:

```text
rustc 1.97.1 (8bab26f4f 2026-07-14), host x86_64-pc-windows-msvc
cargo 1.97.1 (c980f4866 2026-06-30), host x86_64-pc-windows-msvc
```

```text
cargo fmt --all --check
PASS

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS

cargo test --workspace --all-targets --locked --offline
PASS — 58 integration/unit tests, 0 failed; 5 reviewed-model/process tests ignored by default

cargo test --workspace --doc --locked --offline
PASS — 0 documentation tests, 0 failed

cargo test -p flowdictate-audio --test model_security
downloaded_reviewed_model_matches_the_compiled_manifest -- --ignored
PASS — pinned local artifact accepted by the production compiled manifest gate

cargo test -p flowdictate-asr --test reviewed_model_smoke -- --ignored
PASS — verified-handle initialization and one-second synthetic-silence inference; debug 1.99 s, optimized release 1.42 s

cargo test -p flowdictate-asr-worker --test process_boundary --release --locked --offline -- --ignored --test-threads=1
PASS — 3 tests: real worker round trip, forced 10 ms deadline kill/restart, and in-flight cancellation/restart; 1.88 s latest rerun

cargo tree --workspace --locked -d
REVIEWED — build-only shlex 1.3.0/2.0.1 duplicate from bindgen and cc/CMake; exact cargo-deny exception recorded

resolved dependency-name scan for reqwest, hyper, ureq, curl, websocket,
telemetry, analytics, sentry, and opentelemetry
PASS — no matches

cargo-audit 0.22.2 audit --no-fetch --deny warnings
PASS — 1,226 RustSec advisories loaded; no findings

cargo-deny 0.20.2 --offline check advisories bans licenses sources
PASS — 0 errors and 0 warnings in every policy

cargo-cyclonedx 0.5.9, CycloneDX JSON 1.5, strict licenses
PASS — audio SBOM: 38 components/39 dependency nodes
PASS — ASR IPC SBOM: 39 components/40 dependency nodes
PASS — ASR SBOM: 68 components/69 dependency nodes
PASS — pipeline SBOM: 40 components/41 dependency nodes
PASS — ASR worker SBOM: 69 components/70 dependency nodes
PASS — probe SBOM: 39 components/40 dependency nodes
PASS — generated absolute workstation paths removed; all six JSON documents parse
NOTICE — three transitive crates expose deprecated slash-form license metadata; cargo-deny accepted the underlying licenses

cargo build -p flowdictate-capture-probe --release --locked --offline
PASS

release PE import inspection
PASS (static evidence) — probe and optimized ASR worker import core/runtime DLLs only; no named network DLL

non-consent release probe execution
PASS — consent_denied, exit 1, microphone builder not reached

consented release probe execution
PASS (safe refusal) — capture_build:microphone stream build failed, exit 1; no audio report was emitted
```

Dependency evidence for this lockfile:

- 6 exact direct external dependencies: CPAL 0.18.2, Earshot 1.2.2, RTRB 0.4.0, Rubato 5.0.0, SHA-2 0.10.9, and whisper-rs 0.16.0.
- 72 packages resolve for `x86_64-pc-windows-msvc`; 136 entries exist in `Cargo.lock`. Of those, 66/130 respectively are registry packages with checksums; the six workspace packages are local. Process isolation and orchestration added no registry package.
- Resolved license metadata contains MIT/Apache-2.0-compatible expressions plus BSD-3-Clause, ISC, Unlicense, Unicode-3.0, 0BSD, and Zlib alternatives; no missing license field appeared in the Windows graph.
- The pinned audit tools live only under ignored `.tools`; they are not product dependencies or global PATH entries.
- The `cargo-cyclonedx` 0.5.9 tool release itself pins yanked `xml-rs` 0.8.19. This does not occur in the product graph but remains a low-severity tooling-chain caveat.

The recurring Cargo message `could not canonicalize path C:\Users\RYZEN` did not fail any command; its environment cause has not been diagnosed in this milestone.

## Unsupported claims

> Unverified — requires human testing.

This applies to actual microphone enumeration labels, Windows microphone permission UX, a stream that can start on the host's advertised 192 kHz profile, device removal or permission revocation mid-stream, callback scheduling under desktop load, acoustic VAD/ASR accuracy, Indian English/Hinglish quality, low-end-hardware latency and memory, hostile native-model/parser behavior, installed worker authenticity and OS sandboxing, runtime outbound socket/DNS behavior, cross-platform compilation/process behavior, and subjective voice UX.

FlowDictate is not yet a usable dictation product. The library-level capture/VAD-to-final-ASR seam, bounded rolling transcript consensus, and both final-only and streaming session owners exist, but there is no platform hotkey/event loop, overlay, text insertion adapter, encrypted persistence, packaged application, or release artifact.

## Next gate

The live-audio gate remains: a human must run the consent-first Windows probe on a microphone profile CPAL can start while observing outbound DNS/socket activity, then exercise permission denial, device loss, pause failure, and overload. Milestone 2 has since added a dedicated `StreamingLiveSession` that routes active canonical frames through bounded rolling inference and stable-prefix commitment, explicit language selection, a payload-minimal measurement core, and bounded approved-root fixture intake. Platform hotkey/event composition remains after reviewed benchmark results and the live-hardware gates. Packaging must authenticate the worker executable, add an OS-appropriate sandbox/resource policy, and preserve the no-network/no-cloud boundary.
