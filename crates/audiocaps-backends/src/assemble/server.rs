// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sound-server assembly: server state, per-endpoint server views, and endpoints that exist only
//! in a server (Bluetooth, virtual and network sinks and sources).

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use audiocaps_model::{
    Availability, BluetoothCodec, BluetoothCodecState, CapabilitySet, ChannelMap, ClockSettings,
    Codec, CodecCapability, Confidence, CorrelationId, CorrelationKey, CurrentParams, Direction,
    Endpoint, EndpointClass, EndpointId, EndpointName, FieldGroup, LayerCapabilities, LayerDetail,
    LayerKind, Parameter, Policy, ProfileState, Range, RateSet, RouteState, SampleFormat,
    ServerDetail, ServerInfo, ServerState, ServerView, SoundServer, Source, Speaker, Transport,
    UnknownReason, Value, VolumeState,
};

use super::{Ctx, prov, unknown_layer};
use crate::probe::ServerResult;
use crate::server::{Device, Node, NodeDir, ServerDump, ServerKind};

/// One server's node, keyed to the endpoint it belongs to.
pub(crate) struct ViewSrc<'a> {
    pub dump: &'a ServerDump,
    pub node: &'a Node,
    pub live: bool,
}

/// Server nodes grouped by endpoint id.
#[derive(Default)]
pub(crate) struct ServerIndex<'a> {
    pub by_endpoint: BTreeMap<EndpointId, Vec<ViewSrc<'a>>>,
}

fn server_api(kind: ServerKind) -> SoundServer {
    match kind {
        ServerKind::PipeWire => SoundServer::PipeWire,
        ServerKind::PulseAudio => SoundServer::PulseAudio,
    }
}

/// Replaces Bluetooth addresses (six hex pairs joined by `_` or `:`) with a stable hash.
pub(crate) fn mask_bt(name: &str) -> String {
    let b = name.as_bytes();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < b.len() {
        if i + 17 <= b.len() && is_mac(&b[i..i + 17]) {
            let mac = name[i..i + 17].to_ascii_uppercase().replace('_', ":");
            let h = Sha256::digest(mac.as_bytes());
            out.push_str("bt-");
            for x in &h[..4] {
                out.push_str(&format!("{x:02x}"));
            }
            i += 17;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn is_mac(s: &[u8]) -> bool {
    s.iter().enumerate().all(|(i, c)| {
        if i % 3 == 2 {
            *c == b'_' || *c == b':'
        } else {
            c.is_ascii_hexdigit()
        }
    })
}

/// The endpoint id a server node belongs to: the ALSA PCM when it names one, else its own.
pub(crate) fn endpoint_of(n: &Node) -> Option<EndpointId> {
    let dir = n.dir?;
    if let (Some(c), Some(d), Some("alsa")) = (n.alsa_card, n.alsa_device, n.api.as_deref()) {
        let s = match dir {
            NodeDir::Sink => 'p',
            NodeDir::Source => 'c',
        };
        return Some(EndpointId(format!("alsa:card{c}/pcm{d}{s}")));
    }
    Some(EndpointId(format!("node:{}", mask_bt(&n.name))))
}

impl<'a> ServerIndex<'a> {
    pub fn new(pipewire: &'a ServerResult, pulse: &'a ServerResult) -> Self {
        let mut idx = ServerIndex::default();
        for r in [pipewire, pulse] {
            if let ServerResult::Read { dump, live } = r {
                for node in &dump.nodes {
                    if let Some(id) = endpoint_of(node) {
                        idx.by_endpoint.entry(id).or_default().push(ViewSrc {
                            dump,
                            node,
                            live: *live,
                        });
                    }
                }
            }
        }
        idx
    }
}

fn source(kind: ServerKind, live: bool, object: &str, key: &str) -> Source {
    match (kind, live) {
        (ServerKind::PipeWire, true) => Source::ServerParam {
            server: "pipewire".into(),
            object: object.into(),
            key: key.into(),
        },
        (ServerKind::PulseAudio, true) => Source::ServerParam {
            server: "pulseaudio".into(),
            object: object.into(),
            key: key.into(),
        },
        (ServerKind::PipeWire, false) => Source::command("pw-dump"),
        (ServerKind::PulseAudio, false) => Source::command("pactl-list"),
    }
}

/// Maps a server's sample-format name (pw-dump `S24_32LE`, PulseAudio `s24-32le`) to the model.
fn server_format(name: &str) -> SampleFormat {
    match name.to_ascii_lowercase().as_str() {
        "u8" => SampleFormat::U8,
        "s16le" => SampleFormat::S16Le,
        "s16be" => SampleFormat::S16Be,
        "s24_32le" | "s24-32le" => SampleFormat::S24Le,
        "s24le" => SampleFormat::S24_3Le,
        "s32le" => SampleFormat::S32Le,
        "f32le" | "float32le" => SampleFormat::F32Le,
        "f64le" => SampleFormat::F64Le,
        _ => SampleFormat::Other(name.to_string()),
    }
}

/// Maps a passthrough codec name (PipeWire `AC3`, PulseAudio `ac3-iec61937`) to the model.
fn codec(name: &str) -> Option<Codec> {
    let n = name.to_ascii_lowercase();
    let n = n.trim_end_matches("-iec61937");
    Some(match n {
        "pcm" => return None,
        "ac3" => Codec::Ac3,
        "eac3" => Codec::Eac3,
        "dts" => Codec::Dts,
        "dts-hd" | "dtshd" => Codec::DtsHd,
        "truehd" => Codec::TrueHd,
        "mpeg" => Codec::Mpeg1,
        "mpeg2-aac" => Codec::Aac,
        other => Codec::Other(other.to_string()),
    })
}

fn speaker(name: &str) -> Speaker {
    match name.to_ascii_lowercase().as_str() {
        "fl" | "front-left" => Speaker::Fl,
        "fr" | "front-right" => Speaker::Fr,
        "fc" | "front-center" | "mono" => Speaker::Fc,
        "lfe" | "lfe1" => Speaker::Lfe,
        "rl" | "rear-left" => Speaker::Rl,
        "rr" | "rear-right" => Speaker::Rr,
        "sl" | "side-left" => Speaker::Sl,
        "sr" | "side-right" => Speaker::Sr,
        "rc" | "rear-center" => Speaker::Rc,
        "flc" | "front-left-of-center" => Speaker::Flc,
        "frc" | "front-right-of-center" => Speaker::Frc,
        "tfl" | "top-front-left" => Speaker::Tfl,
        "tfr" | "top-front-right" => Speaker::Tfr,
        "tfc" | "top-front-center" => Speaker::Tfc,
        "tc" | "top-center" => Speaker::Tc,
        other => Speaker::Other(other.to_string()),
    }
}

fn bt_codec(name: &str) -> BluetoothCodec {
    match name.to_ascii_lowercase().replace('-', "_").as_str() {
        "sbc" => BluetoothCodec::Sbc,
        "sbc_xq" => BluetoothCodec::SbcXq,
        "aac" => BluetoothCodec::Aac,
        "aptx" => BluetoothCodec::AptX,
        "aptx_hd" => BluetoothCodec::AptXHd,
        "aptx_ll" | "aptx_ll_duplex" => BluetoothCodec::AptXLl,
        "ldac" => BluetoothCodec::Ldac,
        "lc3" | "lc3plus_hr" => BluetoothCodec::Lc3,
        "opus_05" | "opus_g" | "opus" => BluetoothCodec::Opus,
        "msbc" => BluetoothCodec::Msbc,
        "cvsd" => BluetoothCodec::Cvsd,
        other => BluetoothCodec::Other(other.to_string()),
    }
}

impl Ctx<'_> {
    fn device_of<'d>(&self, dump: &'d ServerDump, n: &Node) -> Option<&'d Device> {
        n.device_id
            .and_then(|id| dump.devices.iter().find(|d| d.id == id))
    }

    /// Snapshot-level server state.
    pub(crate) fn server_states(&self) -> Vec<ServerState> {
        let mut out = vec![];
        for (r, api) in [
            (&self.res.pipewire, SoundServer::PipeWire),
            (&self.res.pulse, SoundServer::PulseAudio),
        ] {
            let st = match r {
                ServerResult::NotRun => continue,
                ServerResult::Read { dump, live } => self.server_state(dump, *live),
                ServerResult::Skipped { reason, detail } => blank(api, *reason, detail.clone()),
                ServerResult::Failed(e) => blank(api, UnknownReason::ReadError, e.clone()),
                ServerResult::TimedOut => blank(
                    api,
                    UnknownReason::Timeout,
                    "server backend deadline".into(),
                ),
            };
            out.push(st);
        }
        out
    }

    fn server_state(&self, d: &ServerDump, live: bool) -> ServerState {
        let p = |key: &str| {
            prov(
                LayerKind::SoundServer,
                source(d.kind, live, &d.name, key),
                self.now,
            )
        };
        let endpoint = |name: &Option<String>| {
            name.as_ref().map(|n| {
                let id = d
                    .nodes
                    .iter()
                    .find(|x| &x.name == n)
                    .and_then(endpoint_of)
                    .map(|e| e.0)
                    .unwrap_or_else(|| format!("node:{}", mask_bt(n)));
                Value::measured(id, p("default"))
            })
        };
        let routing = self.field(FieldGroup::Routing);
        ServerState {
            server: server_api(d.kind),
            availability: Availability::Read,
            info: Some(Value::measured(
                ServerInfo {
                    name: d.name.clone(),
                    version: d.version.clone(),
                },
                p("info"),
            )),
            clock: d
                .clock
                .as_ref()
                .filter(|_| self.field(FieldGroup::Rates) || self.field(FieldGroup::Latency))
                .map(|c| {
                    Value::measured(
                        ClockSettings {
                            rate_hz: c.rate,
                            allowed_rates_hz: c.allowed_rates.clone(),
                            quantum: c.quantum,
                            min_quantum: c.min_quantum,
                            max_quantum: c.max_quantum,
                            force_rate_hz: c.force_rate,
                            force_quantum: c.force_quantum,
                        },
                        p("settings"),
                    )
                }),
            default_sink: if routing {
                endpoint(&d.default_sink)
            } else {
                None
            },
            default_source: if routing {
                endpoint(&d.default_source)
            } else {
                None
            },
            policy: (routing && (d.session_manager.is_some() || !d.policy.is_empty())).then(|| {
                Value::measured(
                    Policy {
                        manager: d.session_manager.clone(),
                        settings: d
                            .policy
                            .iter()
                            .map(|(k, v)| Parameter {
                                name: k.clone(),
                                value: v.clone(),
                            })
                            .collect(),
                    },
                    p("sm-settings"),
                )
            }),
            ports: None,
        }
    }

    /// The sound-server layer of an endpoint.
    pub(crate) fn server_layer(&self, id: &EndpointId) -> LayerCapabilities {
        let layer = LayerKind::SoundServer;
        let views = self.servers.by_endpoint.get(id);
        let Some(views) = views.filter(|v| !v.is_empty()) else {
            let availability = match (&self.res.pipewire, &self.res.pulse) {
                (ServerResult::TimedOut, _) | (_, ServerResult::TimedOut) => {
                    unknown_layer(UnknownReason::Timeout, "server backend deadline")
                }
                (ServerResult::Read { .. }, _) | (_, ServerResult::Read { .. }) => unknown_layer(
                    UnknownReason::NotExposed,
                    "no sound server exposes this device (profile off, or unmanaged)",
                ),
                (ServerResult::Skipped { reason, detail }, _) => {
                    unknown_layer(*reason, detail.clone())
                }
                (ServerResult::Failed(e), _) => unknown_layer(UnknownReason::ReadError, e.clone()),
                _ => unknown_layer(UnknownReason::ServiceNotRunning, "no sound server"),
            };
            return LayerCapabilities {
                layer,
                availability,
                caps: CapabilitySet::default(),
                detail: None,
            };
        };
        // Native PipeWire first: it states the device's real formats; pipewire-pulse mirrors them.
        let primary = &views[0];
        let caps = self.server_caps(primary);
        let detail =
            (self.field(FieldGroup::Routing) || self.field(FieldGroup::Passthrough)).then(|| {
                LayerDetail::SoundServer(ServerDetail {
                    views: views.iter().map(|v| self.view(v)).collect(),
                })
            });
        LayerCapabilities {
            layer,
            availability: Availability::Read,
            caps,
            detail,
        }
    }

    fn server_caps(&self, v: &ViewSrc<'_>) -> CapabilitySet {
        let n = v.node;
        let p = prov(
            LayerKind::SoundServer,
            source(v.dump.kind, v.live, &mask_bt(&n.name), "EnumFormat"),
            self.now,
        );
        let raw: Vec<_> = n
            .enum_formats
            .iter()
            .filter(|f| f.subtype == "raw" || f.subtype == "dsp")
            .collect();
        let mut c = CapabilitySet::default();
        if self.field(FieldGroup::Formats) && !raw.is_empty() {
            let mut f: Vec<SampleFormat> = Vec::new();
            for x in raw.iter().flat_map(|r| r.formats.iter()) {
                let s = server_format(x);
                if !f.contains(&s) {
                    f.push(s);
                }
            }
            c.sample_formats = Some(Value::measured(f, p.clone()));
        }
        if self.field(FieldGroup::Rates) && !raw.is_empty() {
            let range = raw.iter().find_map(|r| r.rate_range);
            let set = match range {
                Some((lo, hi)) => RateSet::Continuous {
                    min_hz: lo,
                    max_hz: hi,
                },
                None => {
                    let mut r: Vec<u32> = raw.iter().flat_map(|x| x.rates.clone()).collect();
                    r.sort_unstable();
                    r.dedup();
                    RateSet::Discrete { rates_hz: r }
                }
            };
            c.rates = Some(Value::measured(set, p.clone()));
        }
        if self.field(FieldGroup::Channels)
            && let Some((lo, hi)) = raw
                .iter()
                .filter_map(|r| r.channels)
                .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        {
            c.channels = Some(Value::measured(Range { min: lo, max: hi }, p.clone()));
        }
        if self.field(FieldGroup::ChannelMaps) {
            let maps: Vec<ChannelMap> = raw
                .iter()
                .filter(|r| !r.positions.is_empty())
                .map(|r| ChannelMap {
                    positions: r.positions.iter().map(|s| speaker(s)).collect(),
                })
                .collect();
            if !maps.is_empty() {
                c.channel_maps = Some(Value::measured(maps, p.clone()));
            }
        }
        if self.field(FieldGroup::Passthrough) {
            c.passthrough = Some(match &n.iec958_codecs {
                Some(list) => Value::measured(
                    list.iter()
                        .filter_map(|x| codec(x))
                        .map(|codec| CodecCapability {
                            codec,
                            max_channels: None,
                            rates_hz: vec![],
                            bit_depths: vec![],
                            max_bitrate_kbps: None,
                            flags: vec![],
                        })
                        .collect(),
                    prov(
                        LayerKind::SoundServer,
                        source(v.dump.kind, v.live, &mask_bt(&n.name), "iec958Codecs"),
                        self.now,
                    ),
                ),
                None => Value::unknown(
                    UnknownReason::NotExposed,
                    Some("the server lists no passthrough formats for this node".into()),
                ),
            });
        }
        if self.field(FieldGroup::BluetoothCodecs) && n.api.as_deref() == Some("bluez5") {
            c.bluetooth_codecs = Some(self.bt_codecs(v));
        }
        c
    }

    /// Bluetooth codecs from the device's per-codec profiles (`a2dp-sink-ldac`) and the node's
    /// negotiated codec (`api.bluez5.codec`).
    pub(crate) fn bt_codecs(&self, v: &ViewSrc<'_>) -> Value<Vec<BluetoothCodecState>> {
        let n = v.node;
        let dev = self.device_of(v.dump, n);
        let negotiated = n.props.get("api.bluez5.codec").map(|c| bt_codec(c));
        let mut out: Vec<BluetoothCodecState> = Vec::new();
        for p in dev.map(|d| d.profiles.as_slice()).unwrap_or_default() {
            let Some(name) = p
                .name
                .strip_prefix("a2dp-sink-")
                .or_else(|| p.name.strip_prefix("headset-head-unit-"))
            else {
                continue;
            };
            let c = bt_codec(name);
            if out.iter().any(|x| x.codec == c) {
                continue;
            }
            out.push(BluetoothCodecState {
                negotiated: negotiated.as_ref() == Some(&c),
                codec: c,
                advertised_by_device: p.available != Some(false),
                available_on_host: true,
                quality_mode: None,
            });
        }
        if let Some(c) = negotiated
            && !out.iter().any(|x| x.codec == c)
        {
            out.push(BluetoothCodecState {
                codec: c,
                advertised_by_device: true,
                available_on_host: true,
                negotiated: true,
                quality_mode: None,
            });
        }
        if let Some(q) = n.props.get("bluez5.a2dp.ldac.quality")
            && let Some(l) = out.iter_mut().find(|x| x.codec == BluetoothCodec::Ldac)
        {
            l.quality_mode = Some(q.clone());
        }
        Value::measured(
            out,
            prov(
                LayerKind::SoundServer,
                source(v.dump.kind, v.live, &mask_bt(&n.name), "EnumProfile"),
                self.now,
            ),
        )
    }

    fn view(&self, v: &ViewSrc<'_>) -> ServerView {
        let n = v.node;
        let obj = mask_bt(&n.name);
        let p = |key: &str| {
            prov(
                LayerKind::SoundServer,
                source(v.dump.kind, v.live, &obj, key),
                self.now,
            )
        };
        let routing = self.field(FieldGroup::Routing);
        let dev = self.device_of(v.dump, n);
        let want_dir = match n.dir {
            Some(NodeDir::Sink) => "output",
            Some(NodeDir::Source) => "input",
            None => "",
        };
        ServerView {
            server: server_api(v.dump.kind),
            object: obj.clone(),
            media_class: n.media_class.clone(),
            state: routing
                .then(|| n.state.clone().map(|s| Value::measured(s, p("state"))))
                .flatten(),
            current: routing
                .then(|| {
                    n.format.as_ref().filter(|f| f.rate > 0).map(|f| {
                        Value::measured(
                            CurrentParams {
                                format: if f.subtype == "iec958" {
                                    SampleFormat::Other(format!("iec958:{}", f.format))
                                } else {
                                    server_format(&f.format)
                                },
                                channels: f.channels,
                                rate_hz: f.rate,
                                period_frames: None,
                                buffer_frames: None,
                            },
                            p("Format"),
                        )
                    })
                })
                .flatten(),
            profiles: routing
                .then(|| {
                    dev.map(|d| {
                        Value::measured(
                            d.profiles
                                .iter()
                                .map(|x| ProfileState {
                                    name: x.name.clone(),
                                    description: x.description.clone(),
                                    available: x.available,
                                    active: d.active_profile.as_deref() == Some(x.name.as_str()),
                                })
                                .collect(),
                            p("EnumProfile"),
                        )
                    })
                })
                .flatten(),
            routes: routing
                .then(|| {
                    dev.map(|d| {
                        Value::measured(
                            d.routes
                                .iter()
                                .filter(|r| r.direction == want_dir)
                                .map(|r| RouteState {
                                    name: r.name.clone(),
                                    description: r.description.clone(),
                                    available: r.available,
                                    active: r.active,
                                    detected_passthrough: r
                                        .detected_codecs
                                        .as_ref()
                                        .map(|v| v.iter().filter_map(|c| codec(c)).collect()),
                                })
                                .collect(),
                            p("EnumRoute"),
                        )
                    })
                })
                .flatten(),
            passthrough: None,
            volume: routing
                .then(|| {
                    n.volume_percent.zip(n.muted).map(|(percent, muted)| {
                        Value::measured(VolumeState { percent, muted }, p("Props"))
                    })
                })
                .flatten(),
            is_default: routing.then(|| {
                v.dump.default_sink.as_deref() == Some(n.name.as_str())
                    || v.dump.default_source.as_deref() == Some(n.name.as_str())
            }),
        }
    }

    /// Endpoints that exist only in a server: Bluetooth, virtual and network nodes, and ALSA PCMs
    /// discovery couldn't see (e.g. a sandbox without `/proc/asound`).
    pub(crate) fn server_only_endpoints(&self, known: &[EndpointId]) -> Vec<Endpoint> {
        let mut out = vec![];
        for (id, views) in &self.servers.by_endpoint {
            if known.contains(id) {
                continue;
            }
            let n = views[0].node;
            let direction = match n.dir {
                Some(NodeDir::Source) => Direction::Capture,
                _ => Direction::Playback,
            };
            let factory = n.factory.clone().unwrap_or_default();
            let (class, transport) = if n.api.as_deref() == Some("bluez5") {
                (EndpointClass::Bluetooth, Transport::Bluetooth)
            } else if n.api.as_deref() == Some("alsa") {
                (EndpointClass::Hardware, Transport::Other("alsa".into()))
            } else if let Some(proto) = ["raop", "rtp", "snapcast", "roc", "tunnel", "zeroconf"]
                .iter()
                .find(|p| factory.contains(*p) || n.name.contains(*p))
            {
                (
                    EndpointClass::Network,
                    Transport::Network((*proto).to_string()),
                )
            } else {
                let kind = if factory.is_empty() {
                    "virtual".to_string()
                } else {
                    factory.clone()
                };
                (EndpointClass::Virtual, Transport::Virtual(kind))
            };
            if !self.scope.includes_endpoint(class, direction, id) {
                continue;
            }
            let src = || {
                prov(
                    LayerKind::SoundServer,
                    source(
                        views[0].dump.kind,
                        views[0].live,
                        &mask_bt(&n.name),
                        "props",
                    ),
                    self.now,
                )
            };
            let identity = self.field(FieldGroup::Identity);
            let mut layers = vec![];
            if self.layer(LayerKind::SoundServer) {
                layers.push(self.server_layer(id));
            }
            let bt_address = n.props.get("api.bluez5.address").cloned();
            if class == EndpointClass::Bluetooth && self.layer(LayerKind::Wireless) {
                let host = self.bt_codecs(&views[0]);
                layers.push(
                    self.wireless_layer(bt_address.as_deref(), host.known().map(|v| v.as_slice())),
                );
            }
            let mut correlation = vec![];
            if identity {
                if let Some(a) = &bt_address {
                    correlation.push(super::wireless::bluez_key(a));
                }
                for v in views {
                    let name = mask_bt(&v.node.name);
                    correlation.push(CorrelationKey {
                        id: match v.dump.kind {
                            ServerKind::PipeWire => CorrelationId::PipewireNode { name },
                            ServerKind::PulseAudio => CorrelationId::PulseDevice { name },
                        },
                        confidence: Confidence::Measured,
                    });
                }
            }
            out.push(Endpoint {
                id: id.clone(),
                class,
                direction,
                transport: Value::measured(transport, src()),
                name: identity.then(|| {
                    let name = if n.description.is_empty() {
                        mask_bt(&n.name)
                    } else {
                        mask_bt(&n.description)
                    };
                    // A Bluetooth node's description is the device alias the user chose.
                    let name = if class == EndpointClass::Bluetooth {
                        EndpointName::Personal(audiocaps_model::Sensitive::new(
                            audiocaps_model::SensitiveKind::DeviceName,
                            name,
                        ))
                    } else {
                        EndpointName::Plain(name)
                    };
                    Value::measured(name, src())
                }),
                correlation,
                sink: None,
                midi: None,
                layers,
                effective: vec![],
                paths: vec![],
            });
        }
        out
    }

    /// Bluetooth addresses of server nodes (to skip them in the BlueZ-only list).
    pub(crate) fn server_bt_addresses(&self) -> Vec<String> {
        self.servers
            .by_endpoint
            .values()
            .flat_map(|v| v.iter())
            .filter_map(|v| v.node.props.get("api.bluez5.address").cloned())
            .collect()
    }

    /// Correlation keys for an ALSA endpoint's server nodes.
    pub(crate) fn server_correlation(&self, id: &EndpointId) -> Vec<CorrelationKey> {
        self.servers
            .by_endpoint
            .get(id)
            .map(|views| {
                views
                    .iter()
                    .map(|v| CorrelationKey {
                        id: match v.dump.kind {
                            ServerKind::PipeWire => CorrelationId::PipewireNode {
                                name: mask_bt(&v.node.name),
                            },
                            ServerKind::PulseAudio => CorrelationId::PulseDevice {
                                name: mask_bt(&v.node.name),
                            },
                        },
                        confidence: Confidence::Measured,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn blank(server: SoundServer, reason: UnknownReason, detail: String) -> ServerState {
    ServerState {
        server,
        availability: Availability::Unknown {
            reason,
            detail: Some(detail),
        },
        info: None,
        clock: None,
        default_sink: None,
        default_source: None,
        policy: None,
        ports: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_bluetooth_addresses() {
        // Built at runtime so no address literal sits in the repository.
        let pairs = ["0f", "1e", "2d", "3c", "4b", "5a"];
        let under = format!("bluez_output.{}.1", pairs.join("_").to_ascii_uppercase());
        let colon = format!("bluez_output.{}.1", pairs.join(":"));
        let m = mask_bt(&under);
        assert!(
            m.starts_with("bluez_output.bt-") && m.ends_with(".1"),
            "{m}"
        );
        assert!(!m.contains(&pairs[1..3].join("_").to_ascii_uppercase()));
        assert_eq!(m, mask_bt(&colon));
        assert_eq!(
            mask_bt("alsa_output.pci-0000_00_1f.3"),
            "alsa_output.pci-0000_00_1f.3"
        );
    }

    #[test]
    fn codec_names() {
        assert_eq!(codec("AC3"), Some(Codec::Ac3));
        assert_eq!(codec("truehd-iec61937"), Some(Codec::TrueHd));
        assert_eq!(codec("pcm"), None);
        assert_eq!(server_format("s24-32le"), SampleFormat::S24Le);
        assert_eq!(server_format("S24LE"), SampleFormat::S24_3Le);
    }
}
