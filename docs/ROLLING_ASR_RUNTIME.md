# Rolling ASR Runtime

Status: **Bounded rolling PCM inference and consensus integration implemented**  
Date: 2026-08-27

## Scope

`RollingInference` in `flowdictate-pipeline` owns one preallocated mono 16 kHz
PCM window, one local `TranscriptionBackend`, and one `ConsensusCommitter`. It
accepts exact 256-sample canonical-frame multiples, schedules bounded partial
inference, feeds every hypothesis into consensus, and erases PCM that consensus
has made safe to discard.

This is a library component. It does not open a microphone, create a thread,
touch the filesystem, use the network, log, persist, render UI, or insert text.
`StreamingDictationPipeline` and `StreamingLiveSession` now connect exact active
canonical frames to this owner. The original `LiveSession` remains available as
the verified final-utterance compatibility path.

## Bounds and cadence

- The complete PCM capacity is reserved at construction; pushes within the
  configured maximum do not grow the window.
- The default first inference starts after 16,384 samples (about 1.024 seconds).
- The default cadence requires 8,192 new samples (about 512 milliseconds).
- One `push` runs at most one inference, preventing burst catch-up work.
- All sizes are exact multiples of the 256-sample VAD frame.
- The maximum cannot exceed 480,000 samples (30 seconds at 16 kHz).
- The hard-window limit stops atomically. It never silently drops uncommitted
  audio; the owner must finalize, cancel, or reset the segment.
- Caller-owned commit storage must have a spare slot before inference starts.

## Ownership flow

```text
borrowed exact canonical frames
  -> preallocated rolling PCM window
  -> borrowed by local process-isolated ASR backend
  -> bounded validated transcript hypothesis
  -> monotonic consensus observation
  -> immutable commit delta in caller-owned storage
  -> PCM before (commit timestamp - configured overlap) overwritten and erased
```

Only uncommitted PCM and the consensus engine's bounded uncommitted transcript
suffix are retained. Finalization performs one final inference, emits only the
remaining suffix, then erases the complete segment. Reset restarts the absolute
sample clock; discontinuity and cancellation preserve the clock but advance the
consensus generation so stale partial state cannot join a later segment.

## Language-generation boundary

The production backend exposes only `Automatic` or one of 23 compiled fixed
languages. `StreamingDictationPipeline::set_language_mode` first compares the
current policy. An unchanged policy preserves both worker and consensus
generations. A changed policy erases DSP/VAD pre-roll, rolling PCM, pending
hypothesis text, canonical scratch, and the session sample clock before asking
the process supervisor for a clean worker generation. A replacement failure
cannot restore erased transcript/audio state.

## Failure behavior

- Empty, misaligned, non-finite, or out-of-range PCM fails closed and erases
  volatile audio and pending text.
- ASR failure, cancellation, invalid hypotheses, and timestamp overflow erase
  volatile state and return fixed payload-free errors.
- Consensus or trim failure rolls back any commit appended during that call.
- Hard-window and output-capacity failures leave the prior state unchanged so
  the caller can safely finalize or retry with adequate capacity.
- PCM and owned hypothesis/commit text buffers are overwritten at explicit
  discard and drop boundaries as best-effort memory minimization.

## Verified evidence

Eight deterministic rolling tests cover configuration/alignment, cadence,
stable commit, no-overlap trimming, configured-overlap retention, finalization,
atomic capacity/window limits, malformed PCM, ASR and hypothesis failure,
cancellation, discontinuity, reset, cleanup, and generation changes.

Five streaming-pipeline and nine streaming-session tests cover confirmation
pre-roll routing, live pending hypotheses, immutable commits, bounded ring
drains, stale-audio erasure, output-capacity atomicity, unpadded finalization,
fresh session tokens, cancellation, discontinuity, capture pause failure, and
generation-safe language replacement.

Five additional synthetic benchmark tests cover bounded observation/text
configuration, payload-minimal revision/latency/RTF math, Unicode-scalar
divergence, commit-order violations, and marker-free fixed errors. Rolling and
streaming reports now carry numeric inference sample/time counters; no payload
or persistence path was added.

The tests use fake local backends and synthetic PCM only. Actual partial-text
quality, latency, CPU/memory behavior with continuous live speech, and target
hardware remain unverified until benchmark fixtures and human hardware testing
are completed.
