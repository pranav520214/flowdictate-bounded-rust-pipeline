# Experimental ASR Selection Contract

Status: implemented as a volatile library boundary; no application UI exists.

FlowDictate defaults to `production-whisper`. The experimental Hindi Nemotron
path cannot be selected through the library policy until a caller presents the
current disclosure version and an explicit acceptance. A declined or stale
response creates no opt-in token. On platforms without the reviewed immutable
model-opening boundary, activation fails and Whisper remains selected.

## Required disclosure facts

A future non-technical UI must render every fact exposed by
`ExperimentalNemotronDisclosure::current()` before offering the acceptance
control:

- the backend is experimental and limited to Hindi (`hi-IN`);
- inference is local-only;
- the pinned model is 741,548,352 bytes;
- the reviewed probe measured a 976,093,184-byte peak process working set;
- availability is currently limited to builds with the reviewed Windows gate;
- accepting the model disclosure does not authorize microphone access or
  listening.

The disclosure contract is versioned. Changing a material fact requires a new
version, so a stale acceptance fails closed rather than silently applying to a
different experiment.

## Composition and withdrawal

`LocalAsrSelectionPolicy` is volatile, starts on production Whisper, and owns
no microphone, capture stream, model worker, transcript, user identity, or
timestamp. It consumes the opaque acknowledgement to select Nemotron and can
withdraw that selection immediately and idempotently back to Whisper.

The future application shell must still obtain the separate listening consent
required by the live-session boundary. It must construct a backend only after
both independent decisions have passed. This module intentionally does not
persist acceptance; durable preferences and their deletion controls remain a
later product decision.

## Automated evidence

Unit tests prove the production default, fixed disclosure facts, denial and
stale-version rejection, platform fail-closed behavior, independent listening
consent requirement, and immediate/idempotent withdrawal. The complete offline
workspace suite passes 205 ordinary tests with 15 model/hardware acceptance
tests ignored by default.
