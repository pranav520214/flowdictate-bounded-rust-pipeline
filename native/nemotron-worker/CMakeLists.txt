cmake_minimum_required(VERSION 3.26)
project(flowdictate_nemotron_worker LANGUAGES CXX)

set(NEMO_SPEECH_SOURCE_DIR "" CACHE PATH "Pinned NeMo-Speech.cpp source directory")
set(NEMO_SPEECH_BUILD_DIR "" CACHE PATH "Pinned NeMo-Speech.cpp CPU-ASR build directory")

if(NOT EXISTS "${NEMO_SPEECH_SOURCE_DIR}/include/nemo_speech/asr.h")
    message(FATAL_ERROR "NEMO_SPEECH_SOURCE_DIR must contain the reviewed ASR C header")
endif()
if(NOT EXISTS "${NEMO_SPEECH_BUILD_DIR}/src/asr/nemo_speech_asr_c.lib")
    message(FATAL_ERROR "NEMO_SPEECH_BUILD_DIR must contain the reviewed CPU-ASR import library")
endif()

add_executable(flowdictate-nemotron-worker main.cpp)
target_compile_features(flowdictate-nemotron-worker PRIVATE cxx_std_20)
target_include_directories(
    flowdictate-nemotron-worker PRIVATE "${NEMO_SPEECH_SOURCE_DIR}/include")
target_link_libraries(
    flowdictate-nemotron-worker PRIVATE
    "${NEMO_SPEECH_BUILD_DIR}/src/asr/nemo_speech_asr_c.lib")

if(MSVC)
    target_compile_options(
        flowdictate-nemotron-worker PRIVATE /W4 /WX /permissive- /EHsc /utf-8)
    target_compile_definitions(
        flowdictate-nemotron-worker PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
else()
    target_compile_options(
        flowdictate-nemotron-worker PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()

set_target_properties(flowdictate-nemotron-worker PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")

foreach(runtime_dll IN ITEMS
        ggml-base.dll
        ggml-cpu.dll
        ggml.dll
        nemo_speech_asr.dll
        nemo_speech_asr_c.dll)
    add_custom_command(TARGET flowdictate-nemotron-worker POST_BUILD
        COMMAND "${CMAKE_COMMAND}" -E copy_if_different
            "${NEMO_SPEECH_BUILD_DIR}/bin/${runtime_dll}"
            "$<TARGET_FILE_DIR:flowdictate-nemotron-worker>/${runtime_dll}")
endforeach()
