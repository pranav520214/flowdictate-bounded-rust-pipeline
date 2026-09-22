// SPDX-License-Identifier: MIT OR Apache-2.0
#include "../utterance.h"
#include <cstdlib>
#include <memory>
#include <vector>

namespace {
struct Trace {
    int alive = 0;
    int created = 0;
    int destroyed = 0;
    std::vector<int> events;
};
void require(bool condition) { if (!condition) std::abort(); }
struct NativeStream {
    Trace& trace;
    explicit NativeStream(Trace& state) : trace(state) {
        require(trace.alive == 0);
        ++trace.alive;
        ++trace.created;
        trace.events.push_back(1);
    }
    ~NativeStream() {
        require(trace.alive == 1);
        --trace.alive;
        ++trace.destroyed;
        trace.events.push_back(4);
    }
};
using Owner = Utterance<std::unique_ptr<NativeStream>>;
void run(int scenario) {
    Trace trace;
    auto factory = [&] { return std::make_unique<NativeStream>(trace); };
    {
        Owner owner(nullptr);
        if (scenario == 0) {
            for (int index = 0; index < 1000; ++index) {
                require(owner.start(factory));
                // Rejected overlapping start must not even invoke the factory.
                require(!owner.start(factory));
                require(owner.finish([&](auto*) {
                    require(!owner.start(factory));
                    require(!owner.finish([](auto*) { return true; }));
                    trace.events.push_back(2);
                    trace.events.push_back(3);
                    return true;
                }));
                require(!owner.active() && trace.alive == 0);
                require(!owner.finish([](auto*) { return true; }));
            }
            require(trace.created == 1000 && trace.destroyed == 1000);
            for (std::size_t i = 0; i < trace.events.size(); ++i)
                require(trace.events[i] == static_cast<int>(i % 4) + 1);
        } else if (scenario == 1) {
            require(!owner.start([] { return std::unique_ptr<NativeStream>{}; }));
            require(owner.start(factory)); // Decoder error exits owning scope.
        } else if (scenario == 2 || scenario == 3) {
            require(owner.start(factory));
            require(!owner.finish([](auto*) { return false; }));
            require(trace.alive == 0);
            require(owner.start(factory));
            require(owner.finish([](auto*) { return true; }));
        } else {
            require(owner.start(factory));
            try {
                owner.finish([](auto*) -> bool { throw 1; });
                require(false);
            } catch (int) { require(trace.alive == 0); }
            require(owner.start(factory)); // Shutdown closes the active stream.
        }
    }
    require(trace.alive == 0 && trace.created == trace.destroyed);
}
}
int main(int argc, char** argv) {
    if (argc != 2) return 1;
    run(std::atoi(argv[1]));
}
