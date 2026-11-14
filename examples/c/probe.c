/* SPDX-License-Identifier: LGPL-2.1-or-later */
/* AudioCaps from C: probe, ask "can it play?", watch with a pollable fd.
 * Build: cc -Iinclude examples/c/probe.c -Ltarget/release -laudiocaps -o probe-c
 * Run:   LD_LIBRARY_PATH=target/release ./probe-c [fixture-dir]   (no argument: the live system) */
#include <poll.h>
#include <stdio.h>
#include <string.h>

#include "audiocaps.h"

static const char *PATHS[] = {"alsa_direct", "pulse_audio", "pipe_wire", "jack"};
static const char *FORMATS[] = {"{\"channels\":6,\"rate_hz\":48000,\"bits\":24}", "{\"codec\":\"ac3\"}"};
static const char *ANSWER[] = {"error", "unknown", "no", "yes"}; /* index: answer + 2 */

static void on_update(void *user, const ac_snapshot *snap, const char *diff_json) {
    (void)user;
    printf("watch generation %llu endpoints %zu diff_bytes %zu\n",
           (unsigned long long)ac_snapshot_generation(snap), ac_snapshot_endpoint_count(snap),
           strlen(diff_json));
}

int main(int argc, char **argv) {
    printf("abi %u\nschema %s\n", ac_abi_version(), ac_schema_version());
    ac_context *ctx = argc > 1 ? ac_context_new_fixture(argv[1]) : ac_context_new();
    if (!ctx) {
        fprintf(stderr, "context: %s\n", ac_last_error());
        return 1;
    }
    /* NULL scope: everything. deadline_ms 0: wait for every backend (deterministic output). */
    ac_snapshot *snap = ac_probe(ctx, NULL, "{\"deadline_ms\":0}");
    if (!snap) {
        fprintf(stderr, "probe: %s\n", ac_last_error());
        return 1;
    }
    size_t n = ac_snapshot_endpoint_count(snap);
    printf("generation %llu\nendpoints %zu\n", (unsigned long long)ac_snapshot_generation(snap), n);
    for (size_t i = 0; i < n; i++) {
        const char *id = ac_snapshot_endpoint_id(snap, i);
        for (size_t p = 0; p < 4; p++) {
            for (size_t f = 0; f < 2; f++) {
                char *reason = NULL;
                int a = ac_can_play(snap, id, PATHS[p], FORMATS[f], &reason);
                printf("%s %s %s %s %s\n", id, PATHS[p], f ? "ac3" : "lpcm6", ANSWER[a + 2],
                       reason ? reason : "-");
                ac_string_free(reason);
            }
        }
    }
    char *findings = ac_snapshot_findings_json(snap);
    printf("findings_bytes %zu\n", strlen(findings));
    ac_string_free(findings);
    ac_snapshot_free(snap);

    /* Watch in dispatch mode: poll the fd from your own loop, then dispatch on your thread. */
    ac_watch *w = ac_watch_new(ctx, NULL, "{\"deadline_ms\":0}", AC_WATCH_DISPATCH, on_update, NULL);
    if (!w) {
        fprintf(stderr, "watch: %s\n", ac_last_error());
        return 1;
    }
    struct pollfd pfd = {.fd = ac_watch_fd(w), .events = POLLIN};
    if (poll(&pfd, 1, 10000) > 0)
        ac_watch_dispatch(w);
    ac_watch_free(w);
    ac_context_free(ctx);
    return 0;
}
