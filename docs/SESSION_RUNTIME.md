# Bounded Live Session Runtime

Status: **Final-only and rolling library-level session owners implemented**  
Date: 2026-08-27

## Scope

`LiveSession` in `flowdictate-pipeline` owns one already-built paused capture,
its `AudioConsumer`, the volatile dictation pipeline, fixed native-chunk
scratch, cancellation, and the listening state. It provides the composition
boundary for a future hotkey/UI adapter without adding a filesystem, database,
network, telemetry, logging, or cloud path.

The session does not enumerate or open a microphone. The caller builds a
paused `CaptureStream` and must call `start` only after the product has received
an explicit consented listening interaction. No background or implicit start is
permitted by the contract.

## State and ownership

```text
Idle
  -> set_language_mode: erase queued/volatile state, replace worker generation
  -> start: erase stale ring data, create fresh cancellation token, resume
  -> Listening
     -> drain_once: consume at most one exact native chunk
     -> hotkey_released / stop: pause, bounded final drain, finalize -> Idle
     -> cancel: cancel ASR, pause, erase scratch/ring/pipeline -> Idle
     -> capture pause failure: drop capture, erase scratch/ring -> Faulted
```

- Start erases any audio queued before the authorized interaction.
- Each scheduler drain fills or processes at most one exact hardware chunk.
- The ring capacity bounds stop-time draining; no unbounded catch-up loop exists.
- A discontinuity-epoch change discards the affected read, clears partial native
  input, and resets DSP/VAD/utterance state before later audio is processed.
- Stop discards an incomplete native hardware chunk instead of padding or
  retaining it. The pipeline still includes its own already-produced unpadded
  canonical tail when an active utterance is finalized.
- Only `PipelineOutput` final transcripts leave the owner. Audio is never
  returned, persisted, or included in errors.
- A fresh one-shot cancellation token is installed for each start. A UI thread
  may clone it to interrupt in-flight native inference.
- If the OS refuses to pause capture, the capture object is immediately dropped
  before the bounded ring is drained and the session becomes non-restartable.
- Automatic/fixed language changes are accepted only in `Idle`. Listening and
  faulted sessions reject them without changing the current mode or pending
  state. A changed mode erases queued and volatile state before worker restart;
  reapplying the current mode is a no-op.

## Capacity contract

Callers preallocate transcript output storage using
`maximum_outputs_per_drain` for normal scheduling and
`maximum_outputs_per_stop` before hotkey release or explicit stop. Capacity is
validated before audio is consumed. Reports contain only lifecycle flags and
numeric counters; they never contain audio, transcript text, device metadata,
paths, or model content.

## Verified evidence

Seven focused session tests cover stale pre-start queue erasure, fresh tokens,
one-chunk drain bounds, discontinuity reset, final-only hotkey output,
cancellation of scratch and queued audio, external-token fail-closed behavior,
and capture destruction after a pause failure. (The stale-queue/fresh-token
behaviors share one test.)

The complete ordinary workspace suite passes offline with 98 tests. Formatting,
warnings-denied Clippy across all targets/features, and documentation tests also
pass. Real microphone permission, device loss, pause semantics, scheduling, and
acoustic behavior remain human hardware gates.

## Remaining boundary

This is a library component, not a runnable dictation application. A platform
hotkey/event loop must compose the appropriate owner with the consent UX. The
legacy `LiveSession` remains final-only, while the dedicated rolling equivalent
is described below. Overlay, output validation/refinement, safe text insertion,
packaging, installed-worker authentication/sandboxing, and the live-hardware
privacy test remain unimplemented or unverified.

## Rolling session path

`StreamingLiveSession` applies the same explicit-start, bounded-drain,
pause/fault, cancellation, stale-ring, and discontinuity rules to
`StreamingDictationPipeline`. It differs at the output boundary:

- active exact canonical frames enter rolling inference after bounded speech
  confirmation pre-roll;
- `pending_text` is a borrowed view of the bounded uncommitted hypothesis, not
  a queued or cloned event payload;
- stable/final text leaves only as immutable `ConsensusCommit` deltas appended
  to caller-owned preallocated storage;
- scheduler reports contain numeric counters and a `hypothesis_updated` flag,
  never transcript text;
- hotkey release and explicit stop include the unpadded canonical tail in final
  inference;
- cancellation, discontinuity, external token cancellation, ASR/consensus
  failure, or capture pause failure erase pending PCM and hypothesis state.

Nine focused streaming-session tests cover live pending/commit flow, bounded
drain capacity, stale pre-start erasure, fresh tokens, finalization,
discontinuity, local/external cancellation, capture pause failure cleanup, and
idle-only language generation reset.

## Experimental Nemotron session path

`ExperimentalNemotronLiveSession` applies the same explicit-start and bounded
ring ownership to `ExperimentalNemotronPipeline`. It is a separately named
opt-in library type and does not alter the production Whisper session.

- Construction accepts only an already-paused capture; it never enumerates or
  opens a microphone.
- `start` first erases stale audio and requires a clean native worker generation.
  Reset failure drops the capture owner, enters `Faulted`, and proves that
  `resume_capture` was never called.
- Each drain consumes at most one fixed hardware chunk. Discontinuities erase
  scratch, DSP/VAD state, native partials, and require clean worker recovery.
- Partial text is exposed only as a borrowed view. Final native transcripts are
  moved directly into caller-owned preallocated storage without an internal
  history or event queue.
- Stop work is bounded by ring capacity; incomplete hardware samples are
  overwritten rather than padded, while the DSP/VAD canonical tail is included.
- Cancellation, processing failure, pause failure, and drop overwrite volatile
  scratch and drain the bounded ring. A failed recovery makes the owner faulted.

Eight focused tests cover explicit start through partial/final transfer, local
and external cancellation, capacity-before-consumption atomicity,
discontinuity reset, pause-failure capture destruction, and the
worker-reset-before-resume privacy invariant. Actual microphone permission,
driver behavior, acoustic quality, and user-visible experimental selection
remain unverified.
