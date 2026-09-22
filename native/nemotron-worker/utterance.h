// SPDX-License-Identifier: MIT OR Apache-2.0
#pragma once
#include <utility>

// Sole owner of the per-utterance native handle. The factory is invoked only
// while empty; finish transfers ownership to a local RAII guard before calling
// native code. Thus failure/exception/drain completion all destroy the handle
// before a subsequent start. The recognizer must outlive this owner.
template <typename Handle>
class Utterance final {
public:
    explicit Utterance(Handle empty) : stream_(std::move(empty)) {}
    Utterance(const Utterance&) = delete;
    Utterance& operator=(const Utterance&) = delete;

    template <typename Factory>
    bool start(Factory&& factory) {
        if (stream_ || finalizing_) return false;
        stream_ = factory();
        return static_cast<bool>(stream_);
    }

    bool active() const noexcept { return static_cast<bool>(stream_); }
    auto get() const noexcept { return stream_.get(); }

    template <typename FinishAndDrain>
    bool finish(FinishAndDrain&& finish_and_drain) {
        if (!stream_ || finalizing_) return false;
        finalizing_ = true;
        struct ExitFinalizing {
            bool& flag;
            ~ExitFinalizing() { flag = false; }
        } state{finalizing_};
        // Reverse destruction order: close the native handle before reopening
        // the start gate, including during exception unwinding.
        auto finalizing = std::move(stream_);
        return finish_and_drain(finalizing.get());
    }

private:
    Handle stream_;
    bool finalizing_ = false;
};
