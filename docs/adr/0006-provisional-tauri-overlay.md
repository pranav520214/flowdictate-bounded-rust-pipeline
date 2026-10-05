# ADR-0006: Use a provisional Tauri v2 shell with bundled vanilla UI

## Status
Proposed, conditional on network-isolation and resource tests

## Context
The product needs a tiny cross-platform overlay. Tauri offers native packaging and a small bundled frontend, but it uses a platform WebView with capabilities broader than the UI needs.

## Decision
Prototype a thin Tauri v2 shell and vanilla bundled HTML/CSS/JS. Use no remote origins, no updater, no shell plugin, no arbitrary IPC, no remote asset URLs, `connect-src 'none'`, disabled navigation/new windows, and a minimum capability allowlist. Keep sensitive processing in Rust. Replace Tauri if packaged network-isolation or memory targets fail.

## Consequences

- Positive: rapid cross-platform overlay with little frontend framework overhead.
- Negative: OS WebView adds attack and network-capability surface; platform memory varies.
- Trade-off: Tauri is not accepted merely by design; it must earn release status through measurement.

## Alternatives considered

- Svelte: deferred because the overlay does not yet justify framework/runtime complexity.
- Native per-platform UI: strongest control but higher implementation/audit cost.
- `egui`/other Rust GUI: avoids WebView but needs separate accessibility, text rendering, and resource evaluation.
