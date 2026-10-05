# Deterministic Refinement Runtime

Status: **Cleanup, validation, sanitized fallback, routing, and production/native/streaming final-ASR composition implemented; Milestone 3 incomplete**  
Date: 2026-09-04

`flowdictate-refine` is a separate, dependency-free library boundary for work
that does not justify a language-model invocation. It currently performs one
bounded linear pass over a borrowed stable transcript and creates at most one
owned output buffer.

## Implemented contract

- Input and output are capped at 65,536 UTF-8 bytes; callers may choose a lower
  non-zero bound.
- Unicode whitespace is collapsed to one ASCII space and leading/trailing
  whitespace is removed.
- Whitespace before a small closing-punctuation allowlist and after `(`, `[`,
  or `{` is removed.
- ASCII lowercase letters are capitalized at the beginning of text and after
  an observed sentence-terminal-plus-whitespace boundary.
- Missing whitespace is never invented. URLs, decimals, and abbreviation-like
  tokens therefore remain joined.
- Non-whitespace control characters fail closed. Failures are fixed categories
  and never embed transcript content.
- Output never exceeds input size. The implementation has linear time and
  bounded auxiliary state.

These rules are intentionally conservative. They do not claim linguistic
correctness for every language or punctuation system.

## Explicit local dictionary

`apply_user_dictionary` accepts at most 256 borrowed entries with 256-byte
spoken and written fields. It performs case-sensitive whole-token or phrase
matching, selects the longest match, rejects duplicate spoken forms, and checks
the final size before allocating. Common ASCII and multilingual punctuation
delimit tokens; `_` remains part of programming identifiers. Input punctuation
and spacing outside replacements are copied exactly.

Entries are supplied only by the caller. The refinement crate does not own a
profile, learn from dictation, access storage, serialize, log, or transmit them.
No case folding or implicit abbreviation expansion occurs: users must add every
replacement explicitly. Dictionary output is opaque, non-cloneable,
non-debuggable, bounded, and overwritten on drop. Encrypted storage plus
inspect/edit/delete/import/export controls remain Milestones 6–7 work.

## Repeated fillers and spoken formatting

`SpokenRulesPolicy::Literal` is the default and interprets nothing.
`EnglishV1` is a separately selected, versioned policy. It collapses only
consecutive repeats of `uh`, `um`, `erm`, or `hmm`, preserving the first token;
single fillers, `very very`, non-adjacent repetitions, and other languages are
unchanged. It recognizes only `new line`, `new paragraph`, and `bullet point`,
case-insensitively at whole-word boundaries. The output contains LF line breaks
and plain `- ` bullet markers; it cannot execute commands.

The rule output has the same bounded wipe-on-drop ownership and numeric-only
reporting. Final production/native/streaming adapters accept the explicit
policy only after finality and apply ordinary cleanup first. Literal phrases
cannot become formatting unless the caller opts into `EnglishV1`.

## Output-validation contract

`validate_refined_output` returns the exact borrowed candidate without copying
or interpreting it. Callers must provide explicit limits; the deterministic
profile permits no added bytes and no proportional growth. A future local
editor may receive a separately reviewed policy, but the compiled ratio ceiling
cannot exceed twice the source size and both ratio and additive limits apply.

The validator:

- caps both source and candidate at the configured limit;
- prevents non-empty output from being created from empty source text;
- rejects control scalars, Unicode noncharacters, and explicit Unicode
  bidirectional-formatting controls by default;
- can permit LF only through `LineBreakPolicy::LfOnly`; CR, tabs, and every
  other control remain rejected;
- preserves ordinary Hindi/Urdu text plus zero-width joiner/non-joiner shaping;
- treats prompt instructions, shell syntax, and action-like words as inert text;
- returns only a non-`Clone`, non-`Debug` borrowed view and numeric byte counts.

Length and scalar checks bound unsafe output; they do not prove semantic
equivalence, safe platform insertion, or that an editor model preserved intent.

## Model-free fallback

`refine_without_model` first runs strict deterministic cleanup. Ordinary input
returns that result directly. A non-whitespace control, explicit bidi-formatting
control, or Unicode noncharacter instead destroys the partial cleanup owner and
runs `sanitize_raw_transcript` over the original borrowed input.

The sanitizer preserves visible characters, punctuation, casing, Hindi/Urdu,
and shaping joiners. It removes only those unsafe scalar categories, collapses
Unicode whitespace to one ASCII space, and owns at most one output buffer no
larger than the input. Its report contains only byte counts and removal counts.

Input bounds, invalid configuration, and allocation failure never masquerade as
a successful fallback. They remain payload-free errors. No model or cloud path
is reachable from this composition.

## Metadata-only routing

`select_refinement_route` accepts a compact `RefinementSignals` value and an
explicit `LocalEditorGate`; it never accepts transcript text. The four named
conditions match the reviewed specification: semantic rewrite, ambiguous
self-correction, contextual formatting, and insufficient deterministic
confidence. Multiple conditions occupy one byte and require no allocation.

The default gate is `Disabled`. A route can become `LocalEditorThenModelFree`
only when at least one named condition is present and the gate is `Ready`.
No condition, a disabled editor, or an unavailable editor always selects
`ModelFree`.

No numeric confidence threshold is compiled here. Production signal derivation
and thresholds remain benchmark-driven integration work.

## Optional local-editor fallback boundary

`LocalEditor` is a narrow synchronous interface for a separately audited local
runtime. A selected runtime borrows the final source and can write candidate
UTF-8 only through a byte-capped `LocalEditorOutput`. That sink implements
neither `Clone` nor `Debug`, rejects growth before append, and overwrites its
initialized bytes on clear or drop. Runtime errors are fixed categories for
unavailable, timeout, out-of-memory, context, decoder, bound, or allocation
failure and never contain source or candidate text.

`refine_with_optional_editor` first applies the metadata-only route. A local
candidate is accepted only after the explicit output byte, expansion, control,
bidi, noncharacter, and line-break policy. Runtime failure, empty output for a
non-empty source, or rejected output destroys the candidate and runs
`refine_without_model`; unsafe source scalars therefore reach sanitized raw as
the third and final level. Only failure of that model-free path is returned.
There is no cloud, persistence, or logging fallback.

Candidate acceptance also runs the bounded protected-token semantic verifier.
Numbers, URLs, email-like tokens, paths, flags, and identifiers must retain
their source multiplicity; mutation or deletion is rejected with a fixed,
payload-free category. This is a safety guard, not a semantic-equivalence
claim, and it does not interpret transcript text as instructions.

Production, native, and completed-streaming final adapters expose this same
hierarchy. Native partials are rejected before their text accessor or editor is
called. Streaming still requires and consumes the one-shot commit boundary
before editor routing. The interface and fakes prove bounded orchestration and
failure semantics; the pinned Qwen3 artifact is present, but an isolated
llama.cpp runtime has not yet been evaluated or selected for default use.

## Final-output composition

The production `flowdictate-pipeline` crate depends on this workspace-local
crate and implements `FinalTranscript` for the isolated worker's bounded
`WorkerTranscript`. A caller can invoke `refine_model_free` only through a
`PipelineOutput<T>`, whose fields are private and which the pipeline creates
only after an utterance finalizes.

That adapter borrows the stable source, runs `refine_without_model`, and then
revalidates the candidate with `OutputValidationConfig::deterministic`. Only a
validated opaque wipe-on-drop `RefinementOutput` is returned. Cleanup and
validation failures remain distinct, fixed, payload-free categories. The
adapter performs no transcript logging, persistence, context access, model
call, or network operation.

Experimental-native output uses `refine_native_final`. It checks `is_final`
before calling the transcript's text accessor, so a partial hypothesis returns
the fixed `NotFinal` category without its text entering cleanup. Confirmed final
text then crosses the same shared cleanup-and-validation function.

Streaming output uses a separate completed-session contract. Every successful
drain increments a payload-free session commit count. A successful hotkey
release or explicit stop returns a private, one-shot `StreamingFinalBoundary`;
cancellation and failure return no boundary. `refine_streaming_final` consumes
that boundary and the fresh caller-owned commit vector, requires the exact
session count, validates monotonic sequence/generation/timestamp structure, and
checks total bytes before allocating the assembled source. The temporary source
and all consumed commit chunks are overwritten before the result is returned.
This handles ordinary silence generations without admitting partial hypotheses
or silently accepting an incomplete/reused buffer.

## Privacy and ownership

The cleanup function borrows its input and performs no file, network, model,
logging, context, clipboard, or persistence operation. Cleaned UTF-8 is owned by an
opaque type that implements neither `Clone` nor `Debug`; its initialized bytes
are overwritten on drop, including error unwinding after allocation. The
associated report contains only byte counts, booleans, and an ASCII
capitalization count. Successful validation adds no allocation or transcript
copy; validation failures are fixed payload-free categories.

Routing operates only on copyable enum/bit-mask metadata. It has no transcript,
file, logging, context, model, persistence, or network input.

The optional editor receives transcript text only on the explicitly justified,
ready route. Its output sink is bounded and wipe-on-drop; candidate validation
is zero-copy, and rejected/failed candidates are destroyed before model-free
fallback. A concrete runtime remains responsible for enforcing its process
deadline and local-only execution and must pass dependency and model review.

Production/native final-output composition creates only the existing bounded
refinement owner; the original transcript remains in its existing wipe-on-drop
owner. Streaming composition briefly creates one bounded assembled source,
then explicitly wipes it and the consumed commit chunks before returning. The
native finality check occurs before the text borrow.

This is best-effort process-memory hygiene, not a claim that the compiler, OS,
allocator, or caller has no other copy. The caller still owns and must erase the
raw transcript under its own lifecycle contract.

## Explicitly not implemented

- dictionary persistence, implicit learning, case-folded matching, or profile
  inspect/edit/delete/import/export UI;
- spoken-formatting languages or commands beyond the explicit English V1 set;
- production derivation and calibration of routing signals or thresholds;
- evaluation and selection of a concrete 0.3B–1B local editor model/runtime;
- prompt-cache reuse, which is not applicable unless a model is selected and
  must never retain transcript-bearing state;
- a baseline-hardware latency result.

Those behaviors need explicit language, ambiguity, permission, and failure
semantics before code is added. A reproducible release benchmark now passes the
cleanup and validation p95 budgets on the 6-core/16-GB development host; the
named dual-core/4-GB baseline remains untested. See
`REFINEMENT_BENCHMARK_2026-09-04.md`. The pending human/model/baseline record is
`REFINEMENT_ACCEPTANCE.md`.
