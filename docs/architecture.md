# Architecture (as built)

_Status: end of phase 8. Updated at every phase gate._

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
            │  SystemRoot · Runtime · probe()      │  │
            │  alsa · sysfs · server · bluez ·     │  │
            │  jack · cec · sandbox · watch        │  │
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
| `audiocaps-parse` | Pure parsers over bytes/text: raw ELD and kernel ELD text, EDID/CTA-861 (SADs, speaker allocation, HDMI/HDMI Forum VSDBs), HDA codec dumps, USB Audio Class descriptors, procfs (cards, pcm, PCM `info`/`status`, USB `stream#`, `hw_params`, raw MIDI, sequencer clients), alsa-lib configuration (syntax with includes and quoted keys; resolves a PCM name such as `default` through its plugin chain, including the lazily loaded per-card definitions), os-release, EDID serial redaction. Maps SADs to model `CodecCapability`. Each parser has a cargo-fuzz target in `fuzz/` (separate workspace, never shipped) and is checked against the captures in `tests/captures.rs`. | Do I/O; panic on any input. |
| `audiocaps-backends` | `SystemRoot` (live `/` or a fixture tree), the capture `Manifest` format, the `Runtime` (probe thread, per-backend deadlines, snapshot generations and diffs), `probe`/`probe_with`, and the backends: `alsa` (alsa-lib reader and its serializable dump), `sysfs` (codecs, USB streams, driver, PCM state, MIDI), `server` (PipeWire and PulseAudio clients, and parsers for `pw-dump` / `pactl -f json`), `environment`, and `watch` (change events). `servers` decides passively whether a sound server manages the cards. | Read outside the scope; open `/dev` nodes through `SystemRoot`; open a PCM a server manages or that is in use; write any control; run commands (only the CLI's capture does). |
| `audiocaps-rules` | Quirk packs (`quirks`), effective sets and path reports (`paths`), facts from snapshots (`facts`) and the declarative rule engine with the bundled starter pack (`engine`); `analyze(snapshot, previous, rules)`. No I/O: callers pass pack text. See `docs/rules.md`. | Fetch anything from the network; run code from packs. |
| `audiocaps-ffi` | The C ABI and cbindgen header (phase 9). Only crate allowed `unsafe`. | Contain logic beyond marshalling. |
| `audiocaps-cli` | `capture`, `import`, `check-redaction`, `replay`, `probe` (through the runtime; `--endpoint`, `--repeat`), `watch`, `alsa-dump`, `server-dump`, `schema`. Owns command execution and text/JSON redaction for captures. | Be linked by applications. |

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

## Runtime (phase 3)

- `Runtime::new(root)` starts one probe thread that owns the backends. `Runtime::probe(scope,
  options)` sends the request and waits at most the longest backend deadline plus 250 ms; a probe
  thread stuck in a kernel call yields a snapshot whose layers are `unknown` / `timeout`, and the
  application carries on.
- On the probe thread, `runtime::run` interleaves the backend tasks (`alsa`, `sysfs`,
  `environment`) round-robin, one bounded step at a time (one PCM, one card). Each backend has its
  own deadline (`ProbeOptions`, default 150 ms); a backend past it stops and everything it didn't
  reach is reported as `timeout` while the others finish.
- Each completed probe gets the next generation number and becomes `latest()` (an `Arc` swap);
  `Snapshot::diff` lists added, removed and changed endpoints between generations (read times are
  ignored). Queries on a snapshot never lock.
- Backends whose read is one blocking library call (BlueZ over D-Bus, libjack, CEC ioctls) run
  on a helper thread; the executor polls for their result, so a slow call can't eat another
  backend's deadline. Past its deadline the result is dropped and the helper ends on its own.
- `probe(root, scope)` is the same pipeline without deadlines, for fixtures, tests and tools.

## Probe pipeline (phase 3)

1. **Discovery:** `/proc/asound/cards` and `/proc/asound/pcm`; with MIDI in scope, raw MIDI devices
   (`/proc/asound/cardN/midiD`) and sequencer clients (`/proc/asound/seq/clients`). Kernel
   sequencer ports that belong to a raw device become its correlation keys; other ports
   (applications, Midi Through) are virtual MIDI endpoints. Only in-scope endpoints are kept.
2. **Backends** get work only for in-scope endpoints, layers and field groups:
   - `sysfs`: codec dumps and USB `stream#` files (transport from the codec pins or USB, host caps,
     pins and alt-settings), bus IDs, driver/module/parameters/power, PCM sub-stream state.
   - `alsa`: hardware parameter ranges and channel maps, jack and IEC958 controls, the
     configuration. On a fixture it reads the captured dump (`commands/alsa.json`) instead.
   - `environment`: os-release, kernel, sandbox, server presence (captured output only until
     phase 4).
3. **Assembly** turns the results into the model. New in schema 1.1.0 (additive): per-layer
   `detail` (`host_hardware`, `kernel_driver`, `device`), `Snapshot.routing` (the `default` plugin
   chain per direction), `Endpoint.midi`, ALSA raw MIDI and sequencer correlation keys, the
   `routing`, `driver` and `hardware` field groups, `unknown` reason `managed_by_server`, and the
   `api` provenance source.

## Sound servers and events (phase 4)

- Each server is read into one `ServerDump`: live (`server::pw_live`, `server::pulse_live`) or
  from captured `pw-dump` / `pactl -f json` output (`server::pwdump`, `server::pactl`). Assembly
  turns it into a `sound_server` layer on each endpoint (one view per server API: state, current
  format, profiles, routes, passthrough, volume, default), `Snapshot.servers` (clock, defaults,
  WirePlumber policy), and endpoints that exist only in a server: Bluetooth, virtual and network
  nodes (`node:<name>`, Bluetooth addresses hashed). Server nodes join ALSA endpoints through
  `alsa.card` / `alsa.device`.
- Connect rules: PulseAudio with `NOAUTOSPAWN`; outside a sandbox a client connects only when the
  server process is visible (a socket-activated server that isn't running would otherwise start);
  inside Flatpak or Snap when the socket is granted (decision 2026-11-11). Nothing is written,
  no stream is created, nothing is linked.
- `Runtime::watch(scope)` runs a watch thread with its own PipeWire session (subscribed to node,
  device and metadata changes), a PulseAudio subscription (sink, source, card, server), and an
  inotify watch on `/dev/snd` (create/delete; nothing under it is opened). A change re-probes the
  watched scope, reusing the sessions' current server state; the update is published as a new
  generation and delivered with its diff. Updates that change nothing in scope are dropped.
  `AUDIOCAPS_WATCH_TRACE=1` prints per-task timings to stderr.
- Caches: hardware parameter ranges and HDA codec dumps per card (cleared on hotplug), the ALSA
  configuration text (reloaded only when alsa-lib reloads it) and the resolved routing.

## Bluetooth, JACK and sandboxes (phase 5)

- `bluez`: one `GetManagedObjects` call on the system bus, only when `org.bluez` already owns its
  name. Remote stream endpoints (`MediaEndpoint1`) give the codecs a device advertises; the
  transport (`MediaTransport1`) gives the negotiated one. The wireless layer merges these with the
  host's per-codec profiles from PipeWire: `advertised_by_device`, `available_on_host`,
  `negotiated` per codec. Addresses are `Sensitive` (hashed on export); aliases are personal names.
- `jack`: libjack is loaded at run time; the client opens with `JackNoStartServer`, is never
  activated, and its log output is silenced. Reported as a server state (rate, buffer, ports).
- `sandbox`: Flatpak from `/.flatpak-info`; Snap plugs inferred (sockets, `/dev/snd` listing,
  `/sys/class/sound`, and BlueZ refusing access). Each gap: permission, hidden layers, fix,
  required or not. A layer that can't be read because of a gap says so in its detail.

## Sink chain (phase 6)

- ELD for a PCM, in order: the ALSA `ELD` control (raw bytes; its `device` is the PCM), else the
  kernel's `eld#C.P` text when its `codec_cvt_nid` converter is bound to the PCM in the codec dump
  (`Device: … device=N`, only while a stream is open). Otherwise the PCM's sink chain is
  `not_exposed` with the reason; an ELD is never guessed onto a PCM.
- EDID: the connected DRM connector whose EDID has the ELD's manufacturer and product, preferring
  one on the same PCI slot as the sound card (GPU function 0 vs. its audio function 1). That join is
  an `inferred` `drm_connector` correlation key.
- Descriptors: raw ELD, else EDID CTA blocks, else the kernel text (which has lost flag bits).
  A speaker allocation of 0 (or the kernel's 0xffff stand-in) is `not_exposed`.
- CEC (opt-in): `cec.rs` opens `/dev/cecN` (the only device node the library opens directly, only
  on this path), uses only adapters that already have a logical address, keeps the default
  initiator/no-follower mode, and sends polls, Give Physical Address, Give OSD Name, Give Device
  Vendor ID and, to an audio system, Request Short Audio Descriptor. The adapter whose physical
  address matches the sink's EDID is attached to that sink; audio systems become `downstream`.
- Quirks: `audiocaps-rules::quirks` loads the bundled pack, system packs and the application's
  (`ProbeOptions::quirk_dirs`); entries matching the sink or a downstream CEC device are listed on
  the sink record, and codec additions/removals mark the sink layer's values `inferred` with a
  `quirk` source.

## Opt-ins (phases 6 and 8)

Off by default; each runs only when `ProbeOptions` asks, and does only what it asked:

- `with_cec()`: CEC queries (phase 6).
- `with_software_codecs()`: lists library directories for known codec providers (`codecs.rs`);
  nothing is loaded. A capture without library directories reports `not_exposed`.
- `with_active_probe(endpoint, codecs)`: `alsa/active.rs`, after the passive backends finish.
  Live system only; refused when a sound server runs (`managed_by_server`) or the device is open
  (`busy`). LPCM: digital silence on `hw:` in the requested layout. Compressed: IEC 61937 pause
  bursts in the codec's carrier layout (2 ch 48 kHz, 2 ch 192 kHz, or HBR 8 ch 192 kHz) through
  `hdmi:`/`iec958:` with non-audio AES bytes; alsa-lib's hooks restore the previous IEC 958
  status on close. An accepted result confirms the host side only.

## Analysis (phase 7)

After assembly, when `effective`, `paths` or `findings` is in scope, the probe loads rule packs
(bundled, `/usr/share/audiocaps/rules.d`, `/etc/audiocaps/rules.d`, app directories) and calls
`audiocaps_rules::analyze`: effective sets and path reports per access path, then facts and
rules. The runtime and watches pass the previous snapshot, for `SINK_CHANGED`. Details, the fact
catalogue and every rule: `docs/rules.md`.

## Passive guarantees

- Readers never open device nodes (`SystemRoot` refuses `/dev`).
- The ALSA backend opens a PCM only through alsa-lib, only with `SND_PCM_NONBLOCK`, only when no
  sound server runs (PipeWire, PulseAudio or JACK socket or process; checked passively, nothing is
  connected to; inside Flatpak or Snap a host server can't be ruled out, so it never opens one)
  and only when `/proc` shows a free sub-device. It reads the parameter ranges
  (`snd_pcm_hw_params_any`) and channel maps and closes the handle; parameters are never installed
  and nothing is prepared, started or written. A PCM in use is described from `/proc`
  (`status`, `hw_params`) and reported `busy`; a PCM a server manages is `managed_by_server`.
- Control devices are opened to list and read elements (jacks, IEC958 switches, ELD); nothing is
  written, locked or subscribed.
- The configuration is read with `snd_config_update` (files only, no plugin instantiated, so
  nothing connects to a server). The per-card definitions that alsa-lib loads lazily on open are
  built from the same files (`/usr/share/alsa/cards`, `/usr/share/alsa/pcm`) by the parser.
- `audiocaps capture` reads regular files under `/proc`, `/sys`, `/etc` and `/usr/share/alsa`,
  runs the ALSA reader in-process under the rules above, runs `pw-dump`; `pactl` only when a
  PulseAudio socket exists, with autospawn disabled; `busctl` against BlueZ only when `org.bluez`
  already owns its bus name (no D-Bus activation).
- `scripts/check-passive.sh` runs a command under `strace -f` and fails on any `/dev/snd` node
  other than PCM and control devices, on any PCM opened while a server runs, and on any blocking
  PCM open. `scripts/gate-alsa.sh` checks that playback on a busy device is never touched.
- Note: reading `/proc/asound/cardN/codec#M` makes the HDA driver query the codec (it may resume a
  runtime-suspended codec). It does not open a PCM or change any setting; `alsa-info.sh` does the same.

## Limits at the end of phase 8

- Active probes of compressed formats have only been checked to be skipped on analog outputs in
  the lab; no IEC 958 output was available there, and the production rig runs PipeWire (refused).

## Limits at the end of phase 7

- Five starter rules wait for inputs the library doesn't read yet (see `docs/rules.md`):
  host HBR quirk data, the app's client library version, USB descriptor clock ranges, video
  timing and realtime scheduling.
- JACK isn't an access path in effective sets or path reports yet.

## Limits at the end of phase 6

- On the production rig (AMD GPU) there is no CEC adapter, so a receiver behind the TV is
  `not_exposed`; its formats are only visible as what the TV forwards.
- Captures made while no HDMI stream is open, without the ALSA ELD control (capture format 1),
  can't bind an ELD to a PCM.
- The CEC reader is checked on vivid's emulated adapters; no physical CEC adapter has been used.
- Bluetooth codec gaps are checked against synthetic fixtures and unit tests; no Bluetooth
  hardware has been probed yet (the headset test needs a host exception).
- Snap plug detection is inferred; the sandbox release gate (phase 9) runs real Flatpak and Snap
  builds.
- On a system without a sound server, the first probe in a process opens each free PCM once
  (non-blocking, about a millisecond); an application opening that device at the same instant
  gets EBUSY. Later probes use the cached ranges.
- Jack plug state on systems without a server is read per probe; there is no ALSA control-event
  watch yet (a server's route availability covers it when a server runs).
- Inside Flatpak or Snap, connecting to a granted but stopped socket-activated server can start it.
- HDMI PCMs: the generic HDMI driver binds converters and pins at runtime, so per-PCM host caps
  and pins are `not_exposed` until ELD correlation in phase 6.
- IEC958 channel status (non-audio bit, rate) has no safe reader in the `alsa` crate; only the
  switch state is reported.
- On a machine with a sound server, ALSA hardware ranges stay `managed_by_server`; the server
  layer reports what the server offers.
- A container that shares `/dev/snd` with the host but has its own PID namespace (outside Flatpak
  and Snap) can hide an idle host server; a device that server is playing on is still seen as busy
  from `/proc` and never opened.
- The workstation and HTPC fixtures predate the ALSA dump (capture format 1); their ALSA layer is
  `not_exposed` until they are recaptured.
