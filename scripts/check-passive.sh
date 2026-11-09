#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# check-passive.sh — runs `audiocaps capture` under strace (following child processes) and fails
# if anything opened a sound device node (/dev/snd/*). Prints the tarball path on success.
# Usage (in a guest, or on real hardware with Micaiah's OK): bash scripts/check-passive.sh <label>
set -euo pipefail
label="${1:-system}"
out="${CAPTURE_OUT:-$HOME/captures}"
trace="$(mktemp)"
mkdir -p "$out"
strace -f -qq -e trace=open,openat,openat2 -o "$trace" audiocaps capture --out "$out" --label "$label" >"$trace.out"
opens="$(grep -c 'open' "$trace" || true)"
snd="$(grep -E '"/dev/snd/' "$trace" || true)"
if [ -n "$snd" ]; then
  echo "PASSIVE CHECK FAILED: sound device nodes were opened:"; echo "$snd" | head -n 20; exit 1
fi
echo "passive check: ok ($opens open calls traced, none under /dev/snd)"
cat "$trace.out"
rm -f "$trace" "$trace.out"
