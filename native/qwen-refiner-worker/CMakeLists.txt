cmake_minimum_required(VERSION 3.26)
project(flowdictate_qwen_refiner_worker LANGUAGES CXX)

set(LLAMA_SOURCE_DIR "" CACHE PATH "Pinned llama.cpp source directory")
set(LLAMA_BUILD_DIR "" CACHE PATH "Pinned llama.cpp CPU build directory")

if(NOT EXISTS "${LLAMA_SOURCE_DIR}/include/llama.h")
    message(FATAL_ERROR "LLAMA_SOURCE_DIR must contain the reviewed llama C header")
endif()
if(NOT EXISTS "${LLAMA_BUILD_DIR}/src/libllama.a")
    message(FATAL_ERROR "LLAMA_BUILD_DIR must contain the reviewed llama static library")
endif()

add_executable(flowdictate-qwen-refiner-worker main.cpp)
target_compile_features(flowdictate-qwen-refiner-worker PRIVATE cxx_std_20)
target_compile_definitions(flowdictate-qwen-refiner-worker PRIVATE _WIN32_WINNT=0x0601 NOMINMAX WIN32_LEAN_AND_MEAN)
target_include_directories(flowdictate-qwen-refiner-worker PRIVATE
    "${LLAMA_SOURCE_DIR}/include"
    "${LLAMA_SOURCE_DIR}/ggml/include"
    "${LLAMA_SOURCE_DIR}/ggml/src")
target_link_libraries(flowdictate-qwen-refiner-worker PRIVATE
    "${LLAMA_BUILD_DIR}/src/libllama.a"
    "${LLAMA_BUILD_DIR}/ggml/src/ggml.a"
    "${LLAMA_BUILD_DIR}/ggml/src/ggml-cpu.a"
    "${LLAMA_BUILD_DIR}/ggml/src/ggml-base.a")
if(MSVC)
    target_compile_options(flowdictate-qwen-refiner-worker PRIVATE /W4 /WX /permissive- /EHsc /utf-8)
else()
    target_compile_options(flowdictate-qwen-refiner-worker PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()
set_target_properties(flowdictate-qwen-refiner-worker PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
