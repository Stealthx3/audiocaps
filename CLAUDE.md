# CLAUDE.md — AudioCaps

Standing instructions for Claude Code working in this repository. Read this
file, `docs/DECISIONS.md`, `docs/USAGE-PLAN.md` and the full plan in
`docs/plan/AudioCaps-Development-Plan.pdf` before changing anything.

## What this is

AudioCaps is a library that applications bundle to learn what
audio the system can really deliver, and where anything is lost on the way:
from the TV or AV receiver (EDID/ELD, CEC) back through host hardware, kernel
drivers, ALSA, the sound server (PipeWire, WirePlumber, PulseAudio, JACK) and
the sandbox, to the app. Linux 1.0 first; Windows and macOS later on the same
OS-neutral core.

## Decisions that are settled (do not reopen without asking)

- **Language:** one Rust core plus a cbindgen-generated C header. No other
  implementation languages, no maintained per-language bindings, no published
  Rust crate. Consumers bundle the shared library and follow project docs.
- **License:** LGPL-2.1-or-later. Every source file carries
  `SPDX-License-Identifier: LGPL-2.1-or-later`. Dependencies must be compatible;
  dual MIT/Apache-2.0 crates are taken under MIT. Apache-2.0-only, GPL-only or
  unknown licenses need Micaiah's approval.
- **Sandboxes:** must work fully in-process inside Flatpak and Snap. No host
  service or daemon. A layer that can't be read is `unknown` with a reason,
  never "unsupported".
- **Crates:** `audiocaps-model`, `audiocaps-parse`, `audiocaps-backends`,
  `audiocaps-rules`, `audiocaps-ffi`, `audiocaps-cli`.
- **Backends:** `alsa`, `pipewire-rs`, `libpulse-binding` linked directly;
  `zbus` for D-Bus; only libjack is loaded lazily.
- **Rules and quirk data:** declarative packs bundled with each release,
  overridable from a system directory or the app's bundle. Never fetched from
  the network.

## Guiding principles (apply to every API and implementation choice)

1. **Information on-demand.** Probe and return only the scope the application
   asks for (endpoint classes, endpoints, layers, fields). Nothing outside the
   scope is read, cached or reported. Watchers cover subscribed scopes only.
2. **The application decides.** Active probes, CEC and other opt-ins run only
   when the app calls them, and do exactly what was asked.
3. **Passive by default.** Never open a device a sound server manages, never
   change profiles, formats or volume, never auto-start PulseAudio or JACK.
   Running audio must never be disturbed.

## Working rules

- **Phases and gates.** Work follows the roadmap in the plan (9 phases to Linux
  1.0). Finish a phase on its own branch, meet its gate, then stop: summarize
  what was built and how the gate was verified, and wait for Micaiah's approval
  before starting the next phase.
- **Ask before:** reopening a settled decision, adding a dependency with a new
  license family, installing system packages on the host, touching the host's
  audio configuration, or anything irreversible.
- **Notify:** when a gate is reached or a decision needs Micaiah, send a phone
  push notification if a notification tool is available, otherwise say so
  prominently at the end of the response.
- **Dev and test environment:** build and test inside libvirt/KVM VMs or LXC
  containers on this machine (visible in Virtual Machine Manager). Hardware
  reaches them by USB/PCI passthrough or `/dev/snd` bind-mounts. Never test by
  reconfiguring the host's live audio stack.
- **Standards:** copies of the shared ai-assets standards live in
  `docs/standards/` and apply to this project: REPO-STRUCTURE-STANDARD.md,
  PROGRESS-BAR-STANDARD.md, PHONE-ALERTS-STANDARD.md, VM-LAB-STANDARD.md,
  TOKEN-USAGE-OPTIMIZATION-STANDARD.md, POST-MORTEM-STANDARD.md (masters in
  `/mnt/FileDrive/Business/Development/ai-assets`). If one conflicts with this
  file, ask.
- Build/test only via `scripts/lab.sh` (see `docs/LAB.md`). Guests are
  watchable in virt-manager. Real-hardware tests on the host need a one-time
  exception from Micaiah per test.
- Phone alerts: see PHONE-ALERTS-STANDARD; hooks installed, check with
  `node scripts/phone-alerts.mjs check`.
- Progress: tick `docs/PLAN.md` in the same commit as the work, then run
  `node scripts/progress.mjs` and `node scripts/repo-check.mjs progress`. The
  page is published by GitHub Pages: https://stealthx3.github.io/audiocaps/
- Usage plan: `docs/USAGE-PLAN.md` (read the handoff note first). Append one
  line per session to `docs/postmortem/JOURNAL.md`.

## Repository conventions

- Branches: `main` is always releasable; work on `<type>/<kebab-case>`
  branches (phase work: `feat/phase-N-<topic>`; also `fix/`, `docs/`, `chore/`). Conventional Commit messages
  (`feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`).
- Every user decision or action goes in `docs/DECISIONS.md` (oldest first,
  via `node scripts/repo-check.mjs log`). User-facing changes update
  `docs/CHANGELOG.md` under Unreleased.
- Auxiliary docs live in `docs/`. The README follows REPO-STRUCTURE-STANDARD §5
  (generated progress block, goals, screenshots/mockups, features, docs).
- **Never commit** anything under `docs/plan/` (plan documents and local notes),
  raw captures (`/captures/`), secrets, passwords, IP addresses or MAC addresses.
  Only redacted fixtures go into `tests/fixtures/`.
- Quality bar before any gate: `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test`, `cargo deny check`, and fuzz targets building for every parser.

## Where things are

| Path | Contents |
| --- | --- |
| `docs/plan/AudioCaps-Development-Plan.pdf` | Full plan: goals, architecture, data model, rule catalogue, API, sandbox matrix, roadmap, testing (git-ignored) |
| `docs/plan/local-environment.md` | Home network and test rig details (git-ignored) |
| `docs/DECISIONS.md` | Decision & action log |
| `docs/PLAN.md` | Roadmap checkboxes and open items (drives the progress page) |
| `docs/architecture.md` | Crate boundaries as built |
| `README.md` | Public-facing summary |
