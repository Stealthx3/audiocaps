<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# Release 1.0.0 (Linux)

Status: **released** 2026-11-14 as tag `v1.0.0` on `main`, with the tarball and checksum on the
GitHub release.

## What ships

`audiocaps-1.0.0-linux-x86_64.tar.gz` (built by `scripts/build-portable.sh` and
`scripts/package-release.sh`), with a `.sha256` file:

| Path | Contents |
| --- | --- |
| `lib/libaudiocaps.so.1` | The library (soname `libaudiocaps.so.1`, C ABI 1, only `ac_*` exported); `libaudiocaps.so` links to it |
| `include/audiocaps.h` | Generated C header |
| `bin/audiocaps` | The command-line tool: capture, probe, watch, replay, schema |
| `share/audiocaps/schema/` | Snapshot JSON schema v1 (1.7.0) |
| `share/audiocaps/packs/` | The bundled rule and quirk packs (compiled in; shipped as references for override packs) |
| `share/doc/audiocaps/` | License, notice, README, changelog, consumer guide, rules reference, examples, Flatpak/Snap templates |

The binaries are built in an Ubuntu 24.04 container. They need glibc 2.39 or later and run on
the freedesktop 24.08 Flatpak runtime and in core24 snaps. They link libasound, libpipewire-0.3
and libpulse dynamically; libjack is loaded lazily when present.

## Release gate

| Check | Status |
| --- | --- |
| Phases 1–9 met their gates (`docs/PLAN.md`, 46/46) | done |
| `scripts/gate.sh`: fmt, clippy `-D warnings`, tests, cargo-deny (also for fuzz), fuzz targets build, SPDX | pass |
| `scripts/gate-examples.sh`: C, C++, Python, C# give identical output; header current; soname; exports; valgrind | pass |
| `scripts/gate-sandbox.sh`: Flatpak (Debian 13 guest) and strict Snap (Ubuntu 24.04 guest); results in `packaging/README.md` | pass |
| `scripts/package-release.sh`: tarball builds; the C example built against the shipped files matches the in-tree build | pass |
| Secret and address scan of the history (`scripts/scan-history.sh`) | clean |
| Four core goals through one API (identification, host hardware, software layers, path diagnostics) | met in fixtures and the lab; on real hardware through the HTPC capture (fixture `htpc`) and the phase 6 HTPC run |

## Before tagging (decisions for Micaiah)

1. **Final library name: settled.** Micaiah confirmed "AudioCaps" on 2026-11-14. The soname
   `libaudiocaps.so.1`, the header, the `ac_` prefix and the CLI keep their names.
2. **Sandbox gate on the HTPC.** The plan names the HTPC as the rig that runs the sandbox release
   gate on real hardware. The gate ran in lab guests with emulated HDA and CEC. Running the
   Flatpak and the snap on the HTPC needs a one-time exception for each test.
3. **Remaining open items.** These are hardware questions, not release blockers unless you say
   so: whether the TV-to-receiver link runs as eARC or ARC, and a UAC2 USB DAC (USB coverage is
   UAC1 in the lab plus fixtures).
4. **Publishing.** Tag `v1.0.0` on `main`, push the tag, and create a GitHub release with the
   tarball and checksum. Each of these is outward-facing and waits for a go-ahead.

## Steps to cut the release

1. Merge `chore/release-1.0.0` into `main`.
2. In the build VM: `scripts/lab.sh run 'bash scripts/gate.sh && bash scripts/gate-examples.sh && bash scripts/build-portable.sh && bash scripts/package-release.sh'`.
3. Set the date on the `[1.0.0]` heading in `docs/CHANGELOG.md`, and log the release in
   `docs/DECISIONS.md`.
4. `git tag -s v1.0.0` (or `-a`), then push `main` and the tag.
5. Create the GitHub release from the `[1.0.0]` changelog section, and attach the tarball and
   `.sha256`.

## After 1.0

- ABI: additions only, under soname `.so.1`. `AC_ABI_VERSION` and the soname change only on a
  break.
- Schema: additive within v1 (`schema_version` minor bumps).
- Rule and quirk packs: they update with each release, and system or app directories override
  them. They are never fetched from the network.
- Windows and macOS backends on the same core, estimated at the 1.0 review.
