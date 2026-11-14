#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Regenerates include/audiocaps.h with cbindgen (a build tool, MPL-2.0; not linked).
# `--check` fails when the committed header is out of date.
set -euo pipefail
cd "$(dirname "$0")/.."
. ~/.cargo/env 2>/dev/null || true
command -v cbindgen >/dev/null || cargo install cbindgen --locked --version 0.29.2 >/dev/null
out=include/audiocaps.h
if [ "${1:-}" = --check ]; then
  tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
  cbindgen --quiet --config crates/audiocaps-ffi/cbindgen.toml --crate audiocaps-ffi --output "$tmp" crates/audiocaps-ffi
  diff -u "$out" "$tmp" && echo "header: current"
else
  cbindgen --quiet --config crates/audiocaps-ffi/cbindgen.toml --crate audiocaps-ffi --output "$out" crates/audiocaps-ffi
  echo "header: $out"
fi
