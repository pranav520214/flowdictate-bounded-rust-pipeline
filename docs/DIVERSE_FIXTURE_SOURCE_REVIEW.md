# Diverse public speech fixture source review

Review date: 2026-09-01

## Decision

**Five Google FLEURS `hi_in` Hindi read-speech cases are admitted through the
strict fixture boundary.** Do not treat it as accuracy evidence or as evidence
for conversational speech, noisy speech, Indian English, or Hinglish.

The official Google FLEURS dataset card covers audio and text under CC BY 4.0,
identifies Hindi among its South-Asian languages, documents 16 kHz audio and
raw/normalized transcription fields, and describes the corpus as read speech.
The repository revision
`4683b04af03d2d9549064c7d72060a9a94bb6046` pins the metadata and source audio
objects used by this review.

Admission remained fail closed. Each retained case records the pinned source
revision, source archive/object digest, original FLEURS ID and filename, exact
reference field, derived WAV/TXT SHA-256 values, attribution, license link, and
conversion notice. Only five deterministic reviewed members entered the
approved fixture root; the full archive and unselected members are temporary
verification inputs and are removed after verification.

## Admitted set

- Configuration/split: `hi_in` / development.
- FLEURS row IDs: `1656`, `1652`, `1624`, `1651`, and `1644`.
- Duration and retained shape: 34.680 seconds total, mono, 16 kHz, canonical
  PCM16; individual durations range from 3.600 to 12.000 seconds.
- Pinned source archive SHA-256:
  `9adbca6d6fc70e40c121910941bcd7c8906eee60b402b6d21b4bd160e20030c7`.
- Exact source-member, retained WAV, and transcript SHA-256 values are recorded
  per case in [`../benches/FIXTURE_PROVENANCE.md`](../benches/FIXTURE_PROVENANCE.md).
- Verification: exact hash, UTF-8 reference, fixed Hindi mode, duration, sample
  rate, channels, canonical RIFF shape, finite sample decoding, and approved-root
intake all pass in the ordinary offline suite.

The manifest now records these cases conservatively as `read`,
`uncharacterized`, `monolingual`, `not-reviewed` for accent evidence, and
`corpus-license-reviewed` for voice rights. Automated coverage counts therefore
cannot treat this set as conversational, noisy, code-switched/Hinglish, or
accent-reviewed evidence.

## Candidate findings

| Candidate | Useful evidence | Blocking limitation | Outcome |
|---|---|---|---|
| Google FLEURS `hi_in` | Official Google card covers audio/text under CC BY 4.0; Hindi is explicit; commit-pinned metadata and audio objects; 16 kHz audio plus raw and normalized transcript fields | Read speech only; no separate publisher checksum manifest, so the pinned repository object's digest must be independently verified | **Approved for one controlled Hindi derivation** |
| OpenSLR SLR104 Hindi-English | Official page states CC BY-SA 4.0 and provides Hindi-English code-switched audio plus transcripts | No official checksum/version manifest for the 443 MB test archive | **Reject for current immutable intake gate** |
| Mozilla Common Voice | Broad language/accent coverage and CC0 dataset labeling | Current official terms ask users not to repost or mirror dataset portions outside Mozilla Data Collective | **Reject for checked-in fixtures** |
| AMI Meeting Corpus | Conversational and far-field speech with CC BY 4.0 documentation | No official per-artifact checksum evidence was established in this review | **Defer** |
| IndicVoices-R | Natural Indian-language speech with CC BY 4.0 documentation | Large mutable delivery paths and no publisher checksum evidence were established in this review | **Defer** |

## Required attribution and derivation record

For every retained FLEURS case:

- credit Google FLEURS and cite the FLEURS paper;
- link CC BY 4.0 and identify FlowDictate's canonical PCM conversion as a
  modification;
- preserve the original sample ID, filename, split, language configuration,
  and exact source transcript field;
- verify the commit-pinned source object before extraction;
- publish only non-sensitive numeric benchmark output; never log or upload
  fixture audio or recognized text during testing.

## Scope boundary

This review and admission cover only five deterministic `hi_in` read-speech
fixtures. Their numeric regression result is recorded in
[`HINDI_BENCHMARK_RESULTS_2026-09-01.md`](HINDI_BENCHMARK_RESULTS_2026-09-01.md)
and misses the provisional gate by a large margin. The set contains multiple
sentence IDs, durations, and published gender labels, but no speaker IDs; it is
still not a supported production-quality estimate. It does not close the
milestone's broader Hindi, Indian-English, Hinglish, conversational, noise,
live-hardware, or native-speaker quality gates. Those remain separate evidence
requirements.

## Primary sources

- Google FLEURS [dataset card at the pinned revision](https://huggingface.co/datasets/google/fleurs/blob/4683b04af03d2d9549064c7d72060a9a94bb6046/README.md)
- Google FLEURS [pinned data commit](https://huggingface.co/datasets/google/fleurs/commit/4683b04af03d2d9549064c7d72060a9a94bb6046)
- OpenSLR [SLR104 resource record](https://www.openslr.org/104/)
- Mozilla Common Voice [terms](https://commonvoice.mozilla.org/en/terms)
- Creative Commons [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en)
