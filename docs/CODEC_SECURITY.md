# Codec and Container Security

Status: **Narrow benchmark WAV intake implemented; broad imports remain proposed**

## Architectural separation

| Purpose | Representation | Normal operation |
|---|---|---|
| Device capture | Hardware-native PCM | Required, volatile |
| Live inference | Mono 16 kHz normalized PCM | Required, volatile |
| Regression fixture | Bounded PCM/WAV | Test-only/local |
| Diagnostic export | PCM/WAV | Explicit user action only |
| Lossless dataset/export | FLAC | Deferred, feature-gated |
| Compact authorized recording | Opus | Deferred, feature-gated |
| Broad media import | None | Out of scope |

No encode/decode cycle occurs between microphone capture and ASR. FFmpeg, MP3, AAC, and video containers are excluded from the privileged pipeline.

## Decoder preflight contract

Externally supplied audio is untrusted. Before allocating decoded output or invoking a codec library:

1. open a regular file without following links/reparse points;
2. enforce an absolute input byte limit;
3. inspect magic/container structure, not extension alone;
4. parse integer fields with checked arithmetic;
5. allowlist codec, channel count, sample format, sample rate, and duration;
6. calculate a conservative decoded-byte upper bound;
7. reject if any limit is unknown, inconsistent, or exceeded;
8. decode incrementally into bounded buffers with a cancellation/deadline budget.

Initial import safety ceilings, if import is enabled:

| Property | Hard ceiling |
|---|---:|
| Encoded input file | 256 MiB |
| Channels | 2 |
| Sample rate | 96 kHz |
| Declared duration | 30 minutes |
| Decoded PCM | 512 MiB total, streamed rather than allocated at once |
| Metadata blocks/comments | 1 MiB aggregate |
| Individual packet/frame | Codec-specific validated maximum |

These ceilings do not imply support. v1 may use much smaller per-feature limits, and default builds omit deferred codec imports.

## WAV/PCM

`flowdictate-audio` now implements a deliberately narrower benchmark-only
boundary than the general import ceilings above. It accepts a maximum 32 MiB,
30-second, 96 kHz, mono/stereo canonical RIFF/WAVE containing exactly one
16-byte `fmt ` chunk followed by exactly one `data` chunk. Only PCM16 and finite
normalized IEEE float32 are decoded. Unknown metadata chunks, extensible WAV,
duplicate/reordered chunks, inconsistent sizes/rates/alignment, non-finite or
out-of-range floats, and manifest shape mismatches fail closed.

The paired manifest is capped at 256 KiB/512 records and requires exact
lowercase SHA-256 digests, approved public/synthetic classification, explicit
license/provenance, single-component filenames, and exact rate/channel/duration.
Files must resolve beneath a regular non-reparse approved root and are read from
share-read-only handles before decoding into owned memory. Stable Rust does not
expose Windows hard-link counts; on Windows this is a documented residual, with
content integrity and path-swap risk bounded by same-handle hashing, exact
digests, exclusive writer/delete denial during the read, and immediate owned
decode. Unix builds additionally reject link counts other than one.

The broader parser contract remains:

- accept only RIFF/WAVE forms and PCM/IEEE-float encodings explicitly tested;
- walk chunks using checked offsets, handling padding without trusting declared file length;
- reject overlapping/truncated chunks, duplicate contradictory format chunks, non-finite float samples, unsupported extensible formats, absurd rates/channels/bit depth, and data sizes inconsistent with block alignment;
- stream/downsample outside the callback and never persist imported data implicitly.

## FLAC

FLAC is disabled until a bounded wrapper and fuzz suite exist. If enabled, validate stream info before frame decode; cap block size, channels, sample rate, metadata, duration, and decoded total; reject MD5/container inconsistencies according to a documented policy. FLAC never enters the live capture path.

## Opus

Opus is disabled until explicit local recording is implemented. If enabled, use a narrow Ogg Opus surface, cap page/packet sizes and granule-derived duration, validate channel mapping, and stream output through bounds. Native libopus and wrapper versions are pinned/audited. Opus is never required for ASR.

## File creation/export

- Normal dictation does not create audio files.
- Explicit export requires a user-chosen target and preview of format/duration/estimated size.
- Create a new file exclusively; do not overwrite by default.
- Canonicalize/validate the parent, reject device files and unsafe reparse/symlink targets, write with restrictive permissions where supported, flush, then atomically finalize within the same directory.
- Never derive filenames from transcript text without strict sanitization and user confirmation.

## Fuzz and negative test matrix

- Truncated headers/chunks/pages/frames
- Integer wrap in sizes, rates, durations, offsets, and allocations
- Zero/maximum values and contradictory duplicated metadata
- Decompression bombs and tiny-file/huge-output declarations
- Invalid UTF-8 and oversized metadata/comments
- Symlink/reparse/hardlink/alternate-stream/path traversal cases
- Cancellation at every parsing/decoding boundary
- Corpus minimized and retained locally; no fuzz input is uploaded

## Residual risk

Even a bounded valid file can trigger a vulnerability in native codec code. Deferred codecs should run outside the microphone callback and may be moved to a constrained helper process after a dedicated threat review. Bounds reduce impact but do not prove parser safety.
