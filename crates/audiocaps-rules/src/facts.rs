// SPDX-License-Identifier: LGPL-2.1-or-later
//! Facts: a snapshot flattened into named values that rules compare. Each fact keeps the
//! provenance and confidence of the model value it came from.
//!
//! Names: `g.*` system-wide, `e.*` the endpoint being checked, `app.*` what the application said it
//! will play. A fact that can't be known is simply absent; rules never fire on an absent fact.
//! The catalogue is documented in `docs/rules.md`.

use std::collections::BTreeMap;

use serde_json::{Value as J, json};

use audiocaps_model::{
    AccessPath, Availability, BluetoothCodec, CapabilitySet, Codec, CodecFlag, Confidence,
    DeviceState, Endpoint, LayerDetail, LayerKind, Provenance, RateSet, SampleFormat, Sandbox,
    Snapshot, SoundServer, Source, Transport, Value,
};

/// One fact.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    /// The value.
    pub value: J,
    /// Where it came from.
    pub provenance: Option<Provenance>,
    /// How sure it is.
    pub confidence: Confidence,
}

/// A set of facts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    map: BTreeMap<String, Fact>,
}

impl Facts {
    /// A fact by name.
    pub fn get(&self, name: &str) -> Option<&Fact> {
        self.map.get(name)
    }

    /// Sets a derived fact.
    pub fn set(&mut self, name: &str, value: J) {
        if !value.is_null() {
            self.map.insert(
                name.into(),
                Fact {
                    value,
                    provenance: None,
                    confidence: Confidence::Measured,
                },
            );
        }
    }

    /// Sets a fact from a model value; an unknown value leaves the fact absent.
    pub fn set_value<T>(&mut self, name: &str, v: &Value<T>, f: impl Fn(&T) -> J) {
        if let Value::Known {
            value,
            confidence,
            provenance,
        } = v
        {
            let j = f(value);
            if !j.is_null() {
                self.map.insert(
                    name.into(),
                    Fact {
                        value: j,
                        provenance: Some(provenance.clone()),
                        confidence: *confidence,
                    },
                );
            }
        }
    }

    /// Merges another set (later wins).
    pub fn extend(&mut self, other: &Facts) {
        for (k, v) in &other.map {
            self.map.insert(k.clone(), v.clone());
        }
    }

    /// The same facts without those whose name starts with `prefix`.
    pub fn without_prefix(&self, prefix: &str) -> Facts {
        Facts {
            map: self
                .map
                .iter()
                .filter(|(k, _)| !k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// All names (for documentation and tests).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }

    /// Builds facts from plain JSON (`{"e.sink.codecs": [...], …}`), for rule tests.
    pub fn from_json(j: &J) -> Self {
        let mut f = Facts::default();
        if let Some(m) = j.as_object() {
            for (k, v) in m {
                f.set(k, v.clone());
            }
        }
        f
    }
}

/// The model's snake_case name of a codec.
pub fn codec_name(c: &Codec) -> String {
    match serde_json::to_value(c) {
        Ok(J::String(s)) => s,
        Ok(J::Object(m)) => m
            .get("other")
            .and_then(J::as_str)
            .unwrap_or("other")
            .to_string(),
        _ => "other".into(),
    }
}

fn bt_name(c: &BluetoothCodec) -> String {
    match serde_json::to_value(c) {
        Ok(J::String(s)) => s,
        Ok(J::Object(m)) => m
            .get("other")
            .and_then(J::as_str)
            .unwrap_or("other")
            .to_string(),
        _ => "other".into(),
    }
}

/// Rank of a Bluetooth codec by audio quality (higher is better).
pub fn bt_rank(c: &BluetoothCodec) -> u8 {
    match c {
        BluetoothCodec::Cvsd => 0,
        BluetoothCodec::Msbc => 1,
        BluetoothCodec::Sbc => 2,
        BluetoothCodec::SbcXq | BluetoothCodec::Aac | BluetoothCodec::AptXLl => 3,
        BluetoothCodec::AptX | BluetoothCodec::Lc3 | BluetoothCodec::Opus => 4,
        BluetoothCodec::AptXHd => 5,
        BluetoothCodec::Ldac => 6,
        BluetoothCodec::Other(_) => 2,
    }
}

/// Bits per sample of a sample format.
pub fn format_bits(f: &SampleFormat) -> Option<u8> {
    Some(match f {
        SampleFormat::U8 => 8,
        SampleFormat::S16Le | SampleFormat::S16Be => 16,
        SampleFormat::S24Le | SampleFormat::S24_3Le => 24,
        SampleFormat::S32Le | SampleFormat::F32Le | SampleFormat::F64Le => 32,
        SampleFormat::Iec958SubframeLe => 24,
        SampleFormat::Other(_) => return None,
    })
}

/// Highest rate in a rate set.
pub fn max_rate(r: &RateSet) -> Option<u32> {
    match r {
        RateSet::Discrete { rates_hz } => rates_hz.iter().copied().max(),
        RateSet::Continuous { max_hz, .. } => Some(*max_hz),
    }
}

fn transport_name(t: &Transport) -> String {
    match t {
        Transport::HdmiDp => "hdmi_dp".into(),
        Transport::Analog => "analog".into(),
        Transport::Spdif => "spdif".into(),
        Transport::Usb => "usb".into(),
        Transport::Bluetooth => "bluetooth".into(),
        Transport::Network(_) => "network".into(),
        Transport::Virtual(_) => "virtual".into(),
        Transport::Other(_) => "other".into(),
    }
}

fn caps_facts(f: &mut Facts, prefix: &str, c: &CapabilitySet) {
    if let Some(v) = &c.channels {
        f.set_value(&format!("{prefix}.max_channels"), v, |r| json!(r.max));
    }
    if let Some(v) = &c.rates {
        f.set_value(&format!("{prefix}.max_rate"), v, |r| json!(max_rate(r)));
    }
    if let Some(v) = &c.sample_formats {
        f.set_value(&format!("{prefix}.max_bits"), v, |fs| {
            json!(fs.iter().filter_map(format_bits).max())
        });
    }
}

fn layer(e: &Endpoint, k: LayerKind) -> Option<&audiocaps_model::LayerCapabilities> {
    e.layers.iter().find(|l| l.layer == k)
}

/// System-wide facts.
pub fn global(s: &Snapshot) -> Facts {
    let mut f = Facts::default();
    if let Some(env) = &s.environment {
        f.set_value("g.os.family", &env.os, |o| json!(o.family));
        f.set_value("g.sandbox.kind", &env.sandbox, |sb| {
            json!(match sb {
                Sandbox::None => "none",
                Sandbox::Flatpak { .. } => "flatpak",
                Sandbox::Snap { .. } => "snap",
            })
        });
        f.set_value("g.sandbox.missing", &env.sandbox, |sb| match sb {
            Sandbox::Flatpak { missing, .. } | Sandbox::Snap { missing, .. } => {
                json!(missing.iter().map(|g| &g.permission).collect::<Vec<_>>())
            }
            Sandbox::None => json!([]),
        });
        if let Some(available) = env.realtime.as_ref().and_then(|r| r.available) {
            f.set("g.realtime.available", json!(available));
        }
        f.set_value("g.sandbox.fixes", &env.sandbox, |sb| match sb {
            Sandbox::Flatpak { missing, .. } | Sandbox::Snap { missing, .. } => {
                json!(missing.iter().map(|g| &g.fix).collect::<Vec<_>>())
            }
            Sandbox::None => json!([]),
        });
    }
    let read: Vec<&audiocaps_model::ServerState> = s
        .servers
        .iter()
        .filter(|x| x.availability == Availability::Read)
        .collect();
    if s.scope.layers.includes(&LayerKind::SoundServer) {
        f.set(
            "g.servers.read",
            json!(read.iter().map(|x| x.server).collect::<Vec<_>>()),
        );
        f.set("g.server.any", json!(!read.is_empty()));
    }
    for st in &read {
        if st.server == SoundServer::PulseAudio
            && let Some(Value::Known { value: i, .. }) = &st.info
        {
            f.set("g.pulse.native", json!(!i.name.contains("PipeWire")));
        }
        if st.server == SoundServer::PipeWire
            && let Some(c) = &st.clock
        {
            f.set_value("g.clock.rate", c, |c| json!(c.rate_hz));
            f.set_value("g.clock.allowed_rates", c, |c| {
                if c.allowed_rates_hz.is_empty() {
                    J::Null
                } else {
                    json!(c.allowed_rates_hz)
                }
            });
            f.set_value("g.clock.allowed_max", c, |c| {
                json!(c.allowed_rates_hz.iter().max().copied().or(c.rate_hz))
            });
            f.set_value("g.clock.quantum", c, |c| json!(c.quantum));
        }
    }
    // Cards each server exposes.
    for (api, name) in [
        (SoundServer::PipeWire, "g.pipewire.alsa_cards"),
        (SoundServer::PulseAudio, "g.pulse.alsa_cards"),
    ] {
        let mut cards: Vec<u32> = s
            .endpoints
            .iter()
            .filter(|e| server_views(e).any(|v| v.server == api))
            .filter_map(|e| alsa_card(&e.id.0))
            .collect();
        cards.sort_unstable();
        cards.dedup();
        if read.iter().any(|x| x.server == api) {
            f.set(name, json!(cards));
        }
    }
    if let Some(r) = &s.routing {
        f.set_value("g.routing.playback_kinds", &r.default_playback, |st| {
            json!(st.iter().map(|x| &x.kind).collect::<Vec<_>>())
        });
        f.set_value("g.routing.capture_kinds", &r.default_capture, |st| {
            json!(st.iter().map(|x| &x.kind).collect::<Vec<_>>())
        });
    }
    if let Some(a) = &s.app_request {
        if let Some(t) = &a.endpoint {
            f.set("app.endpoint", json!(t.0));
        }
        if let Some(c) = &a.codec {
            f.set("app.codec", json!(codec_name(c)));
        }
        f.set("app.channels", json!(a.channels));
        f.set("app.rate", json!(a.rate_hz));
        f.set("app.bits", json!(a.bits));
        f.set("app.latency_target_us", json!(a.latency_target_us));
    }
    f
}

fn alsa_card(id: &str) -> Option<u32> {
    let rest = id.strip_prefix("alsa:card")?;
    rest.split('/').next()?.parse().ok()
}

fn server_views(e: &Endpoint) -> impl Iterator<Item = &audiocaps_model::ServerView> {
    layer(e, LayerKind::SoundServer)
        .and_then(|l| match &l.detail {
            Some(LayerDetail::SoundServer(d)) => Some(d.views.iter()),
            _ => None,
        })
        .into_iter()
        .flatten()
}

/// Facts about one endpoint. `previous` is the same endpoint in the previous snapshot, if any.
pub fn endpoint(e: &Endpoint, previous: Option<&Endpoint>) -> Facts {
    let mut f = Facts::default();
    f.set("e.class", json!(e.class));
    f.set("e.direction", json!(e.direction));
    f.set_value("e.transport", &e.transport, |t| json!(transport_name(t)));

    // Driver: jack and power saving.
    if let Some(LayerDetail::KernelDriver(d)) =
        layer(e, LayerKind::KernelDriver).and_then(|l| l.detail.as_ref())
    {
        if let Some(j) = &d.jack {
            f.set_value("e.jack.plugged", j, |j| json!(j.plugged));
        }
        f.set_value("e.driver.module", &d.module, |m| json!(m));
        f.set_value("e.driver.power_save", &d.parameters, |ps| {
            ps.iter()
                .find(|p| p.name == "power_save")
                .and_then(|p| p.value.trim().parse::<i64>().ok())
                .map(|v| json!(v))
                .unwrap_or(J::Null)
        });
    }

    // Sink.
    if let Some(sink) = &e.sink {
        f.set("e.sink.present", json!(true));
        f.set_value("e.sink.key", &sink.identity.sink_key, |k| json!(k));
        f.set_value("e.sink.codecs", &sink.audio_descriptors, |v| {
            json!(v.iter().map(|c| codec_name(&c.codec)).collect::<Vec<_>>())
        });
        f.set_value("e.sink.compressed", &sink.audio_descriptors, |v| {
            json!(
                v.iter()
                    .filter(|c| c.codec != Codec::Lpcm)
                    .map(|c| codec_name(&c.codec))
                    .collect::<Vec<_>>()
            )
        });
        f.set_value("e.sink.atmos_codecs", &sink.audio_descriptors, |v| {
            json!(
                v.iter()
                    .filter(|c| c.flags.contains(&CodecFlag::Joc)
                        || c.flags.contains(&CodecFlag::Atmos))
                    .map(|c| codec_name(&c.codec))
                    .collect::<Vec<_>>()
            )
        });
        f.set_value("e.sink.hbr_codecs", &sink.audio_descriptors, |v| {
            json!(
                v.iter()
                    .filter(|c| c.flags.contains(&CodecFlag::NeedsHbr))
                    .map(|c| codec_name(&c.codec))
                    .collect::<Vec<_>>()
            )
        });
        let lpcm = |v: &Vec<audiocaps_model::CodecCapability>| {
            v.iter().find(|c| c.codec == Codec::Lpcm).cloned()
        };
        f.set_value("e.sink.max_channels", &sink.audio_descriptors, |v| {
            json!(lpcm(v).and_then(|c| c.max_channels))
        });
        f.set_value("e.sink.max_rate", &sink.audio_descriptors, |v| {
            json!(lpcm(v).and_then(|c| c.rates_hz.iter().copied().max()))
        });
        f.set_value("e.sink.max_bits", &sink.audio_descriptors, |v| {
            json!(lpcm(v).and_then(|c| c.bit_depths.iter().copied().max()))
        });
        match &sink.downstream {
            Value::Known { .. } => {
                f.set_value("e.sink.downstream_known", &sink.downstream, |_| json!(true));
            }
            Value::Unknown { .. } => f.set("e.sink.downstream_known", json!(false)),
        }
        if let Some(Value::Known {
            value: bus,
            provenance,
            confidence,
        }) = &sink.cec
        {
            let mut codecs: Vec<String> = bus
                .devices
                .iter()
                .filter(|d| d.device_type == "audio_system")
                .flat_map(|d| d.audio_descriptors.iter().map(|c| codec_name(&c.codec)))
                .collect();
            codecs.sort();
            codecs.dedup();
            if !codecs.is_empty() {
                f.map.insert(
                    "e.sink.downstream_codecs".into(),
                    Fact {
                        value: json!(codecs),
                        provenance: Some(provenance.clone()),
                        confidence: *confidence,
                    },
                );
            }
        }
        if let Some(link) = &sink.link {
            f.set_value("e.link.connection", link, |l| json!(l.connection));
            f.set_value("e.link.connector_kind", link, |l| {
                json!(l.connector.as_deref().map(|c| if c.contains("-DP-") {
                    "dp"
                } else if c.contains("HDMI") {
                    "hdmi"
                } else {
                    "other"
                }))
            });
            f.set_value("e.link.max_tmds_mhz", link, |l| json!(l.max_tmds_mhz));
            f.set_value("e.sink.direct_to_display", link, |l| {
                json!(l.physical_address.map(|pa| pa != 0 && pa & 0x0fff == 0))
            });
        }
        let direct = f
            .get("e.sink.direct_to_display")
            .map(|x| x.value == json!(true));
        let multi = f
            .get("e.sink.max_channels")
            .and_then(|x| x.value.as_u64())
            .is_some_and(|c| c > 2);
        let hd = f.get("e.sink.compressed").is_some_and(|x| {
            x.value
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c == "dts_hd" || c == "true_hd"))
        });
        if let Some(d) = direct {
            // A display reached directly that accepts multichannel or HD audio passes it on to
            // something (ARC/eARC receiver or soundbar): TV speakers are stereo.
            f.set("e.sink.earc_likely", json!(d && (multi || hd)));
        }
        if let (Some(prev), Some(now)) = (
            previous
                .and_then(|p| p.sink.as_ref())
                .and_then(|s| s.identity.sink_key.known()),
            sink.identity.sink_key.known(),
        ) {
            f.set("e.sink.changed", json!(prev != now));
        }
    } else if let Some(Availability::Unknown {
        detail: Some(d), ..
    }) = layer(e, LayerKind::SinkChain).map(|l| &l.availability)
        && d.contains("nothing connected")
    {
        f.set("e.sink.present", json!(false));
    }

    // Host hardware.
    if let Some(h) = layer(e, LayerKind::HostHardware) {
        caps_facts(&mut f, "e.host", &h.caps);
        if let Some(LayerDetail::HostHardware(d)) = &h.detail {
            // AMD/ATI HDMI codecs don't state HBR in the pin caps (the driver uses a vendor
            // verb), so a missing bit there says nothing.
            let ati = d
                .codec
                .as_ref()
                .and_then(|c| c.known())
                .is_some_and(|n| n.starts_with("ATI") || n.starts_with("AMD"));
            if let Some(p) = d.pins.as_ref().filter(|_| !ati) {
                f.set_value("e.host.hbr", p, |pins| {
                    let hdmi: Vec<_> = pins.iter().filter(|p| p.hdmi || p.display_port).collect();
                    if hdmi.is_empty() {
                        J::Null
                    } else {
                        json!(hdmi.iter().any(|p| p.hbr))
                    }
                });
            }
            if let Some(sp) = &d.usb_speed {
                f.set_value("e.usb.speed", sp, |s| json!(s));
            }
        }
    }

    // ALSA.
    if let Some(a) = layer(e, LayerKind::AlsaUserspace) {
        caps_facts(&mut f, "e.alsa", &a.caps);
        if let Some(LayerDetail::Device(d)) = &a.detail
            && let Some(st) = &d.state
        {
            f.set_value("e.alsa.state", st, |s| {
                json!(match s {
                    DeviceState::Idle => "idle",
                    DeviceState::InUse { .. } => "in_use",
                    DeviceState::ManagedByServer { .. } => "managed_by_server",
                })
            });
        }
    }

    // Sound server: the first view (native PipeWire before the PulseAudio API).
    if let Some(l) = layer(e, LayerKind::SoundServer) {
        let views: Vec<_> = server_views(e).collect();
        f.set("e.server.present", json!(!views.is_empty()));
        if !views.is_empty() {
            f.set(
                "e.server.apis",
                json!(views.iter().map(|v| v.server).collect::<Vec<_>>()),
            );
            caps_facts(&mut f, "e.server", &l.caps);
            if let Some(pt) = &l.caps.passthrough {
                f.set_value("e.server.passthrough", pt, |v| {
                    json!(v.iter().map(|c| codec_name(&c.codec)).collect::<Vec<_>>())
                });
            }
            if let Some(v) = views.iter().find_map(|v| v.volume.as_ref()) {
                f.set_value("e.server.volume_percent", v, |v| json!(v.percent));
                f.set_value("e.server.muted", v, |v| json!(v.muted));
            }
            if let Some(c) = views.iter().find_map(|v| v.current.as_ref()) {
                f.set_value("e.server.format_bits", c, |c| json!(format_bits(&c.format)));
                f.set_value("e.server.format_rate", c, |c| json!(c.rate_hz));
                f.set_value("e.server.format_channels", c, |c| json!(c.channels));
            }
            // Detected codecs on the active route (PipeWire).
            let mut detected: Option<Vec<String>> = None;
            for v in &views {
                if let Some(Value::Known { value: routes, .. }) = &v.routes {
                    for r in routes.iter().filter(|r| r.active) {
                        if let Some(d) = &r.detected_passthrough {
                            detected = Some(d.iter().map(codec_name).collect());
                        }
                    }
                }
            }
            f.set("e.server.detected_known", json!(detected.is_some()));
            if let Some(d) = detected {
                f.set("e.server.detected_codecs", json!(d));
            }
            if let Some(Value::Known { value: ps, .. }) =
                views.iter().find_map(|v| v.profiles.as_ref())
                && let Some(p) = ps.iter().find(|p| p.active)
            {
                f.set("e.server.profile", json!(p.name));
            }
        }
    }

    // Bluetooth.
    if let Some(w) = layer(e, LayerKind::Wireless) {
        if let Some(v) = &w.caps.bluetooth_codecs {
            f.set_value("e.bt.advertised", v, |cs| {
                json!(
                    cs.iter()
                        .filter(|c| c.advertised_by_device)
                        .map(|c| bt_name(&c.codec))
                        .collect::<Vec<_>>()
                )
            });
            f.set_value("e.bt.available", v, |cs| {
                json!(
                    cs.iter()
                        .filter(|c| c.available_on_host)
                        .map(|c| bt_name(&c.codec))
                        .collect::<Vec<_>>()
                )
            });
            f.set_value("e.bt.negotiated", v, |cs| {
                json!(
                    cs.iter()
                        .filter(|c| c.negotiated)
                        .map(|c| bt_name(&c.codec))
                        .collect::<Vec<_>>()
                )
            });
            f.set_value("e.bt.negotiated_rank", v, |cs| {
                json!(
                    cs.iter()
                        .filter(|c| c.negotiated)
                        .map(|c| bt_rank(&c.codec))
                        .max()
                )
            });
            f.set_value("e.bt.best_common_rank", v, |cs| {
                json!(
                    cs.iter()
                        .filter(|c| c.advertised_by_device && c.available_on_host)
                        .map(|c| bt_rank(&c.codec))
                        .max()
                )
            });
            f.set_value("e.bt.ldac_quality", v, |cs| {
                json!(
                    cs.iter()
                        .find(|c| c.codec == BluetoothCodec::Ldac && c.negotiated)
                        .and_then(|c| c.quality_mode.clone())
                )
            });
        }
        if let Some(LayerDetail::Wireless(d)) = &w.detail
            && let Some(t) = &d.transport
        {
            f.set_value("e.bt.profile", t, |t| json!(t.profile));
        }
    }

    // Hardware best: sink first (what the far end takes), then ALSA, then the host.
    for (k, order) in [
        (
            "max_rate",
            ["e.sink.max_rate", "e.alsa.max_rate", "e.host.max_rate"],
        ),
        (
            "max_channels",
            [
                "e.sink.max_channels",
                "e.alsa.max_channels",
                "e.host.max_channels",
            ],
        ),
        (
            "max_bits",
            ["e.sink.max_bits", "e.alsa.max_bits", "e.host.max_bits"],
        ),
    ] {
        if let Some(x) = order.iter().find_map(|n| f.get(n)).cloned() {
            f.map.insert(format!("e.hw.{k}"), x);
        }
    }

    // Effective sets and path reports, when computed.
    if let Some(eff) = e
        .effective
        .iter()
        .find(|x| x.access_path == preferred_path(e))
    {
        caps_facts(&mut f, "e.eff", &eff.caps);
    }
    if let Some(p) = e.paths.iter().find(|x| x.access_path == preferred_path(e)) {
        f.set_value("e.path.latency_us", &p.latency_us, |v| json!(v));
    }
    f
}

/// The access path facts describe: native PipeWire, else PulseAudio, else ALSA direct.
pub fn preferred_path(e: &Endpoint) -> AccessPath {
    let apis: Vec<SoundServer> = server_views(e).map(|v| v.server).collect();
    if apis.contains(&SoundServer::PipeWire) {
        AccessPath::PipeWire
    } else if apis.contains(&SoundServer::PulseAudio) {
        AccessPath::PulseAudio
    } else {
        AccessPath::AlsaDirect
    }
}

/// A provenance for facts derived by the engine.
pub fn derived(layer: LayerKind, at: u64, what: &str) -> Provenance {
    Provenance {
        layer,
        source: Source::derived(what.to_string()),
        read_at_ms: at,
    }
}
