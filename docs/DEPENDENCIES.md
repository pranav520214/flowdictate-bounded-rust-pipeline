# Dependency and License Inventory

Status: **Audio, ASR, and deterministic-cleanup dependencies adopted; remaining entries are proposals**  
Metadata and lockfile snapshot: 2026-09-04

`Cargo.lock` is the authority for adopted code. The Windows x86-64 graph resolves 74 packages, including eight local workspace packages and 66 registry packages; the cross-target lockfile contains 138 entries, including eight local and 130 registry packages. Its observed license expressions are MIT/Apache-2.0-compatible, plus BSD-3-Clause, ISC, Unlicense, Unicode-3.0, 0BSD, and Zlib alternatives. No HTTP, WebSocket, updater, analytics, crash-reporting, telemetry, or remote-logging client appeared in the resolved dependency tree scan.

Workspace-local pinned tools now provide the first measured gate: `cargo-audit` 0.22.2, `cargo-deny` 0.20.2, and `cargo-cyclonedx` 0.5.9. RustSec reports no advisory/warning/yanked finding in the product lockfile; cargo-deny passes advisories, bans, licenses, and sources; and per-package CycloneDX 1.5 JSON SBOMs are checked in. Generated absolute workstation paths were replaced consistently with `flowdictate:workspace/...` references before the SBOMs were retained.

The latest `cargo-cyclonedx` installation's own release lockfile contains yanked `xml-rs` 0.8.19. This crate is part of the workspace-local SBOM generator only, not `Cargo.lock`, the generated BOM contents, or any FlowDictate binary. Treat this as a low-severity tooling-chain caveat and re-evaluate the generator on its next release.

## Adopted for Milestone 1

| Package | Exact pin | Features | License declaration | Purpose and enforced boundary |
|---|---:|---|---|---|
| `cpal` | 0.18.2 | default features disabled | Apache-2.0 | Native enumeration and paused PCM input stream; no file/network fallback |
| `rtrb` | 0.4.0 | defaults | MIT OR Apache-2.0 | Fixed-capacity two-second SPSC handoff; whole callback batches drop on overflow |
| `rubato` | 5.0.0 | default features disabled | MIT OR Apache-2.0 | Preallocated worker-only resampling to mono 16 kHz |
| `earshot` | 1.2.2 | `std`; defaults disabled | MIT OR Apache-2.0 | Embedded local VAD over exact 256-sample frames |
| `sha2` | 0.10.9 | default features disabled | MIT OR Apache-2.0 | Streaming SHA-256 for the offline model integrity gate |
| `whisper-rs` | 0.16.0 | default features disabled | Unlicense | Isolated CPU-only Rust wrapper over bundled `whisper.cpp`; accepts only a `VerifiedModel` through the FlowDictate adapter |

All six are runtime-local libraries; none provides cloud inference or a network transport. `whisper-rs` resolves `whisper-rs-sys` 0.15.0, whose source package builds bundled MIT-licensed `whisper.cpp` 1.8.3 and uses CMake/bindgen only at build time. Production FlowDictate code denies unsafe Rust; the reviewed native/FFI dependency is linked only into `flowdictate-asr` and its dedicated child executable. `flowdictate-asr-ipc`, `flowdictate-nemotron-ipc`, `flowdictate-pipeline`, and `flowdictate-refine` add no external dependency. The refinement crate deliberately uses only the standard library for its first bounded deterministic pass; Unicode segmentation, normalization, and regex packages below remain proposals. Two test-only allocator shims locally allow unsafe code solely to count allocations while forwarding unchanged to the system allocator.

## Remaining runtime and build candidates

The table below is the remaining proposed direct dependency surface for v1. Versions are discovery snapshots, not approved pins. Adoption requires exact version/feature pinning, a lockfile diff, feature/source/license/advisory review, and an SBOM diff.

## Runtime and build candidates

| Package | Snapshot | License declaration | Purpose and necessity | Sensitive data | Important transitive/native implications |
|---|---:|---|---|---|---|
| `tauri` | 2.11.5 | Apache-2.0 OR MIT | Thin desktop lifecycle and overlay shell; provisional | Partial text/UI state | `wry`/`tao` and OS WebView; WebView networking surface must be locked down and tested |
| `tauri-build` | 2.6.3 | Apache-2.0 OR MIT | Build metadata for the Tauri shell | No | Build-time graph only; must not introduce runtime update/network plugins |
| `tauri-plugin-global-shortcut` | 2.3.2 | Apache-2.0 OR MIT | Cross-platform hold/release hotkey | Hotkey events only | OS global shortcut APIs; minimize Tauri capability allowlist |
| `rusqlite` | 0.40.2 | MIT | Typed SQLite access with SQLCipher feature | Encrypted profile/history | `libsqlite3-sys`, SQLCipher, crypto backend and C toolchain; must verify cipher at runtime |
| `keyring` | 4.1.6 | MIT OR Apache-2.0 | OS-native protection for random database key | Encryption key | Feature-minimize to approved platform stores; Linux Secret Service availability varies |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Typed internal configuration/manifests/profile schemas | Model/profile metadata | Derive proc macros at build time; all untrusted schemas need size/depth limits |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | Model manifest and explicit profile interchange | Profile/model metadata | Parser is memory-backed; enforce file and collection limits before parsing |
| `toml` | 1.1.4+spec-1.1.0 | MIT OR Apache-2.0 | Local non-sensitive settings | Privacy settings, never secrets | Reject unknown fields where security-relevant; cap file size |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 | Streaming SHA-256 model integrity checks | Model bytes | RustCrypto graph; digest is integrity, not authenticity by itself |
| `zeroize` | 1.9.0 | Apache-2.0 OR MIT | Best-effort clearing of owned secrets/buffers | Keys; selected buffers | Cannot clear OS/runtime/compiler copies; do not overclaim |
| `secrecy` | 0.10.3 | Apache-2.0 OR MIT | Restrict accidental key exposure/formatting | Encryption keys | Wraps application-owned secrets; use with `zeroize` |
| `thiserror` | 2.0.20 | MIT OR Apache-2.0 | Typed errors without embedding sensitive values | No payload by policy | Derive proc macro; error variants use safe codes/metadata only |
| `tracing` | 0.1.44 | MIT | Structured non-sensitive operational events | Must never receive sensitive fields | Compile-time field discipline and privacy-marker regression tests required |
| `tracing-subscriber` | 0.3.23 | MIT | Local release log formatting/filtering | Must never receive sensitive fields | Disable environment-driven verbose modes in release; bounded local retention |
| `unicode-normalization` | 0.1.25 | MIT OR Apache-2.0 | Deterministic multilingual normalization | Transcript | May create string copies; use bounded inputs and define normalization policy |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 | Grapheme-safe cleanup and cursor handling | Transcript | Bounded CPU/input; required for multilingual and RTL-safe operations |
| `regex` | 1.13.1 | MIT OR Apache-2.0 | Precompiled deterministic cleanup rules | Transcript | Compile rules once; no user-provided regex; linear-time engine still needs input bounds |
| `enigo` | 0.6.1 | MIT | Fallback text insertion where native adapters are unavailable | Final text | OS automation/accessibility privileges; keep behind typed plain-text interface and platform review |

## Feature-gated or deferred candidates

| Package | Snapshot | License | Decision gate |
|---|---:|---|---|
| `llama-cpp-2` | 0.1.154 | MIT OR Apache-2.0 | Optional only if deterministic refinement misses measured quality; FFI/native graph and model licenses reviewed separately |
| `hound` | 3.5.1 | Apache-2.0 | Not adopted: the benchmark-only canonical WAV gate is dependency-free; reconsider only for an explicitly reviewed broader diagnostic-import boundary |
| `claxon` | 0.4.3 | Apache-2.0 | Deferred FLAC import/export feature; bounded decode wrapper and fuzzing required |
| `audiopus` | 0.2.0 | ISC | Deferred Opus recording feature; native libopus/license/package audit required |

## Development-only candidates

| Package | Snapshot | License | Purpose |
|---|---:|---|---|
| `criterion` | 0.8.2 | Apache-2.0 OR MIT | Repeatable microbenchmarks; never linked into release |
| `proptest` | 1.11.0 | MIT OR Apache-2.0 | State-machine/parser/refinement properties |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | Isolated tests only; tests assert no sensitive normal-path temp files |
| `cargo-fuzz` / `libFuzzer` toolchain | pin later | Apache-2.0 OR MIT / toolchain terms | Offline fuzz harness, not a product dependency |

## Native and model-license gates

| Component | Expected license | Gate before distribution |
|---|---|---|
| `whisper.cpp` | MIT | Pin source commit, verify bundled source list and build flags, exclude FFmpeg/examples |
| SQLCipher Community Edition | BSD-style upstream terms; revalidate exact bundle | Verify source/license notices, crypto provider, export/package obligations, cipher runtime check |
| OpenSSL if vendored | Apache-2.0 | Prefer OS provider where supportable; otherwise inventory/patch vendored native code |
| libopus if enabled | BSD-3-Clause upstream; revalidate package | Keep outside live inference path; ship notices and fuzz wrapper |
| ASR/refinement model weights | Per-model license | Record exact license, source, architecture, size, quantization, runtime compatibility, and SHA-256 in allowlist |

## Explicit exclusions

No HTTP client, updater, analytics SDK, crash-reporting SDK, remote logger, WebSocket client, embedded remote content, FFmpeg, MP3/AAC/video codec, shell-execution crate, or general plugin loader is proposed.

## Verification commands for the adoption gate

```text
cargo metadata --locked --format-version 1
cargo tree --locked -e features
cargo audit
cargo deny check advisories bans licenses sources
cargo cyclonedx --locked
```

The audit initially recorded on 2026-08-26 and refreshed for the current lockfile is in [`DEPENDENCY_AUDIT_2026-08-26.md`](DEPENDENCY_AUDIT_2026-08-26.md). Re-run the tools whenever `Cargo.toml` or `Cargo.lock` changes; a clean historical result is not a permanent guarantee.

Tool outputs are evidence for a specific lockfile, not permanent guarantees. CI may download build inputs; the resulting production application must be tested independently for network isolation.

## Primary sources

- Crate license fields use SPDX expressions as documented by Cargo: https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields
- `whisper.cpp`: https://github.com/ggml-org/whisper.cpp
- `rusqlite` SQLCipher feature behavior: https://github.com/rusqlite/rusqlite
- Earshot: https://github.com/pykeio/earshot
- keyring-rs: https://github.com/open-source-cooperative/keyring-rs
- SQLCipher: https://github.com/sqlcipher/sqlcipher
