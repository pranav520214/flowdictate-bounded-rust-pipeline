# Whisper tiny multilingual q5_1 provenance

Status: **Downloaded, integrity verified, runtime integrated, and regression benchmarked**  
Reviewed: 2026-08-26

| Field | Reviewed value |
|---|---|
| Model ID | `asr-whisper-tiny-multilingual-q5_1` |
| Artifact | `ggml-tiny-q5_1.bin` |
| Purpose | Local automatic speech recognition |
| Architecture | Whisper tiny, multilingual |
| Quantization | `q5_1` |
| Intended adapter | `whisper.cpp` |
| Source repository | `https://huggingface.co/ggerganov/whisper.cpp` |
| Pinned source revision | `98aa99a0a9db05ae2342309f5096248665f7cba3` |
| Exact size | `32,152,673` bytes |
| SHA-256 | `818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7` |
| License | MIT |

The artifact was downloaded with `hf download` at the pinned revision. Local
SHA-256 and byte length match the Hugging Face file record. The official
whisper.cpp download scripts use the same Hugging Face repository, and the
OpenAI Whisper repository states that its code and model weights are MIT
licensed.

Primary references:

- https://huggingface.co/ggerganov/whisper.cpp/blob/98aa99a0a9db05ae2342309f5096248665f7cba3/ggml-tiny-q5_1.bin
- https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/models/download-ggml-model.cmd
- https://github.com/openai/whisper/blob/main/README.md#license
- https://github.com/openai/whisper/blob/main/model-card.md

The binary is intentionally ignored by Git. Distribution packaging must fetch
or bundle only this exact artifact and must re-run the compiled integrity gate
before any runtime initialization.

The numeric-only reviewed-fixture regression result is recorded in
[`../../docs/BENCHMARK_RESULTS_2026-08-28.md`](../../docs/BENCHMARK_RESULTS_2026-08-28.md).
