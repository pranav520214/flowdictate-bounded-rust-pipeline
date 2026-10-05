# Milestone 0 Security Report

Status: **Documentation draft complete; human architecture/security review pending**  
Date: 2026-08-26

## Changed files

All files are new because the workspace began empty:

```text
README.md
docs/MILESTONE_0_PLAN.md
docs/MILESTONE_0_REPORT.md
docs/ARCHITECTURE.md
docs/DEPENDENCIES.md
docs/THREAT_MODEL.md
docs/PRIVACY_MODEL.md
docs/AUDIO_PIPELINE.md
docs/CODEC_SECURITY.md
docs/MODEL_SECURITY.md
docs/MODEL_BENCHMARKS.md
docs/PERFORMANCE.md
docs/SECURITY_TESTING.md
docs/adr/README.md
docs/adr/0001-local-modular-monolith.md
docs/adr/0002-network-free-runtime.md
docs/adr/0003-volatile-bounded-audio.md
docs/adr/0004-fail-closed-model-integrity.md
docs/adr/0005-encrypted-persistence.md
docs/adr/0006-provisional-tauri-overlay.md
```

## Security-relevant diff

There is no earlier repository baseline, so the complete contents of the new files above are the reviewable diff. The material decisions introduced are:

- a local modular-monolith core with no runtime network client, cloud fallback, telemetry, analytics, updater, shell execution, or general plugin loader;
- volatile, bounded microphone audio with a non-blocking/preallocated callback contract and discontinuity-on-overflow behavior;
- rolling bounded local ASR with a monotonic consensus commit and no record-then-batch architecture;
- model path/type/size/hash/architecture/runtime verification that fails closed before native loading;
- deterministic cleanup first, with a capability-free optional local editor and local-only fallback chain;
- direct plain-text insertion separated from any future command/action system;
- Level 0 context and history off by default, with encrypted persistence available only after SQLCipher and OS key-store verification;
- explicit residual risks for OS WebView networking capability, memory/secure deletion limits, local-account compromise, FFI/native parsers, cross-platform key stores, and focus races;
- feature-gated FLAC/Opus and exclusion of broad media/FFmpeg from the trusted path;
- test gates for privacy markers, outbound sockets/DNS, malformed inputs, resource bounds, prompt injection, model integrity, and storage encryption.

## Verification

Local evidence from the Milestone 0 review:

```text
Required documents:        12/12 present before adding this report
Non-documentation files:   0
Internal Markdown links:   PASS (0 broken)
Threat table rows:         24 (minimum check 24)
Dependency table rows:     31 (minimum check 30)
Mermaid diagrams:          5 (minimum check 4)
Unsupported-claim markers: 4 (minimum check 4)
```

The workspace contains no `Cargo.toml`, lockfile, runtime source, model, audio fixture, database, secret, cloud integration, telemetry integration, or HTTP client. Dependency versions/licenses in `DEPENDENCIES.md` are research metadata and have not been installed or adopted.

## Conflicts and decisions pending

- Human acceptance or replacement of the provisional Tauri/WebView shell.
- First supported platform selection; Windows x86-64 is recommended as the initial measured target, not yet accepted.
- Approval of the proposed dependencies before a lockfile is generated.
- Selection of a private repository/CI provider before external CI configuration.
- Platform spikes for SQLCipher/key custody and safe plain-text insertion.
- Benchmark approval for quality gates, datasets, hard recording limits, and VAD candidate.

## Unsupported claims

> Unverified — requires human testing.

This applies to Windows Hello behavior, Touch ID behavior, Linux authentication integration, actual microphone quality, physical low-end-hardware performance, native-speaker language quality, final subjective UX quality, platform accessibility permissions, focus-race behavior, packaged WebView network behavior, and actual model accuracy/latency/memory.

No claim is made that FlowDictate is secure or production-ready. Milestone 0 defines controls and evidence requirements; later milestones must implement and test them.

## Recommended next step

Review and accept/amend the six ADRs and the pending decisions above. Then begin Milestone 1 with two narrow spikes before broad audio work: (1) packaged-shell outbound/network-capability measurement, and (2) platform SQLCipher plus OS credential-store fail-closed verification. Neither spike may persist real transcript or microphone data.
