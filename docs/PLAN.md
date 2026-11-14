# AudioCaps — Roadmap to Linux 1.0

Public task list for the nine gated phases. The full design lives in the development plan (kept
outside git); this file only tracks progress. Each phase ends at a gate that Micaiah reviews before the
next phase starts. Ticks land in the same commit as the work.

Open items (not phase tasks):

- Confirm whether the TV-to-receiver link runs as eARC or plain ARC.
- Buy a UAC2 USB DAC for testing, or accept fixture-only USB coverage.

## Phase 1 — Scaffold, model, schema

- [x] **1.1** Adopt the shared standards: repo layout, progress page, phone alerts, VM lab, usage plan, journal
- [x] **1.2** Cargo workspace with the six crates, pinned toolchain, rustfmt/clippy settings, SPDX headers
- [x] **1.3** License gate: `deny.toml` for cargo-deny
- [x] **1.4** Data model in `audiocaps-model`: endpoints, layers, capability sets, access paths, path reports, findings, provenance, unknowns, probe scope
- [x] **1.5** JSON schema v1 generated from the model, committed under `schema/`, with redaction rules
- [x] **1.6** `SystemRoot` abstraction: every reader works on `/` or on a captured fixture tree
- [x] **1.7** Capture tool `audiocaps capture`: passive, read-only, redacted tarball
- [x] **1.8** Fixture replay and golden snapshots
- [x] **1.9** Five captures replayed: three lab VMs (PipeWire, PulseAudio, ALSA-only), workstation, HTPC
- [x] **1.10** Docs: README, changelog, decision log, `docs/architecture.md`
- [x] **1.11** Gate: fmt, clippy, tests and cargo-deny clean; history free of plan docs, captures, IPs and MACs

## Phase 2 — Parsers and fuzzing

- [x] **2.1** ELD parser (raw bytes, extra SAD flags kept)
- [x] **2.2** EDID / CTA-861 parser
- [x] **2.3** HDA codec dump and procfs parsers
- [x] **2.4** USB Audio Class descriptor parser
- [x] **2.5** Fuzz targets for every parser
- [x] **2.6** Gate: 24 h fuzz runs clean; parser output matches the captures

## Phase 3 — Runtime, ALSA, capture, MIDI

- [x] **3.1** Single-threaded runtime with per-backend deadlines and scoped snapshots
- [x] **3.2** ALSA backend: cards, PCMs, hw_params, controls, plugin chain
- [x] **3.3** Capture endpoints and MIDI ports
- [x] **3.4** Gate: matches hw_params; never disturbs playback

## Phase 4 — PipeWire, WirePlumber, Pulse

- [x] **4.1** PipeWire backend: nodes, routes, formats, IEC958 codecs, clock
- [x] **4.2** WirePlumber policy and Bluetooth codec settings
- [x] **4.3** PulseAudio backend without auto-spawn
- [x] **4.4** Hotplug and server change events
- [x] **4.5** Gate: matches pw-dump and pactl; hotplug under 20 ms

## Phase 5 — Bluetooth, JACK, sandbox

- [x] **5.1** BlueZ backend over D-Bus: advertised vs negotiated codecs
- [x] **5.2** JACK backend, loaded lazily, never starting a server
- [x] **5.3** Sandbox detection: Flatpak and Snap permissions
- [x] **5.4** Gate: codec and permission gaps reported correctly

## Phase 6 — Sink chain, CEC, quirk data

- [x] **6.1** Sink records: identity, SADs, speaker allocation, stable sink key
- [x] **6.2** ALSA PCM ↔ DRM connector ↔ server node correlation
- [x] **6.3** Optional CEC reads
- [x] **6.4** Quirk data packs
- [x] **6.5** Gate: production rig identified; receiver matches its spec sheet

## Phase 7 — Path reports and rule engine

- [x] **7.1** Effective sets per access path
- [x] **7.2** Path reports: per-hop bit rate, bottleneck, latency
- [x] **7.3** Declarative rule engine and starter rule pack
- [x] **7.4** Gate: each starter rule fires only when it should

## Phase 8 — Software codecs, active probes

- [x] **8.1** Software codec checks (opt-in)
- [x] **8.2** Active probes on idle devices (opt-in)
- [x] **8.3** Gate: opt-ins run only when the app asks

## Phase 9 — C ABI, docs, packaging

- [x] **9.1** C ABI with cbindgen header, events by callback or pollable fd
- [x] **9.2** Consumer docs: C, C++, Python, .NET, mpv mapping
- [x] **9.3** Flatpak and Snap permission templates
- [x] **9.4** Gate: doc examples pass; sandbox release gate passes
