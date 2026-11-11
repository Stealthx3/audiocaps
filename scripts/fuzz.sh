#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# fuzz.sh — seed, run and summarise the cargo-fuzz targets. Runs inside the build guest:
#   scripts/lab.sh run 'bash scripts/fuzz.sh build'
#   scripts/lab.sh run 'bash scripts/fuzz.sh seed'
#   scripts/lab.sh run 'bash scripts/fuzz.sh start 86400'   # detached; all targets in parallel
#   scripts/lab.sh run 'bash scripts/fuzz.sh summary'
# Corpora, logs and crash artifacts stay under fuzz/ (git-ignored).
set -uo pipefail
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
TARGETS=(eld edid hda_codec usb_descriptors procfs alsa_conf)
F=fuzz

seed_files() { # target dir pattern...
  local t="$1"; shift
  mkdir -p "$F/corpus/$t"
  find tests/fixtures -type f \( "$@" \) -size +0 -print0 |
    while IFS= read -r -d '' p; do cp "$p" "$F/corpus/$t/seed-$(sha256sum "$p" | cut -c1-16)"; done
}

case "${1:-}" in
  build)
    (cd "$F" && cargo fuzz build -O 2>&1 | tail -n 20) && echo "fuzz build: ok"
    ;;
  seed)
    seed_files eld -name 'eld#*' -o -name edid
    seed_files edid -name edid
    seed_files hda_codec -name 'codec#*'
    seed_files usb_descriptors -name descriptors
    seed_files alsa_conf -name 'alsa-config*' -o -name 'asound.conf' -o -name '*.conf' -path '*alsa*'
    mkdir -p "$F/corpus/procfs"
    i=0
    # Order matches the selector in fuzz_targets/procfs.rs.
    for pat in cards pcm 'stream[0-9]*' hw_params info status 'midi[0-9]*' clients os-release; do
      find tests/fixtures -type f -name "$pat" -size +0 -print0 |
        while IFS= read -r -d '' p; do
          { printf "\\x$(printf %02x "$i")"; cat "$p"; } > "$F/corpus/procfs/seed-$i-$(sha256sum "$p" | cut -c1-16)"
        done
      i=$((i + 1))
    done
    for t in "${TARGETS[@]}"; do echo "$t: $(ls "$F/corpus/$t" 2>/dev/null | wc -l) seeds"; done
    ;;
  start)
    secs="${2:?seconds}"
    mkdir -p "$F/logs"
    date -u +%FT%TZ > "$F/logs/started"
    echo "$secs" > "$F/logs/duration"
    for t in "${TARGETS[@]}"; do
      mkdir -p "$F/corpus/$t" "$F/artifacts/$t"
      # Redirect the whole subshell (not just cargo) so nothing holds the caller's stdout open,
      # and setsid so the run survives the SSH session that started it.
      (cd "$F" && exec setsid nohup cargo fuzz run -O "$t" "corpus/$t" -- \
        -max_total_time="$secs" -timeout=10 -rss_limit_mb=2048 -print_final_stats=1 \
        -artifact_prefix="artifacts/$t/") < /dev/null > "$F/logs/$t.log" 2>&1 &
    done
    echo "started ${#TARGETS[@]} targets for ${secs}s"
    ;;
  summary)
    [ -f "$F/logs/started" ] && echo "started $(cat "$F/logs/started"), duration $(cat "$F/logs/duration")s"
    fail=0
    for t in "${TARGETS[@]}"; do
      log="$F/logs/$t.log"
      running=$(pgrep -f "release/$t " >/dev/null && echo running || echo stopped)
      last=$(grep -E '^#[0-9]+' "$log" 2>/dev/null | tail -n 1 | awk '{print $1, $3, $4, $(NF-3), $(NF-2), $(NF-1), $NF}')
      execs=$(grep -E 'stat::number_of_executed_units' "$log" 2>/dev/null | awk '{print $2}')
      crashes=$(find "$F/artifacts/$t" -type f 2>/dev/null | wc -l)
      [ "$crashes" -gt 0 ] && fail=1
      grep -qE 'ERROR: libFuzzer|panicked at' "$log" 2>/dev/null && fail=1
      echo "$t: $running, crashes=$crashes, ${execs:+execs=$execs, }last: ${last:-none}"
    done
    [ $fail -eq 0 ] && echo "fuzz: CLEAN" || echo "fuzz: FAILURES (see fuzz/artifacts, fuzz/logs)"
    exit $fail
    ;;
  stop)
    pkill -f 'cargo-fuzz fuzz run'; pkill -f "release/($(IFS='|'; echo "${TARGETS[*]}")) " && echo stopped || echo "nothing running"
    ;;
  *)
    sed -n '3,7p' "$0"; exit 2 ;;
esac
