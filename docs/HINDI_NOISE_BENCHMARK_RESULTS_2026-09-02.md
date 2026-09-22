# Hindi Nemotron Noise Benchmark — 2026-09-02

Status: **Passed for this narrow reviewed matrix; not a broad Hindi-quality or
hardware-acceptance claim**

## Scope

The ignored acceptance test ran the five hash-verified, commit-pinned FLEURS
Hindi read-speech fixtures through the compiled experimental Nemotron Q8 model,
the immutable Windows model lease, the isolated native worker, and the compact
non-overlapping 160 ms streaming protocol. Deterministic white noise was mixed
only in bounded volatile mono 16 kHz memory at 20 dB and 10 dB target SNR.

The worker was reused across every fixture and both noise levels. The final run
used the normal 30-second per-request deadline. Model loading and process startup
are excluded from the reported inference RTF.

## Numeric results

| Target SNR | Fixtures | Source duration | WER | CER | Inference RTF | Reference words | Word errors | Reference characters | Character errors | Hypothesis characters | Devanagari | ASCII Latin |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 dB | 5 | 34.68 s | 9.52% | 1.89% | 0.3138 | 84 | 8 | 317 | 6 | 315 | 315 | 0 |
| 10 dB | 5 | 34.68 s | 20.23% | 8.83% | 0.3104 | 84 | 17 | 317 | 28 | 309 | 309 | 0 |

The fixed seed base was `5065510917279533395`. Per-fixture seeds also include
the SNR and numeric fixture index. The earlier clean feasibility probe on this
same five-case set measured 11.90% WER, 3.15% CER, and 0.3368 steady-state RTF;
the small set is not sufficient to interpret the 20 dB result as an improvement.

## Failure found and corrected

The first reused-process run failed closed: the default 30-second guard returned
`InferenceTimedOut`. A diagnostic run with the compiled 120-second maximum then
returned `IpcFailed` on numeric fixture index 1. Running each case after a clean
process reset passed, localizing the fault to native stream reuse rather than the
verified fixture or perturbation boundary.

The adapter constructed a replacement native stream before releasing the old
stream. The worker now destroys the finished stream first and only then creates
the next stream, preventing overlap of large RNNT stream contexts. Its streaming
geometry was also aligned with the pinned runtime's low-latency preset: 160 ms
chunks, 1.92-second CTC padding fields, and RNNT right-context 1. After rebuilding,
the complete matrix passed in one continuously reused process under the normal
30-second guard.

## Privacy properties

- Fixture rights and hashes pass the strict reviewed intake boundary before use.
- Noise and canonical PCM exist only in an opaque bounded owner that overwrites
  its samples on drop; no augmented recording is written.
- Hypotheses are reduced to numeric WER/CER/script counters and then dropped.
- Test output contains only fixed labels, numeric case indices, and numeric
  aggregates. It contains no transcript, audio, device label, or user path.
- The test and worker perform no network access.

## Reproduction

```text
cargo test -p flowdictate-pipeline --test nemotron_noise_matrix -- --ignored --nocapture
```

This requires the separately reviewed local model and previously built pinned
native worker. The normal workspace suite does not download or distribute them.

## Limits and remaining acceptance

This is deterministic synthetic white noise over five public read-speech clips
on one Windows workstation. It does not establish conversational, spontaneous,
accented, Hinglish/code-switched, real-room/noise, native-speaker, microphone,
partial-stability, low-end hardware, redistribution, or packaged-network
acceptance. Nemotron therefore remains experimental and production still
defaults to Whisper.
