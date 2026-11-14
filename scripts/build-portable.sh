#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Release build for packaging: builds the CLI and libaudiocaps.so.1 in an Ubuntu 24.04 container
# (glibc 2.39), so the binaries run in the freedesktop 24.08 Flatpak runtime and in core24 snaps.
# Also collects the Ubuntu libraries a core24 snap must stage. Output: target/portable/.
# Run in the build VM: scripts/lab.sh run 'bash scripts/build-portable.sh'
set -euo pipefail
cd "$(dirname "$0")/.."
img=docker.io/library/ubuntu:24.04
toolchain="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
mkdir -p target/portable "$HOME/.cache/audiocaps-portable/cargo" "$HOME/.cache/audiocaps-portable/rustup"
podman run --rm --userns=keep-id -u 0 \
  -v "$PWD:/src:Z" -v "$HOME/.cache/audiocaps-portable/cargo:/cargo:Z" \
  -v "$HOME/.cache/audiocaps-portable/rustup:/rustup:Z" \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/src/target/portable/build \
  -e TOOLCHAIN="$toolchain" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  "$img" bash -euo pipefail -c '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends ca-certificates curl build-essential pkg-config \
      libasound2-dev libpipewire-0.3-dev libpulse-dev libjack-jackd2-dev libclang-dev clang \
      libpipewire-0.3-modules libspa-0.2-modules libasound2-data >/dev/null
    if [ ! -x /cargo/bin/cargo ]; then
      curl -sSf https://sh.rustup.rs | sh -s -- -y -q --no-modify-path --profile minimal --default-toolchain "$TOOLCHAIN"
    fi
    export PATH=/cargo/bin:$PATH
    cd /src
    cargo build -q --release --locked -p audiocaps-cli -p audiocaps-ffi
    out=/src/target/portable
    cp build/release/audiocaps "$out/" 2>/dev/null || cp "$CARGO_TARGET_DIR/release/audiocaps" "$out/"
    cp "$CARGO_TARGET_DIR/release/libaudiocaps.so" "$out/libaudiocaps.so.1"
    ldd "$out/audiocaps" "$out/libaudiocaps.so.1" | awk "/=> \\//{print \$3}" | sort -u > "$out/needed-libs.txt"
    objdump -T "$out/audiocaps" "$out/libaudiocaps.so.1" | grep -o "GLIBC_[0-9.]*" | sort -uV | tail -1 > "$out/glibc.txt"
    rm -rf "$out/ubuntu-libs"; mkdir -p "$out/ubuntu-libs"
    xargs -a "$out/needed-libs.txt" cp -L -t "$out/ubuntu-libs/"
    # Runtime data a core24 snap lacks: PipeWire client modules, SPA support plugins and client
    # config; alsa-lib config. (The freedesktop Flatpak runtime ships all of these.)
    rm -rf "$out/snap-data"; d="$out/snap-data"
    mkdir -p "$d/usr/lib/x86_64-linux-gnu/spa-0.2" "$d/usr/share/pipewire" "$d/usr/share"
    cp -a /usr/lib/x86_64-linux-gnu/pipewire-0.3 "$d/usr/lib/x86_64-linux-gnu/"
    cp -a /usr/lib/x86_64-linux-gnu/spa-0.2/support "$d/usr/lib/x86_64-linux-gnu/spa-0.2/"
    # client.conf ships in pipewire-bin (with the tools); take only the file.
    (cd /tmp && apt-get download -qq pipewire-bin && dpkg-deb -x pipewire-bin_*.deb pw)
    cp -a /tmp/pw/usr/share/pipewire/client.conf "$d/usr/share/pipewire/"
    cp -a /usr/share/alsa "$d/usr/share/"
    chown -R "$HOST_UID:$HOST_GID" "$out"
  '
echo "portable: $(ls target/portable) ; needs $(cat target/portable/glibc.txt)"
