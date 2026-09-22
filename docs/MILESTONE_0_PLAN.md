# Milestone 0 Plan and Specification Reconciliation

Status: **Implemented as documentation; awaiting human review**  
Date: 2026-08-26  
Scope: architecture only; no product functionality

## Work plan

1. Freeze privacy and security invariants.
2. Define modules, process/thread boundaries, and failure behavior.
3. Model assets, actors, trust boundaries, threats, mitigations, and verification.
4. Define minimal data flows and retention by history mode.
5. Propose dependencies with license and supply-chain review gates.
6. Define audio, codec, ASR, refinement, storage, and injection interfaces.
7. Establish measurable resource, latency, model-selection, and security-test gates.
8. Record decisions and unresolved conflicts before implementation.

## Milestone 0 deliverable map

| Required deliverable | Location |
|---|---|
| Milestone security report | `MILESTONE_0_REPORT.md` |
| Repository/module structure | `ARCHITECTURE.md` |
| Complete proposed direct dependency list and licenses | `DEPENDENCIES.md` |
| Threat model | `THREAT_MODEL.md` |
| Trust-boundary diagram | `THREAT_MODEL.md` |
| Audio pipeline | `AUDIO_PIPELINE.md` |
| Codec architecture | `CODEC_SECURITY.md` |
| Model selection and benchmark plan | `MODEL_SECURITY.md`, `MODEL_BENCHMARKS.md` |
| Privacy/data-flow diagram | `PRIVACY_MODEL.md` |
| RAM/latency budget | `PERFORMANCE.md` |
| Test strategy | `SECURITY_TESTING.md` |
| Conflicts and impractical requirements | This document |

## Conflicts, qualifications, and privacy-preserving resolutions

1. **A literally complete transitive dependency list cannot exist before a target-specific lockfile is resolved.** This milestone therefore records the complete proposed direct set plus known native/transitive implications. Milestone 1 must pin versions/features, generate `cargo metadata`, run license/advisory tooling, and reject unapproved transitives before adoption.
2. **Tauri uses a platform WebView that is technically capable of networking even if FlowDictate ships no HTTP client.** The Rust core remains network-free. The overlay must use bundled assets only, `connect-src 'none'`, disabled navigation/new-window behavior, a minimal Tauri capability allowlist, and firewall/socket acceptance testing. If the WebView cannot meet the release gate, replace it with a native non-WebView overlay.
3. **Perfect memory zeroization cannot be guaranteed.** Rust-owned key buffers can be zeroized, but OS audio drivers, GUI strings, SQLite/SQLCipher internals, allocator copies, model runtimes, accessibility APIs, and crash dump mechanisms may copy data. The design minimizes lifetime/copies and documents this residual risk without claiming erasure guarantees.
4. **Deleting encrypted SQLite rows is not secure physical erasure on SSD, journaling, or copy-on-write storage.** Default history-off prevents writes. "Delete all data" destroys the database and its OS-held key, then documents that physical remnants may survive below the application layer.
5. **Cross-platform key-store behavior is not uniform.** Persistent history fails closed when an approved OS credential store is unavailable. The product remains usable with history off; it never stores the database key beside the database.
6. **`whisper.cpp` streaming is rolling-window inference, not guaranteed token-by-token streaming.** The design provides bounded, overlapping inference windows and consensus-stabilized partial hypotheses. Measured latency and stability determine whether this satisfies the user experience target.
7. **Allocation-free audio callback behavior requires measurement, not source-level assertion alone.** The callback design forbids allocation and blocking, and Milestone 1 adds allocator instrumentation and overflow tests. Driver/runtime allocations outside FlowDictate are not controllable.
8. **Global hotkeys, direct text insertion, secure-store prompts, and microphone permissions require OS/human verification.** Automated tests cover abstractions and failure behavior; platform behavior is explicitly marked unverified until tested on supported systems.
9. **CI and manual model/software acquisition need network access, while the production inference app must not.** They are separate trust domains. Build/update tooling is not linked into the production core; model import is explicit, local, hash-gated, and never auto-downloads.
10. **FLAC and Opus are requested conditionally but expand parser/native attack surface.** They are deferred and feature-gated. WAV/PCM is initially limited to synthetic test fixtures and explicit diagnostics; normal dictation never creates audio files.
11. **A CI provider/repository remote is not present in the empty workspace.** Milestone 0 verification is local and documented. CI configuration will be the first repository-infrastructure change after the user selects/creates the private remote; no external repository or data transmission is assumed.

## Human decisions required before Milestone 1

- Accept Tauri provisionally subject to the network-isolation gate, or require a native overlay immediately.
- Confirm the first supported release target (recommended: Windows x86-64 first, with portable interfaces preserved).
- Approve Earshot as the initial VAD candidate pending benchmark quality, with deterministic energy gating as a safe fallback.
- Approve the SQLCipher packaging spike as the first persistence task, while keeping history disabled until it passes on each platform.
- Approve the direct dependency proposal and the rule that any new dependency requires a new review row.
