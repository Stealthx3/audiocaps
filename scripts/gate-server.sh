#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# gate-server.sh — phase 4 gate checks inside lab-audiocaps-capture with a sound server running
# (start one first: capture-stack.sh pipewire|pulseaudio).
#
#   pipewire   the live PipeWire backend matches `pw-dump` read at the same moment: server, clock,
#              defaults, policy, devices (profiles, routes) and audio nodes (formats, IEC958 codecs,
#              state, volume)
#   pulse      the live PulseAudio backend matches `pactl -f json info/list`
#   passive    probes under strace open no PCM device while the server runs
#   hotplug    time from a server change (a null sink added/removed through the server) and from a
#              kernel hotplug (the snd-dummy card loaded/unloaded in the guest) to the new
#              snapshot from `audiocaps watch`; every change must arrive in under 20 ms
#
# Usage: bash scripts/gate-server.sh <check>...
set -euo pipefail
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0

compare() { # live.json ref.json label
  python3 - "$1" "$2" "$3" <<'PY'
import json, sys
live, ref, label = (json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), sys.argv[3])
bad = []
def cmp(path, a, b):
    if a != b:
        bad.append(f"{path}: live {a!r} vs reference {b!r}")
for k in ("kind", "name", "version", "clock", "default_sink", "default_source", "session_manager"):
    cmp(k, live.get(k), ref.get(k))
cmp("policy keys", sorted(live.get("policy", {})), sorted(ref.get("policy", {})))
def by_name(xs): return {x["name"]: x for x in xs}
ln, rn = by_name(live["nodes"]), by_name(ref["nodes"])
cmp("node names", sorted(ln), sorted(rn))
for name in sorted(set(ln) & set(rn)):
    a, b = ln[name], rn[name]
    for k in ("media_class", "dir", "api", "factory", "alsa_card", "alsa_device", "state", "format",
              "iec958_codecs", "muted", "profile"):
        cmp(f"node {name} {k}", a.get(k), b.get(k))
    ef = lambda x: sorted(json.dumps(f, sort_keys=True) for f in x.get("enum_formats", []))
    cmp(f"node {name} enum_formats", ef(a), ef(b))
    va, vb = a.get("volume_percent"), b.get("volume_percent")
    if (va is None) != (vb is None) or (va is not None and abs(va - vb) > 1):
        bad.append(f"node {name} volume: live {va} vs reference {vb}")
ld, rd = by_name(live["devices"]), by_name(ref["devices"])
cmp("device names", sorted(ld), sorted(rd))
for name in sorted(set(ld) & set(rd)):
    a, b = ld[name], rd[name]
    for k in ("api", "alsa_card", "active_profile"):
        cmp(f"device {name} {k}", a.get(k), b.get(k))
    prof = lambda x: sorted((p["name"], p.get("available")) for p in x["profiles"])
    cmp(f"device {name} profiles", prof(a), prof(b))
    rt = lambda x: sorted((r["name"], r["direction"], r.get("available"), r["active"]) for r in x["routes"])
    cmp(f"device {name} routes", rt(a), rt(b))
nodes, devs = len(rn), len(rd)
for b in bad: print("  MISMATCH", b)
print(f"{label}: {nodes} nodes, {devs} devices compared, {len(bad)} mismatches")
sys.exit(1 if bad or not nodes else 0)
PY
}

check_pipewire() {
  # Read both twice and keep a consistent pair: node states can change between two reads.
  for _ in 1 2 3; do
    audiocaps server-dump pipewire > "$work/live.json"
    pw-dump > "$work/pw.json"
    audiocaps server-dump pw-dump "$work/pw.json" > "$work/ref.json"
    if compare "$work/live.json" "$work/ref.json" pipewire > "$work/cmp.out"; then cat "$work/cmp.out"; return 0; fi
    sleep 1
  done
  cat "$work/cmp.out"; return 1
}

check_pulse() {
  for _ in 1 2 3; do
    audiocaps server-dump pulse > "$work/live.json"
    pactl -f json info > "$work/info.json"; pactl -f json list > "$work/list.json"
    audiocaps server-dump pactl "$work/info.json" "$work/list.json" > "$work/ref.json"
    if compare "$work/live.json" "$work/ref.json" pulse > "$work/cmp.out"; then cat "$work/cmp.out"; return 0; fi
    sleep 1
  done
  cat "$work/cmp.out"; return 1
}

check_passive() {
  PASSIVE_CMD='audiocaps probe --repeat 30 --quiet' bash scripts/check-passive.sh | head -1
}

check_hotplug() {
  # With a server running: server-side changes (a null sink added and removed through the
  # server). Without one: kernel hotplug (the snd-dummy card loaded and unloaded; a running
  # server would keep it open). One watch process prints event-to-snapshot latency per update.
  local server=""
  for p in pipewire pulseaudio; do pgrep -x "$p" >/dev/null && { server="$p"; break; }; done
  audiocaps watch > "$work/watch.out" 2>&1 &
  local wpid=$!
  sleep 1.5
  for _ in 1 2 3; do
    if [ -n "$server" ]; then
      id="$(pactl load-module module-null-sink sink_name=audiocaps_gate_null)"
      sleep 0.7
      pactl unload-module "$id"
    else
      sudo modprobe snd-dummy pcm_devs=1 pcm_substreams=1
      sleep 0.7
      sudo modprobe -r snd-dummy
    fi
    sleep 0.7
  done
  kill "$wpid" 2>/dev/null || true
  wait "$wpid" 2>/dev/null || true
  python3 - "$work/watch.out" "${server:-kernel}" <<'PY'
import re, sys
text, mode = open(sys.argv[1]).read(), sys.argv[2]
lines = [l for l in text.splitlines() if "Initial" not in l]
lat = [float(m.group(1)) for l in lines for m in re.finditer(r"latency ([0-9.]+) ms", l)]
kernel = [l for l in lines if "Kernel" in l]
print("  " + text.strip().replace("\n", "\n  "))
need = kernel if mode == "kernel" else lines
ok = len(need) >= 6 and lat and max(lat) < 20.0
print(f"hotplug ({mode}): {len(lat)} updates ({len(kernel)} kernel), max {max(lat) if lat else 'n/a'} ms -> {'ok' if ok else 'FAILED'}")
sys.exit(0 if ok else 1)
PY
}

for c in "$@"; do
  case "$c" in
    pipewire) check_pipewire || fail=1 ;;
    pulse) check_pulse || fail=1 ;;
    passive) check_passive || fail=1 ;;
    hotplug) check_hotplug || fail=1 ;;
    *) echo "unknown check $c" >&2; exit 2 ;;
  esac
done
[ "$fail" = 0 ] && echo "gate-server: PASS" || { echo "gate-server: FAIL"; exit 1; }
