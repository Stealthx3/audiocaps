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
| lab-audiocaps-capture | vm | lab-base-debian13 | 2 / 4 GB | capture fixtures: PipeWire, PulseAudio, ALSA-only | clean, + one per stack |

Other projects' lab guests may be running; never run more than 3 guests at once. Shut `main` down
while `capture` runs if needed.

## Provisioning
- scripts/provision.sh: Rust toolchain (rustup, pinned by rust-toolchain.toml), build deps (pkg-config, clang, libasound2-dev, libpipewire-0.3-dev, libpulse-dev, libdbus-1-dev), cargo-deny.

## Commands
- Build: `scripts/lab.sh run 'cargo build --workspace'`
- Test:  `scripts/lab.sh run 'bash scripts/gate.sh 2>&1 | tail -n 50'`
- Watch: `scripts/lab.sh view`

## Host exceptions
- none. Real-hardware tests (workstation capture, `/dev/snd` passthrough) need a one-time OK from Micaiah per test (decision 2026-11-08).

## Log
- 2026-11-08 — doctor OK; Debian 13 base present.
