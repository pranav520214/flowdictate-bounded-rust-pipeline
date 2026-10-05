# Security and Privacy Test Strategy

Status: **Strategy active; audio, ASR, and bounded deterministic-refinement/routing/final-output subsets implemented**

## Automated evidence

The model-free fallback tests prove that ordinary text stays on deterministic
cleanup, unsafe control/bidi/noncharacter scalars select sanitized raw output,
visible Hindi/Urdu/punctuation/casing survives that fallback, output never
exceeds input, and resource-limit failures remain errors. Both output variants
are bounded wipe-on-drop owners and expose only payload-free numeric path/report
metadata.

The metadata-only router tests prove that all four named semantic-editing needs
can select a ready local editor, while the default, no-need, disabled, and
unavailable cases remain model-free. The router accepts no transcript content,
allocates nothing, and compiles no guessed confidence threshold.

The optional-editor tests prove a selected local runtime can write only through
a bounded sink; valid output is accepted only after validation, while timeout,
empty output, disallowed controls, and source/policy bounds fail closed through
deterministic cleanup to sanitized raw. Native partials reach neither text nor
editor access, and completed streaming requires its one-shot boundary. Attempt
reports and errors contain categories only. Protected-token semantic checks
reject candidate mutation or deletion before the sink accepts editor output.

The finalized-output adapter tests prove that ordinary stable worker text crosses
both cleanup and deterministic validation, unsafe scalars use the validated raw
fallback, bounds fail without payload disclosure, and the production worker
transcript satisfies the final-text contract. The experimental-native adapter
rejects a partial before calling its text accessor and applies the same bounded
path to a confirmed final. Completed consensus output crosses a one-shot
commit-count boundary; incomplete, reused, reordered, and oversized commit sets
fail before a successful refined result is returned.

The current ordinary Rust suite has 223 passing integration/unit tests. All behavioral audio is synthetic except six checksum-pinned, license/provenance-reviewed public LibriSpeech regression fixtures and five commit-pinned FLEURS Hindi read-speech fixtures used to revalidate the strict repository intake boundary. In addition to capture/DSP/VAD, consent, model-gate, native-adapter, and IPC framing coverage, the suite tests strict bounded fixture manifests, reviewed voice-rights and diversity-strata consistency, numeric coverage counts, deterministic volatile-noise reproducibility and SNR mechanics, approved-root/non-regular file rejection, exact audio/transcript hashes, UTF-8 transcript policy (including an explicit empty silence reference), canonical PCM16/float32 decoding, malformed WAV chunks/shapes, non-finite/out-of-range samples, explicit bounded LibriSpeech-English and FLEURS-Hindi normalization, numeric-only Devanagari/ASCII-Latin script composition, end-to-end fixture language enforcement, exact 16 kHz pass-through, stereo noncanonical-rate downmix/resampling, cancellation before state change, backend/scorer error cleanup, bounded exact-text WER/CER substitution/deletion/insertion accounting, multilingual Unicode-scalar behavior, empty-reference handling, edit-work limits, bounded payload-minimal benchmark configuration, numeric revision/percentile/RTF math, Unicode divergence, commit ordering, marker-free errors, the complete automatic/fixed language allowlist, rejection of unknown language codes before model allocation, idle-only language changes, clean consensus-generation resets, exact event-aligned canonical frames, bounded speech-confirmation pre-roll, utterance hard limits/state violations, cancellation precedence, dispatch cleanup, cleanup after audio failure, stale pre-start queue erasure, bounded final-only and streaming session drains, live borrowed hypotheses, immutable consensus commits, discontinuity resets, hotkey finalization with an unpadded canonical tail, capture destruction after pause failure, rolling-window cadence and hard limits, configured-overlap PCM trimming, final suffix revision, timestamp/capacity atomicity, PCM/pending-text erasure on cancellation or failure, bounded deterministic whitespace/punctuation cleanup, conservative multilingual case handling, payload-free cleanup errors, output-length limits, zero-copy candidate validation, explicit additive/proportional growth gates, empty-source protection, Unicode noncharacter rejection, explicit bidirectional-control rejection while preserving script shaping joiners, protected-token semantic verification, fail-closed metadata-only refinement routing, bounded local-editor candidate validation and complete offline fallback, borrowed exact-phrase dictionary replacement, literal-default versioned spoken rules with explicit LF-only validation, and final-only production/native/completed-streaming refinement composition.

Nine ignored acceptance tests use separately downloaded reviewed Whisper artifacts: the production compiled-manifest gate, the direct pinned native adapter, a complete process-worker round trip, a forced 10 ms timeout, explicit in-flight cancellation, automatic-to-fixed-English worker replacement, the six-case reviewed LibriSpeech numeric benchmark, the five-case fixed-Hindi FLEURS numeric benchmark, and a direct four-way tiny/base fixed/automatic Hindi comparison. The timeout, cancellation, and language-change tests prove replacement by observing generation 2 and a changed process ID; the language test also proves that reapplying the same mode does not restart. The comparison emitted only numeric metrics: fixed-Hindi tiny/base WER was 188.09%/101.19%; automatic mode returned empty hypotheses and 100% deletion-only WER/CER for both. Both candidates and modes fail the Hindi quality gate, so the production worker remains pinned to the 32.2 MB tiny artifact. The normal automated suite does not require or distribute either local model binary.

Five additional ignored acceptance tests use the separately reviewed Nemotron
artifact: the compiled-model identity gate, direct five-case streaming probe,
isolated native process, native DSP/VAD bridge, and deterministic 20/10 dB
noise matrix. The matrix emits only numeric aggregates, exposed a
large-context overlap during stream replacement, and passes in one reused
process after destroy-before-create lifecycle ordering. Exact narrow results
are 9.52%/20.23% WER and 1.89%/8.83% CER, with all-Devanagari hypotheses. These
tests remain excluded from the ordinary suite and do not distribute the model.

The [2026-09-05 lifecycle regression evidence](NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
adds five model-free native owner tests and eight Windows production-supervisor
tests covering ordering, repeated utterances, timeout, cancellation, disconnect,
decoder/framing/finalization errors, explicit reset, and exception cleanup.
The final rebuilt native matrix passed ten cases in one process with a 30-second
request deadline, no crashes or timeouts, and unchanged narrow accuracy. Timing
and coarse working-set figures are in that report. ASan/UBSan remains unverified
because the installed sanitizer runtime libraries are unavailable.

The locked suite passes with Cargo forced offline. Clippy passes for the workspace, all targets, and all enabled features with warnings denied. The resolved dependency tree scan finds no named HTTP/WebSocket/telemetry/analytics/crash-reporting client. This is source/build evidence, not the packaged socket-observation acceptance test.

RustSec advisory scanning, cargo-deny advisory/ban/license/source policy, and eight privacy-sanitized CycloneDX 1.5 SBOMs now pass for the locked Windows graph. Static release import inspection found only Windows core/COM/audio/runtime libraries and no named network DLL.

Still open: a successful consenting real-device stream (the current host's advertised 192 kHz profile is rejected during CPAL stream construction), permission/removal tests, acoustic fixture benchmarks, native-model fuzzing, packaged worker signature/authenticity and OS sandboxing, full platform packaging, runtime socket/DNS observation, and cross-platform graphs/process semantics. The latest SBOM generator's own locked build uses yanked `xml-rs` 0.8.19; that tool-only caveat does not enter the product graph but remains tracked.

## Evidence principles

The [100-session native soak](NEMOTRON_SOAK_2026-09-05.md) adds a separately
ignored real-model stability test, bringing the manual/ignored total to 16.
Native counts matched at 100 created/finished/destroyed, with one process and no
crashes/restarts/timeouts. The ordinary supervisor suite now has 13 process
tests, and CTest has six entries including 22 production-adapter boundary
cases. Final strict Clippy and all 223 ordinary Rust tests passed. Sanitizers
remain unexecuted due to missing runtime libraries; abruptly orphaned native
compute has no proven Job Object containment. Neither gap is closed by flat
working-set samples or synthetic fault coverage.

- A control is incomplete until a test can fail when the control is removed or bypassed.
- Tests use synthetic or license-reviewed fixtures, never private recordings by default.
- Network is not used by fuzz/tests that process sensitive fixtures.
- Platform behavior is marked human/unverified when automation cannot establish it.
- Security findings block the relevant milestone; no cloud fallback is an accepted workaround.

## Test layers

| Layer | Scope | Examples |
|---|---|---|
| Unit/property | Pure parsers, state machines, cleanup, bounds | Manifest/profile parsing, consensus monotonicity, refinement invariants |
| Integration | Local adapters with fakes/fixtures | Ring → VAD, model gate → fake runtime, SQLCipher/key-store failure, focus race |
| Fuzz | Security-sensitive parsers/state | Profile, WAV, codec wrapper, manifest, sanitizer, consensus, injection boundary |
| Package/system | Built release on target OS | Network isolation, permissions, crash artifacts, hotkey/insertion, resource caps |
| Human evaluation | Semantics/acoustics/platform UX | Language quality, prompt preservation, microphone quality, native auth prompts |

## Required automated matrix

| Requirement | Minimum tests |
|---|---|
| Model hash verification | Known hash succeeds only with matching size/architecture/runtime |
| Unknown/corrupt model rejection | Unknown, changed byte, truncated/appended, wrong-size/hash all fail before runtime call |
| ASR process deadline | Parent kills and reaps an overdue native worker, returns a fixed timeout, then serves from a distinct clean process generation |
| ASR cancellation | Pre-dispatch cancellation avoids IPC; in-flight cancellation kills/reaps and replaces the native worker |
| ASR IPC validation | Reject malformed tags/lengths/IDs/samples/UTF-8/offsets/timestamps before native or caller consumption |
| ASR language policy | Only automatic/compiled fixed codes cross IPC; changed modes erase volatile session state and require a clean worker generation; non-idle changes fail |
| Volatile utterance ownership | Exact VAD frame mapping, bounded confirmation pre-roll, one active buffer, cleanup on every cancel/error/discontinuity |
| Live session lifecycle | Explicit start, stale queue erasure, one-chunk scheduler bound, bounded stop drain, hotkey finalization, cancel cleanup, pause-failure capture drop |
| Ring overflow | Producer never blocks/allocates; discontinuity emitted; memory constant |
| Excessive recording | Stuck hotkey/no silence/continuous noise finalize or stop within hard cap |
| Malformed WAV | Header/chunk/offset/alignment/rate/channel/decoded-size corpus + fuzz |
| FLAC/Opus if enabled | Feature-specific malformed/decompression/cancellation fuzz gates |
| Decompression bounds | Tiny input cannot exceed configured decoded bytes/time/memory |
| Database encryption | Cipher availability and encrypted marker-at-rest check; plaintext SQLite fails gate |
| Credential failures | Missing/locked/corrupt/wrong key disables persistent history without weak fallback |
| Transcript logging prevention | Unique markers absent from release logs/errors/metrics |
| Prompt-injection separation | Adversarial transcript/context cannot alter policy or invoke capabilities |
| Context permissions | Level 0 calls no context API; each level cannot access higher-level sources |
| Path traversal | Relative/absolute/UNC/ADS/case/symlink/reparse/hardlink/race cases |
| Profile import | Size/depth/count/unknown/duplicate/control-character/atomicity/fuzz cases |
| Crash recovery | Mode-correct persistence, encrypted state only, cleanup after success |
| Temp-file prevention | Normal dictation marker absent from temp trees and no audio file created |
| Text sanitization | Invalid Unicode/control/bidi policy/length/expansion cases preserve safe text semantics |
| Deterministic fallback | Local editor failure/invalid output yields deterministic result |
| Raw fallback | Deterministic error yields bounded sanitized raw transcript |
| Consensus commit | Committed prefix never rolls back; window/audio memory stays bounded |
| Rolling inference | Frame-aligned cadence, one inference per push, overlap retention, atomic hard limits, cancellation/discontinuity/failure cleanup |
| Streaming session | Exact active-frame routing, borrowed partial state, caller-owned commits, bounded drain/stop capacity, tail finalization, stale-ring and lifecycle cleanup |
| Benchmark privacy | Preallocated limits, one overwritten prior partial, borrowed final text, numeric-only summary, marker-free errors, no implicit persistence/network |
| Text injection boundary | Exact plain text; no shell/process/action invocation; focus change fails safely |
| Sensitive errors | Panic/error/debug/display never formats sensitive wrapper payloads |

## Privacy regression fixture

Create unique per-run markers for transcript, username-like value, context, dictionary entry, model path, and key-like value. Exercise the full local pipeline in each history mode, then scan:

- application release logs and stderr/stdout;
- application temp/cache/config directories;
- OS temp directory within a test-owned isolated root;
- crash-recovery files and database sidecars;
- unencrypted application storage and exported diagnostics;
- clipboard mock call history;
- serialized UI IPC/event captures.

Expected result: no marker outside its explicitly permitted encrypted store or in-memory test observation. For history off, no persistent marker. Hashes/substrings/encoded forms used by the implementation are included in the scan set.

## Network-isolation acceptance test

Run the packaged release with outbound access blocked and with socket/DNS observation enabled. Exercise microphone capture, VAD, ASR, deterministic refinement, optional approved local editor, personalization, all history modes, verified model loading, overlay, and text insertion.

Pass criteria:

- normal local functions behave identically except manual acquisition/update features outside the app;
- the application initiates no DNS query or outbound socket;
- remote URL/navigation attempts from the overlay are denied;
- static dependency/SBOM review finds no generic HTTP/updater/remote inference/telemetry client;
- failure messages never suggest or activate a cloud fallback.

Automation is OS-specific. A test that merely has no internet connection is insufficient without observing attempted connections.

## Fuzz priorities

1. Imported profile parser
2. WAV/container parser
3. Optional codec wrappers
4. Model manifest parser
5. Transcript sanitization and deterministic rules
6. Consensus commit/event state machine
7. Text injection typed boundary

Fuzzers enforce memory/time limits, keep corpora local, minimize crashes, and produce no external reports automatically. Native/FFI fuzzing uses sanitizers where supported.

## Fault injection

- Device removed/permission revoked mid-callback
- Worker delayed/panicked/cancelled
- Ring full and discontinuous sequence
- Resampler/VAD/ASR initialization and runtime failures
- Model read changed/denied/cancelled during verification
- OOM/context exhaustion/invalid tokens/refinement timeout
- SQLCipher disk full/locked/corrupt/WAL recovery/key-store unavailable
- Focus changes before/during insertion and accessibility permission denied
- Crash at each persistence transaction stage

Every fault has a bounded cleanup deadline and a state-machine assertion.

## CI gates from the first implementation milestone

On Windows, macOS, and Linux where practical:

1. formatting and warnings-as-errors lint;
2. unit/property/integration/security regression tests;
3. documentation/link/schema checks;
4. dependency advisory, license, banned-source, and feature review;
5. SBOM generation and artifact hash publication for releases;
6. fuzz smoke runs and longer scheduled fuzzing;
7. synthetic fixtures only—no live microphone requirement;
8. release-build privacy-marker and forbidden-dependency scan.

Dependency downloads in CI are a build-time external flow and are isolated from the production runtime. Actions/toolchains must be pinned and reviewed.

## Release security report template

### Changed files

List exact repository paths.

### Security-relevant diff

Include the actual reviewed diff or a durable link/hash to it.

### Verification

Include exact commands, tool versions, exit status, and non-sensitive output.

### Unsupported claims

> Unverified — requires human testing.

Use that wording for Windows Hello, Touch ID, Linux authentication integration, actual microphone quality, physical low-end performance, native-speaker language quality, final subjective UX quality, and any platform behavior not directly observed.

## Exit criteria by milestone

- No milestone advances with a missing high/critical threat test assigned to it.
- A deferred codec/model/platform is absent from default builds until its gate passes.
- Security/privacy claims in UI/docs are derived from verified build configuration and test status.
- Unresolved risks remain visible in the threat model and release report; they are never silently converted into assumptions.
