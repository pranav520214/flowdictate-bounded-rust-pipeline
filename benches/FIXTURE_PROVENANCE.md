# Approved public benchmark fixture provenance

Review date: 2026-08-28

## Reviewed evaluation strata

The strict manifest records speaking style, acoustic condition, language mix,
accent-evidence level, and voice-rights basis for every retained voice. All 11
cases are read, monolingual, have no reviewed accent claim, and rely on reviewed
corpus redistribution terms. The six LibriSpeech `dev-clean` cases are marked
clean. The five FLEURS cases remain acoustically uncharacterized because this
review did not establish a stronger source-backed condition. No current case is
counted as conversational, spontaneous, noisy, code-switched/Hinglish, or
accent-reviewed evidence.

## Shared upstream and derivation

- Upstream collection: LibriSpeech ASR corpus, OpenSLR resource SLR12.
- License: CC BY 4.0, as stated by the official SLR12 resource record.
- Attribution: LibriSpeech ASR corpus, prepared by Vassil Panayotov with
  assistance from Daniel Povey and derived from LibriVox audiobooks.
- Source archive: `https://www.openslr.org/resources/12/dev-clean.tar.gz`.
- Published and verified archive MD5: `42e2234ba48799c1f50f24a7926300a1`.
- Source shape: lossless FLAC, signed 16-bit samples, 16 kHz, mono.
- Derived WAV recipe: FFmpeg decode with metadata removed, mono 16 kHz PCM16,
  and bit-exact muxer/codec flags. Each resulting RIFF contains only a 16-byte
  `fmt ` chunk followed by `data`, matching FlowDictate's narrow decoder.

The conversion changes only the container/encoding from lossless FLAC to
lossless PCM16 WAV; it does not resample, mix channels, trim, or normalize the
audio.

## Approved members

Every transcript is the exact matching row from the chapter's first-party
`.trans.txt` member. SHA-256 values cover the retained derived WAV and UTF-8
reference file, including its final newline.

| Utterance ID | Duration ms | Source audio member | Source transcript member | WAV SHA-256 | Transcript SHA-256 |
|---|---:|---|---|---|---|
| `1272-135031-0009` | 1,910 | `LibriSpeech/dev-clean/1272/135031/1272-135031-0009.flac` | `LibriSpeech/dev-clean/1272/135031/1272-135031.trans.txt` | `11d6953d2a8ba28594088deee728160db098904fc3a539fafb49293ae3182af0` | `4ba49561b8873d48254ec5f0f8ac99c3d0923bf40a4b7ea24838a2665d50f606` |
| `2035-147960-0005` | 6,585 | `LibriSpeech/dev-clean/2035/147960/2035-147960-0005.flac` | `LibriSpeech/dev-clean/2035/147960/2035-147960.trans.txt` | `76cb8619e52edd0f41f09e39d8216e6e9493dab7f14ec569ffa8df3869f0bc3c` | `49063c08ada2efa3ddb57aa23c4d926432a30d18b7b2daded08ad6de49869612` |
| `2035-147960-0013` | 2,675 | `LibriSpeech/dev-clean/2035/147960/2035-147960-0013.flac` | `LibriSpeech/dev-clean/2035/147960/2035-147960.trans.txt` | `dc316e4958bfd11aab746235d4ff2693e27b7ae7e66a07280fba37a2da83218b` | `cd5de63444ce71ea1fd88fe7a4e54be57e07f6d3c780fd9907192d8e0b406872` |
| `2086-149214-0002` | 16,745 | `LibriSpeech/dev-clean/2086/149214/2086-149214-0002.flac` | `LibriSpeech/dev-clean/2086/149214/2086-149214.trans.txt` | `ab0384813efed7e2c0ab54eed5e53e62f7093b2f4ba1ae586103bbf357a98721` | `635259bd89c26cba853cac94753ea902a8fdf83cede89bed5da31a4dfa3a6662` |
| `2277-149896-0000` | 6,590 | `LibriSpeech/dev-clean/2277/149896/2277-149896-0000.flac` | `LibriSpeech/dev-clean/2277/149896/2277-149896.trans.txt` | `20e10e583b4918e05cea91316f7bde65a345d2756e64abc7f7ee133e2a0547f5` | `f7c7410e2df49fe86c3e59ad7a3897e9b5fa2c8442025a51eec752a29ce8abb1` |
| `2277-149896-0004` | 1,955 | `LibriSpeech/dev-clean/2277/149896/2277-149896-0004.flac` | `LibriSpeech/dev-clean/2277/149896/2277-149896.trans.txt` | `32cba910e144ceba8155d8d82a591f20c307446708bf617a4fdfe4c3db98c6f0` | `c660bb39b5ea230fb8a9b0b26b4bd22cd5bda0f17b67a85da6ca7ba23f6a1486` |

The set contains four speakers, three short cases below three seconds, two
medium cases near 6.6 seconds, and one long case at 16.745 seconds. Total audio
is 36.460 seconds. It is a fixed regression set, not a demographic or
real-world representativeness sample.

The source archive and all non-selected extracted members were removed after
verification and derivation.

Primary evidence:

- [OpenSLR SLR12 resource and license](https://www.openslr.org/12/)
- [OpenSLR SLR12 published archive checksums](https://www.openslr.org/resources/12/md5sum.txt)
- [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en)

## Google FLEURS Hindi development members

- Upstream collection: Google FLEURS, Hindi configuration `hi_in`, development
  split.
- License: CC BY 4.0, as stated by the official Google dataset card for audio
  and text.
- Attribution: FLEURS corpus by Conneau, Ma, Khanuja, Zhang, Axelrod, Dalmia,
  Riesa, Rivera, Bapna, and collaborators.
- Pinned repository revision:
  `4683b04af03d2d9549064c7d72060a9a94bb6046`.
- Source archive: `data/hi_in/audio/dev.tar.gz`, 131,741,732 bytes.
- Verified source archive SHA-256:
  `9adbca6d6fc70e40c121910941bcd7c8906eee60b402b6d21b4bd160e20030c7`.
- Source metadata: `data/hi_in/dev.tsv`, verified local SHA-256
  `cea87c57a37a0d38ed0afce30e68a35ad7b3945414648430fee09a54ca7b72ba`.
- Derived WAV recipe: FFmpeg decode with metadata removed, mono 16 kHz PCM16,
  and bit-exact muxer/codec flags. The operation preserves duration, sample
  rate, and channel count while quantizing float32 samples to signed PCM16 and
  narrowing RIFF to the approved 16-byte `fmt ` plus `data` layout.
- Reference: exact FLEURS normalized-transcription field, encoded as UTF-8
  with one final LF for the repository text file.

| Row ID | Gender label | Duration ms | Original member | Source WAV SHA-256 | Retained WAV SHA-256 | Transcript SHA-256 |
|---:|---|---:|---|---|---|---|
| `1656` | female | 3,600 | `dev/4006648279216781989.wav` | `9e34c3ddde1a12e9b4925ac1f8621018a4ab21096c870c18b55fab06f132e552` | `4adf0a74332aa66a97caa6cfe454e729df83e1f162f1aa8392f44416e34ddcb8` | `4ae79facb0b863dd4e72b828c07af475e7b7ec9804c6884401c5108281463ca4` |
| `1652` | male | 4,440 | `dev/1487893749760096307.wav` | `b2425324425a9e6fc1ea577ec91bf6bbbf578d0092a8dc7f99e89bb9dd4935f2` | `a85a23e8a468d39779b5436ccd31b3d689cc859bfc6ae468fae74bfc7839135a` | `a89254e1502d4704b38a89dae0d01385afdd89651d067451fd98034c44d720b7` |
| `1624` | female | 6,180 | `dev/386806656898138188.wav` | `cc905543ef67ed1d483745387a567de2de68cfc7de87c57ad9dfe04d8934c977` | `66ffb7dd88c48439485ba74172989f5dcc5464a91d8092e927914beadbad625e` | `e3cd86855da953a2de12d63c3547869874c7628f6755da3a94b06f679790b667` |
| `1651` | male | 8,460 | `dev/13604489588914590815.wav` | `aaee412eb335aab8cf2b75d787bb2c6bfbdc6edc1248fd71b7ef91b11752fb9b` | `64e64ce73c3bc813f53cc8ff05d8da9fc38b62d573035daf498507ef2320c299` | `82a378f03afb6e79bc86f5ba0d64ec92a24a2749f92985d0eafd744ba07ad8bd` |
| `1644` | female | 12,000 | `dev/4200211763915509320.wav` | `0a7c4a051e924e15228ad9ae4d1e8db359728a48877ae39a128579ada6a3f688` | `ba8b1548f7041cbbe957298b47048794c3920926c79ce508d2be173146c14a75` | `b3f7271e381bdfcc0471ca2fb37856c859f3625996c72a8283ae2992889114ae` |

The five cases total 34.680 seconds and cover five distinct FLEURS sentence
IDs, male/female metadata labels, and 3.6–12.0 second durations. FLEURS does not
publish a speaker identifier in this TSV, so this set makes no speaker-diversity
claim.
- Modification notice: FlowDictate converted the source float32 WAV to the
  narrow signed-PCM16 WAV boundary without resampling, mixing, trimming, or
  normalization. It extracted the normalized transcript field into a
  standalone UTF-8 text file and added a final line feed. The full archive,
  metadata file, and all unselected members are temporary verification inputs
  and are not retained.

Primary evidence:

- [Pinned Google FLEURS dataset card](https://huggingface.co/datasets/google/fleurs/blob/4683b04af03d2d9549064c7d72060a9a94bb6046/README.md)
- [Pinned Google FLEURS data commit](https://huggingface.co/datasets/google/fleurs/commit/4683b04af03d2d9549064c7d72060a9a94bb6046)
- [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en)
