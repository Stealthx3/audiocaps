#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# lab-sync-back.sh — copy files generated in the build guest (lockfile, schema, golden snapshots,
# fixtures, and `crates` after `cargo fmt` in the guest) back into the working tree. Only these
# paths are allowed. Push first (lab.sh run does), and don't edit on the host in between.
# Usage: scripts/lab-sync-back.sh Cargo.lock schema tests/fixtures
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/lab.env"
for p in "$@"; do
  case "$p" in
    Cargo.lock|schema|schema/*|tests/fixtures|tests/fixtures/*|crates) ;;
    *) echo "sync-back: refusing $p (not a generated path)" >&2; exit 2 ;;
  esac
  ip="$("$ROOT/scripts/lab.sh" ip main)"
  if [ "$p" = Cargo.lock ] || [[ "$p" == *.* && ! -d "$ROOT/$p" ]]; then
    rsync -a -e "ssh -i ${LAB_SSH_KEY%.pub} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR" \
      "$LAB_USER@$ip:$LAB_DEST/$p" "$ROOT/$p"
  else
    mkdir -p "$ROOT/$p"
    rsync -a -e "ssh -i ${LAB_SSH_KEY%.pub} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR" \
      "$LAB_USER@$ip:$LAB_DEST/$p/" "$ROOT/$p/"
  fi
  echo "sync-back: $p"
done
