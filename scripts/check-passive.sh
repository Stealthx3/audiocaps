#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# check-passive.sh — runs a command (default: `audiocaps capture`) under strace, following child
# processes, and checks every sound device node it opened against the passive rules:
#   - control devices (/dev/snd/controlC*) may be opened; they are only read;
#   - PCM devices (/dev/snd/pcmC*) may be opened only when no sound server is running, and only
#     with O_NONBLOCK;
#   - nothing else under /dev/snd (seq, timer, hwdep, midi) may be opened.
# Prints the command's output on success.
# Usage (in a guest, or on real hardware with Micaiah's OK):
#   bash scripts/check-passive.sh <label>                 # capture
#   PASSIVE_CMD='audiocaps probe --live' bash scripts/check-passive.sh   # any other command
set -euo pipefail
label="${1:-system}"
out="${CAPTURE_OUT:-$HOME/captures}"
trace="$(mktemp)"
mkdir -p "$out"
server=""
for p in pipewire pulseaudio jackd jackdbus; do
  if pgrep -x "$p" >/dev/null; then server="$p"; break; fi
done
if [ -n "${PASSIVE_CMD:-}" ]; then
  # shellcheck disable=SC2086
  strace -f -qq -e trace=open,openat,openat2 -o "$trace" $PASSIVE_CMD >"$trace.out"
else
  strace -f -qq -e trace=open,openat,openat2 -o "$trace" audiocaps capture --out "$out" --label "$label" >"$trace.out"
fi
opens="$(grep -c 'open' "$trace" || true)"
snd="$(grep -E '"/dev/snd/' "$trace" || true)"
other="$(printf '%s\n' "$snd" | grep -E '"/dev/snd/' | grep -vE '"/dev/snd/(pcmC|controlC)' || true)"
pcm="$(printf '%s\n' "$snd" | grep -E '"/dev/snd/pcmC' || true)"
ctl="$(printf '%s\n' "$snd" | grep -cE '"/dev/snd/controlC' || true)"
fail=0
if [ -n "$other" ]; then
  echo "PASSIVE CHECK FAILED: opened sound nodes other than PCM and control devices:"; echo "$other" | head -n 20; fail=1
fi
if [ -n "$pcm" ] && [ -n "$server" ]; then
  echo "PASSIVE CHECK FAILED: opened PCM devices while $server runs:"; echo "$pcm" | head -n 20; fail=1
fi
blocking="$(printf '%s\n' "$pcm" | grep -E '"/dev/snd/pcmC' | grep -v O_NONBLOCK || true)"
if [ -n "$blocking" ]; then
  echo "PASSIVE CHECK FAILED: blocking PCM opens:"; echo "$blocking" | head -n 20; fail=1
fi
[ "$fail" = 0 ] || exit 1
npcm="$(printf '%s\n' "$pcm" | grep -cE '"/dev/snd/pcmC' || true)"
echo "passive check: ok ($opens open calls traced; server: ${server:-none}; control opens: $ctl; PCM opens: $npcm, all O_NONBLOCK)"
cat "$trace.out"
rm -f "$trace" "$trace.out"
