# ADR-0002: Make the production runtime network-free

## Status
Proposed

## Context
Normal operation must not disclose data or depend on connectivity. A dormant cloud fallback or generic updater weakens both auditability and user trust.

## Decision
Do not add network-client crates, remote SDKs, telemetry, analytics, remote assets, update checking, or DNS-dependent behavior to the runtime. Keep acquisition/build tooling separate. Enforce UI CSP/navigation denial and test the packaged process with outbound traffic blocked.

## Consequences

- Positive: smaller exfiltration surface and deterministic offline behavior.
- Negative: model/application updates are manual; Tauri's OS WebView capability remains a residual concern.
- Trade-off: convenience is explicitly subordinate to local ownership.

## Alternatives considered

- Disabled-by-default updater in the main binary: rejected because it retains a generic network path.
- Remote fallback for quality/availability: rejected by the product invariant.
