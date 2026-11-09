# Decision and action log

Decisions record what was chosen and why it matters; actions record what was
done. Newest entries first. The live development plan is the Claude Docs
document "AudioCaps — Development Plan"; local review copies sit in
`docs/plan/` and are git-ignored.

## Decisions

| Date | Decision | Detail |
| --- | --- | --- |
| 2026-11-08 | Plan documents stay out of git | Review copies live in `docs/plan/`, which is git-ignored |
| 2026-11-08 | Production HDMI rig | HTPC (AMD RX 9070 XT) → Hisense 55QD5N → Denon AVR-S660H over ARC/eARC |
| 2026-11-08 | Name | Decide later; "AudioCaps" is the placeholder |
| 2026-11-08 | Development model | Claude builds in libvirt VMs; Micaiah reviews each phase gate |
| 2026-11-08 | Test hardware | Onboard HDA, GPU HDMI to TV and receiver, Bluetooth headset; no USB DAC yet |
| 2026-11-08 | Opt-ins | The application decides; the library does what was asked and only that |
| 2026-11-08 | Rule and quirk updates | Bundled with each release, overridable from a system directory or the app's bundle; no network |
| 2026-11-08 | Languages | As few as possible: Rust core plus generated C header; apps bundle the library and follow project docs |
| 2026-11-08 | First consumers | Icefin Player (through mpv), then Argosy Desktop |
| 2026-11-08 | CEC | Optional, used when the app opts in and the hardware allows |
| 2026-11-08 | 1.0 scope | Adds capture, MIDI, network and virtual sinks, software codec checks; governed by Information on-demand |
| 2026-11-08 | Architecture | Rust core with C ABI; alternative designs dropped |
| 2026-11-08 | Platforms | Linux first; Windows and macOS later |
| 2026-11-08 | Core goals | Sink identification, host hardware, software capabilities, end-to-end bit-rate diagnostics |
| 2026-11-08 | License | LGPL-2.1-or-later |
| 2026-11-08 | Sandboxes | Flatpak and Snap compatibility is required |

## Actions

| Date | Action | Result |
| --- | --- | --- |
| 2026-11-08 | Prepared the hand-over to Claude Code | `CLAUDE.md` with standing rules; `docs/plan/local-environment.md` and `docs/plan/claude-code-kickoff.md` (git-ignored) |
| 2026-11-08 | Prepared the repository scaffold in the project folder | README, license, notice, changelog, git and editor settings, this log; `git init` and the first commit on branch `main` run locally |
| 2026-11-08 | Exported the development plan to PDF for review | `docs/plan/AudioCaps-Development-Plan.pdf` (git-ignored) |
| 2026-11-08 | Rebuilt the development plan around the decided design | Single-plan document with roadmap and decision log |

## Open items

- [ ] Confirm whether the TV-to-receiver link runs as eARC or plain ARC.
- [ ] Buy a UAC2 USB DAC for testing, or accept fixture-only USB coverage.
- [ ] Final library name, after a collision check.
- [ ] Run the sandbox release gate.
- [ ] Start phase 1: Cargo workspace, JSON schema v1, five machine captures, public progress page.
