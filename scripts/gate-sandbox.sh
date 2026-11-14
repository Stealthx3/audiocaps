#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Phase 9 sandbox release gate, run inside lab-audiocaps-capture (Debian 13, AppArmor, PipeWire
# for the lab user, emulated HDA/USB cards, vivid CEC). Uses the portable build from
# scripts/build-portable.sh, copied to ~/portable with `scripts/lab.sh put`.
#
#   setup    install flatpak, snapd, PipeWire stack, rtkit; freedesktop 24.08 runtime; core24
#   flatpak  build the test app (no flatpak-builder) and probe under several permission sets
#   snap     pack a strict snap and probe with auto-connected plugs, then with every plug
#   summary  print the results table
#
# Verifies the plan's open sandbox items: /proc/asound and DRM EDID readable in Flatpak with only
# --socket=pulseaudio; what WirePlumber lets a sandboxed native PipeWire client see; realtime and
# CEC behaviour in a strict snap. And throughout: the library never fails in a sandbox, hidden
# layers are `unknown` with a reason, and missing permissions come with their fix.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC="$PWD"
P="$HOME/portable"
W="$HOME/sandbox-gate"
APP=io.github.stealthx3.AudioCapsProbe
SNAP=audiocaps-probe
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
mkdir -p "$W"

# Raw evidence: the files the library reads, as seen from inside the sandbox.
RAW='for f in /proc/asound/cards /proc/asound/pcm /proc/asound/card1/codec#0 /sys/class/sound /sys/class/drm/card0-Virtual-1/edid /dev/snd /dev/cec0; do
  if [ -d "$f" ]; then ls "$f" >/dev/null 2>&1 && echo "raw $f dir-readable" || echo "raw $f dir-DENIED"
  elif [ -c "$f" ]; then [ -r "$f" ] && [ -w "$f" ] && echo "raw $f rw" || echo "raw $f DENIED"
  elif [ -e "$f" ]; then head -c 8 "$f" >/dev/null 2>&1 && echo "raw $f readable" || echo "raw $f DENIED"
  else echo "raw $f absent"; fi
done'

summarize() { # $1 = label, $2 = snapshot json
  python3 - "$1" "$2" <<'PY'
import json, sys
label, path = sys.argv[1], sys.argv[2]
try:
    s = json.load(open(path))
except Exception as e:
    print(f"{label}: NO SNAPSHOT ({e})"); sys.exit(0)
def av(a):
    return a["state"] if a["state"] != "unknown" else f"unknown:{a.get('reason')}"
layers = {l["layer"]: av(l["availability"]) for l in s.get("layers", [])}
servers = {x["server"]: av(x["availability"]) for x in s.get("servers", [])}
eps = s.get("endpoints", [])
paths = {}
for e in eps:
    for x in e.get("effective", []):
        paths[x["access_path"]] = paths.get(x["access_path"], 0) + 1
env = s.get("environment") or {}
sb = env.get("sandbox", {})
sbv = sb.get("value", {}) if sb.get("state") == "known" else {}
rt = env.get("realtime") or {}
rtm = {m["method"]: (m["grants"].get("value") if m["grants"]["state"] == "known"
                     else "unknown:" + m["grants"].get("reason", "?")) for m in rt.get("methods", [])}
codes = sorted({f["code"] for f in s.get("findings", [])})
cec = [l for e in eps for l in e.get("layers", []) if "cec" in json.dumps(l.get("detail", {}))]
pw_nodes = sum(1 for e in eps if e["id"].startswith("node:") or any(
    c["id"].get("kind") == "pipewire_node" for c in e.get("correlation", [])))
out = {
    "label": label, "sandbox": sbv.get("kind", "none"),
    "missing": [g["permission"] for g in sbv.get("missing", [])],
    "layers": layers, "servers": servers, "endpoints": len(eps), "pipewire_nodes": pw_nodes,
    "paths": paths, "realtime": {"available": rt.get("available"), **rtm}, "findings": codes,
}
print(json.dumps(out))
PY
}

record() { # $1 label, then the command producing a snapshot on stdout; raw evidence in $W/$1.raw
  local label="$1"; shift
  local t0; t0="$(date +%s%N)"
  if "$@" > "$W/$label.json" 2> "$W/$label.err"; then st=ok; else st="exit:$?"; fi
  local ms=$(( ($(date +%s%N) - t0) / 1000000 ))
  echo "{\"label\":\"$label\",\"status\":\"$st\",\"ms\":$ms}" > "$W/$label.status"
  summarize "$label" "$W/$label.json" > "$W/$label.summary"
  echo "$label: $st in ${ms} ms"
}

cmd_setup() {
  # Ubuntu's generic kernel keeps snd-hda-intel and vivid in linux-modules-extra.
  if grep -q '^ID=ubuntu' /etc/os-release; then
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "linux-modules-extra-$(uname -r)" >/dev/null
    sudo modprobe snd-hda-intel || true
  fi
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq flatpak snapd rtkit \
    pipewire pipewire-pulse pipewire-alsa wireplumber pulseaudio-utils v4l-utils \
    xdg-desktop-portal >/dev/null
  sudo loginctl enable-linger "$(id -un)"
  systemctl --user daemon-reload
  systemctl --user start pipewire.socket pipewire-pulse.socket wireplumber.service
  # The Realtime portal lives in xdg-desktop-portal itself (no desktop backend needed).
  # Headless: no desktop backends, so tell the portal to use none (Realtime is built in).
  mkdir -p "$HOME/.config/xdg-desktop-portal"
  printf '[preferred]\ndefault=none\n' > "$HOME/.config/xdg-desktop-portal/portals.conf"
  systemctl --user restart xdg-desktop-portal.service || true
  if [ "${GATE_FLATPAK:-1}" = 1 ]; then
    flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
    flatpak --user install -y --noninteractive flathub org.freedesktop.Platform//24.08 >/dev/null
  fi
  sudo systemctl enable --now snapd.socket snapd.apparmor >/dev/null 2>&1 || true
  sudo snap wait system seed.loaded
  snap list core24 >/dev/null 2>&1 || sudo snap install core24 >/dev/null
  echo "confinement: $(snap debug confinement)"
  # Emulated CEC (see docs/LAB.md): /dev/cec0 (TV side) and /dev/cec1.
  sudo modprobe vivid n_devs=1 num_inputs=1 num_outputs=1 input_types=3 output_types=1
  sleep 1
  sudo chmod a+rw /dev/cec* /dev/video* || true
  # Join the emulated HDMI output and input; TV on cec0, an audio system answering on cec1.
  v4l2-ctl -d /dev/video0 -c hdmi_000_0_is_connected_to=2 || true
  cec-ctl -d0 --tv >/dev/null
  cec-ctl -d1 --audio --osd-name LabReceiver --vendor-id 0x000c03 >/dev/null
  pkill -f "cec-follower -d1" || true
  nohup cec-follower -d1 >/dev/null 2>&1 &
  ls /dev/cec* && echo "setup: done"
}

cmd_flatpak() {
  local B="$W/fp-build" R="$W/fp-repo"
  rm -rf "$B" "$R"
  flatpak build-init "$B" "$APP" org.freedesktop.Platform org.freedesktop.Platform 24.08
  install -Dm755 "$P/audiocaps" "$B/files/bin/audiocaps"
  install -Dm755 "$P/libaudiocaps.so.1" "$B/files/lib/libaudiocaps.so.1"
  install -Dm644 "$SRC/examples/python/probe.py" "$B/files/share/audiocaps/probe.py"
  # Minimum permission set: the sound socket only (packaging/flatpak/finish-args.yml).
  flatpak build-finish "$B" --command=audiocaps --socket=pulseaudio >/dev/null
  flatpak build-export "$R" "$B" >/dev/null
  flatpak --user remote-add --if-not-exists --no-gpg-verify audiocaps-local "$R"
  flatpak --user install -y --noninteractive --reinstall audiocaps-local "$APP" >/dev/null

  # Unsandboxed baseline.
  record host "$P/audiocaps" probe
  record host-cec "$P/audiocaps" probe --cec
  "$P/audiocaps" server-dump pipewire > "$W/host.server-dump.json" 2>&1 || true
  record host-cecdump "$P/audiocaps" cec-dump

  local base=(flatpak run --command=audiocaps)
  record fp-none "${base[@]}" --nosocket=pulseaudio "$APP" probe
  record fp-pulse "${base[@]}" "$APP" probe
  flatpak run --command=sh "$APP" -c "$RAW" > "$W/fp-pulse.raw" 2>&1
  record fp-pipewire "${base[@]}" --filesystem=xdg-run/pipewire-0:ro "$APP" probe
  flatpak run --command=audiocaps --filesystem=xdg-run/pipewire-0:ro "$APP" server-dump pipewire \
    > "$W/fp-pipewire.server-dump.json" 2>&1 || true
  # What WirePlumber grants the sandboxed client, read while it's connected (watch keeps it open).
  flatpak run --command=audiocaps --filesystem=xdg-run/pipewire-0:ro "$APP" watch > /dev/null 2>&1 &
  local wpid=$!; sleep 3
  pw-dump 2>/dev/null | python3 -c '
import json, sys
for o in json.load(sys.stdin):
    if o.get("type", "").endswith(":Client"):
        p = o.get("info", {}).get("props", {})
        print(p.get("application.name"), p.get("pipewire.access"), p.get("pipewire.access.portal.app_id", ""),
              p.get("pipewire.sec.flatpak", ""), "perms:", o.get("permissions"))
' > "$W/wp-clients.txt" || true
  kill "$wpid" 2>/dev/null; wait "$wpid" 2>/dev/null || true
  local full=(--filesystem=xdg-run/pipewire-0:ro --system-talk-name=org.bluez --device=all)
  record fp-full "${base[@]}" "${full[@]}" "$APP" probe
  flatpak run --command=sh "${full[@]}" "$APP" -c "$RAW" > "$W/fp-full.raw" 2>&1
  record fp-full-cec "${base[@]}" "${full[@]}" "$APP" probe --cec
  record fp-pulse-cecdump "${base[@]}" "$APP" cec-dump
  record fp-full-cecdump "${base[@]}" "${full[@]}" "$APP" cec-dump
  # The shared library from Python inside the sandbox (live system).
  flatpak run --command=python3 --env=AUDIOCAPS_LIB=/app/lib/libaudiocaps.so.1 "$APP" \
    /app/share/audiocaps/probe.py > "$W/fp-python.txt" 2> "$W/fp-python.err" \
    && echo "fp-python: ok ($(wc -l < "$W/fp-python.txt") lines)" || echo "fp-python: FAILED"
}

denials() { # AppArmor denials for the test snap since $1
  sudo journalctl -k --since "$1" 2>/dev/null \
    | grep -o 'apparmor="DENIED".*profile="snap\.audiocaps[^"]*".*name="[^"]*"' \
    | sed 's/ pid=[0-9]*//; s/ comm="[^"]*"//; s/ requested_mask="[^"]*"//; s/ denied_mask="[^"]*"//; s/ fsuid=[0-9]*//; s/ ouid=[0-9]*//' \
    | sort | uniq -c | head -60 || true
}

cmd_snap() {
  local D="$W/snap-root"
  rm -rf "$D" "$W"/*.snap
  # Unsandboxed baseline on this host.
  record snaphost "$P/audiocaps" probe
  record snaphost-cecdump "$P/audiocaps" cec-dump
  mkdir -p "$D/meta" "$D/bin" "$D/lib"
  install -m755 "$P/audiocaps" "$D/bin/audiocaps"
  install -m755 "$P/libaudiocaps.so.1" "$D/lib/"
  cp -a "$P/snap-data/usr" "$D/"
  # Stage only what core24 lacks.
  local core=/snap/core24/current
  for l in "$P"/ubuntu-libs/*; do
    n="$(basename "$l")"
    if [ ! -e "$core/usr/lib/x86_64-linux-gnu/$n" ] && [ ! -e "$core/lib/x86_64-linux-gnu/$n" ]; then
      cp -L "$l" "$D/lib/"
    fi
  done
  python3 - "$SRC/packaging/snap/snapcraft.yaml" "$D/meta/snap.yaml" <<'PY'
import sys, re
src = open(sys.argv[1]).read()
# snap.yaml = snapcraft.yaml without parts (files are staged by the gate).
body = src.split("\nparts:")[0]
body = re.sub(r"(?m)^\s*#.*\n", "", body)
body = body.replace("command: bin/audiocaps", "command: bin/audiocaps")
open(sys.argv[2], "w").write(body + "architectures: [amd64]\n")
PY
  (cd "$W" && snap pack "$D" . >/dev/null)
  sudo snap remove --purge "$SNAP" >/dev/null 2>&1 || true
  sudo snap install --dangerous "$W"/${SNAP}_*.snap >/dev/null
  snap connections "$SNAP" > "$W/snap-auto.connections"
  local t_auto; t_auto="$(date '+%Y-%m-%d %H:%M:%S')"
  record snap-auto "$SNAP" probe
  snap run --shell "$SNAP" -c "$RAW" > "$W/snap-auto.raw" 2>&1 || true
  sleep 1
  denials "$t_auto" > "$W/snap-auto-denials.txt"
  local t_full; t_full="$(date '+%Y-%m-%d %H:%M:%S')"
  for plug in pipewire alsa hardware-observe kernel-module-observe bluez audio-record; do
    sudo snap connect "$SNAP:$plug" 2>>"$W/snap-connect.err" || echo "connect $plug failed" >> "$W/snap-connect.err"
  done
  snap connections "$SNAP" > "$W/snap-full.connections"
  record snap-full "$SNAP" probe
  record snap-full-cec "$SNAP" probe --cec
  record snap-full-cecdump "$SNAP" cec-dump
  # Raw evidence inside the snap's confinement (same AppArmor profile as the app).
  snap run --shell "$SNAP" -c "$RAW" > "$W/snap-full.raw" 2>&1 || true
  sleep 1
  denials "$t_full" > "$W/snap-denials.txt"
}

cmd_summary() {
  for f in "$W"/*.status; do
    l="$(basename "$f" .status)"
    python3 -c 'import json,sys; s=json.load(open(sys.argv[1])); m=json.load(open(sys.argv[2])) if open(sys.argv[2]).read().startswith("{") else {}; print(json.dumps({**s, **m}))' \
      "$f" "$W/$l.summary"
  done
  for r in "$W"/*.raw; do echo "== $(basename "$r")"; cat "$r"; done
  echo "== wp-clients (while the sandboxed client is connected)"; cat "$W/wp-clients.txt" 2>/dev/null || true
  for f in "$W"/*.server-dump.json; do
    echo "== $(basename "$f")"
    python3 -c '
import json, sys, collections
t = open(sys.argv[1]).read()
try:
    d = json.loads(t)
except Exception:
    print(t[:200]); sys.exit()
objs = d if isinstance(d, list) else d.get("objects", d.get("pw_dump", []))
if isinstance(objs, list):
    print(dict(collections.Counter(o.get("type", "?").split(":")[-1] for o in objs if isinstance(o, dict))))
else:
    print(list(d.keys())[:12])
' "$f"
  done
  for f in "$W"/*cecdump.json; do echo "== $(basename "$f")"; head -c 600 "$f"; cat "${f%.json}.err" 2>/dev/null | tail -2; echo; done
  echo "== snap denials, auto plugs"; cat "$W/snap-auto-denials.txt" 2>/dev/null || true
  echo "== snap denials, every plug"; cat "$W/snap-denials.txt" 2>/dev/null || true
  echo "== fp-python"; head -4 "$W/fp-python.txt" 2>/dev/null; tail -2 "$W/fp-python.err" 2>/dev/null || true
  for e in "$W"/*.err; do [ -s "$e" ] && { echo "== $(basename "$e")"; tail -3 "$e"; }; done; true
}

"cmd_${1:?setup|flatpak|snap|summary}"
