# LibriSpeech `dev-clean` fixture-set expansion review

Review date: 2026-08-28  
Outcome: **six-case regression set admitted and verified**

## Applied selection

The verified `dev-clean.tar.gz` archive was inspected locally after its
published MD5 matched. Five additional utterances were selected around the
existing anchor to cover four speakers and short, medium, and long durations.

| Utterance ID | Speaker | Duration | Band | Admission |
|---|---:|---:|---|---|
| `1272-135031-0009` | 1272 | 1.910 s | short | retained anchor |
| `2035-147960-0013` | 2035 | 2.675 s | short | admitted |
| `2277-149896-0004` | 2277 | 1.955 s | short | admitted |
| `2035-147960-0005` | 2035 | 6.585 s | medium | admitted |
| `2277-149896-0000` | 2277 | 6.590 s | medium | admitted |
| `2086-149214-0002` | 2086 | 16.745 s | long | admitted |

All six members are mono 16 kHz signed-16-bit FLAC, have matching first-party
transcript rows in the same verified archive, and remain below FlowDictate's
30-second intake ceiling. The fixed order is the manifest order; future runs
must use the same bytes and hashes rather than reselecting cases dynamically.

## Reviewed scoring policy

The corpus benchmark uses the explicit `LibriSpeechEnglish` policy:

- ASCII letters are lowercased;
- digits and apostrophes are retained;
- U+2019 is mapped to an ASCII apostrophe;
- other ASCII punctuation becomes a word break;
- whitespace is collapsed;
- other non-ASCII scalars fail closed.

Normalization occurs only in bounded volatile buffers that are overwritten on
drop. Exact-text scoring remains the default everywhere else. This policy is
appropriate for the uppercase, unpunctuated English LibriSpeech references and
is not silently applied to multilingual evaluation.

## License, provenance, and attribution

The official SLR12 record identifies LibriSpeech as 16 kHz read English speech
derived from LibriVox audiobooks and licenses the corpus under CC BY 4.0. The
archive pin, source members, derived hashes, attribution, and conversion recipe
are recorded in
[`../benches/FIXTURE_PROVENANCE.md`](../benches/FIXTURE_PROVENANCE.md).

Primary sources:

- [OpenSLR SLR12 resource record](https://www.openslr.org/12/)
- [Official archive checksums](https://www.openslr.org/resources/12/md5sum.txt)
- [LibriSpeech paper](https://www.danielpovey.com/files/2015_icassp_librispeech.pdf)
- [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en)

## Limitations

This is a small engineering regression set of clean audiobook speech, not a
representative accuracy benchmark. Four speakers cannot characterize accents,
demographics, dialects, microphones, noise, speaking styles, conversational
dictation, Indian English, or multilingual/code-switching quality. Results may
support only regression checks for these exact reviewed bytes.
