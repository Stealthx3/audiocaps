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
- **2026-11-08** · decision · Toolchain pinned to Rust 1.99.1, edition 2024; builds, tests and cargo fmt run only in lab-audiocaps-main (no Rust toolchain on the host); formatted sources and generated files come back via scripts/lab-sync-back.sh — _why:_ VM Lab Standard: no project builds on the host _(by claude)_
- **2026-11-08** · decision · Dependencies for phase 1: serde, serde_json, schemars 1, sha2, regex, tar, flate2, expect-test (all MIT or MIT OR Apache-2.0); no argument-parser crate — _why:_ keep the dependency and license surface small _(by claude)_
- **2026-11-08** · decision · Model: every value is Value<T> = known (value, measured/inferred confidence, provenance) or unknown (reason, detail); fields outside the probe scope are omitted, never reported as unknown — _why:_ Information on-demand; unknown is never 'unsupported' _(by claude)_
- **2026-11-08** · decision · ProbeScope selects endpoint classes, directions, endpoints, layers, field groups and access paths; ProbeScope::nothing() is the starting point and builders add only what the app asks for — _why:_ scope is part of the API shape from day one _(by claude)_
- **2026-11-08** · decision · Schema v1 is generated by schemars into schema/audiocaps-snapshot-v1.schema.json and checked by a test; v1 changes are additive only — _why:_ published, versioned export format _(by claude)_
- **2026-11-08** · decision · Redaction defaults: serials and Bluetooth addresses hashed (sha256 prefix, stable), user-chosen device names masked; product and driver names stay in the clear — _why:_ keeps sink keys and correlation stable while hiding identifiers _(by claude)_
- **2026-11-08** · decision · SystemRoot refuses every path under /dev and any path escaping a fixture tree; the library never runs commands, only captures do — _why:_ passive by default; safe fixture replay _(by claude)_
- **2026-11-08** · decision · Capture tool runs pw-dump always, pactl only when a PulseAudio socket exists (autospawn disabled), BlueZ only when org.bluez already owns its name; outputs are redacted before they touch disk — _why:_ never start or activate a service _(by claude)_
- **2026-11-08** · decision · Verbatim copies of shared ai-assets scripts (lab.sh, progress.mjs, repo-check.mjs, phone-alerts.mjs) are exempt from the SPDX header rule — _why:_ the standards forbid editing them per project _(by claude)_
- **2026-11-08** · decision · Phase 1 probe reads only /proc/asound/cards and /pcm plus os-release, kernel and captured server info; all other layers report unknown/not_implemented with the phase that adds them — _why:_ no phase 2+ parsers in phase 1 _(by claude)_
- **2026-11-08** · decision · Capture VM lab-audiocaps-capture uses two emulated Intel HDA cards and the standard Debian kernel (the cloud kernel has no sound drivers); one VM with snapshot reverts per stack (ALSA-only, PipeWire 1.4.2, PulseAudio 17.0) — _why:_ three server stacks without three guests _(by claude)_
- **2026-11-08** · decision · Host and HTPC captures use a static musl build of audiocaps so it runs on any x86_64 distro — _why:_ no build tools or Rust needed on the machine being captured _(by claude)_
- **2026-11-08** · decision · Unit tests use RFC 5737/7042 documentation addresses; scripts/scan-history.sh checks all history for plan docs, captures, IPs, MACs and keys, with a reasoned allowlist for synthetic test values — _why:_ CLAUDE.md never-commit rule _(by claude)_
- **2026-11-08** · approval · One-time host exception: run the static audiocaps capture on the workstation under strace (done; no /dev/snd opens)
- **2026-11-08** · approval · One-time exception: Claude ran the capture on the HTPC over SSH with the existing key; binary in /tmp, removed afterwards (no strace on the HTPC)
- **2026-11-08** · decision · Redaction now covers JSON object keys and addresses joined by underscores (BlueZ dev_… paths); audiocaps redact-fixture re-applies rules to existing fixtures; leak scan uses hex/colon boundaries instead of word boundaries — _why:_ a pre-commit review found real Bluetooth addresses in BlueZ object paths; caught before commit or push _(by claude)_
- **2026-11-08** · approval · Approved phase 1 at its gate and the merge of feat/phase-1-scaffold-model-schema into main
- **2026-11-09** · decision · Fuzz engine: cargo-fuzz (libFuzzer); NCSA allowed only in the separate, unshipped fuzz/ workspace; nightly toolchain only in the lab VM — _why:_ coverage-guided fuzzing for the 24 h phase 2 gate; harness is never linked into the library
- **2026-11-09** · decision · Parsers depend on audiocaps-model so SAD decoding maps straight to CodecCapability; CTA format 12 (MAT/MLP) is reported as TrueHD with NeedsHbr, plus Atmos when byte 3 bit 0 is set — _why:_ one place owns the CTA-861 codec mapping _(by claude)_
- **2026-11-09** · decision · Capture tool saves the USB device's raw sysfs descriptors (string indexes only, no serial strings); new fixture lab-usb-uac1 from QEMU's emulated USB Audio device in the capture VM — _why:_ check the UAC parser against kernel output without new hardware _(by claude)_
- **2026-11-09** · action · Started the 24 h fuzz run for the phase 2 gate on lab-audiocaps-main (5 targets, 2026-11-09 08:38 UTC) _(by claude)_
- **2026-11-10** · action · Phase 2 gate met: 24 h fuzz run on lab-audiocaps-main finished clean (5 targets, 86 401 s each, 0 crashes, ~15.9 billion executions in all); gate.sh PASS; history scan clean _(by claude)_
