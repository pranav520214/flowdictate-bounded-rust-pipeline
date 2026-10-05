// SPDX-License-Identifier: MIT OR Apache-2.0
// Silent, bounded child-process adapter for the pinned NeMo-Speech.cpp C ABI.

#include <nemo_speech/asr.h>
#include "utterance.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#include <type_traits>

#if defined(_WIN32)
#include <fcntl.h>
#include <io.h>
#endif

namespace {

constexpr std::array<std::uint8_t, 8> kMagic{'F', 'D', 'N', 'E', 'M', 'O', '0', '1'};
constexpr std::uint16_t kProtocolVersion = 1;
constexpr std::uint32_t kMaximumPathBytes = 32U * 1024U;
constexpr std::uint32_t kMaximumChunkSamples = 2'560U;
constexpr std::uint64_t kMaximumStreamSamples = 480'000U;
constexpr std::size_t kMaximumTranscriptBytes = 65'536U;
constexpr float kStreamingChunkSeconds = 0.16F;
constexpr float kCtcContextSeconds = 1.92F;
constexpr std::int32_t kRnntLowLatencyRightContext = 1;

enum class Command : std::uint8_t { push = 1, finish = 2, shutdown = 3, statistics = 4 };
enum class Response : std::uint8_t {
    ready = 0x80,
    no_result = 0x81,
    result = 0x82,
    runtime_error = 0x83,
    startup_error = 0x84,
    statistics = 0x85,
};
enum class ErrorCode : std::uint8_t {
    invalid_input = 1,
    output_bound = 2,
    native_runtime = 3,
    invalid_output = 4,
};

using Recognizer = std::unique_ptr<nemo_speech_asr_recognizer, decltype(&nemo_speech_asr_destroy)>;
// Process-local numeric accounting; the adapter dispatches on one thread.
struct StreamCounts {
    std::uint64_t created = 0, finished = 0, destroyed = 0, errors = 0, maximum = 0;
} counts;
void close_counted_stream(nemo_speech_asr_stream* stream) noexcept {
    nemo_speech_asr_stream_close(stream);
    ++counts.destroyed;
}
using Stream = std::unique_ptr<nemo_speech_asr_stream, decltype(&close_counted_stream)>;
using Result = std::unique_ptr<nemo_speech_asr_result, decltype(&nemo_speech_asr_result_destroy)>;

template <typename T>
bool read_scalar(T& value) {
    static_assert(std::is_trivially_copyable_v<T>);
    return std::fread(&value, sizeof(value), 1, stdin) == 1;
}

template <typename T>
bool write_scalar(const T value) {
    static_assert(std::is_trivially_copyable_v<T>);
    return std::fwrite(&value, sizeof(value), 1, stdout) == 1;
}

bool write_response(const Response response) {
    return write_scalar(static_cast<std::uint8_t>(response));
}

bool flush_output() { return std::fflush(stdout) == 0; }

bool valid_utf8(const std::string_view text) {
    std::size_t index = 0;
    while (index < text.size()) {
        const auto lead = static_cast<std::uint8_t>(text[index]);
        std::size_t length = 0;
        std::uint32_t codepoint = 0;
        if (lead <= 0x7fU) {
            if (lead < 0x20U || lead == 0x7fU) return false;
            length = 1;
            codepoint = lead;
        } else if ((lead & 0xe0U) == 0xc0U) {
            length = 2;
            codepoint = lead & 0x1fU;
        } else if ((lead & 0xf0U) == 0xe0U) {
            length = 3;
            codepoint = lead & 0x0fU;
        } else if ((lead & 0xf8U) == 0xf0U) {
            length = 4;
            codepoint = lead & 0x07U;
        } else {
            return false;
        }
        if (index + length > text.size()) return false;
        for (std::size_t offset = 1; offset < length; ++offset) {
            const auto continuation = static_cast<std::uint8_t>(text[index + offset]);
            if ((continuation & 0xc0U) != 0x80U) return false;
            codepoint = (codepoint << 6U) | (continuation & 0x3fU);
        }
        if ((length == 2 && codepoint < 0x80U) ||
            (length == 3 && codepoint < 0x800U) ||
            (length == 4 && codepoint < 0x10000U) ||
            (codepoint >= 0xd800U && codepoint <= 0xdfffU) || codepoint > 0x10ffffU) {
            return false;
        }
        index += length;
    }
    return true;
}

bool write_error(const std::uint64_t request_id, const ErrorCode code) {
    ++counts.errors;
    return write_response(Response::runtime_error) && write_scalar(request_id) &&
           write_scalar(static_cast<std::uint8_t>(code)) && flush_output();
}

bool reject_result(const std::uint64_t request_id, const ErrorCode code) {
    static_cast<void>(write_error(request_id, code));
    return false; // Caller exits owning scope and destroys the stream.
}

Stream create_stream(nemo_speech_asr_recognizer* recognizer) {
    auto options = nemo_speech_asr_recognition_options_default();
    options.size = sizeof(options);
    options.language_code = "hi-IN";
    options.interim_results = true;
    options.enable_word_time_offsets = false;
    options.enable_automatic_punctuation = true;
    options.verbatim_transcripts = false;
    options.max_alternatives = 1;
    nemo_speech_asr_stream* raw = nullptr;
    const auto status = nemo_speech_asr_streaming_recognize(recognizer, &options, &raw);
    Stream stream(raw, &close_counted_stream);
    if (stream) {
        ++counts.created;
        counts.maximum = std::max(counts.maximum, counts.created - counts.destroyed);
    }
    if (status != NEMO_SPEECH_ASR_OK) {
        return Stream(nullptr, &close_counted_stream);
    }
    return stream;
}

Recognizer create_recognizer(const std::string& model_path) {
    const nemo_speech_asr_backend_config backend{sizeof(backend), -1};
    const nemo_speech_asr_model_config model{sizeof(model), model_path.c_str(), nullptr};
    const nemo_speech_asr_streaming_config streaming{
        sizeof(streaming), kStreamingChunkSeconds, kCtcContextSeconds,
        kCtcContextSeconds, kRnntLowLatencyRightContext};
    const nemo_speech_asr_endpointing_config endpointing{
        sizeof(endpointing), false, false, 800};
    const nemo_speech_asr_batching_config batching{
        sizeof(batching), false, 1, 0, 1, 0, 1};
    const nemo_speech_asr_recognizer_config config{
        sizeof(config), &backend, &model, &streaming, nullptr, nullptr,
        &endpointing, nullptr, nullptr, &batching};
    nemo_speech_asr_recognizer* raw = nullptr;
    if (nemo_speech_asr_create(&config, &raw) != NEMO_SPEECH_ASR_OK || raw == nullptr) {
        return Recognizer(nullptr, &nemo_speech_asr_destroy);
    }
    return Recognizer(raw, &nemo_speech_asr_destroy);
}

bool write_result(const std::uint64_t request_id, const Result& result) {
    if (nemo_speech_asr_result_alternative_count(result.get()) != 1U) {
        return reject_result(request_id, ErrorCode::invalid_output);
    }
    const char* transcript = nemo_speech_asr_result_transcript(result.get(), 0);
    if (transcript == nullptr) return reject_result(request_id, ErrorCode::invalid_output);
    std::size_t length = 0;
    while (length <= kMaximumTranscriptBytes && transcript[length] != '\0') {
        ++length;
    }
    if (length > kMaximumTranscriptBytes) {
        return reject_result(request_id, ErrorCode::output_bound);
    }
    const std::string_view text(transcript, length);
    if (!valid_utf8(text)) return reject_result(request_id, ErrorCode::invalid_output);
    const auto seconds = nemo_speech_asr_result_audio_processed(result.get());
    if (!std::isfinite(seconds) || seconds < 0.0F || seconds > 30.0F) {
        return reject_result(request_id, ErrorCode::invalid_output);
    }
    const auto milliseconds = static_cast<std::uint32_t>(std::lround(seconds * 1'000.0F));
    const auto text_bytes = static_cast<std::uint32_t>(length);
    const auto final_flag = static_cast<std::uint8_t>(
        nemo_speech_asr_result_is_final(result.get()) ? 1U : 0U);
    return write_response(Response::result) && write_scalar(request_id) &&
           write_scalar(final_flag) && write_scalar(milliseconds) &&
           write_scalar(text_bytes) &&
           (text.empty() || std::fwrite(text.data(), 1, text.size(), stdout) == text.size()) &&
           flush_output();
}

bool pull_one_result(nemo_speech_asr_stream* stream, const std::uint64_t request_id) {
    nemo_speech_asr_result* raw = nullptr;
    const auto status = nemo_speech_asr_stream_next(stream, &raw);
    Result result(raw, &nemo_speech_asr_result_destroy);
    if (status != NEMO_SPEECH_ASR_OK) {
        return reject_result(request_id, ErrorCode::native_runtime);
    }
    if (result == nullptr) {
        return write_response(Response::no_result) && write_scalar(request_id) && flush_output();
    }
    return write_result(request_id, result);
}

bool read_startup(std::string& model_path) {
    std::array<std::uint8_t, kMagic.size()> magic{};
    if (std::fread(magic.data(), 1, magic.size(), stdin) != magic.size() || magic != kMagic) {
        return false;
    }
    std::uint16_t version = 0;
    std::uint32_t path_bytes = 0;
    if (!read_scalar(version) || version != kProtocolVersion || !read_scalar(path_bytes) ||
        path_bytes == 0 || path_bytes > kMaximumPathBytes) {
        return false;
    }
    model_path.resize(path_bytes);
    return std::fread(model_path.data(), 1, path_bytes, stdin) == path_bytes &&
           valid_utf8(model_path) && model_path.find('\0') == std::string::npos;
}

int run() {
#if defined(_WIN32)
    if (_setmode(_fileno(stdin), _O_BINARY) == -1 ||
        _setmode(_fileno(stdout), _O_BINARY) == -1) {
        return 1;
    }
#endif
    std::string model_path;
    if (!read_startup(model_path)) {
        static_cast<void>(write_response(Response::startup_error));
        static_cast<void>(flush_output());
        return 1;
    }
    auto recognizer = create_recognizer(model_path);
    std::fill(model_path.begin(), model_path.end(), '\0');
    if (!recognizer) {
        static_cast<void>(write_response(Response::startup_error));
        static_cast<void>(flush_output());
        return 1;
    }
    Utterance<Stream> stream(Stream(nullptr, &close_counted_stream));
    if (!write_response(Response::ready) || !flush_output()) return 1;

    std::uint64_t total_samples = 0;
    std::uint64_t prior_request_id = 0;
    for (;;) {
        std::uint8_t raw_command = 0;
        if (!read_scalar(raw_command)) return 0;
        const auto command = static_cast<Command>(raw_command);
        if (command == Command::shutdown) return 0;
        std::uint64_t request_id = 0;
        if (!read_scalar(request_id) || request_id == 0 || request_id <= prior_request_id) return 1;
        prior_request_id = request_id;
        if (command == Command::push) {
            std::uint32_t sample_count = 0;
            if (!read_scalar(sample_count) || sample_count == 0 ||
                sample_count > kMaximumChunkSamples ||
                total_samples + sample_count > kMaximumStreamSamples) {
                static_cast<void>(write_error(request_id, ErrorCode::invalid_input));
                return 1;
            }
            std::vector<float> samples(sample_count);
            if (std::fread(samples.data(), sizeof(float), sample_count, stdin) != sample_count) {
                std::fill(samples.begin(), samples.end(), 0.0F);
                return 1;
            }
            const bool valid = std::all_of(samples.cbegin(), samples.cend(), [](const float value) {
                return std::isfinite(value) && value >= -1.0F && value <= 1.0F;
            });
            if (!valid) {
                std::fill(samples.begin(), samples.end(), 0.0F);
                static_cast<void>(write_error(request_id, ErrorCode::invalid_input));
                return 1;
            }
            if (!stream.active() && !stream.start([&] { return create_stream(recognizer.get()); })) {
                std::fill(samples.begin(), samples.end(), 0.0F);
                static_cast<void>(write_error(request_id, ErrorCode::native_runtime));
                return 1;
            }
            const auto status = nemo_speech_asr_stream_push_f32(
                stream.get(), samples.data(), samples.size(), 16'000);
            std::fill(samples.begin(), samples.end(), 0.0F);
            if (status != NEMO_SPEECH_ASR_OK) {
                static_cast<void>(write_error(request_id, ErrorCode::native_runtime));
                return 1;
            }
            total_samples += sample_count;
            if (!pull_one_result(stream.get(), request_id)) return 1;
        } else if (command == Command::finish) {
            // The pinned C API's finish stores exactly one pending final.
            // Drain it into its own RAII owner, then destroy the stream BEFORE
            // acknowledging completion. No replacement exists until next push.
            Result final_result(nullptr, &nemo_speech_asr_result_destroy);
            if (stream.active()) {
                const bool finished = stream.finish([&](auto* active) {
                    if (nemo_speech_asr_stream_finish(active) != NEMO_SPEECH_ASR_OK) return false;
                    ++counts.finished;
                    nemo_speech_asr_result* raw = nullptr;
                    const auto status = nemo_speech_asr_stream_next(active, &raw);
                    final_result.reset(raw);
                    return status == NEMO_SPEECH_ASR_OK && final_result &&
                           nemo_speech_asr_result_is_final(final_result.get());
                });
                if (!finished) {
                    static_cast<void>(write_error(request_id, ErrorCode::native_runtime));
                    return 1;
                }
            }
            total_samples = 0;
            if (final_result) {
                if (!write_result(request_id, final_result)) return 1;
            } else if (!write_response(Response::no_result) || !write_scalar(request_id) ||
                       !flush_output()) return 1;
        } else if (command == Command::statistics) {
            if (!write_response(Response::statistics) || !write_scalar(request_id) ||
                !write_scalar(counts.created) || !write_scalar(counts.finished) ||
                !write_scalar(counts.destroyed) || !write_scalar(counts.errors) ||
                !write_scalar(counts.created - counts.destroyed) ||
                !write_scalar(counts.maximum) || !flush_output()) return 1;
        } else {
            return 1;
        }
    }
}

}  // namespace

int main() noexcept {
    try {
        return run();
    } catch (...) {
        return 1;
    }
}
