# Milestone 3 Implementation Report

Status: **In progress — deterministic cleanup, dictionary, spoken rules, routing, and production/native/streaming final-ASR composition implemented**  
Date: 2026-09-04

## Completed slice

The workspace now contains `flowdictate-refine`, a zero-third-party-dependency
library for conservative deterministic transcript cleanup. The public boundary
provides validated byte limits, one bounded linear pass, an opaque wipe-on-drop
result owner, a payload-free error taxonomy, and non-content cleanup counters.

The implemented rules cover duplicate/Unicode whitespace, leading/trailing
whitespace, an explicit punctuation-spacing subset, and conservative ASCII
sentence-initial capitalization. Missing separators are not inferred, so the
pass does not split URLs, decimals, or abbreviation-like tokens. Hindi text is
covered for whitespace and danda adjacency without applying Latin case rules.

A separate zero-copy output validator now enforces explicit source/output byte
ceilings, additive and proportional growth limits, empty-source behavior,
control-scalar rejection, explicit bidirectional-formatting rejection, and
Unicode-noncharacter rejection. Its deterministic profile permits no growth.
Ordinary Hindi/Urdu and shaping joiners remain valid; prompt and shell syntax
are returned unchanged as inert text. These checks do not claim semantic
equivalence for a future editor model.

Before an optional editor candidate can reach the sink, a deterministic,
bounded semantic safety verifier preserves the multiplicity of protected
numbers, URLs, email-like tokens, paths, flags, and identifiers. Mutations or
deletions are rejected without exposing candidate content in errors or reports;
the verifier is a safety signal, not a proof of semantic equivalence.

The pinned Qwen3 `Q4_K_M` artifact is now loadable through a dedicated,
binary-framed llama.cpp child worker. A smoke request returned the unchanged
`ship v2` text with no stdout diagnostics. The worker is experimental: its Rust
deadline/kill/recovery supervisor, immutable executable/model leases, and
end-to-end `LocalEditor` composition remain open gates.

The model-free composition now returns deterministic cleanup for ordinary text
and a minimal sanitized-raw result only when strict cleanup encounters a
non-whitespace control, bidi-formatting control, or Unicode noncharacter. The
fallback preserves visible punctuation and casing, removes only unsafe scalar
categories, collapses whitespace, and owns a single wipe-on-drop buffer.
Oversize/configuration/allocation failures remain errors rather than being
reported as successful fallback.

The confidence/complexity decision now has a metadata-only contract. Four
named semantic-editing conditions are stored in one byte; no transcript text
crosses the router. It selects the optional-editor path only when a condition
is present and the explicit local-editor gate is ready. The default, disabled,
unavailable, and no-condition states all remain model-free. No model, numeric
threshold, allocation, logging, persistence, or network path was introduced.

A narrow local-editor interface now completes the runtime-independent fallback
contract. The editor borrows text only on a justified ready route and writes
only through a byte-capped wipe-on-drop sink. Every candidate crosses the
explicit output validator. Runtime failure, empty output, or invalid output
destroys the candidate and falls through deterministic cleanup to sanitized raw
text. Fixed failure/report types contain no payload. Production, native, and
completed-streaming final adapters expose the same hierarchy; native partials
are rejected before text or editor access. No concrete model, runtime binding,
prompt cache, dependency, or download was introduced.

An explicit local dictionary now accepts at most 256 borrowed entries with
256-byte spoken and written fields. Matching is case-sensitive, whole-token or
whole-phrase, and longest-first. Duplicate/conflicting spoken forms, unsafe
fields, and output growth beyond the caller's validation policy fail before a
successful result is returned. The dictionary has no persistence, logging,
learning, serialization, or network path. Abbreviation expansion happens only
through an explicit caller-provided entry.

Spoken-rule handling is literal by default. The separately selected,
versioned `EnglishV1` policy collapses only consecutive repeats of `uh`, `um`,
`erm`, and `hmm`, preserving the first token. It recognizes only `new line`,
`new paragraph`, and `bullet point`, emits LF and plain `- ` markers, and
preserves intentional repetition and other languages. LF output is accepted
only when the caller selects the narrow LF-only validation policy. Dictionary
and spoken-rule composition are final-only for production, native, and
completed-streaming outputs.

The final ASR composition seams are now explicit: a finalized production
`PipelineOutput<WorkerTranscript>` can run model-free refinement and must pass
the deterministic no-growth validator before returning its opaque result.
Private `PipelineOutput` fields prevent callers from directly forging this
finalized wrapper. The experimental-native adapter separately checks `is_final` before
borrowing transcript text and sends only a confirmed final through the same
shared cleanup/validation function. A successful streaming stop or hotkey
release now issues a private one-shot boundary for the exact commit count
emitted since start. The streaming adapter consumes that boundary and the fresh
caller-owned commit vector, rejects incomplete/reused/reordered commit sets,
checks the total byte cap before assembly, then wipes both the temporary source
and consumed commits. Live hypotheses remain ineligible. Fifteen focused tests
cover normal cleanup,
sanitized fallback, bounded payload-free failure, both final contracts, and
pre-borrow partial rejection plus completed, missing, reused, and oversized
streaming commit paths, dictionary composition, and literal-default spoken
formatting. They also cover accepted and failed local editing, native partial
non-access, and completed-streaming editor authorization. The live-session
integration test crosses the issued stop boundary into the same validated
output owner.

## Verification

```text
cargo test -p flowdictate-refine
PASS — 50 tests, 0 failed

cargo clippy -p flowdictate-refine --all-targets -- -D warnings
PASS

cargo test -p flowdictate-pipeline --all-targets --locked --offline
PASS — 97 ordinary tests, 0 failed; 2 model/hardware acceptance tests ignored

cargo fmt --all -- --check
PASS

cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
PASS

cargo test --workspace --all-targets --all-features --locked --offline --quiet
PASS — 205 ordinary tests, 0 failed; 15 model/hardware acceptance tests ignored

cargo-audit 0.22.2 audit --no-fetch --deny warnings
PASS — 1,226 advisories loaded; no findings across 138 lock entries

cmake/Ninja Qwen3 native worker build and binary-framed smoke request
PASS — llama.cpp static library and `flowdictate-qwen-refiner-worker.exe`
build; pinned Qwen3 load and `ship v2` request returned `ship v2` with no
stdout diagnostics

cargo-deny 0.20.2 --offline check advisories bans licenses sources
PASS

CycloneDX 1.5 JSON parse and workstation-path scan
PASS — all 8 package SBOMs parse; pipeline SBOM has 42 components/43 dependency nodes; no C:/Users, RYZEN, or file:/// value
```

The Windows graph is now 74 packages: eight local workspace packages and the
same 66 checksum-pinned registry packages. `Cargo.lock` has 138 entries: eight
local and the same 130 registry entries. This slice added no registry, Git,
network, telemetry, updater, logging, or persistence dependency.

A numeric-only release benchmark now exercises a synthetic 65,535-byte input
for 2,048 measured iterations per process. Across five fresh processes on the
AMD Ryzen 5 7600 / 16-GB development host, cleanup p95 was 0.5024–0.5479 ms,
validation p95 was 0.0686–0.0747 ms, and the largest OS-reported process peak
working set was 4,268,032 bytes. All are within the stage budgets on this host,
but it is not the required dual-core/4-GB baseline and is not an end-to-end or
semantic-quality result.

## Privacy and security result

The cleanup path borrows input, holds only one owned output, and returns that
output through a type without `Clone` or `Debug`. Initialized output bytes are
overwritten on drop; errors never contain input; reports expose only bounded
numeric and boolean metadata. Successful output validation borrows the exact
candidate without allocating or copying it and exposes no content through its
error/report types. No transcript is printed or persisted by the implementation
or its tests.

Sanitized fallback owns only its bounded output, overwrites initialized bytes
on drop, and exposes removal counts rather than removed content. A partial
strict-cleanup owner is dropped before fallback is constructed.

The router accepts only named metadata, defaults to the model-free route, and
fails closed to that route unless the optional local editor is explicitly ready.
Its copyable state contains no transcript or inferred content.

The local-editor boundary writes only into a bounded wipe-on-drop sink. Failed,
empty, and validator-rejected candidates are destroyed before deterministic and
sanitized-raw fallback. Fixed attempt reports identify the route and failure
category without transcript content. The dependency-free interface cannot by
itself guarantee an external implementation's process isolation; any concrete
runtime remains subject to separate dependency, model, deadline, and offline
review.

The semantic verifier runs before candidate acceptance and rejects protected
token mutation or deletion with fixed, payload-free categories. It bounds both
source and candidate token counts and does not interpret transcript text as
instructions.

The final-output adapter borrows only a pipeline-owned stable transcript and
returns the existing bounded wipe-on-drop result after validation. Its errors
expose only cleanup/validation categories, including when the source exceeds
the configured bound. No partial transcript type is wired to the adapter.

The experimental-native adapter checks finality before borrowing text. A
partial returns a fixed category and its text accessor is not called; a final
uses the same bounded validated owner as the production path.

The streaming adapter receives no boundary on cancellation or failure. A valid
boundary is consumed once, carries only an expected count, and accepts no text.
Source assembly happens only after commit structure and total size validation;
the temporary source and consumed commit buffers are explicitly overwritten
before the validated result returns.

Dictionary entries are borrowed rather than retained. Dictionary and spoken
rule outputs use opaque wipe-on-drop owners and expose only numeric reports or
payload-free errors. Literal mode interprets no words as commands; the English
policy can emit only inert text formatting, not actions or process execution.

This result does not prove removal of every process-memory copy, semantic
equivalence, or safe platform insertion. LF is admitted only by the explicit
spoken-formatting validator policy; platform insertion remains unimplemented.

## Remaining milestone gates

- obtain human acceptance for the versioned English rule set and define any
  additional language-specific rules only from reviewed native-speaker evidence;
- defer dictionary persistence and inspect/edit/delete/import/export controls
  to Milestones 6–7 while retaining history-off, borrowed-only M3 behavior;
- derive and calibrate production routing signals without giving the router
  transcript content;
- evaluate the pinned Qwen3 refiner with an isolated llama.cpp worker only if
  measured deterministic quality is insufficient, then select and integrate
  the reviewed concrete runtime/model;
- evaluate prompt-cache reuse only if a local model is adopted;
- benchmark median/p95 latency and memory on the named baseline hardware;
- complete multilingual, adversarial, and human acceptance.

The synthetic reviewer cases, local-editor decision record, privacy rules, and
baseline measurement fields are prepared in `REFINEMENT_ACCEPTANCE.md`; their
result fields remain intentionally pending until a human and the named hardware
are available.

Milestone 3 is not complete. The Qwen3 model artifact is pinned and verified,
but no isolated llama.cpp runtime evaluation or default enablement has been
completed; the named dual-core/4-GB baseline has not been measured, and no
semantic or end-to-end user-experience acceptance is claimed.
