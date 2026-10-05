# Milestone 2 Local Streaming ASR Report

Status: **In progress — bounded Nemotron native pipeline is real-model tested; app UI and acceptance gates remain**  
Date: 2026-09-04

## Current requirement map

| Original requirement | Status |
|---|---|
| `whisper.cpp` integration | Implemented and real-model tested |
| Model integrity gate | Implemented and corruption/identity tested |
| Streaming inference | Active canonical frames route through bounded pre-roll, rolling PCM inference, and final unpadded-tail decode |
| Partial hypotheses | Exposed as a bounded borrowed view with numeric update notification; no internal payload queue |
| Consensus commit | Immutable monotonic deltas append to caller-owned preallocated session output |
| Bounded decoding window | Implemented at 480,000 mono 16 kHz samples / 30 seconds |
| Multilingual selection | Automatic detection plus 23 allowlisted fixed languages implemented through versioned parent/worker IPC |
| Stability/latency measurement | Payload-minimal bounded recorder and per-inference sample/time counters implemented; six-case p50/p95/corpus-RTF evidence recorded |
| Recognition scoring | Work-limited numeric-only exact WER/CER plus explicit bounded LibriSpeech English and FLEURS Hindi normalization implemented; English six-case tiny WER 7.07%, Hindi five-case fixed-mode tiny/base WER 188.09%/101.19%, zero Devanagari hypothesis characters, and empty 100% deletion-only automatic-mode results for both models |
| Benchmark fixture intake | Strict approved-root manifest, exact hash, UTF-8 transcript, canonical PCM16/float32 WAV, reviewed voice-rights basis, and explicit speech/acoustic/language-mix/accent-evidence strata implemented; six checksum-pinned CC BY 4.0 LibriSpeech fixtures and five commit-pinned CC BY 4.0 FLEURS Hindi read-speech fixtures admitted |
| Fixture case execution | Verified audio is language-gated, canonicalized, timed, and reduced to numeric WER/CER; six reviewed English cases, five reviewed Hindi cases, and a same-adapter tiny/base Hindi comparison completed |
| Native streaming candidate | Pinned Nemotron Q8 plus pinned NeMo-Speech.cpp CPU runtime evaluated at 160 ms chunks; five-case Hindi WER 11.90%, CER 3.15%, steady-state RTF 0.3368, all-Devanagari hypothesis, and 976,093,184-byte peak working set |
| Native process isolation | Dedicated bounded C++ C-ABI worker plus Rust deadline/cancellation supervisor implemented; reviewed Hindi fixture passes the full verified-model process boundary |
| Deterministic noise robustness | Five reviewed Hindi cases pass volatile 20/10 dB white-noise runs at 9.52%/20.23% WER, 1.89%/8.83% CER, and 0.3138/0.3104 inference RTF with all-Devanagari output; no augmented audio or transcript is emitted |
| Compact native owner | Ten canonical 16 ms frames compact to one non-overlapping 160 ms native push; latest-only partial retention, unpadded final tail, and fail-closed reset are tested |
| Experimental DSP/VAD bridge | Confirmation pre-roll and each subsequent active canonical frame feed the persistent native stream exactly once; partial text remains borrowed and final text transfers to caller-owned storage |
| Experimental session owner | Already-paused capture, bounded ring, explicit start, fresh cancellation, discontinuity recovery, bounded stop, borrowed partial, caller-owned final, and fail-before-resume native reset are implemented without changing the production default |

## Changed files for this slice

```text
crates/flowdictate-pipeline/src/consensus.rs
crates/flowdictate-pipeline/src/benchmark.rs
crates/flowdictate-pipeline/src/fixture_benchmark.rs
crates/flowdictate-pipeline/src/rolling.rs
crates/flowdictate-pipeline/src/streaming.rs
crates/flowdictate-pipeline/src/streaming_session.rs
crates/flowdictate-pipeline/src/lib.rs
crates/flowdictate-audio/src/fixture_intake.rs
crates/flowdictate-audio/src/lib.rs
crates/flowdictate-audio/tests/fixture_intake.rs
crates/flowdictate-audio/tests/reviewed_public_fixture.rs
crates/flowdictate-pipeline/tests/fixture_benchmark.rs
crates/flowdictate-pipeline/tests/nemotron_noise_matrix.rs
crates/flowdictate-asr-ipc/src/lib.rs
crates/flowdictate-asr/src/lib.rs
crates/flowdictate-asr/tests/reviewed_model_comparison.rs
crates/flowdictate-nemotron-ipc/Cargo.toml
crates/flowdictate-nemotron-ipc/src/lib.rs
crates/flowdictate-nemotron-ipc/tests/native_process.rs
native/nemotron-worker/CMakeLists.txt
native/nemotron-worker/main.cpp
crates/flowdictate-pipeline/src/native_streaming.rs
crates/flowdictate-pipeline/src/native_pipeline.rs
crates/flowdictate-pipeline/src/native_session.rs
crates/flowdictate-pipeline/tests/nemotron_native_pipeline.rs
models/whisper-base-q5_1/MODEL_ORIGIN.md
tools/flowdictate-asr-worker/src/main.rs
tools/flowdictate-asr-worker/tests/process_boundary.rs
README.md
docs/AUDIO_PIPELINE.md
docs/ROLLING_ASR_RUNTIME.md
docs/PIPELINE_RUNTIME.md
docs/SESSION_RUNTIME.md
docs/MILESTONE_1_REPORT.md
docs/MILESTONE_STATUS.md
docs/MILESTONE_2_REPORT.md
docs/SECURITY_TESTING.md
docs/BENCHMARK_RUNTIME.md
docs/MODEL_BENCHMARKS.md
docs/NEMOTRON_NATIVE_RUNTIME.md
docs/PERFORMANCE.md
benches/README.md
benches/fixtures.template.csv
benches/fixtures.csv
benches/fixtures/librispeech-dev-clean-*.wav (6 files)
benches/fixtures/librispeech-dev-clean-*.txt (6 files)
benches/FIXTURE_PROVENANCE.md
docs/FIXTURE_SOURCE_REVIEW.md
docs/FIXTURE_SET_EXPANSION_REVIEW.md
docs/BENCHMARK_RESULTS_2026-08-28.md
docs/HINDI_NOISE_BENCHMARK_RESULTS_2026-09-02.md
```

## Security and privacy behavior

- The engine accepts a narrow borrowed transcript-hypothesis trait; the existing
  validated `WorkerTranscript` implements it without exposing IPC internals.
- Hypotheses are capped at the existing 128 segments and 65,536 UTF-8 bytes.
- Relative timestamps are converted to checked absolute timestamps and must be
  ordered, non-overlapping, within observed audio, and outside the immutable
  committed boundary.
- Only complete segments repeated across 2–8 observations can commit. A bounded
  trailing time margin prevents recent unstable text from committing.
- Comparison normalizes whitespace only and preserves the latest exact display
  text. It does not rewrite words or meaning.
- Committed text is emitted once as a monotonic generation/sequence delta and is
  not retained by the engine. Only the bounded uncommitted suffix remains.
- The engine reports the timestamp before which audio may be discarded while
  retaining a configured overlap.
- The rolling owner reserves its complete PCM capacity at construction, accepts
  only canonical-frame multiples, and runs at most one inference per push.
- It stops atomically at the configured/30-second hard window boundary rather
  than silently dropping uncommitted audio.
- Consensus commits drive timestamp-based PCM erasure; the configured overlap
  remains available to subsequent rolling windows.
- Cancellation, discontinuity, malformed PCM, ASR failure, invalid hypotheses,
  and time overflow erase volatile PCM and pending hypothesis state.
- Consensus/trim failure rolls back any output appended during that call.
- `StreamingDictationPipeline` retains only bounded confirmation pre-roll before
  speech starts, then routes exact active frames directly to rolling inference.
- `StreamingLiveSession` preserves explicit start, stale-ring erasure, one-chunk
  drains, bounded stop work, fresh one-shot tokens, and fail-closed capture
  lifecycle behavior without an internal transcript event queue.
- Pending text is borrowed; only immutable commits cross into caller-owned
  preallocated storage. Reports contain flags/counters rather than payloads.
- Hotkey release and explicit stop include the unpadded canonical tail in the
  final local inference.
- A final hypothesis may replace only the uncommitted suffix; committed text is
  filtered or a boundary-straddling segment fails closed.
- Pending and committed owned UTF-8 byte buffers are overwritten on reset/drop.
- Errors contain fixed categories and no transcript payload.
- The base comparison artifact is exact-size/SHA-256 gated and accepted only
  by the ignored direct-adapter comparison harness. The production isolated
  worker retains its single tiny-model identity.
- Language selection is a compact enum: automatic or one of 23 reviewed ISO
  639-1 codes. Arbitrary strings never cross the process boundary.
- The startup protocol was versioned rather than reinterpreting the previous
  boolean field; mismatched or unknown language values fail closed before model
  allocation/native parsing.
- Automatic mode enables local model detection. Fixed mode disables detection
  and supplies one static allowlisted code to `whisper.cpp`.
- A changed language is accepted by `StreamingLiveSession` only while idle.
  Native scratch, queued audio, DSP/VAD pre-roll, rolling PCM, and pending text
  are erased before a clean worker and consensus generation is established.
- Reapplying the current language is a no-op. A failed worker replacement
  attempts a clean rollback generation and exposes only a fixed error category.
- Rolling reports expose only inference window size and wall time; streaming
  reports aggregate those numeric values without transcript/audio payloads.
- The benchmark recorder preallocates its bounds, retains only one previous
  partial, overwrites it on replacement/finish/drop, and returns numeric-only
  stability, percentile, RTF, divergence, and commit-order metrics.
- The recognition scorer borrows reviewed reference/final text, returns only
  numeric edit and WER/CER counters, uses Unicode-scalar CER, overwrites its
  temporary scalar buffers, and caps each edit comparison at four million cells.
- Recognition summaries also count Devanagari-block and ASCII-Latin scalars so
  script mismatch can be diagnosed without retaining or emitting text.
- The fixture runner consumes verified fixture ownership, applies only the
  compiled automatic/fixed language policy, downmixes/resamples through the
  production DSP seam, invokes the local backend once, and returns numeric-only
  source/canonical shape, inference timing/RTF, and recognition metrics.
- Pre-cancelled cases return before changing backend language or processing
  audio. Canonical PCM, final hypothesis, and fixture audio/reference are
  overwritten by their owners before the numeric result returns.
- Fixture intake accepts only the exact bounded review schema, approved public/
  synthetic records, single-component names, exact hashes, bounded UTF-8 text,
  and canonical PCM16/finite-float32 WAV beneath a non-reparse approved root.
- Provenance URLs remain inert and are never fetched. No downloader, broad media
  decoder, private recording, fabricated provenance, dependency, network,
  logging, telemetry, model, or microphone capability was added.

## Focused verification

Sixty-three deterministic tests cover benchmark bounds/math/privacy behavior,
bounded multilingual WER/CER accounting and work limits,
end-to-end fixture language/canonicalization/backend/scoring behavior,
strict manifest/file/hash/transcript/WAV intake,
language wire-code allowlisting and
native configuration, consensus configuration and hypothesis
bounds, stable-prefix monotonicity, changed suffixes, unstable-tail delay,
comparison-safe whitespace, final suffix revision, rolling cadence, overlap and
no-overlap PCM trimming, exact active-frame routing, bounded confirmation
pre-roll, borrowed pending hypotheses, immutable live commits, unpadded-tail
finalization, stale-ring/fresh-token lifecycle, hard limits, capture pause
failure, cancellation/discontinuity/reset cleanup, committed-boundary failure,
and capacity/timestamp atomicity.

The complete workspace verification and dependency evidence are refreshed at
the end of this slice. Real partial stability and latency remain unverified
until the live path is exercised with reviewed fixtures and target hardware.

## Verification

```text
cargo fmt --all --check
PASS

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS

cargo test --workspace --all-targets --locked --offline
PASS — 205 ordinary tests, 0 failed; 15 reviewed-model/process tests ignored by default

cargo test -p flowdictate-nemotron-ipc --test native_process
  --locked --offline -- --ignored --nocapture
PASS — reviewed 6.18-second Hindi fixture crossed the immutable model lease,
Rust supervisor, isolated C++ worker, and NeMo C ABI; final output contained
Devanagari and transcript content was not printed

cargo test -p flowdictate-pipeline --test nemotron_native_pipeline
  --locked --offline -- --ignored --nocapture
PASS — the same reviewed fixture additionally crossed canonical DSP, VAD,
non-overlapping frame routing, native finalization, and caller-owned final
transfer; transcript content was not printed

MSVC native worker build with /W4 /WX /permissive- /EHsc /utf-8
PASS

PE import scan of worker executable and five staged runtime DLLs
PASS — no WinHTTP, WinINet, Winsock, DNSAPI, or URLMon import found

MSVC AddressSanitizer configure
NOT RUN — installed toolchain lacks
clang_rt.asan_dynamic_runtime_thunk-x86_64.lib; no sanitizer claim is made

cargo test --workspace --doc --locked --offline
PASS — 0 documentation tests, 0 failed

cargo test -p flowdictate-asr-worker --test process_boundary --release
  --locked --offline -- --ignored --test-threads=1
PASS — 5 real-model process/fixture tests in 11.75 s; 0 orphan workers

cargo test -p flowdictate-asr --test reviewed_model_comparison --release
  --offline -- --ignored --nocapture --test-threads=1
PASS — identical five-case direct-adapter comparison; tiny/base WER
188.09%/101.19%, corpus RTF 0.1973/0.3890

The same harness also tested automatic language detection for both candidates.
Both returned zero hypothesis characters across the set and exactly 100%
deletion-only WER/CER, ruling out automatic mode as a script-mismatch remedy.

cargo test -p flowdictate-asr --test nemotron_streaming_probe
  -- --ignored --nocapture
PASS — pinned five-case 160 ms native-streaming probe; 11.90% WER, 3.15%
CER, all 314 hypothesis characters in Devanagari, and end-to-end RTF 0.3862

cargo-audit 0.22.2 audit --no-fetch --deny warnings
PASS — 1,226 advisories loaded; no findings across 138 lock entries

cargo-deny 0.20.2 --offline check advisories bans licenses sources
PASS

Markdown relative-link scan
PASS — 0 broken local links

SBOM parse and workstation-path scan
PASS — all eight current SBOMs parse; no C:/Users, RYZEN, or file:/// value
```

The pipeline manifest declares the already-resolved `sha2` package as a
test-only direct dependency for synthetic manifest construction. That change
added no registry package. With the current local Nemotron IPC and refinement
members, the Windows graph is 74 packages (eight workspace and the same 66
checksum-pinned registry packages); eight package SBOMs are retained.

The compiled model gate now includes the exact experimental Nemotron identity.
On Windows it can transfer the retained read-only-sharing verification handle
into an immutable path lease, which prevents write/delete replacement while a
reviewed path-only native runtime reopens the canonical file. Other platforms
fail closed until an equivalent mechanism exists. The ignored probe holds this
lease through NeMo model loading, stages only the five already verified public
Hindi WAVs, reduces transcripts to numeric metrics, overwrites in-memory text,
and removes its unique ignored target directory on drop. The production worker
identity and selection remain unchanged.

The application-facing selection policy now defaults to production Whisper and
requires a current-version, explicit acknowledgement before it can select the
experimental Hindi Nemotron backend. Its fixed disclosure includes local-only
processing, Hindi scope, exact model size, measured memory, platform
availability, and the fact that model acceptance never authorizes microphone
listening. Declines, stale versions, and unsupported platforms fail closed;
withdrawal is immediate and idempotent. The boundary is volatile and stores no
identity or timestamp. No application UI yet renders or composes it.

The fixture manifest now requires reviewed speaking style, acoustic condition,
language composition, accent-evidence level, and voice-rights basis. Cross-field
rules reject synthetic fixtures that claim a natural voice, public voice
fixtures without reviewed corpus rights, monolingual cases with multiple
language tags, and code-switched cases with fewer than two. Numeric coverage
queries prove the current repository contains 11 read-speech cases, zero
conversational/spontaneous cases, zero code-switched cases, and zero reviewed
accent cases; six LibriSpeech cases are clean and five FLEURS cases remain
acoustically uncharacterized.

The benchmark runner now supports a deterministic synthetic-white-noise
scenario at allowlisted 0/10/20/30/40 dB target SNRs. Noise is generated twice
from one fixed seed to normalize and mix it without retaining a second audio
buffer, exists only in the already bounded canonical PCM owner, and is erased
on drop. Invalid settings and silent inputs fail closed; cancellation is checked
during bounded noise work. Synthetic evidence proves reproducibility and a
measured 20 dB mix without clipping.

The reviewed five-case Hindi matrix now passes the isolated Nemotron worker at
20 dB with 9.52% WER, 1.89% CER, and 0.3138 inference RTF, and at 10 dB with
20.23% WER, 8.83% CER, and 0.3104 inference RTF. Every counted hypothesis
character is Devanagari. The fixed-seed perturbation, PCM, and hypotheses remain
volatile; output now contains only numeric aggregates, without per-case progress.

The first reused-process run failed closed with a timeout, and a diagnostic
120-second ceiling returned an IPC failure on numeric fixture index 1. A clean
process between cases passed, localizing the fault to native stream reuse. The
adapter now releases the completed stream before constructing its replacement
and uses the pinned low-latency RNNT context. The final continuously reused
worker passes the complete matrix under the normal 30-second request guard.

The offline workspace suite now passes 223 ordinary tests with 16 explicit
model/hardware acceptance tests ignored by default. Strict workspace Clippy
also passes with warnings denied.

## Next implementation slice

The [100-session soak and native recovery slice](NEMOTRON_SOAK_2026-09-05.md)
is complete within its bounded scope. One production native worker completed
100 synthetic utterances with balanced streams, no restarts/crashes/timeouts,
and observed clean exit. The report preserves exact resource/timing evidence,
model-free fault coverage, sanitizer blockers and abrupt-parent containment risk.
It does not replace the remaining hardware, speech-quality and human gates.

The [bounded lifecycle regression slice](NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
is now complete. The owner enforces finish/drain/destroy/idle before the next
push creates a stream; five native ownership tests and eight model-free actual
supervisor tests cover repeated use and failure recovery. The final rebuilt
30-second one-process matrix passed ten cases with no crashes/timeouts and
unchanged accuracy; latest utterance-path RTF was 0.3342/0.3299. See the report
for precise timings, coarse memory evidence, and unresolved sanitizer limits.
The earlier diagnostic failure and timings above remain historical evidence.

Admit rights-clear immutable conversational, accent, real acoustic-noise, and
Hinglish sources without weakening the reviewed fixture boundary, then run the
same numeric-only native evaluation. Separately perform the consented baseline/live-hardware
acceptance checks. Rendering the versioned selection contract and connecting it
to an application shell remains future work. No broad Hindi, code-switching,
RTL, or native-speaker claim is made before those gates.
