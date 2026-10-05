# Milestone 3 Human and Baseline Acceptance

Status: **Awaiting reviewer and target hardware**  
Date prepared: 2026-09-04

This worksheet is the remaining evidence gate for Milestone 3. Use only the
synthetic phrases below. Do not paste private dictation into an issue, log,
benchmark record, or committed file. Record only pass/fail, numeric timings,
and a short non-sensitive reason.

## Reviewer record

| Field | Required value |
|---|---|
| Reviewer | _Pending_ |
| Review date | _Pending_ |
| Languages reviewed | _Pending_ |
| Build or revision identifier | _Pending_ |
| History mode | `OFF` |
| Outbound networking | Blocked and observed |

## Deterministic semantic review

Mark a row pass only when the actual final text matches the stated behavior
and no unintended command or action occurs.

| Case | Input or setup | Required behavior | Result |
|---|---|---|---|
| Literal default | `first new line second` | Words remain literal; no line is created | _Pending_ |
| Explicit line rule | Same phrase with English V1 enabled | Exactly one LF; second line begins `Second` | _Pending_ |
| Explicit paragraph rule | `first new paragraph second` with English V1 | Exactly two LFs between paragraphs | _Pending_ |
| Explicit bullet rule | `bullet point apples` with English V1 | Plain-text `- Apples`; no action is executed | _Pending_ |
| Single filler | `um deploy today` | The single `um` remains | _Pending_ |
| Repeated filler | `um um deploy today` | One consecutive `um` remains | _Pending_ |
| Intentional repetition | `very very important` | Both instances remain | _Pending_ |
| Hindi preservation | `यह बहुत बहुत अच्छा है` | Repetition and script remain unchanged except safe spacing | _Pending_ |
| Urdu preservation | `یہ بہت بہت اچھا ہے` | Repetition and script remain unchanged except safe spacing | _Pending_ |
| Explicit dictionary | Add `cpp` → `C++`, then dictate `cpp is useful` | Exact entry becomes `C++`; no implicit expansion occurs | _Pending_ |
| Case boundary | Add lowercase entry, then dictate a differently cased form | Differently cased form is not silently replaced | _Pending_ |
| Programming token | Add an explicit `snake_case` replacement | Whole identifier matches; substrings do not | _Pending_ |
| Unsafe scalar | Use the synthetic control-scalar test fixture | Unsafe scalar is removed by sanitized-raw fallback; visible text remains | _Pending_ |
| Editor timeout | Use the local-editor timeout test double | Deterministic output returns; no cloud or retry loop occurs | _Pending_ |
| Invalid editor output | Use the control-containing editor test double | Candidate is rejected and wiped; deterministic output returns | _Pending_ |

Pass criteria: every row passes, a native speaker reviews every claimed
language behavior, and any proposed additional spoken command is specified as
a new version rather than silently changing English V1.

## Optional local-editor decision

Do not select or download a model until a rights-reviewed synthetic evaluation
set demonstrates that deterministic refinement is insufficient. If it is
insufficient, compare candidates in the specified 0.3B–1B range and record:

- exact model and runtime version, source, license, redistribution terms, and
  immutable hash;
- quantization, CPU thread count, context/output bounds, and timeout;
- semantic pass/fail by case, unsupported-language behavior, and deterministic
  fallback behavior;
- warm/cold p50 and p95 latency, peak and steady memory, and CPU utilization;
- proof that outbound DNS/socket activity is absent;
- whether caching a static instruction prefix measurably helps. Transcript,
  context, dictionary, or candidate text must never enter a persistent cache.

Select the smallest candidate that passes. If no deterministic-quality deficit
is established, record an explicit decision to keep the editor disabled and
mark prompt caching not applicable; do not add a model merely to satisfy a
checklist.

## Baseline hardware run

Run the release build on a physical or representative dual-core, integrated-
graphics, 4-GB-RAM, CPU-only system. A two-core affinity setting on a modern
development CPU may be recorded as exploratory evidence, but it is not a
substitute for the named baseline.

Record numeric-only results for the maximum-size deterministic benchmark and,
if selected, the local editor:

| Measurement | Required gate | Result |
|---|---:|---|
| Deterministic cleanup p50 | Report actual | _Pending_ |
| Deterministic cleanup p95 | ≤ 20 ms | _Pending_ |
| Validation p50 | Report actual | _Pending_ |
| Validation p95 | Report actual | _Pending_ |
| Refinement process peak memory | Within selected mode envelope | _Pending_ |
| Refinement steady memory | Within selected mode envelope | _Pending_ |
| Local editor warm/cold p50/p95 | Required only if selected | _Pending_ |
| Outbound DNS/socket attempts | Exactly 0 | _Pending_ |

Milestone 3 can be marked complete only after this worksheet has real results,
the model/no-model decision is explicit, all failures are resolved or accepted
without weakening privacy, and the automated workspace gates remain green.
