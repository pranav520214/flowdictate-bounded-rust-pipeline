# Nemotron 3.5 ASR Streaming 0.6B Q8 provenance

Status: **Downloaded and integrity verified; experimental runtime probe only**  
Reviewed: 2026-09-01

| Field | Reviewed value |
|---|---|
| Model ID | `asr-nemotron-3.5-streaming-0.6b-q8_0-experimental` |
| Artifact | `nemotron-3.5-asr-streaming-0.6b.q8_0.gguf` |
| Purpose | Experimental local streaming Hindi ASR evaluation |
| Architecture | Cache-aware FastConformer-RNNT, 0.6B parameters |
| Quantization | GGUF Q8_0 |
| Intended adapter | NeMo-Speech.cpp C ABI |
| Source repository | `https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b` |
| Pinned model revision | `1c8deaecc64b91f034d73e08dd8b64625eb3395d` |
| Exact size | `741,548,352` bytes |
| SHA-256 | `a5c435f294eea8f88ce68dd27b8c3bfea7f777cb2fbba04fcd30eaa555f429ae` |
| Model license | OpenMDW 1.1 |
| Runtime source | `https://github.com/NVIDIA/NeMo-Speech.cpp` |
| Pinned runtime revision | `4f9676226f667d14608487df744f375db87127f8` |
| Runtime license | Apache-2.0 |

The artifact was downloaded with `hf download` at the pinned model revision.
Its local SHA-256 and byte length match the immutable Hugging Face object
metadata. The runtime was built locally from the pinned source revision with
the CPU-only ASR profile and the runtime's pinned vcpkg baseline
`9e593bb18ea69cc5095e012465dcd675a822ed0d`.

This record does not approve production use. The experimental compiled gate
recognizes this exact identity, and Windows holds its verified read-only-sharing
handle as an immutable path lease while NeMo reopens the canonical path. Other
platforms fail closed until they have an equivalent mechanism. Adoption remains
gated on the bounded isolated C-ABI worker, redistribution notice review,
baseline memory/latency, broader quality, and human acceptance. The GGUF is
intentionally ignored by Git.

Primary references:

- https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b
- https://github.com/NVIDIA/NeMo-Speech.cpp
- https://openmdw.ai/license/1-1/
