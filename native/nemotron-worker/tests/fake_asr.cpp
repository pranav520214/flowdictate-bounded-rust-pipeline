// Model-free C ABI fixture. Linked only into the boundary-test executable.
#include <nemo_speech/asr.h>
#include <cstdlib>
#include <cstring>
#include <memory>
namespace {
int created = 0, destroyed = 0, recognizers = 0, results = 0, expected = 0;
bool fail_create = false;
void require(bool value) { if (!value) std::abort(); }
struct Accounting {
    ~Accounting() {
        require(created == destroyed && created == expected && recognizers == 0 && results == 0);
    }
} accounting;
}
struct nemo_speech_asr_recognizer {};
struct nemo_speech_asr_stream { float mode = 0; bool finished = false; };
struct nemo_speech_asr_result { bool malformed = false; };
extern "C" {
nemo_speech_asr_recognition_options nemo_speech_asr_recognition_options_default() { return {}; }
nemo_speech_asr_status nemo_speech_asr_create(const nemo_speech_asr_recognizer_config* cfg, nemo_speech_asr_recognizer** out) {
    require(cfg && cfg->model && cfg->streaming && cfg->streaming->rnnt_right_context == 1);
    const auto* path = cfg->model->path;
    expected = std::strcmp(path, "expect2") == 0 ? 2 : std::strcmp(path, "expect0") == 0 ? 0 : 1;
    fail_create = std::strcmp(path, "create-error") == 0;
    *out = std::make_unique<nemo_speech_asr_recognizer>().release();
    ++recognizers;
    return NEMO_SPEECH_ASR_OK;
}
void nemo_speech_asr_destroy(nemo_speech_asr_recognizer* value) {
    std::unique_ptr<nemo_speech_asr_recognizer> owned(value);
    require(created == destroyed);
    --recognizers;
}
nemo_speech_asr_status nemo_speech_asr_streaming_recognize(nemo_speech_asr_recognizer* recognizer, const nemo_speech_asr_recognition_options* options, nemo_speech_asr_stream** out) {
    require(recognizer && options && created == destroyed);
    *out = std::make_unique<nemo_speech_asr_stream>().release();
    ++created;
    return fail_create ? NEMO_SPEECH_ASR_ERROR_RUNTIME : NEMO_SPEECH_ASR_OK;
}
nemo_speech_asr_status nemo_speech_asr_stream_push_f32(nemo_speech_asr_stream* stream, const float* samples, size_t length, int32_t rate) {
    require(stream && samples && length > 0 && length <= 2560 && rate == 16000);
    stream->mode = samples[0];
    return stream->mode == -1.0F ? NEMO_SPEECH_ASR_ERROR_RUNTIME : NEMO_SPEECH_ASR_OK;
}
nemo_speech_asr_status nemo_speech_asr_stream_finish(nemo_speech_asr_stream* stream) {
    require(stream && !stream->finished);
    stream->finished = true;
    return stream->mode == -0.5F ? NEMO_SPEECH_ASR_ERROR_RUNTIME : NEMO_SPEECH_ASR_OK;
}
nemo_speech_asr_status nemo_speech_asr_stream_next(nemo_speech_asr_stream* stream, nemo_speech_asr_result** out) {
    require(stream && out);
    *out = nullptr;
    if (!stream->finished) return NEMO_SPEECH_ASR_OK;
    auto result = std::make_unique<nemo_speech_asr_result>();
    result->malformed = stream->mode == 0.25F;
    *out = result.release();
    ++results;
    return stream->mode == 0.5F ? NEMO_SPEECH_ASR_ERROR_RUNTIME : NEMO_SPEECH_ASR_OK;
}
void nemo_speech_asr_stream_close(nemo_speech_asr_stream* stream) {
    require(stream && created == destroyed + 1);
    std::unique_ptr<nemo_speech_asr_stream> owned(stream);
    ++destroyed;
}
bool nemo_speech_asr_result_is_final(const nemo_speech_asr_result*) { return true; }
size_t nemo_speech_asr_result_alternative_count(const nemo_speech_asr_result* result) { return result->malformed ? 2U : 1U; }
const char* nemo_speech_asr_result_transcript(const nemo_speech_asr_result*, size_t) { return ""; }
float nemo_speech_asr_result_audio_processed(const nemo_speech_asr_result*) { return 0.0F; }
void nemo_speech_asr_result_destroy(nemo_speech_asr_result* result) {
    std::unique_ptr<nemo_speech_asr_result> owned(result);
    --results;
}
}
