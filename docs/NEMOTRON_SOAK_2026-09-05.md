# Nemotron bounded soak and failure-recovery evidence

Date: 2026-09-05  
Status: **100-session validation complete; sanitizer execution blocked**

## Scope and baseline

This slice preserves the existing `finish -> drain -> destroy -> idle` owner.
It adds numeric accounting, bounded observed clean shutdown, model-free fault
tests, and a manual real-model soak. It does not change decoding configuration,
model identity, consent, backend selection, UI, packaging, or Qwen integration.

Before editing, formatting, strict offline all-target/all-feature Clippy, the
ordinary workspace suite (216 passed, 15 ignored), and native CTest (5/5)
passed. The earlier [lifecycle report](NEMOTRON_LIFECYCLE_REGRESSION_2026-09-05.md)
remains historical evidence, including the original noisy-run failure.

## Implementation and test tiers

- `native/nemotron-worker/main.cpp`: numeric counters at actual native handle
  acquisition, successful finish, completed destruction, and runtime errors;
  statistics command 4 returns fixed-size response 0x85 with six u64 values.
  The existing sole-owner implementation in `utterance.h` is unchanged.
- `crates/flowdictate-nemotron-ipc/src/lib.rs`: deadline-bound typed statistics
  query validates request identity, frame length and counter consistency.
  `shutdown` consumes the owner, checks idle/balanced counters, closes pipes,
  and observes exit status. After the statistics query (normal request deadline
  and recovery policy), EOF exit has a separate five-second deadline. Failure
  drops the owner and terminates/reaps any remaining child. Leases outlive exit.
- `crates/flowdictate-nemotron-ipc/tests/native_soak.rs`: actual production
  supervisor, immutable verified model lease and rebuilt production native
  executable. Synthetic 3.2-second tone/silence utterances enter twenty 160 ms
  pushes plus finish. Hypotheses are dropped without inspecting their text.
- `crates/flowdictate-pipeline/tests/support/process_memory.rs`: existing
  test-only working-set query extended with numeric handle/thread snapshots;
  query-only Win32 handles are closed. No new package dependency was added.
- `native/nemotron-worker/tests/{fake_asr.cpp,boundary.py,CMakeLists.txt}`:
  production adapter compiled against a test-only synthetic C ABI, using the
  pinned public header. The Python runner uses only the standard library.
  Neither fake backend nor runner is linked into the production executable.

| Tier | Coverage / execution |
|---|---|
| Ordinary smoke | Existing 100 utterances through actual supervisor and synthetic process |
| Ordinary ownership | Existing 1,000 instrumented native-owner cycles |
| Ordinary recovery | Repeated cancellations, timeouts, stale responses and owned-child kills |
| Model-free adapter boundary | 22 malformed/failure/shutdown cases, one CTest entry |
| Manual native short | **100 real-model sessions executed successfully** |
| Manual native extended | 1,000 supported; not executed or claimed in this slice |

`FLOWDICTATE_NEMOTRON_SOAK_SESSIONS` explicitly selects 100..2000 sessions;
default 100, invalid values fail. Resource snapshots occur after session 1 and
every 10 sessions for the short tier, or every 50 for larger tiers, plus the
final session. Lifecycle counters and unchanged worker generation/PID are
checked after **every** completed utterance. Production per-request timeout
remains 30 seconds. No retry or per-utterance process reset masks a failed run.

Test-only material-growth guards are 64 MiB above the first completed-session
working set, eight additional handles and two additional threads relative to
that same sample. These are coarse regression alarms, not a leak detector or
product memory budget. The maximum count bounds the run; it is not a promise
that a 2,000-session run has been validated.

## Final real-model result

Command, using the existing reviewed model and rebuilt production worker:

```text
cargo test -p flowdictate-nemotron-ipc --test native_soak production_worker_native_soak --locked --offline -- --ignored --nocapture
```

Exit 0; 1 manual test passed in **123.88 seconds**. No real-model soak failed
before this successful result; no failure was erased by retrying. The stimulus
was synthetic PCM, not live speech or a repeated reviewed corpus. This is
real-model lifecycle/inference stability evidence, not speech-quality evidence.

| Metric | Observed |
|---|---:|
| Sessions requested / completed | 100 / 100 |
| Audio push/finish requests completed | 2,100 |
| Synthetic audio processed | 320 seconds |
| Worker starts / clean exits | 1 / 1 |
| Worker crashes / restarts / disables | 0 / 0 / 0 |
| Request deadline / timeouts | 30,000 ms / 0 |
| Streams created / finished / destroyed | 100 / 100 / 100 |
| Maximum simultaneous / final active streams | 1 / 0 |
| Native stream errors | 0 |
| Cancellation / timeout events in this normal soak | 0 / 0 |

Native counters measure this worker generation only. Cancellation/timeout event
counts belong to the supervisor/harness: a forcibly killed worker cannot report
destructor completion. Those fault tests below establish OS process termination
and clean replacement, not destructor execution under forced kill.

## Recovery and boundary evidence

All 13 supervisor integration tests passed, including these five additions:

- `repeated_cancellation_stress_then_clean_shutdown`: 60 active cancellations,
  interleaved with 30 successful utterances; generation 61, fresh zero-active
  state after each cancellation and balanced clean shutdown.
- `repeated_timeout_recovery_stays_bounded`: 10 injected request timeouts using
  the existing synthetic process seam and a 100 ms test deadline, each followed
  by a successful utterance; exactly one replacement per failure. This does not
  change the real-model/product 30-second guard.
- `killed_idle_and_inflight_worker_recover_without_stale_results`: externally
  terminates only the test-owned worker while idle and during a blocked request;
  both IPC failures recover, generation 3, successful next utterance and exit.
- `stale_response_id_cannot_complete_replacement_session`: 20 simulated
  previous-request responses rejected, each followed by clean replacement and
  successful new utterance. Old pipes are discarded, IDs remain monotonic.
- `shutdown_rejects_active_stream_and_bounds_unresponsive_exit`: active shutdown
  fails closed; a synthetic child that ignores EOF for ten seconds triggers the
  five-second exit guard and cleanup instead of silently hanging.

New ordinary tests also include
`statistics_reject_truncation_stale_ids_and_unbalanced_counts` (all frame
truncations, wrong ID, impossible counts/overlap) and
`soak_iteration_configuration_is_bounded`.

CTest `production_adapter_malformed_and_failure_matrix` passed 22 cases:
wrong startup version/magic, oversized model-path length, unknown command, zero
request ID, truncated PCM, zero/oversized/extreme audio length, idle/duplicate
finish, replayed request ID, non-finite/out-of-range audio, push after finish,
stats framing, active EOF, explicit shutdown, create error returning a non-null
handle, decode error, finish error, final drain error returning an owned result,
and malformed final result. Compound cases cover several assertions. The fake
C ABI asserts expected creation counts, exactly-once stream destruction,
released recognizer/results, no overlap, and bounded arguments at each call.
Malformed input must be rejected before any prohibited native invocation.
Duplicate finish with a fresh request ID is a valid idle no-result operation;
replaying an ID is rejected; push after finish starts a fresh stream.

The first external-kill test attempt received Windows `ERROR: Access denied`
inside the sandbox. It passed after an approved, targeted outside-sandbox run,
and the final complete suite ran with that same permission. This was a test
environment restriction, not a native recovery failure. An initial PowerShell
runner could not execute under host script policy; it was replaced with the
standard-library Python runner without changing execution policy.

## Performance

| Scope | Microseconds |
|---|---:|
| Verified model hashing/lease acquisition | 15,457,093 |
| Worker launch through ready | 94,053 |
| Cold first complete utterance | 1,257,138 |
| Warm 99 utterances: p50 | 1,057,261 |
| Warm p90 | 1,182,112 |
| Warm p95 | 1,211,602 |
| Warm p99 / max | 1,275,500 / 1,275,500 |
| Total native push/finish request round trips | 107,670,102 |
| Total utterance outer-loop overhead | 56,095 |
| Statistics query plus clean EOF shutdown | 61,740 |

Percentiles use nearest rank. Request timers include native processing,
pipe transport, thread scheduling and owned-response cleanup; they are **not
pure native inference**. Complete-utterance timers additionally include
synthetic sample preparation. Statistics queries and resource snapshots occur
outside those timers. Model readiness does not mean every mapped weight/cache
has already been faulted in; first-utterance work is separate. Synthetic
utterances differ from the earlier Hindi clips, so these timings are not an
accuracy or speed comparison against that corpus. No native compute timer or
isolated IPC-only benchmark was fabricated.

## Resource behavior

Working set is the Windows coarse RSS-like measure. Sampling is not continuous.

| Metric | Observed |
|---|---:|
| Post-model ready working set | 43,573,248 bytes |
| First completed-session baseline | 795,054,080 bytes |
| Minimum post-session working set | 790,196,224 bytes |
| Maximum sampled working set | 795,238,400 bytes |
| Final pre-shutdown working set | 790,454,272 bytes |
| Final minus completed-session baseline | -4,599,808 bytes |
| Post-session resource snapshots | 11 |
| Initial / maximum sampled / final handles | 88 / 91 / 91 |
| Initial / sampled minimum / maximum / final threads | 6 / 6 / 9 / 6 |

No monotonic working-set growth was observed over 100 sessions at the sampled
intervals. This is **not proof of leak freedom**. Allocator caches, mapped model
pages and OS residency are included. Transient within-request peaks may be
missed; pre-model process RSS was not captured. Handle samples count all types
together, not attribution to a particular pipe/event/DLL. Threads remained
bounded within the observed range and returned to six, but individual native
pool/thread lifetimes were not traced.

## Sanitizers: not executed

Probes used only the existing local toolchains and wrote build intermediates
under `target/nemotron-lifecycle-tests`; no sanitizer runtime was shipped.

MSVC x64 **19.44.35228**, installed tool directory
`C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207`:

```text
cl.exe /nologo /std:c++20 /EHsc /W4 /WX /fsanitize=address /Zi /MD native/nemotron-worker/tests/lifecycle.cpp /Fe:target/nemotron-lifecycle-tests/asan-lifecycle.exe /Fo:target/nemotron-lifecycle-tests/asan-lifecycle.obj /Fd:target/nemotron-lifecycle-tests/asan-lifecycle.pdb
LINK : fatal error LNK1104: cannot open file 'clang_rt.asan_dynamic_runtime_thunk-x86_64.lib'
```

The required thunk import library is absent from that installation's `lib/x64`.
The installed MinGW-w64 GCC **13.2.0**, `C:/Strawberry/c/bin/g++.exe`, also failed:

```text
g++.exe -std=c++20 -Wall -Wextra -Werror -fsanitize=address,undefined -fno-omit-frame-pointer native/nemotron-worker/tests/lifecycle.cpp -o target/nemotron-lifecycle-tests/asan-ubsan-lifecycle.exe
ld.exe: cannot find -lasan: No such file or directory
ld.exe: cannot find -lubsan: No such file or directory
collect2.exe: error: ld returned 1 exit status
```

Neither an instrumented lifecycle binary nor an instrumented model worker ran.
Resolving this requires a reviewed development toolchain/runtime installation
with the missing sanitizer libraries; it is not a reason to modify production
release DLL staging. No sanitization findings were suppressed.

## Privacy and containment limits

No transcript, hypothesis, source/augmented/noisy PCM, prompt, or textual diff
was persisted. The soak writes only numeric aggregate output and has no file
write path. The synthetic stack PCM is cleared after each push; parent PCM and
hypothesis owners retain their existing wipe/drop policy. The reviewed speech
corpus was not consumed for this soak. Internal native-cache/pagefile erasure
is not established by these measurements.

Active pipe EOF and normal shutdown cleanup were tested against the production
adapter and instrumented C ABI. Abrupt parent death during stuck real native
compute was **not** proven safe: no Windows kill-on-parent-close Job Object is
implemented in this worker/supervisor. Pipe EOF cannot interrupt a permanently
stuck native call. This remains a containment risk, not a process-management
redesign in this slice. Transport uses inherited private pipes and bounded
version/request-ID checks; this report does not claim cryptographic IPC peer
authentication, executable signing, or packaged OS sandboxing.

## Final normal gates and milestone status

Executed with the existing MSVC environment and local LIBCLANG_PATH:

```text
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo test --locked --offline --workspace --quiet
ctest --test-dir target/nemotron-lifecycle-tests --output-on-failure
```

All exited 0: **223 Rust tests passed, 0 failed, 16 ignored; native CTest 6/6**.
The native production build also passed with warnings as errors. To enable the
optional model-free boundary target, configure its test build with
`-DNEMO_SPEECH_SOURCE_DIR=<reviewed-local-source>`; CMake locates the existing
Python interpreter. Real-model tests remain manual/ignored.

This closes the bounded 100-session stability check, stream accounting,
repeated model-free recovery/stale-response coverage, native malformed-input
checks, coarse resource observation, and bounded clean-exit evidence. It does
not close unlimited/1,000-session native stability, sanitizer execution,
abrupt-parent containment, baseline/live hardware, broader speech/acoustic
coverage, native-speaker/human acceptance, redistribution review, non-Windows
leases, UI selection composition or packaged network observation. Milestone 2
remains **in progress**, not production-ready or acceptance-complete.
