# Qwen3 Refiner Runtime Slice

Status: **Native worker built and smoke-tested; Rust supervisor integration and
quality acceptance remain open**  
Date: 2026-09-05

The pinned `Qwen3-0.6B-Q4_K_M.gguf` artifact is loaded by a dedicated
llama.cpp child process. The worker receives a bounded UTF-8 frame over stdin,
applies a fixed conservative chat policy, and returns one bounded UTF-8 frame
over stdout. Diagnostics are not written to stdout; transcript bytes are not
logged, persisted, or sent to a network service. The worker clears its source
and output buffers before returning to its loop.

The prompt explicitly treats the delimited transcript as untrusted data. Qwen3
thinking blocks are removed deterministically before the result leaves the
worker; malformed or unterminated blocks fail the request. The Rust refinement
layer remains the acceptance boundary: output validation and protected-token
semantic verification must run before any candidate can replace deterministic
cleanup.

## Reproducible local build

The vendored llama.cpp source is configured without network, GPU, BLAS, or
OpenMP dependencies. On the reviewed Windows host, the older MinGW headers
require `_WIN32_WINNT=0x0601` so llama.cpp's optional Windows 10 throttling API
is compiled out. The worker links only the locally built static llama/ggml
libraries:

```text
cmake -S native/qwen-refiner-worker -B .tools/build-qwen-refiner-worker -G Ninja \
  -DLLAMA_SOURCE_DIR=.tools/nemo-speech-cpp/llama.cpp \
  -DLLAMA_BUILD_DIR=.tools/nemo-speech-cpp/build-llama-cpu \
  -DCMAKE_CXX_FLAGS=-D_WIN32_WINNT=0x0601
cmake --build .tools/build-qwen-refiner-worker \
  --target flowdictate-qwen-refiner-worker -j 4
```

The checked local binary was produced at
`.tools/build-qwen-refiner-worker/bin/flowdictate-qwen-refiner-worker.exe`.
The `.tools` tree is intentionally ignored and is not a distributable product
artifact.

## Evidence

- llama.cpp static library built successfully with Qwen3 model support.
- Qwen3 model load succeeded from the pinned, manifest-verified path.
- A binary-framed `ship v2` request returned `ship v2` with exit code 0.
- The worker's stderr was discarded during the smoke request; stdout contained
  only the ready/result protocol bytes.

## Remaining gates

- add the Rust process supervisor with startup/deadline/kill/recovery tests;
- pass a `VerifiedModelPathLease` and an immutable worker executable lease into
  that supervisor rather than accepting unchecked paths;
- compose the worker through `LocalEditor`, then exercise validator and
  protected-token fallback cases end to end;
- measure cold/warm latency and memory on the named dual-core/4-GB baseline;
- complete prompt-injection, multilingual, semantic-quality, and human review.

Until those gates pass, this worker is an experimental local runtime and is
not enabled by default.
