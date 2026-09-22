# Local ASR benchmark evidence — 2026-08-28

Status: **Six-case clean-English regression evidence; not a production-quality claim**

## Six-case normalized regression result

The approved set contains six utterances from four LibriSpeech `dev-clean`
speakers, totalling 36.460 seconds. Each case ran once through the same isolated
worker in fixed English mode. Scoring used the reviewed `LibriSpeechEnglish`
policy documented in
[`FIXTURE_SET_EXPANSION_REVIEW.md`](FIXTURE_SET_EXPANSION_REVIEW.md).

| Metric | Result |
|---|---:|
| Fixture count | 6 |
| Total source duration | 36.460 seconds |
| Total inference time | 7,976,908 microseconds |
| Per-case inference p50 | 1,287,515 microseconds |
| Per-case inference p95 (maximum of 6) | 1,418,279 microseconds |
| Corpus RTF | 0.2187 |
| Reference words | 113 |
| Word substitutions / deletions / insertions | 6 / 1 / 1 |
| Word errors | 8 |
| Normalized WER | 7.07% |
| Reference characters | 470 |
| Character errors | 24 |
| Normalized CER | 5.10% |

This passes the provisional `≤15%` WER gate for the exact clean-English
regression set. It does not validate conversational or accented dictation,
noise robustness, partial-hypothesis stability, multilingual quality, or the
target baseline hardware.

## Post-admission revalidation — 2026-09-01

After the fixture manifest gained one separate, then-unmeasured FLEURS Hindi intake
case, the optimized English regression test was changed to select the six
English entries explicitly and rerun. It completed with the same 8 word errors,
7.07% WER, 24 character errors, and 5.10% CER. This run measured 9,398,356
microseconds total inference time and corpus RTF 0.2577; timing variation does
not replace the original dated result or establish baseline-hardware latency.
The Hindi entries were not included in this English recognition run.

## Initial exact-text smoke baseline

## Reviewed inputs

- Fixture: `librispeech-dev-clean-1272-135031-0009`
- Fixture source: OpenSLR SLR12 `dev-clean.tar.gz`, archive MD5
  `42e2234ba48799c1f50f24a7926300a1`
- Fixture shape: 1.910 seconds, mono, 16 kHz, PCM16
- Fixture license: CC BY 4.0
- Model: `asr-whisper-tiny-multilingual-q5_1`
- Model SHA-256:
  `818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7`
- Runtime: pinned local `whisper.cpp` 1.8.3 isolated worker, release build,
  fixed English mode
- Runs: five consecutive inferences in one clean local worker generation

The fixture loader verified the manifest, paths, hashes, transcript, language
policy, and WAV shape before each run. The model gate verified the model path,
size, identity, compatibility tuple, and SHA-256 before worker startup. No
network, audio persistence, transcript logging, or hypothesis output occurred.

### Numeric-only result

| Metric | Result |
|---|---:|
| Inference p50 | 1,254,756 microseconds |
| Inference p95 (maximum of 5) | 1,343,784 microseconds |
| RTF p50 | 0.6569 |
| RTF p95 (maximum of 5) | 0.7035 |
| Word errors | 5 |
| Exact-text WER | 100.00% |
| Character errors | 17 |
| Exact non-whitespace Unicode-scalar CER | 94.44% |

Recognition metrics were identical across all five runs. Exact-text scoring is
deliberately case- and punctuation-sensitive and performs no dataset-specific
normalization. The final hypothesis was reduced to these counters and erased;
it was not printed or stored.

### Interpretation

The initial exact-text result demonstrates why normalization policy must be
declared before comparing models. It remains useful as a casing/punctuation-
sensitive baseline, but it must not be compared directly with the normalized
six-case result. The pinned-inventory preflight in
[`MODEL_COMPARISON_PREFLIGHT.md`](MODEL_COMPARISON_PREFLIGHT.md) found no smaller
complete multilingual candidate and no measured reason to download a larger
one yet. The next benchmark step is separately reviewed conversational, accent,
noise, and multilingual evidence; a larger-model comparison is conditional on
the current smallest candidate missing an approved mandatory gate.
