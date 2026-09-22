# FlowDictate Benchmark Fixture Intake

Status: **Measurement harness and bounded intake implemented; eleven reviewed public fixtures admitted**

This directory defines the human-reviewed intake boundary for local benchmark
audio. `flowdictate-audio` parses only the exact schema and approved rows under
hard bounds. No benchmark downloads a source, and no private recording is
admitted merely because a row exists.

## Admission rule

A fixture may enter an active local benchmark set only after every required
field in `fixtures.template.csv` is complete and independently reviewed:

- `fixture_id` is a non-personal, case-local identifier;
- `relative_audio_path` and `expected_transcript_path` remain below a dedicated
  benchmark root and identify regular non-link files;
- both files have exact SHA-256 digests;
- license, provenance URL/revision, and redistribution permission are known;
- language tags and fixed/automatic language mode are explicit;
- duration, sample rate, and channel count have been verified before decoding;
- classification is `synthetic-public` or `reviewed-public` for repository
  fixtures; private evaluation material remains outside the repository;
- `review_status` is `approved` only after license, provenance, content, and
  privacy review.

Blank or `pending` records are never benchmark inputs. Absolute paths, parent
traversal, UNC paths, reparse points, alternate data streams, unknown licenses,
remote URLs as audio inputs, and unverified hashes fail intake. Unix link counts
other than one are rejected. Stable Rust does not expose the Windows hard-link
count; Windows instead retains the same share-read-only handle through bounded
read/hash and decodes only the verified owned bytes.

## Admitted set

[`fixtures.csv`](fixtures.csv) admits six English LibriSpeech regression cases
from four speakers, totalling 36.460 seconds. Their source archive, official
checksum, member paths, lossless conversion recipe, attribution, and output
digests are recorded in [`FIXTURE_PROVENANCE.md`](FIXTURE_PROVENANCE.md). This
small clean-audiobook set is too narrow to support a general quality claim.

Five separately reviewed Google FLEURS `hi_in` development cases add 34.680
seconds of Hindi read speech. Their pinned source revision, archive hash,
selected rows/members, conversion notice, and retained hashes are recorded in
the same provenance file. The cases span five sentence IDs, both published
gender labels, and several durations, but no speaker IDs are available. Their
numeric result does not represent Hindi conversational or noisy speech.

## Result boundary

Benchmark output may contain only numeric metrics, fixed model/language codes,
compiled build versions, non-personal fixture IDs, and reviewed artifact
digests. It must not contain audio, partial/final transcript text, absolute
paths, usernames, device labels, IP addresses, or stable machine/user IDs.

The implemented in-memory recorder and fixture loader are documented in
[`../docs/BENCHMARK_RUNTIME.md`](../docs/BENCHMARK_RUNTIME.md). A bounded WAV
decoder is benchmark-only and does not authorize a broad media parser.
