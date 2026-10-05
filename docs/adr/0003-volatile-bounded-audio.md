# ADR-0003: Keep normal-path audio volatile and bounded

## Status
Proposed

## Context
Microphone audio is highly sensitive and can create denial-of-service risk when sessions or decoders are unbounded.

## Decision
Use a preallocated SPSC ring from the real-time callback to an audio worker. Never write normal-path audio to disk. Bound ring capacity, utterance duration, rolling ASR window, and queued segments. Finalize or cancel on release, silence, explicit stop, or maximum-duration boundary.

## Consequences

- Positive: predictable memory and greatly reduced storage leakage.
- Negative: overload may drop frames; crash recovery cannot recover volatile audio.
- Trade-off: privacy and bounded behavior take priority over perfect recovery.

## Alternatives considered

- Temporary WAV files: rejected because they create recoverable sensitive artifacts and races.
- Unbounded in-memory capture: rejected because stuck shortcuts/VAD can exhaust memory.
