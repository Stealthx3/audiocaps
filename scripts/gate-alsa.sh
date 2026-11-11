#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# gate-alsa.sh — phase 3 gate checks against real (emulated) ALSA devices, inside
# lab-audiocaps-capture. Needs the audiocaps CLI installed, alsa-utils, strace and python3.
#
#   hw-params  every PCM the ALSA backend opened: its ranges match
#              `aplay/arecord -D hw:C,D --dump-hw-params` (access, formats, channels, rate,
#              period and buffer time)
#   busy       a PCM that is playing is never opened by a probe, its playback runs to the end
#              with no underruns, and the probe reports it in use with the running parameters
#   race       one process probes in a tight loop (as an application would) while another starts
#              playback on the same free device again and again; counts EBUSY failures. Ranges
#              are cached per process, so only the first probe opens the device.
#   server     with a sound server running (start one first: capture-stack.sh pipewire), probes
#              open no PCM at all, also while the server plays
#   routing    the resolved `default` chains match what alsa-lib builds (`aplay -v` / `arecord -v`
#              on `default`, plugin by plugin, card and device); no server only
#   timing     probe times: full scope and one endpoint
#   midi       MIDI endpoints (load snd-virmidi first) are reported
#
# Usage: bash scripts/gate-alsa.sh <check>...
set -euo pipefail
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0
say() { printf '%s\n' "$*"; }

check_hw_params() {
  audiocaps alsa-dump > "$work/dump.json"
  python3 - "$work/dump.json" <<'PY'
import json, re, subprocess, sys
dump = json.load(open(sys.argv[1]))
if dump.get("managing_server"):
    print(f"hw-params: skipped, {dump['managing_server']} runs (no PCM is opened)"); sys.exit(0)
def parse(text):
    out = {}
    for line in text.splitlines():
        m = re.match(r"^([A-Z_]+):\s*(.*)$", line.strip())
        if m: out[m.group(1)] = m.group(2).strip()
    return out
def rng(v):
    nums = [int(x) for x in re.findall(r"\d+", v)]
    return (nums[0], nums[-1]) if nums else None
bad = checked = 0
for card in dump["cards"]:
    for p in card["pcms"]:
        r = p["result"]
        if r.get("result") != "ranges":
            print(f"  hw:{card['index']},{p['device']} {p['stream']}: not opened ({r.get('reason') or r.get('message')})")
            continue
        tool = "aplay" if p["stream"] == "playback" else "arecord"
        dev = f"hw:{card['index']},{p['device']}"
        res = subprocess.run([tool, "-D", dev, "--dump-hw-params", "-d", "1", "-t", "raw", "/dev/null"],
                             capture_output=True, text=True, timeout=20)
        hw = parse(res.stderr + res.stdout)
        if "FORMAT" not in hw:
            print(f"  {dev} {p['stream']}: {tool} gave no dump: {res.stderr.strip()[:200]}"); bad += 1; continue
        checked += 1
        want = {
            "ACCESS": sorted(hw["ACCESS"].split()),
            "FORMAT": sorted(hw["FORMAT"].split()),
            "CHANNELS": rng(hw["CHANNELS"]),
            "RATE": rng(hw["RATE"]),
            "PERIOD_TIME": rng(hw["PERIOD_TIME"]),
            "BUFFER_TIME": rng(hw["BUFFER_TIME"]),
        }
        got = {
            "ACCESS": sorted(r["access"]),
            "FORMAT": sorted(r["formats"]),
            "CHANNELS": tuple(r["channels"]),
            "RATE": tuple(r["rate"]),
            "PERIOD_TIME": tuple(r["period_us"]),
            "BUFFER_TIME": tuple(r["buffer_us"]),
        }
        diffs = [k for k in want if want[k] != got[k]]
        if diffs:
            bad += 1
            for k in diffs: print(f"  MISMATCH {dev} {p['stream']} {k}: alsa-utils {want[k]} vs audiocaps {got[k]}")
        else:
            print(f"  ok {dev} {p['stream']}: {len(got['FORMAT'])} formats, ch {got['CHANNELS']}, rate {got['RATE']}, rates {r['rates']}")
print(f"hw-params: {checked} PCMs compared, {bad} mismatches")
sys.exit(1 if bad or not checked else 0)
PY
}

first_free_playback() {
  python3 - "$1" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
for c in d["cards"]:
    for p in c["pcms"]:
        if p["stream"] == "playback" and p["result"].get("result") == "ranges" and "S16_LE" in p["result"]["formats"] \
           and p["result"]["channels"][0] <= 2 <= p["result"]["channels"][1] and 48000 in p["result"]["rates"]:
            print(f"{c['index']} {p['device']}"); sys.exit(0)
sys.exit(1)
PY
}

check_busy() {
  audiocaps alsa-dump > "$work/dump.json"
  read -r card dev < <(first_free_playback "$work/dump.json") || { say "busy: no free S16/48k playback PCM"; return 1; }
  say "busy: playing silence on hw:$card,$dev for 30 s while probing"
  aplay -q -D "hw:$card,$dev" -t raw -f S16_LE -r 48000 -c 2 -d 30 /dev/zero 2> "$work/aplay.err" &
  local apid=$!
  sleep 1
  # Probes under strace (slower); all must finish while aplay is still playing.
  strace -f -qq -e trace=openat -o "$work/probe.trace" audiocaps probe --repeat 100 --quiet 2> "$work/times"
  audiocaps probe > "$work/snap.json"
  kill -0 "$apid" 2>/dev/null || { say "busy: playback ended before the probes did; test invalid"; fail=1; }
  audiocaps probe --repeat 100 --quiet 2> "$work/times"
  local status=0
  wait "$apid" || status=$?
  local opened
  opened="$(grep -c "/dev/snd/pcmC${card}D${dev}p" "$work/probe.trace" || true)"
  python3 - "$work/snap.json" "$card" "$dev" "$apid" <<'PY' || fail=1
import json, sys
s = json.load(open(sys.argv[1])); card, dev, pid = sys.argv[2], sys.argv[3], int(sys.argv[4])
e = next(e for e in s["endpoints"] if e["id"] == f"alsa:card{card}/pcm{dev}p")
alsa = next(l for l in e["layers"] if l["layer"] == "alsa_userspace")
d = alsa["detail"]; state = d["state"]["value"]; cur = d["current"]["value"]
print(f"  state {state}, current {cur}")
ok = state == {"state": "in_use", "owner_pid": pid} and cur["rate_hz"] == 48000 and cur["channels"] == 2 and cur["format"] == "s16_le"
ok &= alsa["caps"]["rates"]["reason"] == "busy"
sys.exit(0 if ok else 1)
PY
  local underruns
  underruns="$(grep -ci underrun "$work/aplay.err" || true)"
  say "  aplay exit $status, underruns $underruns, opens of the busy PCM by 100 probes: $opened"
  say "  probe ms while playing: $(awk -F'[: ]+' '{print $3}' "$work/times" | sort -n | awk '{a[NR]=$1} END {print "min " a[1] ", median " a[int((NR+1)/2)] ", max " a[NR]}')"
  if [ "$status" != 0 ] || [ "$underruns" != 0 ] || [ "$opened" != 0 ]; then fail=1; say "busy: FAILED"; else say "busy: ok"; fi
}

check_race() {
  audiocaps alsa-dump > "$work/dump.json"
  read -r card dev < <(first_free_playback "$work/dump.json") || { say "race: no free playback PCM"; return 1; }
  say "race: 100 playback starts on hw:$card,$dev while probes run in a loop"
  audiocaps probe --repeat 100000 --quiet 2>/dev/null &
  local ppid=$!
  local busy=0 ok=0
  for _ in $(seq 1 100); do
    if aplay -q -D "hw:$card,$dev" -t raw -f S16_LE -r 48000 -c 2 -s 4800 /dev/zero 2> "$work/race.err"; then
      ok=$((ok + 1))
    elif grep -qi busy "$work/race.err"; then
      busy=$((busy + 1))
    fi
    kill -0 "$ppid" 2>/dev/null || break
  done
  kill "$ppid" 2>/dev/null || true
  wait "$ppid" 2>/dev/null || true
  say "race: $ok starts succeeded, $busy failed with EBUSY (a probe held the free device at that instant)"
}

check_server() {
  local server=""
  for p in pipewire pulseaudio jackd; do pgrep -x "$p" >/dev/null && { server="$p"; break; }; done
  [ -n "$server" ] || { say "server: no sound server running"; return 1; }
  say "server: $server runs; probing idle, then while it plays"
  PASSIVE_CMD='audiocaps probe --repeat 50 --quiet' bash scripts/check-passive.sh >/dev/null && say "  idle: no PCM opened" || fail=1
  timeout 12 pacat --raw --format=s16le --rate=48000 --channels=2 /dev/zero 2> "$work/pacat.err" &
  local ppid=$!
  sleep 2
  PASSIVE_CMD='audiocaps probe --repeat 50 --quiet' bash scripts/check-passive.sh >/dev/null && say "  playing: no PCM opened" || fail=1
  audiocaps probe > "$work/snap.json"
  wait "$ppid" || true
  python3 - "$work/snap.json" <<'PY' || fail=1
import json, sys
s = json.load(open(sys.argv[1]))
states = {}
for e in s["endpoints"]:
    for l in e["layers"]:
        if l["layer"] == "alsa_userspace" and l.get("detail", {}).get("state", {}).get("value"):
            states[e["id"]] = l["detail"]["state"]["value"]["state"]
reasons = {e["id"]: l["caps"]["rates"]["reason"] for e in s["endpoints"] for l in e["layers"]
           if l["layer"] == "alsa_userspace" and "rates" in l["caps"] and "reason" in l["caps"]["rates"]}
print("  states:", states); print("  rate reasons:", sorted(set(reasons.values())))
ok = any(v == "in_use" for v in states.values()) and set(reasons.values()) <= {"managed_by_server", "busy"}
sys.exit(0 if ok else 1)
PY
}

check_routing() {
  audiocaps probe > "$work/snap.json"
  aplay -v -D default -d 1 -t raw -f S16_LE -r 48000 -c 2 /dev/zero > "$work/play.v" 2>&1 || true
  arecord -v -D default -d 1 -t raw -f S16_LE -r 48000 -c 2 /dev/null > "$work/rec.v" 2>&1 || true
  python3 - "$work/snap.json" "$work/play.v" "$work/rec.v" <<'PY'
import json, re, sys
s = json.load(open(sys.argv[1]))
labels = {"plug": "Plug PCM", "softvol": "Soft volume PCM", "dmix": "Direct Stream Mixing PCM",
          "dsnoop": "Direct Snoop PCM", "hw": "Hardware PCM", "route": "Route conversion PCM",
          "rate": "Rate conversion PCM", "linear": "Linear conversion PCM"}
bad = 0
for key, path in (("default_playback", sys.argv[2]), ("default_capture", sys.argv[3])):
    text = open(path).read()
    real = [m.group(0) for m in re.finditer("|".join(re.escape(v) for v in labels.values()), text)]
    hw = re.search(r"Hardware PCM card (\d+) '[^']*' device (\d+)", text)
    stages = s["routing"][key]["value"]
    ours = [labels[x["kind"]] for x in stages if x["kind"] in labels]
    last = stages[-1]
    # alsa-lib inserts conversion plugins (route, rate, linear) inside plug only as needed.
    real_core = [r for r in real if r not in ("Route conversion PCM", "Rate conversion PCM", "Linear conversion PCM")]
    if hw is None and not real:
        print(f"  {key}: alsa-lib can't open default in this direction (no such PCM); ours {[x['kind'] for x in stages]} -> n/a")
        continue
    ok = ours == real_core and hw is not None and last["kind"] == "hw" and str(last.get("device", 0)) == hw.group(2)
    print(f"  {key}: ours {[x['kind'] for x in stages]} | alsa-lib {real_core} hw {hw.groups() if hw else None} -> {'ok' if ok else 'MISMATCH'}")
    bad += not ok
print(f"routing: {'ok' if not bad else 'FAILED'}")
sys.exit(1 if bad else 0)
PY
}

check_timing() {
  audiocaps probe --repeat 30 --quiet 2> "$work/full"
  local ep
  ep="$(audiocaps probe | python3 -c 'import json,sys; s=json.load(sys.stdin); print(next(e["id"] for e in s["endpoints"] if e["id"].endswith("p")))')"
  audiocaps probe --endpoint "$ep" --repeat 30 --quiet 2> "$work/one"
  for f in full one; do
    say "timing $f: $(awk -F'[: ]+' '{print $3}' "$work/$f" | sort -n | awk '{a[NR]=$1} END {print "min " a[1] " ms, median " a[int((NR+1)/2)] " ms, max " a[NR] " ms over " NR}')"
  done
}

check_midi() {
  audiocaps probe | python3 -c '
import json, sys
s = json.load(sys.stdin)
m = [e for e in s["endpoints"] if e["direction"] == "midi"]
for e in m: print("  ", e["id"], e["class"], e.get("midi"))
print(f"midi: {len(m)} endpoints"); sys.exit(0 if m else 1)' || fail=1
}

for c in "$@"; do
  case "$c" in
    hw-params) check_hw_params || fail=1 ;;
    busy) check_busy || fail=1 ;;
    race) check_race || fail=1 ;;
    server) check_server || fail=1 ;;
    routing) check_routing || fail=1 ;;
    timing) check_timing ;;
    midi) check_midi ;;
    *) echo "unknown check $c" >&2; exit 2 ;;
  esac
done
[ "$fail" = 0 ] && echo "gate-alsa: PASS" || { echo "gate-alsa: FAIL"; exit 1; }
