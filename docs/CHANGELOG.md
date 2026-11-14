# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). The version covers the C ABI
(`libaudiocaps.so.1`, `AC_ABI_VERSION`) and the snapshot schema (`schema_version`, additive
within v1).

## [Unreleased]

## [1.0.0] - 2026-11-14

First release for Linux. AudioCaps reports what audio the system can really deliver, from the
TV or receiver back through the host hardware, drivers, ALSA, the sound server and the sandbox
to the application, and where anything is lost on the way. It runs fully in-process inside
Flatpak and Snap, is passive by default, and probes only what the application asks for.

Ships as `libaudiocaps.so.1` with the C header `audiocaps.h` (C ABI 1), the `audiocaps`
command-line tool and snapshot schema 1.7.0.

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

- Runtime: a probe thread with per-backend deadlines (150 ms default), round-robin backend tasks,
  snapshot generations, `latest()` and `Snapshot::diff`.
- ALSA backend: hardware parameter ranges, channel maps, jack and IEC958 controls and the
  `default` plugin chain, under strict passive rules (a PCM is opened non-blocking only when no
  sound server runs and the device is free; nothing is ever configured or written).
- Host hardware and driver detail: transport measured from HDA codec pins or USB, converter and
  USB alt-setting capabilities, pins, bus IDs, module, parameters, power state and jack state.
- Capture endpoints get the same detail; MIDI endpoints for raw MIDI devices and sequencer ports.
- Schema 1.1.0 (additive): per-layer `detail`, `routing`, `midi`, new correlation keys, field
  groups `routing`, `driver` and `hardware`, reason `managed_by_server`, provenance source `api`.
- `audiocaps probe` runs through the runtime (`--endpoint`, `--repeat`, `--quiet`); new
  `audiocaps alsa-dump`.
- Capture format 2: the ALSA dump (`commands/alsa.json`) and alsa-lib's card and PCM definition
  files; the three lab fixtures were recaptured with a USB Audio device and virtual MIDI.
- Parsers for alsa-lib configuration, PCM `info`/`status`, raw MIDI and sequencer clients, and
  HDA converter-to-PCM bindings; fuzz target `alsa_conf`.

- PipeWire backend (native client): audio nodes with their formats, negotiated format, IEC958
  passthrough codecs, state and volume; devices with profiles and routes; graph clock; defaults;
  WirePlumber policy (`sm-settings`); Bluetooth codecs from per-codec profiles.
- PulseAudio backend (libpulse, autospawn off): sinks, sources, cards, profiles, ports,
  passthrough formats; works against native PulseAudio and pipewire-pulse.
- Server views on every endpoint, snapshot-level server state, and endpoints that exist only in
  a server (Bluetooth, virtual, network); Bluetooth addresses in names are hashed.
- `Runtime::watch` and `audiocaps watch`: hotplug (inotify on `/dev/snd`) and server change events,
  with the new snapshot and its diff.
- `audiocaps server-dump` (live, or parsed from `pw-dump` / `pactl -f json`).
- Schema 1.2.0 (additive): `sound_server` layer detail, `Snapshot.servers`.

- BlueZ backend over D-Bus (zbus): devices, the codecs their stream endpoints advertise, and the
  negotiated transport codec; never activates bluetoothd. The wireless layer merges advertised,
  host-supported and negotiated codecs, so gaps (e.g. LDAC advertised but missing on the host)
  show directly. Bluetooth-only endpoints when no server exposes a device.
- JACK backend: libjack loaded at run time, `JackNoStartServer`, never activated; rate, buffer
  size and physical ports with latency. The capture tool saves it (`commands/jack.json`).
- Sandbox detection: Flatpak permissions from `/.flatpak-info`, Snap plugs inferred from what the
  snap can reach; each missing permission names the layers it hides and the exact fix, and
  unreadable layers say which permission is missing.
- Schema 1.3.0 (additive): wireless layer detail, permission gaps, JACK ports.

- Sink records for HDMI/DP outputs: identity (PNP id and name, product, model, hashed serial,
  stable sink key), short audio descriptors, speaker allocation, sync delay and HDMI link limits
  (TMDS/FRL, latency, physical address), from the ALSA ELD control, the kernel's ELD text and the
  DRM connector's EDID.
- Correlation of ALSA PCM, DRM connector and sound-server node.
- CEC, opt-in (`ProbeOptions::with_cec`, `--cec`): devices on the bus, an audio system's
  descriptors behind the TV; configured adapters only, query messages only. `audiocaps cec-dump`.
- Quirk packs: bundled, `/usr/share/audiocaps/quirks.d`, `/etc/audiocaps/quirks.d` and the
  application's directories, later ones overriding by id; first entry: Denon AVR-S660H.
- New HTPC fixture (capture format 2) and a synthetic eARC + CEC fixture.
- Schema 1.4.0 (additive): sink link, CEC bus, applied quirks.

- Effective sets per access path and path reports (per-hop format, bit rate, ceiling, latency,
  losses, bottleneck, end-to-end latency).
- Declarative rule engine with the 29-rule starter pack (codes from the plan), distro-keyed
  hints, overrides from `/usr/share/audiocaps/rules.d`, `/etc/audiocaps/rules.d` and the app
  (`--rule-dir`); findings in the snapshot. `docs/rules.md` lists every rule and fact.
- App request inputs (`--app-endpoint`, `--app-channels`, `--app-rate`, `--app-bits`,
  `--latency-target-us`); fuzz target `rule_pack`; `scripts/gate-rules.sh`.
- Schema 1.5.0 (additive): route `detected_passthrough`, link `connector`/`connection`,
  `Snapshot.app_request`.

- Software codec check (opt-in, `--software-codecs`): codec providers the app could load
  (FFmpeg, ALSA's a52 AC-3 encoder plugin, liba52, libdca, GStreamer libav, PipeWire Bluetooth
  codec plugins), found on disk and never loaded; codec lists marked inferred.
- Active probe (opt-in, `--active-probe`): about 100 ms of silence per requested format on one
  idle device (silent PCM, or IEC 61937 pause bursts through `hdmi:`/`iec958:`, whose hooks
  restore the IEC 958 status); refused on busy or server-managed devices.
- Schema 1.6.0 (additive): `Snapshot.software_codecs`, `Endpoint.active_probe`.
- C API: `libaudiocaps.so.1` and the generated header `include/audiocaps.h`. Opaque context,
  snapshot and watch handles; JSON scope and options; `ac_can_play` (yes, no or unknown with the
  reason); watches by callback thread or pollable fd plus dispatch.
- Probe option `redaction`: redaction stays on by default; apps can opt out (`"redact": false`).
- Consumer guide (`docs/consumers.md`) with C, C++, Python (ctypes) and .NET (P/Invoke) examples
  that print identical output, and an mpv option mapping.
- Realtime availability in the environment layer (`RLIMIT_RTPRIO`, rtkit, the Realtime portal),
  read passively; the `NO_REALTIME` finding now fires.
- Schema 1.7.0 (additive): `EnvironmentInfo.realtime`.
- Flatpak `finish-args` and Snap plug templates (`packaging/`), with what each permission
  unlocks and the sandbox release gate results; `scripts/build-portable.sh` builds binaries that
  run on the freedesktop 24.08 runtime and core24.

### Fixed

- Inside a snap, sound-server sockets are found next to the snap's private runtime directory, so
  PulseAudio and PipeWire are read from strict snaps; a connection the confinement refuses is
  reported `not_granted` with the plug to connect.
- Flatpak's placeholder `pipewire-0` file is no longer mistaken for the PipeWire socket.
- A CEC adapter that can't be opened for lack of permission is reported `not_granted`.
- Snap plug inference: `alsa` is judged by `/proc/asound`, `hardware-observe` by the DRM EDID,
  and `/sys/module` needs `kernel-module-observe`; a capture claims no plug state.
- The realtime read runs on its own helper thread, so a slow bus can't delay other backends.
- Watches in a snap stop retrying a server the confinement refused.

### Changed

- Documentation follows the repository structure standard: decision log in
  `docs/DECISIONS.md`, changelog in `docs/CHANGELOG.md`.
- Hardware ranges and HDA codec dumps are cached per process and cleared on hotplug, so a free
  PCM is opened at most once and codecs aren't queried on every probe.
- HDMI/DP converters no longer report host channel and rate limits (they don't limit the link).
- ALSA routing evaluates configuration hooks (`load`, `pulse_load_if_running`), so the default
  device on a PulseAudio system resolves to the pulse plugin; the capture tool saves
  `/usr/share/alsa/*.conf`. The lab-pulseaudio fixture was recaptured.
- BlueZ, JACK and CEC reads run on helper threads, so a blocking call (e.g. opening a
  pipewire-jack client) can't push another backend past its deadline.
- The `pw-dump` parser keeps audio devices only, as the live client does.
- The native PipeWire client no longer fails when PipeWire 1.6 reports an error for one object
  (e.g. no Format on a suspended node); only core errors end the session.
- Bluetooth node names from a sound server are treated as personal (masked on export).
- ALSA routing resolution handles definitions that are `@func` calls (USB-Audio) and `card_name`.
- `scripts/check-passive.sh` allows control-device reads and non-blocking PCM opens when no
  sound server runs; it fails on any PCM opened while a server runs.
