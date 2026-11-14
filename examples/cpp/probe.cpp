// SPDX-License-Identifier: LGPL-2.1-or-later
// AudioCaps from C++: the C API with RAII handles.
// Build: c++ -std=c++17 -Iinclude examples/cpp/probe.cpp -Ltarget/release -laudiocaps -o probe-cpp
#include <poll.h>

#include <cstdio>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>

#include "audiocaps.h"

namespace ac {
struct ContextFree { void operator()(ac_context *c) const { ac_context_free(c); } };
struct SnapshotFree { void operator()(ac_snapshot *s) const { ac_snapshot_free(s); } };
struct WatchFree { void operator()(ac_watch *w) const { ac_watch_free(w); } };
struct StringFree { void operator()(char *s) const { ac_string_free(s); } };
using Context = std::unique_ptr<ac_context, ContextFree>;
using Snapshot = std::unique_ptr<ac_snapshot, SnapshotFree>;
using Watch = std::unique_ptr<ac_watch, WatchFree>;
using String = std::unique_ptr<char, StringFree>;

template <typename T> T check(T h, const char *what) {
    if (!h) throw std::runtime_error(std::string(what) + ": " + ac_last_error());
    return h;
}
}  // namespace ac

int main(int argc, char **argv) try {
    static const char *paths[] = {"alsa_direct", "pulse_audio", "pipe_wire", "jack"};
    static const char *formats[] = {R"({"channels":6,"rate_hz":48000,"bits":24})", R"({"codec":"ac3"})"};
    static const char *answer[] = {"error", "unknown", "no", "yes"};

    std::printf("abi %u\nschema %s\n", ac_abi_version(), ac_schema_version());
    ac::Context ctx = ac::check(ac::Context(argc > 1 ? ac_context_new_fixture(argv[1]) : ac_context_new()), "context");
    {
        ac::Snapshot snap = ac::check(ac::Snapshot(ac_probe(ctx.get(), nullptr, R"({"deadline_ms":0})")), "probe");
        size_t n = ac_snapshot_endpoint_count(snap.get());
        std::printf("generation %llu\nendpoints %zu\n", (unsigned long long)ac_snapshot_generation(snap.get()), n);
        for (size_t i = 0; i < n; i++) {
            const char *id = ac_snapshot_endpoint_id(snap.get(), i);
            for (const char *path : paths) {
                for (int f = 0; f < 2; f++) {
                    char *raw = nullptr;
                    int a = ac_can_play(snap.get(), id, path, formats[f], &raw);
                    ac::String reason(raw);
                    std::printf("%s %s %s %s %s\n", id, path, f ? "ac3" : "lpcm6", answer[a + 2],
                                reason ? reason.get() : "-");
                }
            }
        }
        ac::String findings(ac_snapshot_findings_json(snap.get()));
        std::printf("findings_bytes %zu\n", std::strlen(findings.get()));
    }
    auto on_update = [](void *, const ac_snapshot *s, const char *diff) {
        std::printf("watch generation %llu endpoints %zu diff_bytes %zu\n",
                    (unsigned long long)ac_snapshot_generation(s), ac_snapshot_endpoint_count(s), std::strlen(diff));
    };
    ac::Watch w = ac::check(
        ac::Watch(ac_watch_new(ctx.get(), nullptr, R"({"deadline_ms":0})", AC_WATCH_DISPATCH, on_update, nullptr)),
        "watch");
    pollfd pfd{ac_watch_fd(w.get()), POLLIN, 0};
    if (poll(&pfd, 1, 10000) > 0) ac_watch_dispatch(w.get());
    return 0;
} catch (const std::exception &e) {
    std::fprintf(stderr, "%s\n", e.what());
    return 1;
}
