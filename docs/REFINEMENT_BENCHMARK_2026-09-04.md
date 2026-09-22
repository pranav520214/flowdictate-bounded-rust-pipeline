# Deterministic Refinement Benchmark — 2026-09-04

Status: **Current-development-host evidence; documented baseline not yet tested**

## Method

The release-mode `refinement_benchmark` example performs 128 warm-up passes and
2,048 measured passes over a fixed synthetic 65,535-byte transcript. It measures
the bounded model-free cleanup and zero-copy validation stages separately using
the monotonic Rust clock. Output contains only fixed labels, byte counts,
durations, and the no-model rate; it never prints transcript content.

Five fresh processes were measured. Windows process working set was sampled in a
tight parent-process loop, and both the largest sampled value and the OS-reported
peak were retained. This is useful development evidence, but it is not a whole
application memory result and short-lived peaks below the polling interval may
still be missed.

## Host

```text
CPU: AMD Ryzen 5 7600, 6 cores / 12 logical processors
RAM: 16,228,163,584 bytes
OS: Microsoft Windows 11 Pro 10.0.26200 (build 26200)
Build: cargo release profile, locked and offline
```

This host does not match the required dual-core CPU / 4 GB RAM baseline.

## Results

Each run completed 2,048/2,048 iterations with zero stderr bytes and a 10,000
basis-point no-model rate.

| Run | Cleanup p50 | Cleanup p95 | Validation p50 | Validation p95 | Process peak working set |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.3743 ms | 0.5024 ms | 0.0635 ms | 0.0717 ms | 4,263,936 B |
| 2 | 0.3710 ms | 0.5278 ms | 0.0635 ms | 0.0686 ms | 4,071,424 B |
| 3 | 0.3699 ms | 0.5309 ms | 0.0641 ms | 0.0700 ms | 4,026,368 B |
| 4 | 0.3737 ms | 0.5440 ms | 0.0642 ms | 0.0714 ms | 4,259,840 B |
| 5 | 0.3719 ms | 0.5479 ms | 0.0641 ms | 0.0747 ms | 4,268,032 B |

All five current-host p95 results are below the design budgets of 20 ms for
deterministic cleanup and 10 ms for output validation. This does not establish
the baseline-hardware gate, end-to-end latency, CPU utilization, steady-state
application memory, semantic quality, or optional-editor cost.

## Reproduction

```text
cargo build -p flowdictate-refine --example refinement_benchmark --release --locked --offline
target/release/examples/refinement_benchmark.exe
```

The parent measurement process should launch the executable directly and record
its peak working set; measuring `cargo run` would include Cargo rather than only
the benchmark process.
