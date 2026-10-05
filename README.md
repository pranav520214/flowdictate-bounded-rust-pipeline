# FlowDictate

FlowDictate is a privacy-first, offline desktop voice-input system. **Milestones 1–3 remain in progress:** the repository now contains bounded native capture and local DSP/VAD, volatile local ASR paths, and the first conservative deterministic-refinement slice. There is still no runnable end-to-end live dictation loop, transcript insertion, persistent history, telemetry, updater, or network client.

## Non-negotiable invariants

- Normal dictation works with outbound networking blocked.
- Microphone audio is volatile by default and never written to disk in the normal path.
- Transcript, context, dictionary, profile, and key material never leave the device.
- Unknown or modified model files fail closed before any model runtime sees them.
- Buffers and decoding windows are bounded.
- Dictation emits text only; it cannot execute commands.
- History defaults to off. Persistent history is permitted only when encryption and OS-protected key storage are available.
- Production logs exclude audio, transcripts, prompts, context, dictionary contents, clipboard data, and secrets.

## Project documents

- [Milestone plan](docs/MILESTONE_0_PLAN.md)
- [Milestone 0 report](docs/MILESTONE_0_REPORT.md)
- [Milestone 1 implementation report](docs/MILESTONE_1_REPORT.md)
- [Milestone 2 implementation report](docs/MILESTONE_2_REPORT.md)
- [Milestone 3 implementation report](docs/MILESTONE_3_REPORT.md)
- [Milestone status audit](docs/MILESTONE_STATUS.md)
- [Dependency security audit](docs/DEPENDENCY_AUDIT_2026-08-26.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Dependency and license proposal](docs/DEPENDENCIES.md)
- [Threat model](docs/THREAT_MODEL.md)
- [Privacy model](docs/PRIVACY_MODEL.md)
- [Audio pipeline](docs/AUDIO_PIPELINE.md)
- [Volatile utterance pipeline](docs/PIPELINE_RUNTIME.md)
- [Bounded live session runtime](docs/SESSION_RUNTIME.md)
- [Codec security](docs/CODEC_SECURITY.md)
- [Model security](docs/MODEL_SECURITY.md)
- [ASR runtime boundary](docs/ASR_RUNTIME.md)
- [Rolling ASR runtime](docs/ROLLING_ASR_RUNTIME.md)
- [Streaming ASR benchmark runtime](docs/BENCHMARK_RUNTIME.md)
- [Model benchmarks](docs/MODEL_BENCHMARKS.md)
- [Model comparison preflight and result](docs/MODEL_COMPARISON_PREFLIGHT.md)
- [Diverse fixture source review](docs/DIVERSE_FIXTURE_SOURCE_REVIEW.md)
- [Hindi benchmark evidence](docs/HINDI_BENCHMARK_RESULTS_2026-09-01.md)
- [Hindi deterministic-noise benchmark evidence](docs/HINDI_NOISE_BENCHMARK_RESULTS_2026-09-02.md)
- [Experimental Nemotron streaming decision](docs/adr/0007-experimental-nemotron-streaming-asr.md)
- [Experimental Nemotron native runtime](docs/NEMOTRON_NATIVE_RUNTIME.md)
- [Nemotron lifecycle regression evidence](docs/NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
- [Nemotron 100-session soak and recovery evidence](docs/NEMOTRON_SOAK_2026-09-05.md)
- [Experimental ASR selection contract](docs/EXPERIMENTAL_BACKEND_SELECTION.md)
- [Deterministic refinement runtime](docs/REFINEMENT_RUNTIME.md)
- [Deterministic refinement benchmark](docs/REFINEMENT_BENCHMARK_2026-09-04.md)
- [Milestone 3 human and baseline acceptance](docs/REFINEMENT_ACCEPTANCE.md)
- [Qwen3 refiner runtime slice](docs/QWEN_REFINER_RUNTIME.md)
- [Security testing](docs/SECURITY_TESTING.md)
- [Performance and resource budgets](docs/PERFORMANCE.md)
- [Architecture decisions](docs/adr/README.md)

## Current implementation boundary

The `flowdictate-audio` crate implements validated mono/stereo PCM capture planning, a two-second bounded SPSC handoff, allocation-regression-tested callback conversion, worker-side downmixing/resampling, real RMS/peak levels, exact 16 ms Earshot VAD framing, bounded utterance segmentation, discontinuity reset, an explicit codec allowlist, an offline model identity/integrity gate, and a strict approved-root benchmark fixture loader for canonical PCM16/finite-float32 WAV plus bounded UTF-8 references. Its manifest requires reviewed voice rights and explicit speech-style, acoustic-condition, language-mix, and accent-evidence strata with fail-closed consistency rules. The fixture loader never fetches provenance URLs or admits a broad media container. The default CPAL stream is created paused and can start only through an explicit `resume` call.

The `flowdictate-asr` crate links exact `whisper-rs` 0.16.0 / bundled `whisper.cpp` 1.8.3 with GPU and logging backends disabled. The native runtime now executes in the silent `flowdictate-asr-worker` child behind `flowdictate-asr-ipc`: the parent transfers only independently reverified model bytes and bounded mono 16 kHz PCM, validates bounded transcript frames, and kills/recreates the child when its wall-clock deadline expires. The versioned startup contract accepts automatic detection or one of 23 compiled fixed languages—never arbitrary strings. Release acceptance tests prove timeout recovery and automatic-to-fixed language replacement by observing a new process ID and worker generation.

The `flowdictate-pipeline` crate joins the implemented seams without adding an external dependency. Each VAD event is paired with its exact 256 canonical samples; start-confirmation pre-roll and one active utterance are bounded by the 30-second ASR limit. Silence, maximum duration, hotkey release, or explicit stop dispatch directly to the isolated worker. Discontinuity or cancellation discards and overwrites pending audio; cancellation during native inference kills and replaces the child.

Its legacy `LiveSession` owner gates an already-paused capture behind explicit `start`, erases stale pre-start audio, drains at most one exact chunk per scheduler call, resets on discontinuity epochs, and performs bounded hotkey-stop/cancel cleanup. It preserves the previously verified final-utterance API.

Milestone 2 adds a dedicated `StreamingLiveSession` path. Active canonical frames flow through bounded confirmation pre-roll into rolling local inference and consensus; callers borrow the current pending hypothesis and receive immutable commit deltas in preallocated storage. Hotkey release includes the unpadded canonical tail, while cancellation, discontinuity, capture failure, and external cancellation erase PCM and pending text. Language changes are idle-only and erase volatile/queued state before a clean worker and consensus generation; reapplying the current mode is a no-op. A bounded measurement recorder reduces borrowed partial/final text to numeric revision, divergence, percentile, RTF, and commit-order metrics while retaining only one overwritten previous partial. A work-limited scorer reduces borrowed reviewed reference/final text to numeric substitution/deletion/insertion and WER/CER results using Unicode-scalar CER. The end-to-end fixture runner consumes a hash-verified WAV/reference pair, applies its allowlisted language mode, canonicalizes it through the production downmix/resampling boundary, invokes the local backend, and returns only numeric timing/accuracy results. It can also mix allowlisted fixed-SNR white noise entirely in volatile canonical memory without adding a noise corpus or writing augmented audio. Six checksum-pinned, CC BY 4.0 LibriSpeech fixtures provide a 36.46-second clean-English regression result with an explicit normalization policy. Five commit-pinned CC BY 4.0 FLEURS Hindi cases exposed 188.09%/101.19% WER and no Devanagari output from Whisper tiny/base. A separate pinned Nemotron Q8 native-streaming feasibility probe scored 11.90% WER and 3.15% CER with all-Devanagari output and 0.3368 steady-state RTF, but used 976,093,184 bytes peak working set. The same reviewed clips now pass a volatile deterministic-noise matrix at 20/10 dB with 9.52%/20.23% WER, 1.89%/8.83% CER, 0.3138/0.3104 inference RTF, and all-Devanagari output. That matrix exposed and verified a native lifecycle fix: the adapter destroys a finished stream before creating its replacement, so one isolated process can serve consecutive utterances without overlapping large contexts. Its compiled experimental identity now loads under a tested immutable Windows path lease. A bounded Rust supervisor and dedicated C++ process call the pinned NeMo-Speech C ABI directly, while a compact owner feeds persistent native state in non-overlapping 160 ms chunks. The experimental DSP/VAD bridge routes confirmation pre-roll and each new active canonical frame exactly once, keeps only borrowed partial text, and transfers final text into caller-owned storage. Its explicitly named session owner adds an already-paused capture, bounded ring, explicit start, fresh cancellation, bounded stop, fail-before-resume reset, and a complete synthetic fault matrix covering capacity, discontinuity, external cancellation, and pause failure. A reviewed Hindi fixture passes the native bridge. A versioned application-selection contract now retains Whisper as the default, requires explicit current-version Nemotron acceptance, fails closed off Windows, separates model selection from listening consent, and supports immediate withdrawal. No application UI renders or composes it yet. Nemotron remains experimental until redistribution, broader quality, baseline hardware, and human gates pass. The production isolated worker therefore remains pinned to tiny. The code remains a library seam; no platform hotkey/event loop, overlay, text insertion, or end-to-end application exists yet.

Milestone 3 now has a dependency-free `flowdictate-refine` library seam. Its bounded cleanup, sanitized fallback, and output validator enforce explicit byte/growth/control/bidirectional/noncharacter policies and return wipe-on-drop opaque owners. A deterministic protected-token verifier rejects candidate edits that mutate or remove numbers, URLs, emails, paths, flags, or identifiers before any editor output can be accepted. A borrowed local dictionary performs bounded, case-sensitive, longest exact-token/phrase replacement; it has no persistence or implicit learning, and explicit entries are the only abbreviation-rewrite mechanism. Literal speech is the default. An opt-in English V1 rule set collapses only consecutive `uh`, `um`, `erm`, and `hmm`, and recognizes only `new line`, `new paragraph`, and `bullet point`; intentional repetition and other languages remain unchanged. LF output requires its own narrow validator policy. A metadata-only router represents the four documented semantic-editing needs without transcript access. Its audited local-runtime interface accepts output only through a bounded wipe-on-drop sink, validates every candidate, and proves local failure/empty/invalid output falls through deterministic cleanup to sanitized raw text without a cloud path. Finalized production, experimental-native, and completed streaming-consensus outputs cross final-only boundaries; native partials are rejected before text or editor access, while streaming uses a one-shot boundary and consumes/wipes the exact commit buffer. Production routing-signal calibration, an evidence-justified 0.3B–1B editor evaluation and selection, prompt-cache evaluation, baseline-hardware measurement, and human semantic acceptance remain open.

Synthetic tests, formatting, Clippy, and the full locked build pass offline. Actual microphone enumeration, permission UX, driver scheduling, and acoustic VAD quality remain unverified and require human testing on target hardware.

## Manual microphone probe

The development-only `flowdictate-capture-probe` is the sole current live-hardware harness. It displays the privacy disclosure and must receive the exact phrase `I CONSENT` before it opens the default microphone. Duration is compiled to 1–30 seconds; output contains only the negotiated format and numeric health counters. It never prints device labels or audio.

```text
cargo run -p flowdictate-capture-probe --release --locked --offline -- 10
```

Do not run this command unless you are ready for the OS microphone permission flow. A denied or different consent phrase exits before `build_default_capture` is called.
