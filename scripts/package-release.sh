#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Packages a release tarball from the portable build (scripts/build-portable.sh), then checks it
# from a clean directory: the C example builds against the shipped header and library and prints
# the same output as the in-tree build on a fixture; the CLI reports the right version.
# Output: target/dist/audiocaps-<version>-linux-x86_64.tar.gz and its .sha256.
# Run in the build VM: scripts/lab.sh run 'bash scripts/build-portable.sh && bash scripts/package-release.sh'
set -euo pipefail
cd "$(dirname "$0")/.."
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
name="audiocaps-$version-linux-x86_64"
P=target/portable
D="target/dist/$name"
[ -x "$P/audiocaps" ] && [ -f "$P/libaudiocaps.so.1" ] || { echo "run scripts/build-portable.sh first" >&2; exit 1; }
rm -rf target/dist; mkdir -p "$D"/{bin,lib,include,share/audiocaps,share/doc/audiocaps}

install -m755 "$P/audiocaps" "$D/bin/"
install -m755 "$P/libaudiocaps.so.1" "$D/lib/"
ln -s libaudiocaps.so.1 "$D/lib/libaudiocaps.so"
install -m644 include/audiocaps.h "$D/include/"
cp -r schema "$D/share/audiocaps/"
# The bundled rule and quirk packs are compiled in; the files are shipped as references for
# writing override packs (rules.d / quirks.d).
mkdir -p "$D/share/audiocaps/packs"
cp crates/audiocaps-rules/packs/rules-core.json crates/audiocaps-rules/packs/quirks-core.json "$D/share/audiocaps/packs/"
cp LICENSE NOTICE README.md docs/CHANGELOG.md docs/consumers.md docs/rules.md "$D/share/doc/audiocaps/"
cp -r examples packaging "$D/share/doc/audiocaps/"
strip --strip-unneeded "$D/bin/audiocaps" "$D/lib/libaudiocaps.so.1"

tar -C target/dist --owner=0 --group=0 --sort=name --mode="u+rwX,go+rX,go-w" -czf "target/dist/$name.tar.gz" "$name"
(cd target/dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

# Check the tarball from scratch.
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
tar -C "$T" -xzf "target/dist/$name.tar.gz"
R="$T/$name"
fail=0
v="$("$R/bin/audiocaps" --version)"
[[ "$v" == "audiocaps $version "* ]] && echo "cli version: $v" || { echo "cli version FAIL: $v"; fail=1; }
soname="$(readelf -d "$R/lib/libaudiocaps.so.1" | sed -n 's/.*SONAME.*\[\(.*\)\]/\1/p')"
[ "$soname" = libaudiocaps.so.1 ] && echo "soname: ok" || { echo "soname FAIL: $soname"; fail=1; }
cc -std=c11 -Wall -Werror -I"$R/include" examples/c/probe.c -L"$R/lib" -laudiocaps -o "$T/probe"
LD_LIBRARY_PATH="$R/lib" "$T/probe" tests/fixtures/htpc > "$T/shipped.txt"
if [ -f target/examples-gate/htpc.c.txt ]; then
  cmp -s "$T/shipped.txt" target/examples-gate/htpc.c.txt \
    && echo "C example vs in-tree build: identical" || { echo "C example FAIL: output differs"; fail=1; }
fi
lic="$(find "$R" -type f | wc -l)"
echo "files: $lic; size: $(du -h "target/dist/$name.tar.gz" | cut -f1); $(cat "target/dist/$name.tar.gz.sha256")"
[ "$fail" = 0 ] && echo "package: PASS" || { echo "package: FAIL"; exit 1; }
