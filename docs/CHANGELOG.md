# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project will
use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository scaffold: README, LGPL-2.1-or-later license, notice, editor and git
  settings, and the decision and action log.
- Public progress page on GitHub Pages, generated from `docs/PLAN.md`, with a
  progress block at the top of the README.
- Project tooling from the shared standards: repo check, lab VMs, phone alerts.

- Cargo workspace with the six crates (`audiocaps-model`, `-parse`, `-backends`,
  `-rules`, `-ffi`, `-cli`), pinned toolchain and a cargo-deny license gate.
- Data model and JSON schema v1 (`schema/audiocaps-snapshot-v1.schema.json`) with
  scoped probing, explicit unknowns, provenance, confidence and redaction.
- `audiocaps capture`, `import`, `check-redaction`, `replay`, `probe` and `schema`
  commands.
- Fixture replay with golden snapshots; five captured fixtures (workstation, HTPC,
  and PipeWire, PulseAudio and ALSA-only lab VMs) plus a synthetic one.
- `audiocaps redact-fixture` to re-apply redaction rules to an existing fixture.
- Parsers for raw ELD bytes and the kernel's ELD text (extra SAD flags such as
  E-AC-3 JOC and MAT Atmos kept), EDID with CTA-861 extensions (SADs, speaker
  allocation, HDMI and HDMI Forum blocks with latency and TMDS/FRL limits), HDA
  codec dumps (pins, HDMI/DP/HBR caps, pin defaults, DP-MST devices), USB Audio
  Class 1/2 descriptors (clocks, terminals, alternate settings, endpoint sync), USB
  `stream#` files and PCM `hw_params`.
- Fuzz targets for every parser (cargo-fuzz, separate `fuzz/` workspace) and
  `scripts/fuzz.sh` to seed, run and summarise them.
- `audiocaps capture` now saves the raw USB `descriptors` of audio devices; new
  `lab-usb-uac1` fixture from an emulated USB Audio device.

### Changed

- Documentation follows the repository structure standard: decision log in
  `docs/DECISIONS.md`, changelog in `docs/CHANGELOG.md`.
