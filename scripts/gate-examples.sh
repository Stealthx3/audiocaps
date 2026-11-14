#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Phase 9 gate: the consumer examples (C, C++, Python, .NET) build against the generated header and
# the shared library, run on fixtures, and print identical output. Also checks the header is current
# and the soname. Run in the build VM: scripts/lab.sh run 'bash scripts/gate-examples.sh'
set -euo pipefail
cd "$(dirname "$0")/.."
. ~/.cargo/env 2>/dev/null || true
out="target/examples-gate"; rm -rf "$out"; mkdir -p "$out"
fail=0; say() { printf '%-44s %s\n' "$1" "$2"; }

cargo build --release -q -p audiocaps-ffi
lib=target/release/libaudiocaps.so
soname="$(readelf -d "$lib" | sed -n 's/.*SONAME.*\[\(.*\)\]/\1/p')"
[ "$soname" = libaudiocaps.so.1 ] && say "soname" "ok ($soname)" || { say soname "FAIL ($soname)"; fail=1; }
ln -sf libaudiocaps.so "target/release/libaudiocaps.so.1"
if bash scripts/gen-header.sh --check >/dev/null; then say "header current" ok; else say "header current" FAIL; fail=1; fi

# Only ac_* symbols are exported.
extra="$(nm -D --defined-only "$lib" | awk '$2 ~ /^[TD]$/ {print $3}' | grep -v '^ac_' || true)"
[ -z "$extra" ] && say "exports ac_* only" ok || { say "exports ac_* only" "FAIL: $(echo $extra | head -c 200)"; fail=1; }

cc -std=c11 -Wall -Wextra -Werror -Iinclude examples/c/probe.c -Ltarget/release -laudiocaps -o "$out/probe-c"
c++ -std=c++17 -Wall -Wextra -Werror -Iinclude examples/cpp/probe.cpp -Ltarget/release -laudiocaps -o "$out/probe-cpp"
# The header must also compile as C++ on its own and as strict C89-compatible C99.
echo '#include "audiocaps.h"' | cc -std=c99 -pedantic -Werror -Iinclude -x c -c - -o /dev/null
echo '#include "audiocaps.h"' | c++ -std=c++11 -pedantic -Werror -Iinclude -x c++ -c - -o /dev/null
say "C and C++ build (-Werror, header c99/c++11)" ok
have_cs=0
if command -v mcs >/dev/null && command -v mono >/dev/null; then
  mcs -nologo -out:"$out/probe-cs.exe" examples/dotnet/Probe.cs && have_cs=1
fi

export LD_LIBRARY_PATH="$PWD/target/release"
for fx in htpc synthetic-hdmi-desktop synthetic-snap-jack; do
  dir="tests/fixtures/$fx"
  "$out/probe-c" "$dir" > "$out/$fx.c.txt"
  "$out/probe-cpp" "$dir" > "$out/$fx.cpp.txt"
  AUDIOCAPS_LIB="$lib" python3 examples/python/probe.py "$dir" > "$out/$fx.py.txt"
  langs="c cpp py"
  if [ "$have_cs" = 1 ]; then mono "$out/probe-cs.exe" "$dir" > "$out/$fx.cs.txt"; langs="$langs cs"; fi
  lines="$(wc -l < "$out/$fx.c.txt")"
  grep -q '^watch generation 2 ' "$out/$fx.c.txt" || { say "$fx: watch event" FAIL; fail=1; }
  # The htpc capture has measured ranges and passthrough for its HDMI path; synthetic fixtures don't.
  if [ "$fx" = htpc ]; then
    grep -q '^alsa:card1/pcm9p pipe_wire ac3 yes -$' "$out/$fx.c.txt" || { say "$fx: can_play yes" FAIL; fail=1; }
  fi
  for l in $langs; do
    if cmp -s "$out/$fx.c.txt" "$out/$fx.$l.txt"; then :; else
      say "$fx: $l vs c" FAIL; diff "$out/$fx.c.txt" "$out/$fx.$l.txt" | head -10; fail=1
    fi
  done
  say "$fx ($lines lines)" "identical: $langs"
done
[ "$have_cs" = 1 ] || say ".NET example" "SKIPPED (no mcs/mono)"

# Errors come back through ac_last_error, not crashes.
cat > "$out/errors.c" <<'C'
#include <stdio.h>
#include <string.h>
#include "audiocaps.h"
int main(void) {
    int bad = 0;
    if (ac_context_new_fixture("/nonexistent") || !*ac_last_error()) bad |= 1;
    ac_context *c = ac_context_new_fixture("tests/fixtures/htpc");
    if (ac_probe(c, "{\"layers\":\"some\"}", NULL) || !strstr(ac_last_error(), "scope")) bad |= 2;
    if (ac_probe(c, NULL, "{\"nope\":1}") || !strstr(ac_last_error(), "options")) bad |= 4;
    ac_snapshot *s = ac_probe(c, "{\"fields\":[\"effective\"],\"layers\":\"all\"}", "{\"deadline_ms\":0}");
    char *r = NULL;
    if (ac_can_play(s, "x", "warp_drive", "{}", &r) != AC_ERROR || r) bad |= 8;
    if (ac_can_play(s, "nope", "pipe_wire", "{}", &r) != AC_UNKNOWN || !r) bad |= 16;
    ac_string_free(r);
    if (ac_snapshot_endpoint_id(s, 100000) || ac_snapshot_endpoint_json(s, "nope")) bad |= 32;
    ac_snapshot *s2 = ac_snapshot_clone(s);
    ac_snapshot_free(s);
    char *j = ac_snapshot_json(s2);
    if (!j || !strstr(j, "\"schema_version\"")) bad |= 64;
    ac_string_free(j);
    ac_snapshot_free(s2);
    ac_snapshot_free(NULL); ac_string_free(NULL); ac_watch_free(NULL); ac_context_free(NULL);
    ac_context_free(c);
    printf("errors %d\n", bad);
    return bad;
}
C
cc -std=c11 -Wall -Werror -Iinclude "$out/errors.c" -Ltarget/release -laudiocaps -o "$out/errors"
if command -v valgrind >/dev/null; then
  valgrind -q --error-exitcode=9 --leak-check=full --show-possibly-lost=no --errors-for-leak-kinds=definite "$out/errors" >/dev/null \
    && say "error paths (valgrind)" ok || { say "error paths (valgrind)" FAIL; fail=1; }
  valgrind -q --error-exitcode=9 --leak-check=full --show-possibly-lost=no --errors-for-leak-kinds=definite "$out/probe-c" tests/fixtures/htpc >/dev/null \
    && say "C example (valgrind)" ok || { say "C example (valgrind)" FAIL; fail=1; }
else
  "$out/errors" >/dev/null && say "error paths" ok || { say "error paths" FAIL; fail=1; }
fi
[ "$fail" = 0 ] && echo "examples gate: PASS" || { echo "examples gate: FAIL"; exit 1; }
