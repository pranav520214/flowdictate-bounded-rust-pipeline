# Performance, RAM, and Latency Budgets

Status: **Design targets only — not benchmark results**

The rolling pipeline now measures per-inference canonical sample count and
local backend wall time, and the bounded benchmark recorder can calculate
p50/p95 and real-time factor. These are measurement capabilities, not achieved
target values; no reviewed fixture or baseline-hardware result exists yet.

## Baseline

Dual-core CPU, integrated graphics, 4 GB RAM, CPU-only. FlowDictate must leave sufficient memory and CPU for the focused application and operating system.

## Bounded data budgets

| Buffer/data | Initial design budget | Rationale |
|---|---:|---|
| Native capture ring | ≤ 2 seconds; about 0.73 MiB at 48 kHz stereo `f32`, plus fixed metadata | Absorb scheduling jitter without long sensitive retention |
| Resampler/VAD working buffers | ≤ 4 MiB | Fixed chunked DSP headroom |
| Rolling normalized ASR audio | ≤ 30 s; about 1.83 MiB as mono 16 kHz `f32` | Whisper-compatible bounded context |
| Pending/partial text per segment | ≤ 256 KiB UTF-8 hard cap; normal cap much smaller | Prevent pathological decoder/UI growth |
| Queued finalized segments | 2 | Prevent inference backlog |
| Context input | Default 0; Level 2 hard cap initially 32 KiB | Minimize privacy and model-context risk |
| Profile import | File/entry/string/depth caps defined before parser adoption | Prevent parser/allocation abuse |

## Process memory targets

These are envelope targets to validate, not current measurements:

| Mode | Target steady RSS | Target peak RSS | Composition intent |
|---|---:|---:|---|
| Eco | ≤ 350 MiB | ≤ 500 MiB | Tiny multilingual ASR, deterministic refinement, model unload after idle |
| Balanced | ≤ 700 MiB | ≤ 1.0 GiB | Smallest qualifying ASR warm; optional tiny editor only if evidence supports it |
| Quality | ≤ 1.4 GiB | ≤ 2.0 GiB | User-installed approved larger models; still leaves 4 GB system headroom |

If Tauri/WebView or a model causes a mode to exceed its envelope on baseline hardware, reduce the component or do not offer that mode on that machine. No mode may rely on swap for normal operation.

## Latency budget

| Stage | Warm-path design target |
|---|---:|
| Capture callback | Complete within one callback period; p95 < 25% of period |
| Ring → normalized frame | p95 ≤ 20 ms wall latency |
| VAD decision processing | p95 ≤ one 16 ms frame beyond configured hangover |
| Rolling ASR | RTF p50 ≤ 0.8, p95 ≤ 1.0 on baseline |
| Deterministic cleanup | p95 ≤ 20 ms for capped transcript |
| Optional local editor | Must justify cost; p95 target ≤ 500 ms for capped edit |
| Output validation | p95 ≤ 10 ms |
| Text insertion dispatch | p95 ≤ 50 ms excluding target-app delay |
| End of speech → final text visible | p50 ≤ 1.0 s; p95 ≤ 2.0 s warm |
| First useful partial | p50 ≤ 1.0 s after speech start |

Silence-finalization threshold is reported separately from processing latency so the product does not hide a long wait inside the VAD configuration.

## Metrics policy

Metrics are local, payload-free, bounded, and resettable. Measure monotonic durations, CPU, RSS, queue/ring occupancy, safe error codes, model identifier/hash, and build/environment. Do not include audio, transcript, prompt, context, dictionary, filenames, user/device IDs, or stable cross-session identifiers.

Reports include median, p95, peak and steady RSS, CPU, real-time factor, cold/warm state, failures, hardware, OS, power mode, model hash, runtime version, and build flags. Targets are never presented as achieved results.

## Overload behavior

1. Preserve callback responsiveness and hard memory caps.
2. Drop a complete incoming slot and mark discontinuity rather than block/grow.
3. Cancel optional local refinement before degrading ASR.
4. Segment/finalize bounded work and surface a warning.
5. Stop safely after repeated overload rather than thrash.

## Benchmark milestones

- M1: callback allocation count, ring overflow, resampler/VAD latency and memory.
- M2: ASR load/RTF/window/partial stability and smallest-model decision.
- M3: deterministic/no-LLM rate and optional editor cost.
- M4–5: hotkey/insertion/overlay end-to-end latency and UI RSS/GPU impact.
- M6: SQLCipher/key-store latency and storage sizes without payload logging.
- M8: per-language/RTL rendering and insertion.
- M10: final mode defaults selected from reproducible results.

## Unsupported claims

Unverified — requires human testing: actual low-end responsiveness, thermal behavior, battery impact, integrated-GPU overlay cost, subjective latency, and cross-platform text insertion speed.
