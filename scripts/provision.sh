#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# provision.sh — idempotent setup of the build guest (lab-audiocaps-main). Runs inside the guest only.
# Usage: scripts/lab.sh run 'bash scripts/provision.sh'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y -qq build-essential pkg-config clang libclang-dev git curl strace \
  libasound2-dev libpipewire-0.3-dev libspa-0.2-dev libpulse-dev libjack-jackd2-dev libdbus-1-dev >/dev/null
if ! command -v rustup >/dev/null && [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
fi
. "$HOME/.cargo/env"
# rust-toolchain.toml in the repo pins the version and components; this installs it.
rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install stable --profile minimal -c rustfmt,clippy
command -v cargo-deny >/dev/null || cargo install --locked cargo-deny --quiet
# Fuzzing (phase 2): fuzz/rust-toolchain.toml pins a nightly; cargo-fuzz drives libFuzzer.
(cd fuzz 2>/dev/null && rustup show active-toolchain >/dev/null 2>&1) || true
command -v cargo-fuzz >/dev/null || cargo install --locked cargo-fuzz --quiet
grep -q 'cargo/env' "$HOME/.bashrc" || echo '. "$HOME/.cargo/env"' >> "$HOME/.bashrc"
rustc --version; cargo deny --version
