# Public speech fixture source review

Review date: 2026-08-28

## Decision

**Admit one controlled LibriSpeech derivation.** The official SLR12 record
supplies the corpus license and provenance, its checksum list content-pins the
source archive, and that verified archive contains both the selected lossless
audio member and its first-party transcript. FlowDictate converted one 1.91-second
FLAC to a narrow PCM16 WAV without resampling, mixing, trimming, or normalization.
The exact derivation evidence is recorded in
[`../benches/FIXTURE_PROVENANCE.md`](../benches/FIXTURE_PROVENANCE.md).

## Candidates

| Candidate | Evidence present | Blocking evidence gap | Recommendation |
|---|---|---|---|
| Mozilla DeepSpeech 0.9.3 example audio | The official fixed release identifies `audio-0.9.3.tar.gz` as example audio and the official documentation invokes `audio/2830-3980-0043.wav`. The release is tied to tag `v0.9.3` / commit `f2e9c85`. | The release page's MPL-2.0 wording is not unambiguous about the example recording itself, no first-party ground-truth transcript is published beside the asset, the file is only offered inside a tar archive, and the release page supplies no content checksum. A tag-qualified release URL alone does not prove immutable asset bytes. | **Reject for admission.** Reconsider only with a first-party transcript plus an explicit audio-license statement and a recorded digest. |
| LibriSpeech SLR12 | OpenSLR identifies LibriSpeech as read English speech derived from LibriVox and licenses SLR12 as CC BY 4.0. The official page provides checksums for its published archives. | The official distribution is a large FLAC archive, so admission requires a documented, checksum-verified derivation rather than a direct WAV download. | **Admit one controlled derivation.** The archive MD5 matched, the audio/transcript members were paired in the archive, and the resulting canonical WAV is SHA-256 pinned. |
| Uberi SpeechRecognition `tests/english.wav` | At commit `0a675d1e5232dbeae1be8d071c923fb37f0f163a`, the repository directly exposes a 236 KB WAV, and its first-party test asserts the exact text `one two three`. The repository carries a BSD 3-Clause license. | The repository does not document who recorded the voice, its upstream provenance, or a separate copyright/license statement for the recording. Treating a software-repository license as proof of rights in an undocumented voice recording would be an unsupported inference. File conformance to FlowDictate's strict RIFF parser is also unverified because this review intentionally did not download it. | **Reject for admission.** This is the closest operational candidate, but it needs maintainer-provided recording provenance and explicit audio-rights confirmation before byte-level intake validation. |

## Primary sources

- Mozilla DeepSpeech [0.9.3 release](https://github.com/mozilla/DeepSpeech/releases/tag/v0.9.3) and [commit-pinned quick-start documentation](https://github.com/mozilla/DeepSpeech/blob/f2e9c85880dff94115ab510cde9ca4af7ee51c19/doc/index.rst).
- OpenSLR [LibriSpeech SLR12 resource record](https://www.openslr.org/12/) and its official [archive checksum list](https://www.openslr.org/resources/12/md5sum.txt).
- SpeechRecognition commit [`0a675d1e5232dbeae1be8d071c923fb37f0f163a`](https://github.com/Uberi/speech_recognition/commit/0a675d1e5232dbeae1be8d071c923fb37f0f163a), pinned [`tests/english.wav`](https://github.com/Uberi/speech_recognition/blob/0a675d1e5232dbeae1be8d071c923fb37f0f163a/tests/english.wav), pinned [reference-text assertion](https://github.com/Uberi/speech_recognition/blob/0a675d1e5232dbeae1be8d071c923fb37f0f163a/tests/test_recognition.py), and pinned [BSD 3-Clause license](https://github.com/Uberi/speech_recognition/blob/0a675d1e5232dbeae1be8d071c923fb37f0f163a/LICENSE.txt).

## Safe next move

The controlled LibriSpeech set has now expanded to six admitted cases with a
numeric-only local-model regression result. Keep the two rejected convenience
WAV sources outside the approved root unless their missing audio-rights
evidence is later supplied.
