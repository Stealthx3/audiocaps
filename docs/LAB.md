# Lab — audiocaps
_Standard: VM-LAB-STANDARD v1.2 · Created 2026-11-08_

## Host (from lab.sh doctor)
- virt-install 5.1.0, virt-manager: yes, KVM: yes, LXC driver: yes
- Network: default (NAT) · Pool: default (/var/lib/libvirt/images)
- Host: 16 CPUs, 125 GB RAM

## Guest decision (§2)
| Role | Kind | Rule | "Yes" answers | Note |
|---|---|---|---|---|
| main | vm | 2 | Q7, Q8 | builds and tests; reverts keep the toolchain clean |
| capture | vm | 2 | Q2, Q5, Q7, Q8 | runs real sound-server stacks (systemd user units) with an emulated HDA card; one snapshot per stack |

## Guests (watch in virt-manager → QEMU/KVM)
| Guest | Kind | Base | vCPU/RAM | Purpose | Snapshots |
|---|---|---|---|---|---|
| lab-audiocaps-main | vm | lab-base-debian13 | 4 / 6 GB | Rust build, tests, cargo-deny | clean, provisioned |
| lab-audiocaps-capture | vm | lab-base-debian13 | 4 / 6 GB | capture fixtures: PipeWire, PulseAudio, ALSA-only; 2 emulated ich9 HDA cards, standard (non-cloud) kernel; snapshot `usb-audio` adds an emulated USB Audio (UAC1) device and the phase 2 CLI | audio-base, tooling, usb-audio |

Other projects' lab guests may be running; never run more than 3 guests at once. Shut `main` down
while `capture` runs if needed.

## Provisioning
- scripts/provision.sh: Rust toolchain (rustup, pinned by rust-toolchain.toml), build deps (pkg-config, clang, libasound2-dev, libpipewire-0.3-dev, libpulse-dev, libdbus-1-dev), cargo-deny.

## Fuzzing (phase 2)
- `fuzz/` is its own Cargo workspace on a pinned nightly (`fuzz/rust-toolchain.toml`), built with cargo-fuzz in `lab-audiocaps-main` only.
- `scripts/lab.sh run 'bash scripts/fuzz.sh seed'` copies fixture files into `fuzz/corpus/`; `... fuzz.sh start <seconds>` runs every target detached (survives the SSH session); `... fuzz.sh summary` prints status, executions and crash count (exit 1 on any crash); `... fuzz.sh stop`.
- Corpora, logs and artifacts are git-ignored, and `lab.sh push` leaves them in place.

## Capture workflow
- `scripts/lab.sh revert tooling capture`, then `scripts/lab.sh run -r capture 'bash scripts/capture-stack.sh <alsa-only|pipewire|pulseaudio>'` (runs `check-passive.sh`), then `scripts/lab.sh pull '/home/lab/captures/*' captures capture` before the next revert.

## Commands
- Build: `scripts/lab.sh run 'cargo build --workspace'`
- Test:  `scripts/lab.sh run 'bash scripts/gate.sh 2>&1 | tail -n 50'`
- Watch: `scripts/lab.sh view`

## Host exceptions
- Real-hardware tests need a one-time OK from Micaiah per test (decision 2026-11-08).
- 2026-11-08 — workstation capture: static `dist/audiocaps` run once on the host under `strace` via `scripts/check-passive.sh` (approved by Micaiah; passed, 1672 opens, none in /dev/snd).
- 2026-11-08 — HTPC capture: run once over SSH from the host, binary in /tmp and removed afterwards (approved by Micaiah; no strace installed there, so passivity rests on the code path and the VM/workstation traces).

## Log
- 2026-11-08 — doctor OK; Debian 13 base present.
- 2026-11-08 — lab-audiocaps-main created and provisioned (Rust 1.99.1, cargo-deny 0.20.2).
- 2026-11-08 — lab-audiocaps-capture: cloud kernel has no sound drivers; installed linux-image-amd64 and set GRUB_DEFAULT to it. Added ich9 HDA with virt-xml (VM came with one HDA already → two cards). Old `clean` snapshot unusable after the device change (seed ISO gone); kept `audio-base` and `tooling` instead.
- 2026-11-08 — Lost one capture by reverting before pulling; workflow now pulls first.
- 2026-11-09 — main: nightly-2026-11-01 and cargo-fuzz 0.13.2 added (provision.sh).
- 2026-11-09 — capture: added an emulated USB Audio device (`<sound model='usb'/>`, QEMU usb-audio, UAC1) for the `lab-usb-uac1` fixture. Saved as snapshot `usb-audio` (shut off). The `tooling` snapshot predates it, lacks the device, and has an interrupted dpkg: run `apt-get --fix-broken install` before provisioning from it.
- 2026-11-09 — 24 h fuzz run for the phase 2 gate started 08:38 UTC on main; keep main running until it ends.
