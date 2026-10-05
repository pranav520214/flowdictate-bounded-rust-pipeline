# FlowDictate Milestone Status Audit

Status: **Reverified against the original specification**  
Date: 2026-09-05

This audit distinguishes source implementation, automated evidence, and human
or packaged acceptance. A milestone is not marked complete merely because part
of a later milestone already exists.

| Milestone | Status | Verified evidence | Remaining gate |
|---|---|---|---|
| 0 — Architecture and threat model | Deliverables complete; acceptance provisional | Required architecture, dependency, threat, privacy, audio, codec, model, performance, testing, and ADR documents are present | Human review of provisional Tauri/platform choices and pending decisions remains recorded |
| 1 — Secure audio subsystem | Code substantially complete; acceptance incomplete | CPAL enumeration/capture adapter, paused stream, bounded allocation-free ring callback, normalization, resampling, VAD, codec policy, synthetic and long-session tests | Successful microphone permission/capture run, device loss/revocation, pause behavior, overload scheduling, and socket/DNS observation on target hardware |
| 2 — Local streaming ASR | In progress | Existing pinned/isolation/bounds/consensus/benchmark seams; six passing clean-English cases; five reviewed Hindi cases; Whisper tiny/base failed at 188.09%/101.19% WER with no Devanagari; pinned Nemotron Q8 passed the narrow clean set at 11.90% WER, 3.15% CER, 0.3368 steady-state RTF, all-Devanagari output, and 976,093,184-byte peak working set; the volatile five-case white-noise matrix passed at 20/10 dB with 9.52%/20.23% WER, 1.89%/8.83% CER, 0.3138/0.3104 inference RTF, and all-Devanagari output; compiled experimental identity, immutable Windows model-path lease, bounded isolated C-ABI worker, corrected destroy-before-create stream reuse, Rust deadline/cancellation supervisor, compact non-overlapping 160 ms owner, DSP/VAD bridge, consent-gated bounded session owner, complete synthetic session fault matrix, passing real-fixture/process/session tests, versioned opt-in/withdrawal composition, fail-closed benchmark strata/voice-rights metadata, and a bounded deterministic volatile-noise runner | Render and connect the selection contract in a future app shell, admit rights-clear immutable conversational/accent/Hinglish and real acoustic-noise evidence, add equivalent non-Windows immutable model opening, review redistribution notices, test baseline hardware/live partials, and obtain native-speaker/human acceptance |
| 3 — Lightweight refinement | In progress | Dependency-free bounded cleanup/fallback, zero-copy validator with explicit LF-only mode, deterministic protected-token semantic verifier, borrowed exact phrase dictionary and explicit-only abbreviations, literal-default English V1 repeated-filler/line/paragraph/bullet rules, metadata-only router, bounded optional-editor sink and complete local-editor/deterministic/sanitized fallback, final production/native adapters, and one-shot completed-streaming composition; payload-free errors/reports and wipe-on-drop owners; 50 refinement plus 15 focused composition tests; release p95/process-peak evidence on the non-baseline development host; pinned Qwen3 refiner artifact with compiled manifest/hash evidence; isolated native llama.cpp worker build and smoke evidence | Dictionary persistence/UI belongs to M6–7; Rust deadline/kill/recovery supervisor and immutable leases, end-to-end LocalEditor composition, production routing-signal calibration, runtime/model evaluation and selection, prompt-cache applicability, dual-core/4-GB baseline benchmark, and human acceptance |
| 4 — Text insertion and global hotkey | Not started | Library-level stop/release semantics only | Platform global hotkey, focused-target identity, typed plain-text insertion, rollback/error reporting and focus-race tests |
| 5 — Ambient UI | Not started | UI states and privacy constraints documented | Overlay, actual level waveform, partial/final text, processing/success/warning/privacy states and packaged network denial |
| 6 — Encrypted persistence | Not started | SQLCipher/key-custody design only; history remains off | Encrypted database, OS-protected key, history modes, crash recovery, deletion and privacy dashboard |
| 7 — Personalization | Not started | Privacy/data model only | Explicit dictionary/pronunciation/profile and inspect/delete/export/import/reset controls |
| 8 — Multilingual and RTL | Partially front-loaded; milestone incomplete | Multilingual model identity, automatic detection, and 23 allowlisted fixed ISO language selections | Language fixtures/quality gates, code switching, Arabic/Urdu RTL rendering and insertion |
| 9 — Security hardening | Partially front-loaded; milestone incomplete | Threat model, dependency audit, process fault tests, model corruption tests, bounded-state tests, SBOMs | Complete threat review, parser/state fuzzing, filesystem/context/privacy-marker tests, codec corruption and packaged fault injection |
| 10 — Optimization | Not started | Resource budgets, benchmark method, and fixed clean-English regression timings documented | Model/quantization comparison, no-LLM path, CPU and warm/cold peak/steady memory on baseline hardware |
| 11 — Packaging | Not started | None | Windows, macOS and Linux packages; supported signing, checksums, release SBOM and reproducibility procedure |

## Audit conclusion

The [100-session native soak and recovery slice](NEMOTRON_SOAK_2026-09-05.md)
passed with one worker, 100 matched stream creations/destructions, maximum
concurrency one, zero restarts/crashes/timeouts, observed clean exit, and bounded
sampled memory/handles/threads. Final ordinary gates: 223 Rust tests passed,
16 ignored; native CTest 6/6 including 22 adapter boundary cases. Sanitizer
execution remains blocked by precisely recorded missing runtime libraries.
This closes bounded 100-session evidence, not extended/unlimited runtime,
abrupt-parent containment, hardware or human acceptance.

The [2026-09-05 lifecycle regression slice](NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
is complete: explicit finish/drain/destroy/idle ownership, five model-free
native tests and eight actual-supervisor tests, 216 passing ordinary Rust
tests (15 ignored), and ten real-model noisy cases passing in one worker under
the restored 30-second request deadline. Latest 20/10 dB WER is 9.52%/20.23%,
CER 1.89%/8.83%, and utterance-path RTF 0.3342/0.3299. Historical timings in the
inventory above are earlier observations. Coarse working-set and timing limits
are recorded in the report; this result does not close Milestone 2 acceptance.

No milestone beyond Milestone 0 is acceptance-complete. Milestone 1 has strong
synthetic/source evidence but remains blocked on a successful consented hardware
run and platform fault/network observation. Work may continue on independent
Milestone 2 software seams, but those results do not close the Milestone 1 human
gate.

The next bounded implementation sequence is:

1. define the remaining deterministic-refinement semantics and compose only
   stable final ASR text through the cleanup/output-validation boundary; do not
   adopt a local editor until measured evidence justifies it;
2. add conversational, accent, real acoustic-noise, and Hinglish strata plus
   native review while preserving the experimental selection contract and its
   production-Whisper/non-Windows fail-closed defaults;
3. separately complete the Milestone 1 live-hardware acceptance checklist.

## Unsupported claims

> Unverified — requires human testing.

This includes actual microphone permission UX and successful capture, device
loss/revocation behavior, acoustic VAD/ASR quality, native-speaker language
quality, physical low-end performance, platform hotkeys/insertion, packaged
network isolation, and subjective partial-text stability.
