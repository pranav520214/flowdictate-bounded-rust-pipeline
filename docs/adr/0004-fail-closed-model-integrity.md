# ADR-0004: Fail closed before loading models

## Status
Proposed

## Context
Model files are executable-adjacent untrusted input parsed by native runtimes. Warnings without enforcement do not protect integrity.

## Decision
Accept only normalized regular-file paths under approved model roots. Validate identifier, architecture, runtime version, exact file size, and streaming SHA-256 against a compiled-in signed release manifest before passing a path/handle to a runtime. Unknown or mismatched models are not loaded.

## Consequences

- Positive: corruption and unapproved substitution fail closed.
- Negative: legitimate user-converted models require an explicit local trust/import workflow and new allowlist entry.
- Trade-off: flexibility is subordinate to integrity.

## Alternatives considered

- Warning-and-load: rejected.
- Arbitrary runtime plugins: rejected.
- Hash only after load: rejected because the parser would already be exposed.
