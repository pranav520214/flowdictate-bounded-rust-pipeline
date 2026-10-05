# Nemotron utterance lifecycle regression evidence

Date: 2026-09-05  
Status: **Bounded lifecycle slice complete; Nemotron remains experimental**

## Defect and restored guardrail

The original 30-second noisy run exposed a repeated-utterance timeout/native
failure. Correcting RNNT right-context to the pinned low-latency value of 1
alone did not resolve it. The historical 120-second diagnostic returned an IPC
failure; restarting between fixtures avoided the failure. Adapter inspection
identified replacement-stream construction while the previous native stream
was still alive. Destroy-before-create ordering then allowed the single-worker
matrix to pass. These historical failures remain evidence, not discarded runs.

Before this slice's code changes, the restored default 30-second test was rerun
and completed successfully: 1 test passed in 41.09 seconds, ten cases, one
worker; 20/10 dB WER 9.52%/20.23%, CER 1.89%/8.83%, all counted hypothesis
characters Devanagari. The product-shaped matrix was rerun after the final
native rebuild and workspace checks; its final evidence appears below.

## Production ownership invariant

The recognizer/model outlives the utterance owner. At most one native utterance
stream exists per worker; idle means **no stream**, not an eagerly allocated
replacement. Normal utterances reuse the worker process.

```text
Idle -> first valid push creates stream -> Streaming
     -> Finish -> DrainFinal -> DestroyStream -> Idle
```

- `native/nemotron-worker/utterance.h`: noncopyable owner; start rejects active
  and finalizing states before invoking the factory. Finish transfers the
  handle into a local RAII guard. Its destruction precedes reopening the start
  gate, including exception unwinding; reentrant start/finish is rejected.
- `native/nemotron-worker/main.cpp`: starts with no stream; creates only on
  valid PCM, drains a final into a separate result owner, destroys the stream
  before acknowledging finish, and waits for the next push to create another.
  Native handles returned on error are owned immediately. Malformed native
  results now terminate the owning scope instead of continuing with bad state.
- `crates/flowdictate-nemotron-ipc/src/lib.rs`: cancellation of an active stream
  before dispatch now kills/reaps the old worker and establishes a clean
  generation. Cancellation is rechecked when a response arrives. Missing or
  non-final finish responses for an active utterance fail closed and recover.
  An interrupted transcript-frame read wipes its partial buffer before error.

Native pointers remain in C++ rather than introducing Rust FFI. `Stream` and
`Result` are unique owners with the matching C ABI deleters. Raw borrows must
not escape their owner. The pinned `src/asr/c_api.cpp` implementation stores
exactly one pending final on finish; stream_next moves it into an independent
result allocation and clears the pending slot. Thus one final drain is correct
for this pin and that result can outlive stream destruction. Revisit this
contract on any native-runtime upgrade.

| Exit path | Cleanup / recovery |
|---|---|
| Normal finish | Drain independent final; RAII destroys stream before response; remain idle in same worker |
| Stream creation or partial decode error | Immediately owned handles and enclosing scope clean up; worker terminates |
| Finish/drain failure | Local finalization guard destroys stream; worker terminates |
| Malformed native result or failed response write | Reject and exit owning scope; supervisor recovers on error/disconnect |
| Timeout or active cancellation | Parent terminates and reaps worker before one bounded replacement attempt |
| IPC disconnect | Parent terminates any remaining child and attempts one clean replacement |
| EOF, shutdown, invalid command, caught C++ exception | Owning scope unwinds; stream precedes recognizer destruction |
| Explicit supervisor reset | Terminate/reap old child before replacement |
| Cancellation while idle | Return cancelled without creating a stream or restarting the worker |

Forced process termination uses OS resource reclamation, **not a claim that
C++ destructors execute on kill**. Invalid caller PCM is rejected before IPC;
it does not falsely mark an existing active utterance idle. The bounded
recovery policy remains unchanged. Neither per-fixture process resets nor a
120-second acceptance override remain. The generic configuration API's existing
120-second maximum is not the default or an acceptance-test override.

## Exact regression tests

Model-free CTest tests exercise the same production `Utterance` owner:

1. `sequential_and_repeated_utterances`: 1,000 create/finish/drain/destroy cycles;
   exact order, zero overlap, exactly-once destruction, and reentrant rejection.
2. `creation_and_decode_failure_cleanup`: failed creation and active-scope exit.
3. `finalization_and_drain_failure_cleanup`: failed finalization callback closes
   the handle and permits fresh ownership.
4. `cancel_then_reuse`: abandoned finalization followed by a clean new owner.
5. `shutdown_and_exception_cleanup`: unwinding and active-scope shutdown.

Ordinary Windows Rust integration tests use the actual production supervisor
with a small synthetic protocol process, not a real model:

1. `repeated_utterances_reuse_one_process_with_fresh_state` (100 utterances)
2. `timeout_kills_and_restarts_before_reuse`
3. `cancelled_active_stream_is_destroyed_before_next_utterance`
4. `cancellation_during_decode_restarts_cleanly`
5. `decoder_error_and_disconnect_restart_cleanly`
6. `explicit_reset_discards_active_stream`
7. `malformed_result_and_nonfinal_finish_restart_cleanly`
8. `idle_cancellation_does_not_restart_worker`

Sources: `native/nemotron-worker/tests/{CMakeLists.txt,lifecycle.cpp}` and
`crates/flowdictate-nemotron-ipc/tests/{supervisor_lifecycle.rs,support/worker.rs}`.
The native tests need a C++20 compiler/CMake but no vendor library or model.
Rust tests compile the fixture process with the local Rust toolchain. Its only
temporary file payloads are the executable and a three-byte non-sensitive model
stub; no decoded speech or augmented audio is written. These process tests are
Windows-only because the production immutable model-path lease is Windows-only.

## Normal gates and manual command

Executed offline on the development Windows host with the existing MSVC x64
environment and local LIBCLANG_PATH configured:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --all-targets --all-features --locked --offline --quiet
cmake --build target/nemotron-worker-build --config Release
cmake -S native/nemotron-worker/tests -B target/nemotron-lifecycle-tests -G Ninja
cmake --build target/nemotron-lifecycle-tests
ctest --test-dir target/nemotron-lifecycle-tests --output-on-failure
```

All exited 0: formatting and strict Clippy passed; **216 ordinary Rust tests
passed, 0 failed, 15 ignored**; native build passed; **5/5 CTest tests passed**.
The Rust total includes the eight new supervisor tests plus existing workspace
coverage; it is not a claim that every test was introduced by this slice.

The separate manual/model-dependent test remains ignored by default:

```text
cargo test -p flowdictate-pipeline --test nemotron_noise_matrix --locked --offline -- --ignored --nocapture
```

Final run: exit 0, 1 test passed in 40.27 seconds. The native executable was
rebuilt with the final owner implementation before this run. No downloads,
live microphone use, or model changes were required.

## Final real-model aggregates

Five reviewed Hindi clips at each fixed-seed synthetic SNR; 34,680 ms source
audio per stratum. Ten cases passed, zero failed; one worker generation and
unchanged process ID throughout; **30,000 ms request deadline**, zero observed
worker crashes and zero timeouts. Script validity: 100% of counted hypothesis
characters were Devanagari. A failing test exits before success totals.

| SNR | Cases | WER | CER | Utterance-path elapsed | Utterance-path RTF |
|---|---:|---:|---:|---:|---:|
| 20 dB | 5 | 9.52% | 1.89% | 11,590,369 us | 0.3342 |
| 10 dB | 5 | 20.23% | 8.83% | 11,442,078 us | 0.3299 |

| Timing scope | Observed |
|---|---:|
| Model verification plus cold worker startup | 16,935,218 us |
| First request round trip | 990 us |
| Cold first complete utterance | 1,591,633 us |
| Warm utterances (9), nearest-rank p50 | 2,195,947 us |
| Warm p95 / max | 3,870,030 / 3,870,030 us |
| Total native worker push/finish + IPC round trips | 23,024,817 us |
| Measured outer-loop overhead | 7,630 us |

These are **real-model wall-clock measurements**, not synthetic control-plane
benchmarks. Request timings include IPC/thread scheduling and native work.
The first push can buffer audio without running inference, so its 990 us is
not cold model inference latency. The cold complete utterance includes model
work. Pure internal native compute was not separately instrumented. The outer
loop difference includes memory probes and bookkeeping; fixture loading,
perturbation, and accuracy scoring are outside the utterance timer. RTF uses
full utterance-path elapsed time; it is not a pure kernel/inference RTF.

## Coarse memory evidence

The ignored harness queries Windows process working set with a small local
Win32 helper in `tests/support/process_memory.rs`; no profiling dependency was
added. It samples after each push and after finish acknowledgment, which now
occurs after native stream destruction.

| Sample | Working-set bytes |
|---|---:|
| Before first stream | 43,560,960 |
| First active stream, first push | 43,802,624 |
| Next stream, first push | 795,545,600 |
| Maximum sampled during pushes | 795,688,960 |
| After first stream destruction | 795,545,600 |
| After last stream destruction | 795,688,960 |
| Post-destruction minimum / maximum | 795,545,600 / 795,688,960 |

Post-destruction samples were not strictly increasing at every utterance.
Their range was 143,360 bytes across ten utterances. Model residency, native
allocator caching, and process bookkeeping are included; retained working set
does not mean the destroyed stream still exists. These coarse samples do not
prove leak freedom or capture a transient peak inside a native call. Exact
allocation/destruction ordering is covered separately by the instrumented
owner tests, not inferred from working-set changes.

## Privacy and remaining risk

No source/decoded transcript, partial hypothesis, per-fixture textual diff, or
noisy PCM/waveform artifact was persisted by this slice. The matrix has no
write path and emits only aggregate numeric metrics; previous per-case progress
logging was removed. Existing reviewed fixture files were read, not copied or
modified. Rust-owned PCM and hypothesis buffers retain their existing wipe/drop
policy, including partial frame-read failure. Native stream/result allocations
use their pinned C ABI destruction functions; complete erasure of internal
native caches or OS paging is not established by these tests.

AddressSanitizer/UndefinedBehaviorSanitizer verification remains unavailable:
the attempted local MinGW sanitizer link failed because `-lasan` and `-lubsan`
runtime libraries were missing. Earlier MSVC ASan limitations remain recorded
in the runtime report. No sanitizer success is claimed and no toolchain was
downloaded to bypass this limitation.

This closes the bounded lifecycle implementation/regression slice, not
Milestone 2 or production readiness. Long-run native leak/fuzz testing, runtime
upgrades, non-Windows process/lease behavior, baseline/live hardware, broader
speech/noise coverage, native-speaker acceptance, and packaged network-isolation
observation remain unverified. No UI, installer, Qwen, download, or new ASR
feature work was undertaken for this slice.
