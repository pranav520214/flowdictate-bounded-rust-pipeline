# Local Hindi ASR regression evidence — 2026-09-01

Status: **Nemotron experimental probe passed the narrow gate; production switch remains gated**

## Reviewed input

- Fixtures: five deterministic `fleurs-hi-in-dev-*` cases from five distinct
  FLEURS sentence IDs.
- Dataset/split: commit-pinned Google FLEURS `hi_in` development data.
- License: CC BY 4.0.
- Shape: 34.680 seconds total, mono, 16 kHz, canonical PCM16; individual cases
  range from 3.600 to 12.000 seconds.
- Models: integrity-gated multilingual Whisper tiny `q5_1` and base `q5_1`
  from the same pinned upstream revision.
- Runtime: pinned local `whisper.cpp` 1.8.3 direct CPU adapter, optimized
  release, two threads, fixed Hindi mode. This manual comparison deliberately
  bypasses process-transport overhead equally for both candidates; the
  production isolated worker remains pinned to tiny.
- Scoring: explicit bounded `FleursHindi` policy; ASCII is lowercased,
  printable Unicode scalars are preserved, a reviewed punctuation set becomes
  word breaks, whitespace is collapsed, and control characters fail closed.

## Numeric-only like-for-like comparison

| Metric | Tiny `q5_1` | Base `q5_1` |
|---|---:|---:|
| Artifact bytes | 32,152,673 | 59,707,625 |
| Fixture count | 5 | 5 |
| Source duration | 34.680 seconds | 34.680 seconds |
| Total inference time | 6,842,588 microseconds | 13,491,645 microseconds |
| Per-case inference p50 | 1,288,824 microseconds | 2,681,489 microseconds |
| Per-case inference p95 (maximum of 5) | 1,693,536 microseconds | 2,877,398 microseconds |
| Corpus real-time factor | 0.1973 | 0.3890 |
| Reference words | 84 | 84 |
| Word substitutions / deletions / insertions | 80 / 4 / 74 | 75 / 9 / 1 |
| Word errors | 158 | 85 |
| WER | 188.09% | 101.19% |
| Reference characters | 317 | 317 |
| Character errors | 434 | 334 |
| CER | 136.90% | 105.36% |

## Numeric script-composition diagnosis

| Metric | Reference | Tiny hypothesis | Base hypothesis |
|---|---:|---:|---:|
| Non-whitespace characters | 317 | 434 | 299 |
| Devanagari-block characters | 317 | 0 | 0 |
| ASCII Latin letters | 0 | 426 | 148 |

These counters are computed after the same reviewed normalization. They expose
no text payload. Both models produced zero Devanagari-block characters for an
all-Devanagari reference set; tiny's output was almost entirely ASCII Latin,
while base contained a mixture of ASCII Latin and other non-Devanagari
characters. This proves an orthographic/script mismatch. It does not establish
whether any non-Devanagari output was a semantically correct transliteration;
that requires local native-speaker review.

## Fixed versus automatic decoding diagnosis

The same ignored harness also replaced only the in-memory fixture language
policy and repeated every case with automatic detection. Source files, hashes,
references, model bytes, thread count, normalization, and scoring were
unchanged.

| Model and mode | WER | CER | Hypothesis characters | Devanagari characters | Word substitutions / deletions / insertions |
|---|---:|---:|---:|---:|---:|
| Tiny, fixed Hindi | 188.09% | 136.90% | 434 | 0 | 80 / 4 / 74 |
| Tiny, automatic | 100.00% | 100.00% | 0 | 0 | 0 / 84 / 0 |
| Base, fixed Hindi | 101.19% | 105.36% | 299 | 0 | 75 / 9 / 1 |
| Base, automatic | 100.00% | 100.00% | 0 | 0 | 0 / 84 / 0 |

Automatic mode returned empty hypotheses for all five cases with both models;
its 100% WER/CER is therefore deletion-only, not an improvement. Fixed Hindi is
required to obtain any output on this set, but it does not produce native
Devanagari script. Language-mode selection is not the missing remedy.

WER and CER may exceed 100% when insertions combine with substitutions or
deletions. The final hypothesis was reduced to numeric counters and erased; it
was not printed, logged, persisted, or uploaded.

## Interpretation

This five-case result misses the provisional Hindi WER threshold of 22% by a
large margin across 84 reference words. The set remains small, read-speech, and
not representative of conversational/noisy Hindi. It contains both published
gender labels but no speaker identifiers, so it cannot support a speaker-
diversity claim or a production Hindi-quality estimate.

Base reduced word errors from 158 to 85, a 46.2% relative reduction, but its
101.19% WER still misses the provisional 22% gate by a wide margin. Its
artifact is 85.7% larger and this run took 1.97 times as much inference time;
both candidates remained faster than real time on this development host.

The comparison therefore does **not** justify changing the production model.
The next useful work is diagnosis and broader reviewed Hindi evidence rather
than escalating model size again. Selection still requires a passing diverse
dataset, peak-memory and baseline-hardware evidence, approved thresholds, and
human/native-speaker review.

The strict WER/CER failure remains valid for FlowDictate's intended native-
script insertion behavior. Introducing transliteration-aware scoring or an
offline transliteration stage would change product semantics and must not be
done without an explicit product decision and native-speaker acceptance set.

## Nemotron native-streaming feasibility probe

After the like-for-like Whisper comparison, the same five reviewed fixtures and
the same bounded `FleursHindi` scorer were used to evaluate the pinned NVIDIA
Nemotron 3.5 ASR Streaming 0.6B Q8 GGUF. NeMo-Speech.cpp 0.1.0 was built locally
from pinned source with its CPU-ASR profile. The CLI was used only as an
isolated feasibility harness with an explicit local model path, fixed `hi-IN`,
CPU backend, concurrency one, and 160 ms native streaming chunks. It performed
no model pull and emitted no transcript payload to test output.

| Metric | Nemotron Q8 streaming |
|---|---:|
| Artifact bytes | 741,548,352 |
| Fixture count / source duration | 5 / 34.680 seconds |
| Word substitutions / deletions / insertions | 7 / 2 / 1 |
| Word errors / WER | 10 / 11.90% |
| Character errors / CER | 10 / 3.15% |
| Hypothesis characters | 314 |
| Devanagari / ASCII Latin hypothesis characters | 314 / 0 |
| End-to-end RTF including load and warmup | 0.3862 |
| Steady-state streaming RTF after warmup | 0.3368 |
| Model load / warmup | 20 ms / 3,795 ms |
| Peak process working set | 976,093,184 bytes |
| Repeated-output mismatches | 0 |

This narrow result passes the provisional 22% Hindi WER and RTF ≤ 1.0 gates on
the development workstation and corrects the script mismatch observed with
Whisper. It does not establish conversational, noisy, code-switched,
native-speaker, baseline-hardware, or live-partial acceptance.

The candidate therefore advances only to an experimental adapter. The Windows
probe now holds FlowDictate's exact verified handle as an immutable path lease
while the native C API reopens the model, and mutation tests prove write/delete
replacement remains blocked until the lease drops. Non-Windows loading still
fails closed without an equivalent mechanism. Production adoption also remains
blocked by the dedicated bounded worker, the near-1.0 GiB process footprint,
redistribution review, and broader automated/human evidence. ADR 0007 records
these gates. The production worker remains unchanged.
