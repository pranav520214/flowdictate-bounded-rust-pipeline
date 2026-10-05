# Model comparison preflight and first result

Status: **Pinned base comparison completed; both candidates fail the Hindi gate**  
Reviewed: 2026-09-01  
Pinned upstream revision: `98aa99a0a9db05ae2342309f5096248665f7cba3`

## Question and evidence

FlowDictate currently admits `ggml-tiny-q5_1.bin`, a 32.2 MB complete
multilingual Whisper model. The official [pinned `whisper.cpp` model
inventory](https://huggingface.co/ggerganov/whisper.cpp/tree/98aa99a0a9db05ae2342309f5096248665f7cba3)
lists no smaller complete multilingual model:

| Relevant pinned artifact | Listed size | Classification | Preflight outcome |
|---|---:|---|---|
| `ggml-tiny-q5_1.bin` | 32.2 MB | Multilingual, quantized, complete | Current smallest candidate |
| `ggml-base-q5_1.bin` | 59.7 MB | Multilingual, quantized, complete | Larger fallback candidate |
| `ggml-tiny.bin` | 77.7 MB | Multilingual, full precision, complete | Larger fallback candidate |
| `ggml-tiny.en-q5_1.bin` | 32.2 MB | English-only, complete | Excluded by the multilingual requirement |
| `ggml-tiny-encoder.mlmodelc.zip` | 15 MB | Encoder/Core ML component | Not a complete ASR model candidate |

The `whisper.cpp` project states that models are multilingual unless their
name includes `.en`, and documents the GGML files as the C/C++-loadable model
format in its [official model documentation](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md#available-models).
OpenAI's [official Whisper model table](https://github.com/openai/whisper/blob/main/README.md#available-models-and-languages)
also identifies `tiny` as the smallest multilingual architecture and `tiny.en`
as its English-only counterpart.

## Decision

The five-case Hindi result subsequently gave tiny-q5_1 188.09% WER against the
provisional 22% gate, satisfying the predeclared trigger for a base-q5_1
comparison. The base artifact was acquired from this exact pinned revision and
independently matched its 59,707,625-byte length and SHA-256
`422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898`.

On identical fixture bytes, normalization, fixed-Hindi decoding, two CPU
threads, and direct optimized adapter execution, tiny scored 188.09% WER at
0.1973 corpus RTF while base scored 101.19% WER at 0.3890 RTF. Base reduced
word errors by 46.2% but still failed the gate, used an 85.7% larger artifact,
and took 1.97 times the inference time in this run.

Do **not** switch the production model or escalate to another larger candidate
from this evidence. Expand and diagnose the reviewed Hindi set first. The
production isolated worker remains pinned to tiny; base is admitted only to an
ignored, numeric-only local comparison harness.

## Scope limitations

- The inventory section is a preflight. The added result is a narrow five-case
  read-speech comparison, not a production Hindi-quality estimate.
- The six clean LibriSpeech cases are a fixed regression set, not representative
  evidence for conversational speech, Indian English, noise, multilingual use,
  live streaming stability, or baseline hardware.
- The “no smaller candidate” conclusion is limited to complete multilingual
  artifacts in the exact pinned official repository revision. It does not claim
  that no third-party conversion or future upstream artifact exists; any such
  candidate would require separate license, provenance, runtime-compatibility,
  integrity, and quality review before admission.
- Upstream documentation outside the pinned Hugging Face tree is used only to
  interpret model naming and architecture roles; candidate presence and sizes
  come from the pinned tree itself.
