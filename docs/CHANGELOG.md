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
- Fixture replay with golden snapshots.

### Changed

- Documentation follows the repository structure standard: decision log in
  `docs/DECISIONS.md`, changelog in `docs/CHANGELOG.md`.
