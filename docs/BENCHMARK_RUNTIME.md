# Streaming ASR Benchmark Runtime

Status: **End-to-end bounded fixture execution and scoring implemented; speech results unverified**  
Date: 2026-08-28

## Implemented boundary

`flowdictate-pipeline` now emits local inference measurements directly from the
rolling owner:

- canonical samples presented to each local ASR call;
- wall time spent only inside the local transcription backend;
- aggregate inference counts, samples, and duration through streaming pipeline
  and live-session reports.

`StreamingBenchmarkRecorder` accepts borrowed partial/final text and numeric
timing/commit metadata. It produces a numeric-only summary containing:

- partial observations, revisions, revision rate, and revised Unicode scalar
  count;
- time to first non-empty partial;
- Unicode-scalar divergence between the last partial and final text;
- inference call/sample totals, p50/p95 wall time, and real-time factor;
- immutable commit count and generation/sequence ordering violations.

`10_000` basis points represents either a 100% revision rate or an inference
real-time factor of `1.0`, depending on the field.

## Recognition scoring

`measure_recognition` compares a borrowed reviewed reference with a borrowed
final hypothesis and returns numeric-only word/character substitution,
deletion, insertion, total-error, and WER/CER counters. It retains neither
transcript after the call nor any transcript in the result or error.

- WER uses exact case-sensitive tokens separated by Unicode whitespace.
- CER uses exact case-sensitive Unicode scalar values and excludes Unicode
  whitespace, avoiding UTF-8 byte-count distortion for multilingual text.
- Punctuation, casing, and Unicode normalization remain explicit dataset-review
  decisions; the scorer never silently changes text semantics.
- Empty references return an undefined rate rather than division by zero.
- Each side is bounded to the existing 64 KiB transcript limit, 4,096 tokens,
  and 16,384 non-whitespace scalars. Each edit comparison is capped at four
  million dynamic-programming cells.
- Temporary owned scalar buffers are overwritten on drop; edit rows contain
  numeric counters only.

## Privacy and ownership

- Text enters only as a borrowed `&str`.
- The recorder retains at most one previous partial within the existing 64 KiB
  transcript hard limit.
- The previous buffer is overwritten before replacement and on finish/drop.
- Final text is reduced immediately to a numeric divergence and is not retained.
- Numeric inference timings are bounded and preallocated at construction.
- The recorder intentionally implements neither `Clone` nor `Debug`.
- Errors contain fixed categories and never include text, audio, paths, fixture
  IDs, model content, or device data.
- No filesystem, network, logging, telemetry, persistence, microphone, or media
  decoding capability was added.

## Measurement protocol

For exact per-window p50/p95 and real-time factor, feed every
`RollingInferenceReport` whose `inference_ran` field is true into
`observe_inference`, then feed the borrowed `pending_text` into
`observe_partial` with elapsed time from the case start. Feed each emitted
commit's generation and sequence to `observe_commit`. At the user finalization
boundary, call `finish` with the reviewed final transcript.

Streaming pipeline/session reports aggregate multiple inference calls that may
occur in one hardware chunk or stop. They are suitable for end-to-end totals,
but exact per-window percentile input must come from individual rolling reports.
Silence/VAD waiting time remains separate from backend inference time.

## Fixture intake

[`../benches/fixtures.template.csv`](../benches/fixtures.template.csv) defines
the review fields. [`../benches/fixtures.csv`](../benches/fixtures.csv) contains
six approved, checksum-pinned LibriSpeech cases and five commit-pinned FLEURS
Hindi cases; their attribution and derivation are recorded in
[`../benches/FIXTURE_PROVENANCE.md`](../benches/FIXTURE_PROVENANCE.md).

`flowdictate-audio` now parses that exact 21-field header and approved records
under 256 KiB/512-entry limits. It permits only single-component `.wav`/`.txt`
filenames, lowercase SHA-256, HTTPS provenance metadata, known redistribution,
approved public/synthetic classifications, reviewed voice-rights basis,
speech-style/acoustic/language-mix/accent-evidence strata, and bounded
language/rate/channel/duration fields. Cross-field inconsistencies, duplicate
IDs, or duplicate paths fail closed.

Loading independently constrains both files to a regular, non-reparse approved
root, reads them through share-read-only handles, verifies exact hashes, checks
the transcript as bounded UTF-8 (including an explicit empty silence reference),
and decodes only canonical PCM16 or finite
normalized float32 RIFF/WAVE up to 32 MiB, 30 seconds, 96 kHz, and two channels.
URLs are never fetched. Audio/transcript buffers are overwritten on failure or
drop where owned. The admitted fixture proves real reviewed-file intake, but it
does not become a speech-quality result until a configured local model runs it.

## Fixture case execution

`run_verified_benchmark_fixture` consumes one already verified fixture and:

1. maps its reviewed automatic/fixed language policy to the same 23-language
   allowlist used by parent/worker IPC;
2. downmixes mono/stereo decoded samples and, when needed, uses the production
   sinc-resampler boundary to produce mono 16 kHz PCM;
3. optionally mixes fixed-seed white noise at an allowlisted 0/10/20/30/40 dB
   target SNR entirely inside the volatile canonical buffer;
4. rejects empty or greater-than-30-second canonical audio and rejects noise
   scenarios whose source has no measurable signal;
5. invokes a `LanguageConfigurableBackend` exactly once and times only that
   local inference call;
6. reduces the final hypothesis and reviewed reference to numeric WER/CER;
7. drops/overwrites the hypothesis, canonical PCM, and fixture-owned audio/text
   before returning a numeric-only `FixtureBenchmarkSummary`.

The perturbation path uses no external noise corpus, creates no augmented file,
and performs no network access. Its fixed seed and target SNR are returned as
non-sensitive scenario metadata. The original runner remains an unchanged-audio
wrapper around this boundary.

The production `AsrWorker` and `WorkerTranscript` implement the required
backend/transcript traits, so the same runner can use the isolated native worker.
Synthetic tests use a payload-erasing fake and establish orchestration only;
they do not claim model quality or native-worker performance.

## Verified synthetic evidence

Five deterministic recorder tests establish configuration bounds, numeric
stability and latency math, Unicode-scalar rather than UTF-8-byte divergence,
monotonic commit ordering checks, observation/inference limits, and marker-free
errors. Ten synthetic fixture-intake tests cover strict manifests,
diversity/rights consistency, path-like input rejection, exact hashes, UTF-8
transcripts, PCM16/float32 decoding,
container/chunk/shape failures, non-finite samples, and non-regular files. One
repository-fixture test additionally revalidates the approved manifest, hashes,
PCM shape, duration, language policy, classification, and transcript. This
evidence validates intake only; it is not a recognition-quality or
hardware-performance result.

Six recognition-scoring tests establish exact substitution/deletion/insertion
accounting, Unicode-scalar behavior, whitespace stability, explicit casing and
punctuation semantics, empty-reference handling, work limits, and marker-free
errors. These synthetic expected/hypothesis pairs prove metric mechanics only.

Seven end-to-end synthetic fixture cases establish exact 16 kHz pass-through,
stereo 8 kHz downmix/resampling, reviewed language switching, pre-cancelled
no-op behavior, unsupported language rejection, backend/scorer failure mapping,
numeric-only output, marker-free errors, fixed-seed noise reproducibility, and
noise-configuration rejection. A separate synthetic unit case measures the
requested 20 dB SNR without clipping. These prove perturbation mechanics only;
they do not themselves claim model noise quality.

## Reviewed native noise evidence

The ignored `nemotron_noise_matrix` acceptance test borrows the same verified
fixtures and perturbation policy, then feeds the volatile canonical PCM through
the isolated native worker in non-overlapping 160 ms chunks. It retains no
augmented audio and prints no hypotheses. Fixed case progress and aggregate
WER/CER/script/RTF counters are numeric only.

The five reviewed Hindi fixtures passed at 20 dB with 9.52% WER, 1.89% CER,
0.3138 inference RTF, and 315/315 hypothesis characters in Devanagari. At 10 dB
they passed with 20.23% WER, 8.83% CER, 0.3104 inference RTF, and 309/309
hypothesis characters in Devanagari. Exact scope, seed, failure discovery,
privacy properties, and limitations are recorded in
[the dated result](HINDI_NOISE_BENCHMARK_RESULTS_2026-09-02.md).

## Unsupported claims

> Unverified — requires broader reviewed fixtures and human testing.

The narrow deterministic-noise WER/CER and workstation inference RTF above are
established only for the five reviewed read-speech clips. No conversational,
real acoustic-noise, mixed-language, native-speaker, partial-stability,
baseline-hardware p50/p95, CPU/RSS, code-switching, or RTL quality claim is
established.
