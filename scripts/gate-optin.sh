#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# gate-optin.sh — phase 8 live checks inside lab-audiocaps-capture (no sound server running):
# opt-ins run only when the app asks, and do only what it asked.
#
#   default  a probe without opt-ins writes no audio, lists no library directory, opens no CEC node
#   codecs   --software-codecs lists library directories (and loads none of the codec libraries)
#   active   --active-probe plays silence on exactly the named device; LPCM accepted
#   refuse   a busy device is refused without being opened; with PipeWire running every device is
#            refused (run last: starts PipeWire)
# Usage: bash scripts/gate-optin.sh <check>...
set -euo pipefail
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
fail=0
LIBDIR="/usr/lib/$(uname -m)-linux-gnu"
trace() { strace -f -qq -e trace=openat,ioctl,write -o "$work/t" "$@" > "$work/out"; }
writes() { grep -c 'SNDRV_PCM_IOCTL_WRITEI_FRAMES' "$work/t" || true; }
# Devices that received audio: map each PID's fds from openat results to /dev/snd paths.
written() {
  python3 - "$work/t" <<'PY'
import re, sys
fds, out = {}, set()
for line in open(sys.argv[1]):
    m = re.match(r'(\d+)\s+openat\(.*"(/dev/snd/[^"]+)".*\)\s+=\s+(\d+)', line)
    if m:
        fds[(m.group(1), m.group(3))] = m.group(2); continue
    m = re.match(r'(\d+)\s+ioctl\((\d+), SNDRV_PCM_IOCTL_WRITEI_FRAMES', line)
    if m and (m.group(1), m.group(2)) in fds:
        out.add(fds[(m.group(1), m.group(2))])
print(" ".join(sorted(out)))
PY
}
dirlists() { grep -E "openat\(.*\"($LIBDIR|/usr/lib64|/usr/lib)(/[a-z0-9.-]+)*\", .*O_DIRECTORY" "$work/t" | grep -vc spa-0.2/support || true; }
codec_loads() { grep -cE '"[^"]*(libavcodec|pcm_a52|libspa-codec-bluez5)[^"]*\.so[^"]*", O_RDONLY\|O_CLOEXEC' "$work/t" || true; }
endpoint() { audiocaps probe | python3 -c "import json,sys; s=json.load(sys.stdin); print(next(e['id'] for e in s['endpoints'] if e['id'].startswith('alsa:') and e['id'].endswith('p')))"; }
check_default() {
  trace audiocaps probe
  local w d c; w="$(writes)"; d="$(dirlists)"; c="$(grep -c '/dev/cec' "$work/t" || true)"
  echo "default: PCM writes $w, library directory listings $d, CEC opens $c"
  [ "$w" = 0 ] && [ "$d" = 0 ] && [ "$c" = 0 ] || fail=1
}
check_codecs() {
  trace audiocaps probe --software-codecs
  local d l; d="$(dirlists)"; l="$(codec_loads)"
  local n; n="$(python3 -c "import json; s=json.load(open('$work/out')); print([p['name'] for p in s['software_codecs']['value']['providers']])")"
  echo "codecs: library directory listings $d, codec libraries loaded $l, providers $n"
  [ "$d" -gt 0 ] && [ "$l" = 0 ] || fail=1
}
check_active() {
  local ep hw; ep="$(endpoint)"; hw="$(echo "$ep" | sed -E 's/alsa:card([0-9]+)\/pcm([0-9]+)p/pcmC\1D\2p/')"
  trace audiocaps probe --active-probe "$ep" --active-codecs lpcm,ac3
  local w dev others; w="$(writes)"; dev="$(written)"
  others="$(echo "$dev" | tr ' ' '\n' | grep -v "^/dev/snd/$hw$" | grep . || true)"
  python3 - "$work/out" "$ep" <<'PY' || fail=1
import json, sys
s = json.load(open(sys.argv[1])); ep = sys.argv[2]
e = next(e for e in s["endpoints"] if e["id"] == ep)
if "value" not in e["active_probe"]:
    print("  not run:", e["active_probe"]); sys.exit(1)
r = e["active_probe"]["value"]["results"]
print("  results:", [(x["codec"], x["outcome"]["outcome"], x["outcome"].get("reason") or x["outcome"].get("error", "")) for x in r])
others = [x["id"] for x in s["endpoints"] if x["id"] != ep and x.get("active_probe")]
ok = r[0]["outcome"]["outcome"] == "accepted" and r[1]["outcome"]["outcome"] == "skipped" and not others
sys.exit(0 if ok else 1)
PY
  echo "active on $ep: PCM writes $w to ${dev:-nothing}; other devices written: ${others:-none}"
  [ "$w" -gt 0 ] && [ -z "$others" ] || fail=1
}
check_refuse() {
  local ep hw; ep="$(endpoint)"; hw="$(echo "$ep" | sed -E 's/alsa:card([0-9]+)\/pcm([0-9]+)p/hw:\1,\2/')"
  aplay -q -D "$hw" -t raw -f S16_LE -r 48000 -c 2 -d 5 /dev/zero 2>/dev/null & local pid=$!; sleep 1
  trace audiocaps probe --active-probe "$ep" --active-codecs lpcm
  wait "$pid"; local st=$?
  local busy; busy="$(python3 -c "import json; s=json.load(open('$work/out')); e=[e for e in s['endpoints'] if e['id']=='$ep'][0]; print(e['active_probe'].get('reason'))")"
  echo "refuse (busy): reason $busy, PCM writes $(writes), aplay exit $st"
  [ "$busy" = busy ] && [ "$(writes)" = 0 ] && [ "$st" = 0 ] || fail=1
  bash scripts/capture-stack.sh pipewire gate-optin >/dev/null 2>&1 || true
  trace audiocaps probe --active-probe "$ep" --active-codecs lpcm
  local srv; srv="$(python3 -c "import json; s=json.load(open('$work/out')); e=[e for e in s['endpoints'] if e['id']=='$ep'][0]; print(e['active_probe'].get('reason'))")"
  echo "refuse (PipeWire): reason $srv, PCM writes $(writes)"
  [ "$srv" = managed_by_server ] && [ "$(writes)" = 0 ] || fail=1
}
for c in "$@"; do
  case "$c" in default) check_default ;; codecs) check_codecs ;; active) check_active ;; refuse) check_refuse ;;
    *) echo "unknown check $c" >&2; exit 2 ;; esac
done
[ "$fail" = 0 ] && echo "gate-optin: PASS" || { echo "gate-optin: FAIL"; exit 1; }
