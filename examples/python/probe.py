#!/usr/bin/env python3
# SPDX-License-Identifier: LGPL-2.1-or-later
"""AudioCaps from Python with ctypes (no binding package; bundle libaudiocaps.so.1 with the app).

Run: AUDIOCAPS_LIB=target/release/libaudiocaps.so python3 examples/python/probe.py [fixture-dir]
"""
import ctypes as C
import json
import os
import select
import sys

lib = C.CDLL(os.environ.get("AUDIOCAPS_LIB", "libaudiocaps.so.1"))
lib.ac_abi_version.restype = C.c_uint32
lib.ac_schema_version.restype = C.c_char_p
lib.ac_last_error.restype = C.c_char_p
lib.ac_context_new.restype = C.c_void_p
lib.ac_context_new_fixture.restype = C.c_void_p
lib.ac_context_new_fixture.argtypes = [C.c_char_p]
lib.ac_context_free.argtypes = [C.c_void_p]
lib.ac_probe.restype = C.c_void_p
lib.ac_probe.argtypes = [C.c_void_p, C.c_char_p, C.c_char_p]
lib.ac_snapshot_free.argtypes = [C.c_void_p]
lib.ac_snapshot_generation.restype = C.c_uint64
lib.ac_snapshot_generation.argtypes = [C.c_void_p]
lib.ac_snapshot_endpoint_count.restype = C.c_size_t
lib.ac_snapshot_endpoint_count.argtypes = [C.c_void_p]
lib.ac_snapshot_endpoint_id.restype = C.c_char_p
lib.ac_snapshot_endpoint_id.argtypes = [C.c_void_p, C.c_size_t]
# Owned strings come back as c_void_p so they can be freed with ac_string_free.
lib.ac_snapshot_json.restype = C.c_void_p
lib.ac_snapshot_json.argtypes = [C.c_void_p]
lib.ac_snapshot_findings_json.restype = C.c_void_p
lib.ac_snapshot_findings_json.argtypes = [C.c_void_p]
lib.ac_string_free.argtypes = [C.c_void_p]
lib.ac_can_play.restype = C.c_int
lib.ac_can_play.argtypes = [C.c_void_p, C.c_char_p, C.c_char_p, C.c_char_p, C.POINTER(C.c_void_p)]
CALLBACK = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.c_char_p)
lib.ac_watch_new.restype = C.c_void_p
lib.ac_watch_new.argtypes = [C.c_void_p, C.c_char_p, C.c_char_p, C.c_uint32, CALLBACK, C.c_void_p]
lib.ac_watch_fd.argtypes = [C.c_void_p]
lib.ac_watch_dispatch.argtypes = [C.c_void_p]
lib.ac_watch_free.argtypes = [C.c_void_p]
AC_WATCH_DISPATCH = 0


def take(ptr):
    """Copies an owned string and frees it."""
    if not ptr:
        return None
    try:
        return C.string_at(ptr).decode()
    finally:
        lib.ac_string_free(ptr)


def check(handle, what):
    if not handle:
        raise RuntimeError(f"{what}: {lib.ac_last_error().decode()}")
    return handle


PATHS = ["alsa_direct", "pulse_audio", "pipe_wire", "jack"]
FORMATS = [("lpcm6", {"channels": 6, "rate_hz": 48000, "bits": 24}), ("ac3", {"codec": "ac3"})]
ANSWER = {1: "yes", 0: "no", -1: "unknown", -2: "error"}
OPTIONS = json.dumps({"deadline_ms": 0}).encode()


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    print(f"abi {lib.ac_abi_version()}\nschema {lib.ac_schema_version().decode()}")
    ctx = check(lib.ac_context_new_fixture(sys.argv[1].encode()) if len(sys.argv) > 1
                else lib.ac_context_new(), "context")
    snap = check(lib.ac_probe(ctx, None, OPTIONS), "probe")
    n = lib.ac_snapshot_endpoint_count(snap)
    print(f"generation {lib.ac_snapshot_generation(snap)}\nendpoints {n}")
    for i in range(n):
        eid = lib.ac_snapshot_endpoint_id(snap, i)
        for path in PATHS:
            for label, fmt in FORMATS:
                reason = C.c_void_p()
                a = lib.ac_can_play(snap, eid, path.encode(), json.dumps(fmt).encode(), C.byref(reason))
                print(f"{eid.decode()} {path} {label} {ANSWER[a]} {take(reason.value) or '-'}")
    findings = take(lib.ac_snapshot_findings_json(snap))
    print(f"findings_bytes {len(findings.encode())}")
    lib.ac_snapshot_free(snap)

    @CALLBACK
    def on_update(_user, s, diff):
        print(f"watch generation {lib.ac_snapshot_generation(s)} "
              f"endpoints {lib.ac_snapshot_endpoint_count(s)} diff_bytes {len(diff)}")

    w = check(lib.ac_watch_new(ctx, None, OPTIONS, AC_WATCH_DISPATCH, on_update, None), "watch")
    if select.select([lib.ac_watch_fd(w)], [], [], 10)[0]:
        lib.ac_watch_dispatch(w)
    lib.ac_watch_free(w)
    lib.ac_context_free(ctx)


if __name__ == "__main__":
    main()
