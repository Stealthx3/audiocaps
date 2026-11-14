#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# gate-rules.sh — phase 7 live checks inside lab-audiocaps-capture: a rule fires when its
# condition is created on a real system, and stops when it's removed.
#
#   rate     (PipeWire running) RATE_CAPPED_BY_SERVER fires with the default 48 kHz graph and
#            stops after clock.allowed-rates includes the hardware's rates (restored afterwards)
#   busy     (no server) DEVICE_BUSY_NO_SERVER fires while aplay holds a device, not after
#   latency  LATENCY_BUDGET_EXCEEDED fires for a 1 ms target and not for a 1 s one
#   routing  (PulseAudio running) the default device resolves to the pulse plugin
#
# Usage: bash scripts/gate-rules.sh <check>...
set -euo pipefail
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
fail=0
count() { # code [probe args…]
  local code="$1"; shift
  audiocaps probe "$@" | python3 -c "import json,sys; s=json.load(sys.stdin); print(sum(f['code']=='$code' for f in s.get('findings',[])))"
}
check_rate() {
  local before after
  before="$(count RATE_CAPPED_BY_SERVER)"
  pw-metadata -n settings 0 clock.allowed-rates "[ 44100 48000 88200 96000 ]" >/dev/null
  sleep 1
  after="$(count RATE_CAPPED_BY_SERVER)"
  pw-metadata -n settings 0 clock.allowed-rates "[ 48000 ]" >/dev/null
  echo "rate: default graph $before finding(s), widened graph $after"
  [ "$before" -gt 0 ] && [ "$after" = 0 ] || fail=1
}
check_busy() {
  local dev before during after
  dev="$(audiocaps probe | python3 -c "import json,sys; s=json.load(sys.stdin); print(next(e['id'] for e in s['endpoints'] if e['id'].endswith('p') and e['id'].startswith('alsa:')))")"
  local hw; hw="$(echo "$dev" | sed -E 's/alsa:card([0-9]+)\/pcm([0-9]+)p/hw:\1,\2/')"
  before="$(count DEVICE_BUSY_NO_SERVER)"
  aplay -q -D "$hw" -t raw -f S16_LE -r 48000 -c 2 -d 6 /dev/zero 2>/dev/null &
  local pid=$!; sleep 1
  during="$(count DEVICE_BUSY_NO_SERVER)"
  wait "$pid" || true
  after="$(count DEVICE_BUSY_NO_SERVER)"
  echo "busy: before $before, while $hw plays $during, after $after"
  [ "$before" = 0 ] && [ "$during" = 1 ] && [ "$after" = 0 ] || fail=1
}
check_latency() {
  local ep tight loose
  ep="$(audiocaps probe | python3 -c "import json,sys; s=json.load(sys.stdin); print(next(e['id'] for e in s['endpoints'] if any(p['latency_us'].get('state')=='known' for p in e.get('paths',[]))))")"
  tight="$(count LATENCY_BUDGET_EXCEEDED --app-endpoint "$ep" --latency-target-us 1000)"
  loose="$(count LATENCY_BUDGET_EXCEEDED --app-endpoint "$ep" --latency-target-us 1000000)"
  echo "latency on $ep: 1 ms target $tight, 1 s target $loose"
  [ "$tight" = 1 ] && [ "$loose" = 0 ] || fail=1
}
check_routing() {
  local kinds; kinds="$(audiocaps probe | python3 -c "import json,sys; s=json.load(sys.stdin); print([x['kind'] for x in s['routing']['default_playback'].get('value',[])])")"
  local real; real="$(aplay -v -D default -d 1 -t raw -f S16_LE -r 48000 -c 2 /dev/zero 2>&1 | grep -c 'PulseAudio PCM' || true)"
  echo "routing: ours $kinds, aplay -v shows the PulseAudio plugin: $real"
  [ "$kinds" = "['pulse']" ] && [ "$real" -ge 1 ] || fail=1
}
for c in "$@"; do
  case "$c" in
    rate) check_rate ;; busy) check_busy ;; latency) check_latency ;; routing) check_routing ;;
    *) echo "unknown check $c" >&2; exit 2 ;;
  esac
done
[ "$fail" = 0 ] && echo "gate-rules: PASS" || { echo "gate-rules: FAIL"; exit 1; }
