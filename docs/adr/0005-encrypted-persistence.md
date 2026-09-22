# ADR-0005: Use SQLCipher with OS-protected key custody

## Status
Proposed

## Context
Optional history and personalization can contain sensitive transcripts and vocabulary. A database encrypted with a nearby key offers little protection.

## Decision
Default history to off. When persistent storage is enabled, generate a random database key, store it only through an approved OS credential backend, use SQLCipher, verify cipher availability at runtime, and fail closed to stateless operation if either control is unavailable. Never derive keys from speech or transcript content.

## Consequences

- Positive: database theft alone does not reveal content; key custody uses native user protections.
- Negative: platform packaging and recovery are harder; a compromised logged-in user remains in scope as residual risk.
- Trade-off: persistent features may be unavailable on unsupported systems rather than stored weakly.

## Alternatives considered

- Plain SQLite: rejected for transcript/profile persistence.
- Key file beside database: rejected.
- User-spoken passphrase: rejected because authentication material must not derive from spoken words.
