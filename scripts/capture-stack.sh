#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# capture-stack.sh — inside lab-audiocaps-capture: install one sound stack, start it for the lab
# user, then run the passive check and a capture. Start from a revert to the 'tooling' snapshot.
# Usage: scripts/lab.sh run -r capture 'bash scripts/capture-stack.sh <alsa-only|pipewire|pulseaudio>'
set -euo pipefail
stack="${1:?stack}"
export DEBIAN_FRONTEND=noninteractive
case "$stack" in
  alsa-only) ;;
  pipewire)
    sudo apt-get install -y -qq pipewire pipewire-pulse pipewire-alsa wireplumber pulseaudio-utils >/dev/null
    systemctl --user daemon-reload
    systemctl --user start pipewire.socket pipewire-pulse.socket wireplumber.service
    sleep 3 ;;
  pulseaudio)
    sudo apt-get install -y -qq pulseaudio pulseaudio-utils >/dev/null
    systemctl --user daemon-reload
    systemctl --user start pulseaudio.socket pulseaudio.service
    sleep 3 ;;
  *) echo "unknown stack $stack" >&2; exit 2 ;;
esac
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
bash scripts/check-passive.sh "lab-$stack"
