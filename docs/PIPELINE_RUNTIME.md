# Volatile Utterance Pipeline

Status: **Bounded final-utterance orchestration and cancellation implemented**  
Date: 2026-08-27

## Scope

`flowdictate-pipeline` connects the implemented audio/VAD boundary directly to
the process-isolated ASR supervisor. It has no filesystem, database, UI,
network, telemetry, or logging dependency. The crate adds no registry package.

## Ownership flow

```text
exact hardware chunk borrowed from caller
  -> preallocated downmix/resampler/VAD state
  -> exact 256-sample canonical frame paired with each SegmentEvent
  -> bounded start-confirmation pre-roll
  -> one preallocated volatile utterance (maximum 480,000 samples)
  -> borrowed directly by cancellable ASR supervisor call
  -> utterance bytes overwritten and length cleared
  -> validated transcript returned to caller-owned preallocated output
```

The pipeline never stores the caller's hardware input. Application-owned mono,
resampled, VAD-frame, pre-roll, active-utterance, IPC PCM, and transcript byte
buffers are overwritten at their explicit discard/drop boundaries. This is
best-effort memory minimization; it cannot prove removal of compiler, native
runtime, OS, driver, or hardware copies.

## State and bounds

- VAD start-confirmation frames form the only pre-roll and are kept with a
  rolling fixed maximum.
- Exactly one active utterance exists. Its configured complete-frame limit must
  fit the ASR maximum of 480,000 mono 16 kHz samples.
- Event `n` maps to canonical samples `n * 256..(n + 1) * 256`; incomplete
  tails are retained only inside the audio processor.
- Silence and maximum duration dispatch automatically. Hotkey release and
  explicit stop drain the unpadded incomplete tail and dispatch once.
- Discontinuity resets DSP/VAD state and discards the affected utterance.
- Caller transcript storage must have prevalidated spare capacity for the
  maximum finalizations in one input chunk, preventing container reallocation.
- Errors are fixed categories and do not format audio or transcript payloads.

## Cancellation

`CancellationToken` is a cloneable one-shot atomic signal. Cancellation before
processing or dispatch erases pending audio without IPC. During inference the
ASR supervisor polls the token at a 10 ms interval, kills and reaps the native
worker, joins the pipe thread after termination, and launches a verified clean
generation before returning `Cancelled`. If termination/recovery cannot be
confirmed, it returns `RecoveryFailed`.

Cancellation takes precedence over output-capacity errors so a cancelled
session cannot retain audio merely because its caller supplied the wrong
output container.

## Verified evidence

- Six pipeline tests cover bounded confirmation pre-roll, exact dispatch
  length, state violations, hard-limit rejection, cancellation cleanup, and
  cleanup after audio failure.
- Audio tests prove completed VAD events have exact canonical frame slices and
  reject undersized canonical output.
- The real-model optimized release test cancels a maximum-length inference in
  flight and observes `Cancelled`, generation 2, and a changed process ID.
- The full ordinary workspace suite passes offline with 98 tests; warnings-
  denied Clippy, formatting, and doc tests pass.

## Remaining boundary

The bounded library-level live session owner is now implemented and documented
in `SESSION_RUNTIME.md`. A platform hotkey/event loop still needs to compose it
with consent UX. `StreamingDictationPipeline` now routes active canonical frames
through the rolling hypothesis/consensus owner, and `StreamingLiveSession`
exposes borrowed pending text plus caller-owned immutable commits. UI and text
insertion remain unimplemented.
