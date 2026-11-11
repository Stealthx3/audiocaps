#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# provision-capture.sh — idempotent setup of lab-audiocaps-capture: build tools, then build and
# install the audiocaps CLI. Runs inside the guest: scripts/lab.sh run -r capture 'bash scripts/provision-capture.sh'
# Sound-server stacks are installed per snapshot by scripts/capture-stack.sh, not here.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y -qq build-essential pkg-config curl strace alsa-utils dbus-user-session libasound2-dev libpipewire-0.3-dev libpulse-dev clang python3 >/dev/null
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
fi
. "$HOME/.cargo/env"
cargo build --release -p audiocaps-cli -q
sudo install -m 0755 target/release/audiocaps /usr/local/bin/audiocaps
audiocaps 2>&1 | head -1 || true
echo "cards: $(cat /proc/asound/cards 2>/dev/null | head -1)"
