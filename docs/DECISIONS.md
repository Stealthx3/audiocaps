# Decision & action log

User decisions and actions on this project, oldest first. Maintained by Claude per REPO-STRUCTURE-STANDARD;
add entries with `node scripts/repo-check.mjs log <kind> "<text>" --why "<reason>"`.

Entries up to the adoption line were migrated from the former `docs/decision-and-action-log.md`
(newest-first tables) without changing their content. Open items live in `docs/PLAN.md`.

- **2026-11-08** · decision · Sandboxes: Flatpak and Snap compatibility is required
- **2026-11-08** · decision · License: LGPL-2.1-or-later
- **2026-11-08** · decision · Core goals: sink identification, host hardware, software capabilities, end-to-end bit-rate diagnostics
- **2026-11-08** · decision · Platforms: Linux first; Windows and macOS later
- **2026-11-08** · decision · Architecture: Rust core with C ABI; alternative designs dropped
- **2026-11-08** · decision · 1.0 scope adds capture, MIDI, network and virtual sinks, software codec checks; governed by Information on-demand
- **2026-11-08** · decision · CEC is optional, used when the app opts in and the hardware allows
- **2026-11-08** · decision · First consumers: Icefin Player (through mpv), then Argosy Desktop
- **2026-11-08** · decision · Languages: as few as possible; Rust core plus generated C header; apps bundle the library and follow project docs
- **2026-11-08** · decision · Rule and quirk updates bundled with each release, overridable from a system directory or the app's bundle; no network
- **2026-11-08** · decision · Opt-ins: the application decides; the library does what was asked and only that
- **2026-11-08** · decision · Test hardware: onboard HDA, GPU HDMI to TV and receiver, Bluetooth headset; no USB DAC yet
- **2026-11-08** · decision · Development model: Claude builds in libvirt VMs; Micaiah reviews each phase gate
- **2026-11-08** · decision · Name: decide later; "AudioCaps" is the placeholder
- **2026-11-08** · decision · Production HDMI rig: HTPC (AMD RX 9070 XT) → Hisense 55QD5N → Denon AVR-S660H over ARC/eARC
- **2026-11-08** · decision · Plan documents stay out of git — _why:_ review copies live in `docs/plan/`, which is git-ignored
- **2026-11-08** · action · Rebuilt the development plan around the decided design (single-plan document with roadmap and decision log)
- **2026-11-08** · action · Exported the development plan to PDF for review (`docs/plan/AudioCaps-Development-Plan.pdf`, git-ignored)
- **2026-11-08** · action · Prepared the repository scaffold (README, license, notice, changelog, git and editor settings, decision log); `git init` and first commit on `main`
- **2026-11-08** · action · Prepared the hand-over to Claude Code: `CLAUDE.md`, plus git-ignored `docs/plan/local-environment.md` and `docs/plan/claude-code-kickoff.md`
- **2026-11-08** · action · Adopted REPO-STRUCTURE-STANDARD v1, PROGRESS-BAR-STANDARD v1, PHONE-ALERTS-STANDARD v1, VM-LAB-STANDARD v1.2, TOKEN-USAGE-OPTIMIZATION-STANDARD v1 and POST-MORTEM-STANDARD v1 — _why:_ shared ai-assets standards included with the phase 1 kickoff
- **2026-11-08** · decision · Where the repo standard and CLAUDE.md differed, the standard wins: log moves to `docs/DECISIONS.md`, changelog to `docs/CHANGELOG.md`, branches use `<type>/<kebab-case>` (phase 1: `feat/phase-1-scaffold-model-schema`); CLAUDE.md updated to match
- **2026-11-08** · decision · Host the public progress page on GitHub Pages instead of a claude.ai Artifact
- **2026-11-08** · action · Created the public GitHub repository Stealthx3/audiocaps and pushed `main` — _why:_ needed for GitHub Pages; free-plan Pages requires a public repo
- **2026-11-08** · decision · Keep the existing git author identity for this repo
- **2026-11-08** · decision · Use `expect-test` (MIT OR Apache-2.0) for golden snapshots instead of `insta` — _why:_ insta is Apache-2.0 only
- **2026-11-08** · decision · Captures run in VMs until real hardware is needed; then Claude asks for a one-time host exception per hardware test
