# Architecture (as built)

_Status: end of phase 1. Updated at every phase gate._

AudioCaps is one Rust workspace that builds a single shared library (`libaudiocaps.so`, from
`audiocaps-ffi`) and a command-line tool (`audiocaps`, from `audiocaps-cli`). No Rust crate is
published; applications bundle the shared library and use the generated C header.

```text
            ┌───────────────────┐      ┌───────────────────┐
            │  audiocaps-ffi    │      │  audiocaps-cli    │
            │  C ABI (phase 9)  │      │  capture, replay, │
            │  libaudiocaps.so  │      │  import, schema   │
            └─────────┬─────────┘      └────┬─────────┬────┘
                      │                     │         │
                      ▼                     ▼         │
            ┌──────────────────────────────────────┐  │
            │  audiocaps-backends                  │  │
            │  SystemRoot · manifest · probe()     │  │
            │  (ALSA/PipeWire/Pulse/BlueZ/JACK:    │  │
            │   phases 3–5)                        │  │
            └──────┬───────────────────────┬───────┘  │
                   │                       │          │
                   ▼                       ▼          ▼
     ┌──────────────────────┐   ┌──────────────────────────┐
     │  audiocaps-parse     │   │  audiocaps-rules         │
     │  pure parsers, no I/O│   │  rule packs (phase 7)    │
     └──────────┬───────────┘   └────────────┬─────────────┘
                │                            │
                ▼                            ▼
            ┌──────────────────────────────────────┐
            │  audiocaps-model                     │
            │  types · scope · redaction · schema  │
            └──────────────────────────────────────┘
```

Arrows point at dependencies. The model depends on nothing in the workspace; nothing depends on
the CLI or the FFI crate.

## Crates

| Crate | Owns | Must not |
|---|---|---|
| `audiocaps-model` | Every exported type: `ProbeScope`, `Snapshot`, `Endpoint`, `LayerCapabilities`, `CapabilitySet`, `EffectiveSet`, `PathReport`/`Hop`, `Finding`, `Value<T>` with `Provenance` and `Confidence`, `Sensitive` and `RedactionPolicy`. Generates the JSON schema. | Do I/O, or assume Linux (Linux identifiers appear only as data). |
| `audiocaps-parse` | Pure parsers over bytes/text: raw ELD and kernel ELD text, EDID/CTA-861 (SADs, speaker allocation, HDMI/HDMI Forum VSDBs), HDA codec dumps, USB Audio Class descriptors, procfs (cards, pcm, USB `stream#`, `hw_params`), os-release, EDID serial redaction. Maps SADs to model `CodecCapability`. Each parser has a cargo-fuzz target in `fuzz/` (separate workspace, never shipped) and is checked against the captures in `tests/captures.rs`. | Do I/O; panic on any input. |
| `audiocaps-backends` | `SystemRoot` (live `/` or a fixture tree), the capture `Manifest` format, and `probe(root, scope)`. Later: one module per subsystem with per-backend deadlines on a single-threaded executor. | Read outside the scope; open `/dev` nodes through `SystemRoot`; run commands (only the CLI's capture does). |
| `audiocaps-rules` | Declarative rule packs that turn snapshots into findings (phase 7). | Fetch anything from the network. |
| `audiocaps-ffi` | The C ABI and cbindgen header (phase 9). Only crate allowed `unsafe`. | Contain logic beyond marshalling. |
| `audiocaps-cli` | `capture`, `import`, `check-redaction`, `replay`, `probe`, `schema`. Owns command execution and text/JSON redaction for captures. | Be linked by applications. |

## Key types

- **`ProbeScope`** — endpoint classes, directions, endpoint ids, layers, field groups and access
  paths, each a `Selection` (`all` or `only[...]`). `ProbeScope::nothing()` is the starting point;
  fields outside the scope are *omitted*, not reported as unknown.
- **`Value<T>`** — `known { value, confidence: measured|inferred, provenance }` or
  `unknown { reason, detail }`. Reasons: `not_granted`, `busy`, `library_missing`,
  `service_not_running`, `not_exposed`, `timeout`, `parse_error`, `read_error`, `not_implemented`,
  `not_requested`. Nothing is ever reported as "unsupported" because it couldn't be read.
- **`Endpoint`** — id (`alsa:card1/pcm3p`), class, direction, transport, name, correlation keys
  (ALSA card/PCM, PipeWire node, Pulse device, DRM connector, BlueZ device), optional `SinkRecord`,
  per-layer capabilities, effective sets per access path, path reports.
- **`PathReport`** — hops (`application`, `sound_server`, `alsa`, `driver_host`, `link`,
  `bluetooth_link`, `sink`, `sink_chain`) with format, bit rate, ceiling, latency and losses; the
  bottleneck index; an end-to-end latency estimate.
- **`Finding`** — stable string code, severity, endpoint, access path, hop, evidence with
  provenance, confidence, and a distro-family hint.
- **`Sensitive`** — serials, Bluetooth addresses and personal device names; redacted on export by
  default (hash or mask), opt-out per kind.

## Systems, fixtures and captures

`SystemRoot::Live` reads `/`; `SystemRoot::Fixture` reads an extracted capture:

```text
<fixture>/
  manifest.json      format, tool version, capture time, commands run, redaction rules, notes
  root/              files at their real paths: proc/asound/…, sys/…, etc/os-release
  commands/          pw-dump.json, pactl-info.json, pactl-list.json, bluez.json
```

Both refuse paths under `/dev`; fixture reads also refuse anything that escapes the tree through
`..` or symlinks. The capture clock (`captured_at_ms`) replaces "now", so replays are deterministic.
Golden snapshots live next to each fixture as `tests/fixtures/<name>.snapshot.json`.

## Passive guarantees (phase 1)

- Readers never open device nodes (`SystemRoot` refuses `/dev`).
- `audiocaps capture` reads regular files under `/proc`, `/sys`, `/etc` only, and runs `pw-dump`;
  `pactl` only when a PulseAudio socket exists, with autospawn disabled; `busctl` against BlueZ only
  when `org.bluez` already owns its bus name (no D-Bus activation).
- `scripts/check-passive.sh` runs the capture under `strace -f` and fails if any process opened
  `/dev/snd/*`.
- Note: reading `/proc/asound/cardN/codec#M` makes the HDA driver query the codec (it may resume a
  runtime-suspended codec). It does not open a PCM or change any setting; `alsa-info.sh` does the same.

## Phase 1 limits

The phase 1 probe builds endpoints from `/proc/asound/pcm` and reports the environment (os-release,
kernel, sandbox, and server presence from captured command output). Every other layer is
`unknown` / `not_implemented` with the phase that adds it. Transport is inferred from PCM names.
