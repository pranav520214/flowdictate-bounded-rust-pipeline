# Dependency and Stack Security Audit

## Project: FlowDictate

Initial date: 2026-08-26; updated: 2026-09-04  
Scope: Windows x86-64 Rust workspace and locked dependencies

## Stack inventory

| Component | Version | Role | Status |
|---|---:|---|---|
| Rust/Cargo | 1.97.1 | Build/test toolchain | Measured locally |
| `flowdictate-audio` | 0.1.0 | Local bounded capture/DSP/VAD library | Workspace-only |
| `flowdictate-asr-ipc` | 0.1.0 | Bounded ASR binary protocol and worker supervisor | Workspace-only |
| `flowdictate-asr` | 0.1.0 | Bounded local native ASR adapter | Workspace-only |
| `flowdictate-nemotron-ipc` | 0.1.0 | Experimental bounded Nemotron process contract | Workspace-only |
| `flowdictate-pipeline` | 0.1.0 | Volatile VAD-to-process-ASR orchestration | Workspace-only |
| `flowdictate-refine` | 0.1.0 | Bounded deterministic transcript cleanup | Workspace-only; no dependencies |
| `flowdictate-asr-worker` | 0.1.0 | Silent process boundary for native inference | Workspace-only |
| `flowdictate-capture-probe` | 0.1.0 | Consent-first human hardware probe | Workspace-only, not a product runtime |
| CPAL | 0.18.2 | Native PCM input | Exact pin |
| Earshot | 1.2.2 | Embedded local VAD | Exact pin |
| RTRB | 0.4.0 | Bounded SPSC handoff | Exact pin |
| Rubato | 5.0.0 | Worker-side resampling | Exact pin |
| SHA-2 | 0.10.9 | Streaming model SHA-256 integrity check | Exact pin |
| `whisper-rs` | 0.16.0 | Safe wrapper for local `whisper.cpp` | Exact pin; default features disabled |
| `whisper-rs-sys` | 0.15.0 | Native build/FFI with bundled `whisper.cpp` 1.8.3 | Exact transitive resolution |
| `cargo-audit` | 0.22.2 | RustSec lockfile audit | Workspace-local tool |
| `cargo-deny` | 0.20.2 | Advisory/license/ban/source policy | Workspace-local tool |
| `cargo-cyclonedx` | 0.5.9 | CycloneDX SBOM generator | Workspace-local tool; caveat below |

Windows resolution contains 74 packages: eight local workspace packages and 66 registry packages. `Cargo.lock` contains 138 package entries: eight local and 130 registry packages with registry checksums. The Nemotron IPC and deterministic-refinement boundaries added only local packages and did not expand the registry dependency surface. No Git dependency, alternate registry, optional direct dependency, override, patch, CI workflow, container, infrastructure-as-code file, or private package source is present.

## Known vulnerabilities

| Package | Installed | Finding | Reachability | Remediation |
|---|---:|---|---|---|
| Product graph | Locked | No RustSec advisory, warning, unsound, unmaintained, or yanked finding | Runtime/build graph | Re-run on every lockfile change |

`cargo-audit --deny warnings` loaded 1,226 current RustSec advisories and exited successfully. `cargo-deny` independently passed the advisory policy with zero errors, warnings, or ignored advisory findings. No CVE suppression or advisory ignore exists; the only cargo-deny exception is the separately documented build-only duplicate-version entry.

## License and source policy

`cargo-deny` passed licenses and sources with zero errors or warnings. Allowed SPDX identifiers are narrowly recorded in `deny.toml`: 0BSD, Apache-2.0, BSD-3-Clause, ISC, MIT, Unlicense, Unicode-3.0, and Zlib. Registry source is limited to crates.io; unknown registries and Git sources are denied. Wildcard dependencies are denied.

The first policy run found the local probe depended on `flowdictate-audio` by path without a version. That was fixed by adding the exact `=0.1.0` requirement; the locked offline rerun passed.

The ASR graph contains build-only `shlex` 1.3.0 through bindgen and 2.0.1 through `cc`/CMake. These incompatible major requirements cannot currently unify. `deny.toml` explicitly skips only `shlex@1.3.0` during duplicate detection with the dependency-chain reason recorded inline; unmatched skip entries warn so the exception becomes visible when it is no longer needed.

## Supply-chain risks

| Severity | Component | Evidence | Remediation |
|---|---|---|---|
| Low | `cargo-cyclonedx` installation only | Release 0.5.9's own lockfile pins yanked `xml-rs` 0.8.19 | Do not distribute `.tools`; re-evaluate/replace generator on its next release |
| Low | Native/platform boundary | CPAL uses Windows COM/audio APIs and a build script | Keep CPAL exact-pinned; retain narrow sample/config allowlist and live hardware tests |
| Medium | Native ASR parser/inference | `whisper-rs-sys` compiles bundled `whisper.cpp` 1.8.3 and GGML C/C++; model parsing crosses FFI | Exact pins/default features off, independent in-memory model re-verification, bounded IPC/input/output, parent kill/restart deadline; add packaged signature/sandbox and fuzzing |
| Low | Windows binding generation | `whisper-rs-sys` needs libclang; bundled fallback bindings failed Windows ABI layout checks | Generate on Windows with pinned libclang; never use the incompatible fallback on this target |
| Low | Build-time macros/scripts | Custom builds: CPAL, `num-traits`, `proc-macro2`, `quote`; proc macros: `visibility`, `windows-implement`, `windows-interface` | Inventory again on lockfile changes; CI must use locked inputs |
| Informational | No repository/CI metadata | Lockfile exists locally but no VCS/CI can enforce it | Choose a private repository before adding SHA-pinned CI |

The SBOM tool caveat does not enter FlowDictate's `Cargo.lock`, SBOM component list, or release probe. The workspace-local `.tools` directory is ignored and is not added to PATH.

## SBOM evidence

- `crates/flowdictate-audio/flowdictate-audio.cdx.json`: CycloneDX 1.5 JSON, 38 components, 39 dependency nodes.
- `crates/flowdictate-asr-ipc/flowdictate-asr-ipc.cdx.json`: CycloneDX 1.5 JSON, 39 components, 40 dependency nodes.
- `crates/flowdictate-asr/flowdictate-asr.cdx.json`: CycloneDX 1.5 JSON, 68 components, 69 dependency nodes.
- `crates/flowdictate-nemotron-ipc/flowdictate-nemotron-ipc.cdx.json`: CycloneDX 1.5 JSON, 40 components, 41 dependency nodes.
- `crates/flowdictate-pipeline/flowdictate-pipeline.cdx.json`: CycloneDX 1.5 JSON, 42 components, 43 dependency nodes.
- `crates/flowdictate-refine/flowdictate-refine.cdx.json`: CycloneDX 1.5 JSON, 0 dependency components, 1 dependency node.
- `tools/flowdictate-asr-worker/flowdictate-asr-worker.cdx.json`: CycloneDX 1.5 JSON, 69 components, 70 dependency nodes.
- `tools/flowdictate-capture-probe/flowdictate-capture-probe.cdx.json`: CycloneDX 1.5 JSON, 39 components, 40 dependency nodes.
- Strict SPDX parsing was enabled. Cargo-cyclonedx completed but reported legacy slash-form license metadata for `cexpr`, `minimal-lexical`, and `version_check`; cargo-deny's SPDX/license-file evaluation accepted their licenses under policy.
- All eight JSON documents parse successfully.
- Cargo-cyclonedx embedded the absolute local workspace path in generated BOM references. Those references were consistently replaced with `flowdictate:workspace/...`; retained SBOMs contain no username or absolute workstation path.

## Static release inspection

The offline release probe build imports Windows kernel, synchronization, COM, multimedia-device, WinRT error, Visual C++ runtime, math, locale, stdio, and heap DLLs. The optimized ASR worker imports Windows kernel/runtime, synchronization, cryptographic primitives, environment, filesystem, stdio, and heap DLLs. Neither statically imports WinHTTP, WinINet, Winsock, DNSAPI, URLMon, or another named network runtime. This is static evidence only; dynamic behavior still requires monitored runtime observation.

## Model artifact evidence

The separately downloaded multilingual Whisper tiny `q5_1` artifact is not a
Rust package and therefore does not appear in the Cargo SBOM. Its independent
provenance record is `models/whisper-tiny-q5_1/MODEL_ORIGIN.md`. The local
32,152,673-byte file matches upstream SHA-256
`818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7`
and passes the production compiled-manifest gate. The pinned adapter then
loaded those same verified bytes with bundled `whisper.cpp` 1.8.3 and completed
one second of synthetic-silence inference in debug and optimized release tests.
The process acceptance tests also completed a worker round trip, proved a
forced 10 ms deadline kills and replaces the native process, and proved an
explicit in-flight cancellation does the same. These tests do not establish
adversarial parser safety or speech quality.

## Prioritized action plan

1. Human gate: run the consent-first probe with DNS/socket observation, then test permission denial and device removal.
2. Re-run this audit whenever either manifest or `Cargo.lock` changes; do not add advisory exceptions without reachability evidence and human review.
3. Replace or refresh `cargo-cyclonedx` when a release no longer pins the yanked tool-only XML crate.
4. Before distribution, authenticate/sign the packaged worker, add an OS-appropriate sandbox/resource policy, and test the installed parent/worker pair against substitution attempts.
5. Establish a private repository and SHA-pinned CI that runs locked/offline-capable tests, cargo-audit, cargo-deny, and reproducible SBOM generation.
