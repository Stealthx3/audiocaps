<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# Rules, facts and path reports

_Phase 7. Generated rule table from `crates/audiocaps-rules/packs/rules-core.json`._

## How findings are made

1. **Effective sets and path reports** (`audiocaps-rules::paths`) are computed from each endpoint's
   per-layer capabilities, per access path (ALSA direct, PulseAudio, PipeWire).
2. **Facts** (`audiocaps-rules::facts`): the snapshot is flattened into named values, each with
   its provenance and confidence. A value that can't be known is simply absent.
3. **Rules** (`audiocaps-rules::engine`) are declarative: a condition over facts, the facts to
   report as evidence, a confidence and hints per distro family. A comparison with an absent fact
   is false, so **a rule never fires on an unknown**.

Findings are computed only when `findings` is in the probe scope (effective sets and paths when
`effective` / `paths` are), and only for endpoints in scope.

## Pack format and overrides

```json
{ "format": 1, "rules": [ { "code": "…", "severity": "info|warning|error",
  "hop": "application|sound_server|alsa|driver_host|link|bluetooth_link|sink|sink_chain|whole_path",
  "scope": "endpoint|system", "once": false,
  "when": { "all": [ { "nonempty": "e.sink.compressed" }, { "eq": ["e.server.passthrough", []] } ] },
  "evidence": ["e.sink.compressed"], "confidence": "measured|inferred",
  "hint": { "generic": "… {e.sink.compressed} …", "debian": "…", "fedora": "…", "arch": "…", "suse": "…" } } ] }
```

Operators: `all`, `any`, `not`, `known`, `nonempty`, `eq`, `ne`, `gt`, `ge`, `lt`, `le`,
`contains`, `intersects`, `subset`, `not_subset`. The first operand is a fact name; later
operands are literals, or fact names when written `$name`. Hints fill `{fact}` placeholders. The
evaluator is a closed interpreter: packs are data and can't run code.

Load order: the bundled pack, then `/usr/share/audiocaps/rules.d/*.json`,
`/etc/audiocaps/rules.d/*.json`, then the application's directories (`ProbeOptions::rule_dirs`,
`--rule-dir`). A later rule with the same code replaces the earlier one; `"when": null` disables
it. Packs are never fetched from the network. Quirk packs load the same way from `quirks.d`.

## Starter rules

| Code | Severity | Hop | Fires when | Status |
|---|---|---|---|---|
| `SINK_NOT_PRESENT` | info | sink | Jack reports no device, or the ELD is empty or invalid. | active |
| `SINK_CHANGED` | info | sink | The sink's identity changed since the previous snapshot (device swapped, receiver powered on or off). | active |
| `SINK_ATMOS_NOT_ENABLED` | warning | sound_server | The sink advertises E-AC-3 with JOC or TrueHD with Atmos, but passthrough for them is disabled on the server node. | active |
| `SINK_ARC_LIMITED` | warning | sink_chain | The device behind the TV advertises more formats over CEC than the TV's own EDID allows. | active |
| `SINK_DOWNSTREAM_UNKNOWN` | info | sink_chain | An ARC/eARC setup is likely, but there's no CEC access to confirm the downstream device. | active |
| `SINK_HBR_RISK` | info | sink | TrueHD or DTS-HD MA advertised on hardware with known HBR failures. | waits for `e.host.hbr_known_bad (host quirk data)` |
| `PASSTHROUGH_NOT_ENABLED` | warning | sound_server | The ELD lists compressed formats, but the server node allows PCM only. | active |
| `PASSTHROUGH_UNDETECTED` | info | sound_server | An ELD is present, but the server exposes no detected-codec property. | active |
| `RATE_CAPPED_BY_SERVER` | info | sound_server | The hardware allows higher rates, but the server's allowed rates force resampling. | active |
| `CHANNELS_DOWNMIXED` | warning | sound_server | The hardware or sink supports more channels than the active profile carries. | active |
| `NOT_BIT_PERFECT` | info | sound_server | Volume, EQ, a filter chain or format conversion alters samples on the path (software volume on a digital output). | active |
| `SERVER_CONFLICT` | error | sound_server | Two servers claim one card, or standalone JACK holds a card PipeWire expects. | active |
| `CLIENT_SERVER_SKEW` | info | application | The app's client library version differs from the host server's (common in sandboxes). | waits for `g.client.pipewire_version` |
| `ALSA_PLUGIN_CONVERSION` | info | alsa | The default device routes through dmix, plug or softvol conversions. | active |
| `DEVICE_BUSY_NO_SERVER` | warning | alsa | Another process holds the hardware device directly; read-only values are used instead. | active |
| `HOST_LIMITS_SINK` | warning | driver_host | The sink accepts more than the host pin or device can send. | active |
| `HBR_UNSUPPORTED_PATH` | warning | driver_host | An HBR format is advertised but the pin or driver can't carry it. | active |
| `USB_BANDWIDTH_LIMITED` | info | driver_host | USB class or bus speed caps rate or channels below the device's claims. | waits for `e.usb.claimed_max_rate (descriptor clock ranges)` |
| `BIT_DEPTH_TRUNCATED` | info | whole_path | Bit depth drops somewhere on the path. | active |
| `POWER_SAVE_ACTIVE` | info | driver_host | Codec power saving is on (wake-up delay, clicks). | active |
| `LINK_BANDWIDTH_LIMITED` | warning | link | The active video mode leaves too little room for the requested audio. | waits for `e.link.audio_headroom_bps (video timing)` |
| `ADAPTER_IN_PATH` | info | link | A DP-to-HDMI adapter or dock is detected on an HBR path. | active |
| `BT_CODEC_UNAVAILABLE` | warning | bluetooth_link | The device advertises LDAC or aptX, but the codec isn't enabled or its plugin isn't installed. | active |
| `BT_BITRATE_LIMITED` | warning | bluetooth_link | The negotiated codec or quality mode is below what both ends support. | active |
| `BT_HEADSET_PROFILE_ACTIVE` | warning | bluetooth_link | The headset profile is active, dropping playback to voice quality. | active |
| `NO_REALTIME` | info | whole_path | No realtime scheduling is available. | `g.realtime.available` (rlimit, rtkit, portal; phase 9) |
| `SANDBOX_PERMISSION_MISSING` | warning | application | A layer is hidden because a Flatpak permission or Snap plug is missing; names the exact fix. | active |
| `APP_FORMAT_BELOW_SINK` | info | application | The app requests less than the path can deliver. | active |
| `LATENCY_BUDGET_EXCEEDED` | warning | whole_path | The end-to-end latency estimate misses an app-supplied target. | active |

"Waits for" rules are complete and tested, but the library doesn't produce their input facts yet,
so they can't fire on a real system until it does.

The gate checks each rule against `packs/rules-core.cases.json`: at least one case where it
fires and one near miss where it must stay silent, and no other rule fires on a case
(`crates/audiocaps-rules/tests/rules.rs`). `crates/audiocaps-backends/tests/findings.rs` checks
the exact findings on every fixture, and `scripts/gate-rules.sh` toggles rules on a live system.

## Fact catalogue

System (`g.*`): `g.os.family`, `g.sandbox.kind`, `g.sandbox.missing`, `g.sandbox.fixes`,
`g.servers.read`, `g.server.any`, `g.pulse.native`, `g.clock.rate`, `g.clock.allowed_rates`,
`g.clock.allowed_max`, `g.clock.quantum`, `g.pipewire.alsa_cards`, `g.pulse.alsa_cards`,
`g.routing.playback_kinds`, `g.routing.capture_kinds`.

Application (`app.*`, from `ProbeOptions::app` / `--app-*`): `app.endpoint`, `app.codec`,
`app.channels`, `app.rate`, `app.bits`, `app.latency_target_us`. With `app.endpoint` set, app
facts exist only on that endpoint.

Endpoint (`e.*`):

- identity: `e.class`, `e.direction`, `e.transport`
- driver: `e.jack.plugged`, `e.driver.module`, `e.driver.power_save`
- sink: `e.sink.present`, `e.sink.key`, `e.sink.changed`, `e.sink.codecs`, `e.sink.compressed`,
  `e.sink.atmos_codecs`, `e.sink.hbr_codecs`, `e.sink.max_channels`, `e.sink.max_rate`,
  `e.sink.max_bits`, `e.sink.downstream_known`, `e.sink.downstream_codecs`,
  `e.sink.direct_to_display`, `e.sink.earc_likely`
- link: `e.link.connection`, `e.link.connector_kind`, `e.link.max_tmds_mhz`
- host: `e.host.max_channels`, `e.host.max_rate`, `e.host.max_bits`, `e.host.hbr`, `e.usb.speed`
- ALSA: `e.alsa.state`, `e.alsa.max_channels`, `e.alsa.max_rate`, `e.alsa.max_bits`
- server: `e.server.present`, `e.server.apis`, `e.server.passthrough`, `e.server.max_channels`,
  `e.server.max_rate`, `e.server.max_bits`, `e.server.volume_percent`, `e.server.muted`,
  `e.server.format_bits`, `e.server.format_rate`, `e.server.format_channels`,
  `e.server.detected_known`, `e.server.detected_codecs`, `e.server.profile`
- Bluetooth: `e.bt.advertised`, `e.bt.available`, `e.bt.negotiated`, `e.bt.negotiated_rank`,
  `e.bt.best_common_rank`, `e.bt.ldac_quality`, `e.bt.profile`
- best of sink, ALSA and host: `e.hw.max_rate`, `e.hw.max_channels`, `e.hw.max_bits`
- effective set and path (preferred access path): `e.eff.max_channels`, `e.eff.max_rate`,
  `e.eff.max_bits`, `e.path.latency_us`

## Path reports

Hops, application first: application, sound server (server paths), ALSA device, driver/host, and
for HDMI/DP the link, the sink and (with CEC) the device behind it. Each hop carries the best LPCM
format the path delivers, its bit rate, a ceiling where known (USB bus speed, the HDMI audio
maximum of 8 × 192 kHz × 24 bit, the sink's own maximum), its latency (server quantum, sink sync
delay) and losses (resampling, downmix, software volume, dropped passthrough codecs). The
bottleneck is the hop of the layer that first narrowed channels or rate. End-to-end latency is the
sum of the hops that state one, marked inferred.
