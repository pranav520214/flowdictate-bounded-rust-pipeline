# Model Selection Benchmark Plan

Status: **Clean-English regression and tiny/base Hindi comparison recorded — diverse benchmark remains**

## Implemented measurement infrastructure

`flowdictate-pipeline::StreamingBenchmarkRecorder` now computes bounded numeric
partial revision, Unicode divergence, first-partial timing, inference p50/p95,
real-time factor, and commit-order metrics. Rolling/streaming reports expose
only inference window sizes and local backend durations. The recorder retains
one bounded previous partial in volatile memory and overwrites it on
replacement/finish/drop; results contain no transcript or audio payload.

`flowdictate-pipeline::measure_recognition` now provides bounded numeric-only
word/character substitution, deletion, insertion, WER, and CER results. It uses
exact reviewed text semantics, Unicode-scalar rather than UTF-8-byte CER, and a
four-million-cell work ceiling. Dataset-specific normalization remains an
explicit reviewed step.

[`../benches/fixtures.template.csv`](../benches/fixtures.template.csv) remains
the header-only review template. [`../benches/fixtures.csv`](../benches/fixtures.csv)
admits six checksum-pinned, CC BY 4.0 LibriSpeech regression cases through the
bounded fixture gate. They establish the real public intake and clean-English
regression path, not general accuracy, acoustic stability, or hardware performance.

The verified fixture runner now applies the reviewed language mode, canonicalizes
decoded mono/stereo audio through the production downmix/resampling seam, calls
the local backend once, measures inference time/RTF, and returns numeric WER/CER.
It performs no network, persistence, logging, or transcript-result emission.

The first six-case numeric-only regression result is recorded in
[`BENCHMARK_RESULTS_2026-08-28.md`](BENCHMARK_RESULTS_2026-08-28.md). It is one
small, clean audiobook set and therefore cannot support a production-quality
claim across conversational, accented, noisy, multilingual, streaming, or
baseline-hardware use.

[`MODEL_COMPARISON_PREFLIGHT.md`](MODEL_COMPARISON_PREFLIGHT.md) records the
candidate-inventory gate. In the exact pinned official inventory, the current
32.2 MB multilingual `ggml-tiny-q5_1.bin` is the smallest complete candidate.
Tiny later failed the five-case Hindi gate. The triggered comparison used the
pinned 59.7 MB base-q5_1 candidate on identical bytes and settings: WER improved
from 188.09% to 101.19%, while corpus RTF rose from 0.1973 to 0.3890. Both fail
the provisional 22% Hindi gate, so the evidence rejects a production model
switch and points to dataset expansion and diagnosis before further escalation.
Numeric script counters found 317/317 Devanagari reference characters but zero
Devanagari characters in either model's hypotheses. Tiny emitted 426 ASCII
Latin letters across 434 hypothesis characters; base emitted 148 across 299.
This identifies a script mismatch without exposing text, but semantic
transliteration quality remains unverified.
Repeating the same four model/mode combinations with automatic language
detection produced empty hypotheses for all five cases in both models: 100%
deletion-only WER/CER. Fixed Hindi is therefore necessary for non-empty output
on this set, and automatic detection does not resolve the script mismatch.

## Decision rule

Choose the smallest local model that satisfies all mandatory quality, latency, memory, license, integrity, and fallback gates on baseline hardware. A larger model cannot be selected solely because it scores better.

## ASR candidates

Initial matrix:

- multilingual Whisper tiny: reviewed quantizations such as Q4/Q5/Q8 where supported;
- multilingual Whisper base: same quantization sweep, evaluated only if tiny misses a mandatory gate;
- another small Whisper-compatible multilingual model only after equal license/provenance/runtime review.

English-only variants are excluded because multilingual/code-switching is a core requirement. Accelerators are optional test dimensions, never baseline requirements.

## Dataset design

Use locally stored, license-reviewed, redistributable public data plus synthetic fixtures. Never upload private correction history or user recordings.

The first diverse-source gate is recorded in
[`DIVERSE_FIXTURE_SOURCE_REVIEW.md`](DIVERSE_FIXTURE_SOURCE_REVIEW.md). It
approves only a controlled, commit-pinned Google FLEURS `hi_in` derivation for
Hindi read speech. Five cases totalling 34.680 seconds pass the repository's
integrity and canonical-WAV intake gate. Their numeric result is recorded in
[`HINDI_BENCHMARK_RESULTS_2026-09-01.md`](HINDI_BENCHMARK_RESULTS_2026-09-01.md):
188.09% WER under the explicit FLEURS Hindi policy. The completed like-for-like
base-q5_1 comparison improved this to 101.19% WER but still failed the 22% gate.
Neither result supports selecting base or making a Hindi-quality claim.

Strata:

- English: clean, conversational, Indian English, technical/programming, names/acronyms;
- Hindi and Hinglish/code-switching;
- initial smoke sets for Punjabi, Bengali, Marathi, Tamil, Telugu, Urdu, Gujarati, Kannada, Malayalam;
- later Arabic, German, French, Spanish, Italian, Portuguese, Dutch, Polish, Turkish, Indonesian, Japanese, Korean;
- noise/SNR, microphone distance, speaking rate, accents, false starts, punctuation commands, lists;
- RTL rendering/insertion fixtures for Urdu and Arabic.

Each record stores dataset/license/provenance, language tags, duration, source sample rate, expected transcript, sensitive-data classification, and permitted use. Private evaluation sets remain local and are never incorporated into a public artifact without explicit rights.

## Measurements

| Area | Metrics |
|---|---|
| Recognition | WER, CER, mixed-language token error, keyword/entity recall, deletion/insertion/substitution rates |
| Streaming stability | Partial revision rate, committed-prefix rollback count (must be zero), final/partial divergence, time to first partial |
| Latency | Cold/warm model load; p50/p95 end-of-speech to final; per-window inference; real-time factor |
| Resources | Peak/steady RSS, model bytes, CPU p50/p95, allocations, thermal/throttling notes |
| Robustness | OOM, timeout, long silence/noise, invalid audio, cancellation, repeated sessions |
| Privacy/security | No network, no audio files, no marker leakage, verified model only |

Provisional starting quality gates for the first production claim—not results:

| Stratum | Starting gate |
|---|---|
| English clean/conversational | WER ≤ 15% and keyword recall ≥ 95% |
| Indian English/technical | WER ≤ 20% and domain keyword recall ≥ 90% |
| Hindi | WER ≤ 22% |
| Hinglish code-switching | mixed-token error ≤ 25%; no systematic language deletion |
| Noisy initial set | WER ≤ 30% at the defined SNR band |
| Streaming | zero committed-prefix rollback; bounded 30 s window |

Thresholds require reviewer approval and dataset confidence intervals. No unmeasured language is advertised as production-quality.

## Refinement benchmark

Run deterministic cleanup alone first. Test categories include whitespace, punctuation, filler repetition, false starts, self-correction, lists, casing, abbreviations, identifiers, Indian English, Hinglish, professional/informal tone, prompt-injection text, and strings containing commands/code.

Metrics:

- exact/acceptable cleanup rate;
- meaning-preservation blind review;
- unsupported fact/entity/number additions (target: zero);
- percentage requiring no local LLM (initial target ≥ 95% of ordinary dictation set);
- router false-positive and false-negative rates;
- latency/memory cost per accepted improvement;
- fallback success under model failure/invalid output.

Only if the deterministic path misses an approved threshold may 0.3B–1B local editor candidates enter the matrix. Fine-tuning is considered before increasing size, uses only licensed/synthetic/explicitly authorized local data, and remains local.

## Hardware matrix

- Baseline: dual-core CPU, integrated graphics, 4 GB RAM, CPU-only.
- Reference development machine: recorded exact CPU/RAM/OS/power mode.
- At least one Windows x86-64 baseline before Windows release.
- macOS/Linux claims require their own measured systems.
- Warm and cold runs, plugged/battery where relevant, and background-load scenario.

## Protocol

1. Record artifact hashes, versions, build flags, thread counts, power mode, and environment.
2. Warm up only where the measured scenario calls for warm state.
3. Randomize sample order; run enough repetitions for p50/p95 and confidence intervals.
4. Capture timing at every pipeline stage with payload-free metrics.
5. Record failures and exclusions; never silently drop difficult samples.
6. Publish raw non-sensitive benchmark results, scripts, manifests, and summary.
7. Choose the smallest candidate passing every mandatory gate; document the decision in an ADR.

## Result template

```text
Status: measured | unverified
Artifact SHA-256:
Runtime/build flags:
Hardware/OS/power mode:
Dataset version/license:
WER/CER by stratum:
End-of-speech p50/p95:
RTF p50/p95:
Peak/steady RSS:
CPU p50/p95:
Failures/exclusions:
Privacy/network checks:
Decision:
```
