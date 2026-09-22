# Whisper base multilingual q5_1 comparison provenance

Status: **Downloaded and integrity verified for controlled comparison only**  
Reviewed: 2026-09-01

| Field | Reviewed value |
|---|---|
| Model ID | `asr-whisper-base-multilingual-q5_1-comparison` |
| Artifact | `ggml-base-q5_1.bin` |
| Purpose | Local ASR comparison candidate; not the adopted production default |
| Architecture | Whisper base, multilingual |
| Quantization | `q5_1` |
| Intended adapter | `whisper.cpp` |
| Source repository | `https://huggingface.co/ggerganov/whisper.cpp` |
| Pinned source revision | `98aa99a0a9db05ae2342309f5096248665f7cba3` |
| Exact size | `59,707,625` bytes |
| SHA-256 | `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898` |
| License | MIT |

The artifact was downloaded from the exact pinned revision. Its local byte
length and SHA-256 match the immutable Hugging Face file record. It is admitted
only to a manually invoked, ignored comparison test. The production isolated
worker retains its compile-time tiny-model identity and cannot select this
candidate.

Primary references:

- https://huggingface.co/ggerganov/whisper.cpp/blob/98aa99a0a9db05ae2342309f5096248665f7cba3/ggml-base-q5_1.bin
- https://github.com/openai/whisper/blob/main/README.md#license

The binary is intentionally ignored by Git. No runtime path downloads models,
performs remote inference, or emits fixture or hypothesis text.
