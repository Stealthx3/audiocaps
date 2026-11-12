// SPDX-License-Identifier: LGPL-2.1-or-later
//! Assembly: turns what the backends read into the model. Every value gets provenance; anything
//! in scope that wasn't read is `unknown` with the reason (timeout, busy, managed by a server,
//! not exposed, not implemented yet). Field groups outside the scope are omitted.

use audiocaps_model::{
    Availability, CapabilitySet, ChannelMap, Confidence, CorrelationId, CorrelationKey,
    CurrentParams, DeviceDetail, DeviceState, Direction, DriverDetail, Endpoint, EndpointClass,
    EndpointName, FieldGroup, HardwareDetail, Iec958State, JackState, LayerCapabilities,
    LayerDetail, LayerKind, MidiKind, MidiPort, PinInfo, ProbeScope, Provenance, Range, RateSet,
    RedactionPolicy, RouteStage, Routing, SCHEMA_VERSION, SampleFormat, Snapshot, SnapshotSource,
    Source, Speaker, Subdevices, SystemLayer, Transport, UnknownReason, UsbAltSetting, Value,
};
use audiocaps_parse::alsa_conf::{self, CardRef, Stream};
use audiocaps_parse::hda::{DefaultDevice, HdaCodec, Node, PortConnectivity};
use audiocaps_parse::procfs::{CardEntry, StreamDirection, UsbStream};

use crate::alsa::dump::{ControlDump, HwRanges, NotOpened, PcmResult, StreamDir};
mod server;
mod sink;
mod wireless;

use crate::probe::{Found, Kind, PROC_PCM, Results, ServerResult};
use crate::root::SystemRoot;
use crate::sysfs::{CardData, Dir};
use server::ServerIndex;

/// Builds the snapshot.
pub(crate) fn snapshot(
    root: &SystemRoot,
    scope: &ProbeScope,
    now: u64,
    cards: &[CardEntry],
    found: Vec<Found>,
    res: Results,
) -> Snapshot {
    let source = match root {
        SystemRoot::Live => SnapshotSource::Live,
        SystemRoot::Fixture(f) => SnapshotSource::Fixture {
            name: f.name().to_string(),
        },
    };
    let ctx = Ctx {
        servers: ServerIndex::new(&res.pipewire, &res.pulse),
        root,
        scope,
        now,
        cards,
        res: &res,
    };
    let layers = LayerKind::ALL
        .iter()
        .filter(|l| scope.layers.includes(l))
        .map(|&layer| SystemLayer {
            layer,
            availability: match ctx.system_availability(root, layer) {
                Availability::Unknown { reason, detail } => Availability::Unknown {
                    reason,
                    detail: Some(ctx.with_fix(layer, detail.as_deref().unwrap_or(""))),
                },
                a => a,
            },
        })
        .collect();
    let environment = if scope.layers.includes(&LayerKind::Environment) {
        res.environment.clone().or_else(|| {
            res.env_timed_out.then(|| audiocaps_model::EnvironmentInfo {
                os: timeout(),
                kernel: timeout(),
                sandbox: timeout(),
                servers: vec![],
            })
        })
    } else {
        None
    };
    let routing = (scope.fields.includes(&FieldGroup::Routing)
        && scope.layers.includes(&LayerKind::AlsaUserspace))
    .then(|| ctx.routing());
    let mut endpoints: Vec<Endpoint> = found.iter().map(|f| ctx.endpoint(f)).collect();
    let known: Vec<_> = found.iter().map(|f| f.id.clone()).collect();
    endpoints.extend(ctx.server_only_endpoints(&known));
    endpoints.extend(ctx.bt_only_endpoints(&ctx.server_bt_addresses()));
    endpoints.sort_by(|a, b| a.id.cmp(&b.id));
    let servers = if scope.layers.includes(&LayerKind::SoundServer) {
        let mut v = ctx.server_states();
        v.extend(ctx.jack_state());
        v
    } else {
        vec![]
    };
    let environment = environment.map(|mut e| {
        ctx.live_presence(&mut e);
        ctx.wireless_environment(&mut e);
        e
    });
    let mut snapshot = Snapshot {
        schema_version: SCHEMA_VERSION.to_string(),
        generation: 1,
        taken_at_ms: now,
        source,
        scope: scope.clone(),
        redaction: RedactionPolicy::keep_all(),
        layers,
        environment,
        routing,
        servers,
        endpoints,
        findings: vec![],
    };
    snapshot.redact(&RedactionPolicy::default());
    snapshot
}

type Chains = (alsa_conf::Chain, alsa_conf::Chain);

static ROUTING_CACHE: std::sync::Mutex<Option<(u64, Chains)>> = std::sync::Mutex::new(None);

/// Resolves the `default` chains, cached by configuration text and cards (parsing the
/// configuration and the per-card files is the slowest part of a probe).
fn resolve_cached(text: &str, cards: &[CardRef], root: &SystemRoot) -> Result<Chains, String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    for c in cards {
        (c.index, &c.id, &c.driver, &c.name).hash(&mut h);
    }
    if let SystemRoot::Fixture(f) = root {
        f.name().hash(&mut h);
    }
    let key = h.finish();
    let mut cache = ROUTING_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, c)) = cache.as_ref()
        && *k == key
    {
        return Ok(c.clone());
    }
    let mut tree = alsa_conf::parse(text).map_err(|e| format!("{e:?}"))?;
    // alsa-lib loads the per-card definitions lazily; build them from the same files.
    let loader = |p: &str| root.read_string(p).ok();
    alsa_conf::load_cards(&mut tree, cards, &loader);
    let chains = (
        alsa_conf::resolve_pcm(&tree, "default", Stream::Playback, cards),
        alsa_conf::resolve_pcm(&tree, "default", Stream::Capture, cards),
    );
    *cache = Some((key, chains.clone()));
    Ok(chains)
}

fn timeout<T>() -> Value<T> {
    Value::unknown(
        UnknownReason::Timeout,
        Some("the backend did not finish before its deadline".to_string()),
    )
}

fn unknown_layer(reason: UnknownReason, detail: impl Into<String>) -> Availability {
    Availability::Unknown {
        reason,
        detail: Some(detail.into()),
    }
}

fn prov(layer: LayerKind, source: Source, at: u64) -> Provenance {
    Provenance {
        layer,
        source,
        read_at_ms: at,
    }
}

struct Ctx<'a> {
    servers: ServerIndex<'a>,
    root: &'a SystemRoot,
    scope: &'a ProbeScope,
    now: u64,
    cards: &'a [CardEntry],
    res: &'a Results,
}

impl Ctx<'_> {
    fn field(&self, f: FieldGroup) -> bool {
        self.scope.fields.includes(&f)
    }

    fn layer(&self, l: LayerKind) -> bool {
        self.scope.layers.includes(&l)
    }

    fn card_id(&self, index: u32) -> String {
        self.cards
            .iter()
            .find(|c| c.index == index)
            .map(|c| c.id.clone())
            .unwrap_or_else(|| index.to_string())
    }

    /// Source for values from the ALSA dump: the alsa-lib call live, the captured output otherwise.
    fn alsa_source(&self, call: String) -> Source {
        if self.res.alsa_live {
            Source::api("alsa-lib", call)
        } else {
            Source::command(crate::alsa::dump::CAPTURE_NAME)
        }
    }

    fn system_availability(&self, root: &SystemRoot, layer: LayerKind) -> Availability {
        let readable = |p: &str| match root.read_dir(p) {
            Ok(_) => Availability::Read,
            Err(e) => io_unknown(&e, p),
        };
        match layer {
            LayerKind::HostHardware => readable("/sys/class/sound"),
            LayerKind::KernelDriver => readable("/sys/module"),
            LayerKind::AlsaUserspace => match root.read(PROC_PCM) {
                Ok(_) if self.res.alsa.is_some() || self.res.alsa_live => Availability::Read,
                Ok(_) => Availability::Partial,
                Err(e) => io_unknown(&e, PROC_PCM),
            },
            LayerKind::Environment => Availability::Partial,
            LayerKind::SinkChain => {
                if crate::probe::sink_wanted(self.scope) {
                    Availability::Partial
                } else {
                    unknown_layer(
                        UnknownReason::NotRequested,
                        "the sink field group is not in scope",
                    )
                }
            }
            LayerKind::SoundServer => {
                let rs = [&self.res.pipewire, &self.res.pulse];
                if rs.iter().any(|r| matches!(r, ServerResult::Read { .. })) {
                    Availability::Read
                } else if rs.iter().any(|r| matches!(r, ServerResult::TimedOut)) {
                    unknown_layer(UnknownReason::Timeout, "server backend deadline")
                } else {
                    match rs.iter().find_map(|r| match r {
                        ServerResult::Skipped { reason, detail } => Some((*reason, detail.clone())),
                        ServerResult::Failed(e) => Some((UnknownReason::ReadError, e.clone())),
                        _ => None,
                    }) {
                        Some((r, d)) => unknown_layer(r, d),
                        None => unknown_layer(UnknownReason::ServiceNotRunning, "no sound server"),
                    }
                }
            }
            LayerKind::Wireless => match &self.res.bluez {
                Some(crate::probe::Outcome::Read { .. }) => Availability::Read,
                Some(crate::probe::Outcome::Unknown { reason, detail }) => {
                    unknown_layer(*reason, detail.clone())
                }
                None => unknown_layer(UnknownReason::NotRequested, "not in scope"),
            },
        }
    }

    /// On the live system, server presence comes from the server backends that ran.
    fn live_presence(&self, env: &mut audiocaps_model::EnvironmentInfo) {
        use audiocaps_model::{ServerInfo, ServerPresence, SoundServer};
        let mut live = vec![];
        for (r, api) in [
            (&self.res.pipewire, SoundServer::PipeWire),
            (&self.res.pulse, SoundServer::PulseAudio),
        ] {
            if let ServerResult::Read { dump, live: true } = r {
                let p = prov(
                    LayerKind::SoundServer,
                    Source::ServerParam {
                        server: format!("{api:?}").to_ascii_lowercase(),
                        object: dump.name.clone(),
                        key: "info".into(),
                    },
                    self.now,
                );
                live.push(ServerPresence {
                    server: api,
                    info: Value::measured(
                        ServerInfo {
                            name: dump.name.clone(),
                            version: dump.version.clone(),
                        },
                        p.clone(),
                    ),
                });
                if let Some(sm) = &dump.session_manager {
                    live.push(ServerPresence {
                        server: SoundServer::WirePlumber,
                        info: Value::measured(
                            ServerInfo {
                                name: sm.clone(),
                                version: None,
                            },
                            p,
                        ),
                    });
                }
            }
        }
        if !live.is_empty() {
            env.servers = live;
        }
    }

    // -----------------------------------------------------------------------------------------
    // routing

    fn routing(&self) -> Routing {
        let missing = || -> Option<Value<Vec<RouteStage>>> {
            if self.res.alsa_timed_out && self.res.alsa.as_ref().is_none_or(|d| d.config.is_none())
            {
                return Some(timeout());
            }
            match &self.res.alsa {
                None => Some(Value::unknown(
                    UnknownReason::NotExposed,
                    self.res.alsa_missing.clone(),
                )),
                Some(d) if d.config.is_none() => Some(Value::unknown(
                    UnknownReason::ReadError,
                    d.config_error
                        .clone()
                        .or_else(|| Some("the ALSA configuration was not read".into())),
                )),
                Some(_) => None,
            }
        };
        if let Some(v) = missing() {
            return Routing {
                default_playback: v.clone(),
                default_capture: v,
            };
        }
        let text = self
            .res
            .alsa
            .as_ref()
            .and_then(|d| d.config.as_deref())
            .unwrap_or("");
        let cards: Vec<CardRef> = self
            .cards
            .iter()
            .map(|c| CardRef {
                index: c.index,
                id: c.id.clone(),
                driver: c.driver.clone(),
                name: c.name.clone(),
            })
            .collect();
        let chains = match resolve_cached(text, &cards, self.root) {
            Ok(c) => c,
            Err(e) => {
                let v = Value::unknown(UnknownReason::ParseError, Some(e));
                return Routing {
                    default_playback: v.clone(),
                    default_capture: v,
                };
            }
        };
        let chain = |c: &alsa_conf::Chain| {
            let mut stages: Vec<RouteStage> = c
                .stages
                .iter()
                .map(|s| RouteStage {
                    name: s.name.clone(),
                    kind: s.kind.clone(),
                    card: s.card.clone(),
                    device: s.device,
                    rate_hz: s.rate,
                })
                .collect();
            if let Some(why) = &c.stopped {
                stages.push(RouteStage {
                    name: why.clone(),
                    kind: "unresolved".into(),
                    card: None,
                    device: None,
                    rate_hz: None,
                });
            }
            Value::inferred(
                stages,
                prov(
                    LayerKind::AlsaUserspace,
                    self.alsa_source("snd_config_update".into()),
                    self.now,
                ),
            )
        };
        Routing {
            default_playback: chain(&chains.0),
            default_capture: chain(&chains.1),
        }
    }

    // -----------------------------------------------------------------------------------------
    // endpoints

    fn endpoint(&self, f: &Found) -> Endpoint {
        match f.kind {
            Kind::Pcm { card, device, dir } => self.pcm_endpoint(f, card, device, dir),
            Kind::RawMidi { card, device } => self.raw_midi_endpoint(f, card, device),
            Kind::Seq { .. } => self.seq_endpoint(f),
        }
    }

    fn card_data(&self, card: u32) -> Option<&CardData> {
        self.res.cards.get(&card)
    }

    fn pcm_endpoint(&self, f: &Found, card: u32, device: u32, dir: Dir) -> Endpoint {
        let identity = self.field(FieldGroup::Identity);
        let data = self.card_data(card);
        let entry = self.cards.iter().find(|c| c.index == card);
        let hw = data.map(|d| HostView::new(d, card, device, dir));

        let mut correlation = vec![];
        if identity {
            if let Some(c) = entry {
                correlation.push(CorrelationKey {
                    id: CorrelationId::AlsaCard {
                        card: c.index,
                        id: c.id.clone(),
                    },
                    confidence: Confidence::Measured,
                });
            }
            correlation.push(CorrelationKey {
                id: CorrelationId::AlsaPcm { card, device },
                confidence: Confidence::Measured,
            });
            correlation.extend(self.server_correlation(&f.id));
        }
        let transport = hw
            .as_ref()
            .and_then(|h| h.transport(self.now))
            .unwrap_or_else(|| {
                infer_transport(&f.name, entry.map(|c| c.driver.as_str()), self.now)
            });

        let hdmi = dir == Dir::Playback && matches!(transport.known(), Some(Transport::HdmiDp));
        let mut sink_view =
            (hdmi && self.layer(LayerKind::SinkChain)).then(|| self.sink_view(data, card, device));
        if let Some(k) = sink_view.as_mut().and_then(|v| v.correlation.take())
            && identity
        {
            correlation.push(k);
        }
        let sink = sink_view.as_mut().and_then(|v| v.record.take());
        let mut sink_layer = sink_view.map(|v| v.layer);

        let mut layers = vec![];
        for &layer in LayerKind::ALL
            .iter()
            .filter(|l| self.layer(**l) && **l != LayerKind::Environment)
        {
            layers.push(match layer {
                LayerKind::HostHardware => self.host_layer(hw.as_ref(), dir),
                LayerKind::KernelDriver => {
                    self.driver_layer(data, card, Some((device, hw.as_ref())))
                }
                LayerKind::AlsaUserspace => {
                    self.alsa_layer(data, card, device, dir, transport.known())
                }
                LayerKind::SinkChain => sink_layer.take().unwrap_or_else(|| {
                    plain(
                        layer,
                        unknown_layer(
                            UnknownReason::NotExposed,
                            "only HDMI/DP outputs have a sink chain",
                        ),
                    )
                }),
                LayerKind::SoundServer => self.server_layer(&f.id),
                LayerKind::Wireless => plain(
                    layer,
                    unknown_layer(UnknownReason::NotExposed, "not a wireless endpoint"),
                ),
                LayerKind::Environment => unreachable!(),
            });
        }
        Endpoint {
            id: f.id.clone(),
            class: EndpointClass::Hardware,
            direction: f.direction,
            transport,
            name: identity.then(|| {
                Value::measured(
                    EndpointName::Plain(f.name.clone()),
                    prov(LayerKind::AlsaUserspace, Source::file(PROC_PCM), self.now),
                )
            }),
            correlation,
            sink,
            midi: None,
            layers,
            effective: vec![],
            paths: vec![],
        }
    }

    fn host_layer(&self, hw: Option<&HostView<'_>>, dir: Dir) -> LayerCapabilities {
        let layer = LayerKind::HostHardware;
        let Some(hw) = hw else {
            let a = if self.res.sysfs_timed_out {
                unknown_layer(UnknownReason::Timeout, "sysfs backend deadline")
            } else {
                unknown_layer(UnknownReason::NotExposed, "card not readable")
            };
            return plain(layer, a);
        };
        let (mut caps, availability) = hw.caps(self, dir);
        if !self.field(FieldGroup::Formats) {
            caps.sample_formats = None;
        }
        if !self.field(FieldGroup::Rates) {
            caps.rates = None;
        }
        if !self.field(FieldGroup::Channels) {
            caps.channels = None;
        }
        let detail = self
            .field(FieldGroup::Hardware)
            .then(|| LayerDetail::HostHardware(hw.detail(self, dir)));
        LayerCapabilities {
            layer,
            availability,
            caps,
            detail,
        }
    }

    fn driver_layer(
        &self,
        data: Option<&CardData>,
        card: u32,
        pcm: Option<(u32, Option<&HostView<'_>>)>,
    ) -> LayerCapabilities {
        let layer = LayerKind::KernelDriver;
        if !self.field(FieldGroup::Driver) {
            return plain(layer, Availability::Read);
        }
        let Some(d) = data else {
            return plain(
                layer,
                if self.res.sysfs_timed_out {
                    unknown_layer(UnknownReason::Timeout, "sysfs backend deadline")
                } else {
                    unknown_layer(UnknownReason::NotExposed, "card not readable")
                },
            );
        };
        let dev = format!("/sys/class/sound/card{card}/device");
        let p = |path: String| prov(layer, Source::file(path), self.now);
        let driver = match (&d.entry, &d.driver) {
            (Some(e), _) => Value::measured(e.driver.clone(), p("/proc/asound/cards".into())),
            (None, Some(drv)) => Value::measured(drv.clone(), p(format!("{dev}/uevent"))),
            _ => Value::unknown(UnknownReason::NotExposed, None),
        };
        let module = match &d.module {
            Some(m) => Value::measured(m.clone(), p(format!("{dev}/uevent"))),
            None => Value::unknown(UnknownReason::NotExposed, Some(format!("{dev}/uevent"))),
        };
        let parameters = match (&d.params, &d.module) {
            (Some(v), Some(m)) => {
                Value::measured(v.clone(), p(format!("/sys/module/{m}/parameters")))
            }
            _ => Value::unknown(UnknownReason::NotExposed, None),
        };
        let power = match &d.power {
            Some(s) => Value::measured(s.clone(), p(format!("{dev}/power/runtime_status"))),
            None => Value::unknown(
                UnknownReason::NotExposed,
                Some(format!("{dev}/power/runtime_status")),
            ),
        };
        let jack = pcm.and_then(|(device, hw)| self.jack(card, device, hw));
        let availability = if [driver.known().is_some(), module.known().is_some()]
            .iter()
            .all(|x| *x)
        {
            Availability::Read
        } else {
            Availability::Partial
        };
        LayerCapabilities {
            layer,
            availability,
            caps: CapabilitySet::default(),
            detail: Some(LayerDetail::KernelDriver(DriverDetail {
                driver,
                module,
                parameters,
                power,
                jack,
            })),
        }
    }

    fn controls(&self, card: u32) -> Result<&[ControlDump], (UnknownReason, Option<String>)> {
        let Some(dump) = &self.res.alsa else {
            return Err(if self.res.alsa_timed_out {
                (UnknownReason::Timeout, Some("alsa backend deadline".into()))
            } else {
                (UnknownReason::NotExposed, self.res.alsa_missing.clone())
            });
        };
        match dump.card(card) {
            Some(c) => match (&c.controls, &c.controls_error) {
                (Some(v), _) => Ok(v),
                (None, Some(e)) => Err((control_reason(e), Some(e.clone()))),
                (None, None) => Err((UnknownReason::NotExposed, None)),
            },
            None if self.res.alsa_timed_out => {
                Err((UnknownReason::Timeout, Some("alsa backend deadline".into())))
            }
            None => Err((
                UnknownReason::NotExposed,
                Some("no controls for this card in the ALSA data".into()),
            )),
        }
    }

    /// The jack control for a PCM: `HDMI/DP,pcm=N Jack` for HDMI, or the jack named after the
    /// pin's default device for analog endpoints.
    fn jack(&self, card: u32, device: u32, hw: Option<&HostView<'_>>) -> Option<Value<JackState>> {
        let controls = match self.controls(card) {
            Ok(c) => c,
            Err((r, d)) => return Some(Value::unknown(r, d)),
        };
        let jacks: Vec<&ControlDump> = controls
            .iter()
            .filter(|c| c.name.ends_with(" Jack") && c.kind == "boolean")
            .collect();
        let hdmi = format!("pcm={device} Jack");
        let pick = jacks.iter().find(|c| c.name.ends_with(&hdmi)).or_else(|| {
            let pins = hw.map(|h| h.pins_devices()).unwrap_or_default();
            jacks.iter().find(|c| {
                let n = c.name.trim_end_matches(" Jack");
                pins.iter().any(|d| jack_matches(*d, n))
            })
        })?;
        Some(Value::measured(
            JackState {
                control: pick.name.clone(),
                plugged: pick.values.first().is_some_and(|v| *v != 0),
            },
            prov(
                LayerKind::KernelDriver,
                self.alsa_source(format!("snd_hctl_elem_read(hw:{card}, {})", pick.name)),
                self.now,
            ),
        ))
    }

    fn alsa_layer(
        &self,
        data: Option<&CardData>,
        card: u32,
        device: u32,
        dir: Dir,
        transport: Option<&Transport>,
    ) -> LayerCapabilities {
        let layer = LayerKind::AlsaUserspace;
        let stream = match dir {
            Dir::Playback => StreamDir::Playback,
            Dir::Capture => StreamDir::Capture,
        };
        let s = dir.suffix();
        let pdir = format!("/proc/asound/card{card}/pcm{device}{s}");
        let result = self
            .res
            .alsa
            .as_ref()
            .and_then(|d| d.card(card))
            .and_then(|c| c.pcm(device, stream))
            .map(|p| &p.result);
        let caps_wanted = [
            FieldGroup::Formats,
            FieldGroup::Rates,
            FieldGroup::Channels,
            FieldGroup::ChannelMaps,
            FieldGroup::Latency,
            FieldGroup::DirectAccess,
        ]
        .iter()
        .any(|f| self.field(*f));
        let pcm_source = self.alsa_source(format!("snd_pcm_hw_params_any(hw:{card},{device})"));
        let p = prov(layer, pcm_source, self.now);

        let (caps, mut availability, period) = match (result, caps_wanted) {
            (_, false) => (CapabilitySet::default(), Availability::Read, None),
            (Some(PcmResult::Ranges(r)), true) => {
                let (c, period) = self.caps_from_ranges(r, &p);
                (c, Availability::Read, Some(period))
            }
            (Some(PcmResult::NotOpened { reason, detail }), true) => {
                let (reason, detail) = match reason {
                    NotOpened::Busy => (
                        UnknownReason::Busy,
                        format!(
                            "in use{}; ranges are read only from a free device",
                            detail
                                .as_ref()
                                .map(|d| format!(" ({d})"))
                                .unwrap_or_default()
                        ),
                    ),
                    NotOpened::ManagedByServer => (
                        UnknownReason::ManagedByServer,
                        format!(
                            "{} manages the cards; read through the server from phase 4",
                            detail.as_deref().unwrap_or("a sound server")
                        ),
                    ),
                };
                (
                    self.unknown_caps(reason, &detail),
                    Availability::Partial,
                    Some(Value::unknown(reason, Some(detail))),
                )
            }
            (Some(PcmResult::Error { errno, message }), true) => {
                let reason = errno_reason(*errno);
                (
                    self.unknown_caps(reason, message),
                    unknown_layer(reason, message.clone()),
                    Some(Value::unknown(reason, Some(message.clone()))),
                )
            }
            (None, true) => {
                let (reason, detail) = if self.res.alsa_timed_out {
                    (UnknownReason::Timeout, "alsa backend deadline".to_string())
                } else {
                    (
                        UnknownReason::NotExposed,
                        self.res
                            .alsa_missing
                            .clone()
                            .unwrap_or_else(|| "no ALSA data for this PCM".into()),
                    )
                };
                (
                    self.unknown_caps(reason, &detail),
                    unknown_layer(reason, detail.clone()),
                    Some(Value::unknown(reason, Some(detail))),
                )
            }
        };

        let want_detail = self.field(FieldGroup::DirectAccess)
            || self.field(FieldGroup::Latency)
            || self.field(FieldGroup::Passthrough);
        let detail = want_detail.then(|| {
            let mut d = DeviceDetail {
                device: format!("hw:CARD={},DEV={device}", self.card_id(card)),
                state: None,
                subdevices: None,
                current: None,
                period_us: None,
                iec958: None,
            };
            if self.field(FieldGroup::DirectAccess) {
                let proc_state = data.and_then(|d| d.pcms.get(&(device, dir)));
                match proc_state {
                    Some(ps) => {
                        let state = match ps.in_use() {
                            Some(owner) => DeviceState::InUse { owner_pid: owner },
                            None => match self
                                .res
                                .alsa
                                .as_ref()
                                .and_then(|d| d.managing_server.clone())
                            {
                                Some(server) => DeviceState::ManagedByServer { server },
                                None => DeviceState::Idle,
                            },
                        };
                        d.state = Some(Value::measured(
                            state,
                            prov(layer, Source::file(format!("{pdir}/sub0/status")), self.now),
                        ));
                        d.subdevices = ps.info.as_ref().and_then(|i| {
                            Some(Value::measured(
                                Subdevices {
                                    total: i.subdevices_count?,
                                    available: i.subdevices_avail?,
                                },
                                prov(layer, Source::file(format!("{pdir}/info")), self.now),
                            ))
                        });
                        d.current = ps.current.as_ref().and_then(|h| {
                            Some(Value::measured(
                                CurrentParams {
                                    format: sample_format(h.format.as_deref()?),
                                    channels: h.channels?,
                                    rate_hz: h.rate_hz?,
                                    period_frames: h.period_size,
                                    buffer_frames: h.buffer_size,
                                },
                                prov(
                                    layer,
                                    Source::file(format!("{pdir}/sub0/hw_params")),
                                    self.now,
                                ),
                            ))
                        });
                    }
                    None => {
                        d.state = Some(if self.res.sysfs_timed_out {
                            timeout()
                        } else {
                            Value::unknown(UnknownReason::NotExposed, Some(pdir.clone()))
                        });
                    }
                }
            }
            if self.field(FieldGroup::Latency) {
                d.period_us = period.clone();
            }
            if self.field(FieldGroup::Passthrough)
                && matches!(transport, Some(Transport::Spdif | Transport::HdmiDp))
            {
                d.iec958 = Some(self.iec958(card, device));
            }
            LayerDetail::Device(d)
        });
        if detail.is_some() && matches!(availability, Availability::Unknown { .. }) {
            availability = Availability::Partial;
        }
        LayerCapabilities {
            layer,
            availability,
            caps,
            detail,
        }
    }

    fn iec958(&self, card: u32, device: u32) -> Value<Iec958State> {
        let controls = match self.controls(card) {
            Ok(c) => c,
            Err((r, d)) => return Value::unknown(r, d),
        };
        let switch = controls
            .iter()
            .find(|c| c.name == "IEC958 Playback Switch" && c.device == device);
        match switch {
            Some(c) => Value::measured(
                Iec958State {
                    enabled: c.values.first().map(|v| *v != 0),
                    non_audio: None,
                    rate_hz: None,
                },
                prov(
                    LayerKind::AlsaUserspace,
                    self.alsa_source(format!("snd_hctl_elem_read(hw:{card}, {})", c.name)),
                    self.now,
                ),
            ),
            None => Value::unknown(
                UnknownReason::NotExposed,
                Some(format!("no IEC958 switch bound to PCM device {device}")),
            ),
        }
    }

    fn unknown_caps(&self, reason: UnknownReason, detail: &str) -> CapabilitySet {
        fn u<T>(on: bool, reason: UnknownReason, detail: &str) -> Option<Value<T>> {
            on.then(|| Value::unknown(reason, Some(detail.to_string())))
        }
        CapabilitySet {
            sample_formats: u(self.field(FieldGroup::Formats), reason, detail),
            rates: u(self.field(FieldGroup::Rates), reason, detail),
            channels: u(self.field(FieldGroup::Channels), reason, detail),
            channel_maps: u(self.field(FieldGroup::ChannelMaps), reason, detail),
            latency_us: u(self.field(FieldGroup::Latency), reason, detail),
            direct_access: u(self.field(FieldGroup::DirectAccess), reason, detail),
            ..CapabilitySet::default()
        }
    }

    fn caps_from_ranges(&self, r: &HwRanges, p: &Provenance) -> (CapabilitySet, Value<Range>) {
        let m = |v| Some(Value::measured(v, p.clone()));
        let mut c = CapabilitySet::default();
        if self.field(FieldGroup::Formats) {
            c.sample_formats = m(r.formats.iter().map(|f| sample_format(f)).collect());
        }
        if self.field(FieldGroup::Rates) {
            c.rates = Some(Value::measured(rate_set(r), p.clone()));
        }
        if self.field(FieldGroup::Channels) {
            c.channels = Some(Value::measured(
                Range {
                    min: r.channels.0,
                    max: r.channels.1,
                },
                p.clone(),
            ));
        }
        if self.field(FieldGroup::ChannelMaps) {
            c.channel_maps = Some(if r.chmaps.is_empty() {
                Value::unknown(
                    UnknownReason::NotExposed,
                    Some("the driver offers no channel maps".into()),
                )
            } else {
                Value::measured(
                    r.chmaps
                        .iter()
                        .map(|(_, pos)| ChannelMap {
                            positions: pos.iter().map(|s| speaker(s)).collect(),
                        })
                        .collect(),
                    p.clone(),
                )
            });
        }
        if self.field(FieldGroup::Latency) {
            c.latency_us = Some(Value::measured(
                Range {
                    min: r.buffer_us.0,
                    max: r.buffer_us.1,
                },
                p.clone(),
            ));
        }
        if self.field(FieldGroup::DirectAccess) {
            c.direct_access = Some(Value::measured(true, p.clone()));
        }
        let period = Value::measured(
            Range {
                min: r.period_us.0,
                max: r.period_us.1,
            },
            p.clone(),
        );
        (c, period)
    }

    // -----------------------------------------------------------------------------------------
    // MIDI

    fn raw_midi_endpoint(&self, f: &Found, card: u32, device: u32) -> Endpoint {
        let identity = self.field(FieldGroup::Identity);
        let data = self.card_data(card);
        let raw = data.and_then(|d| d.midi.get(&device));
        let file = format!("/proc/asound/card{card}/midi{device}");
        let entry = self.cards.iter().find(|c| c.index == card);
        let transport = match (data, entry.map(|e| e.driver.as_str())) {
            (Some(d), _) if d.is_usb() => Value::measured(
                Transport::Usb,
                prov(
                    LayerKind::HostHardware,
                    Source::file(format!("/proc/asound/card{card}/usbid")),
                    self.now,
                ),
            ),
            (_, Some("VirMIDI")) => Value::measured(
                Transport::Virtual("virmidi".into()),
                prov(
                    LayerKind::KernelDriver,
                    Source::file("/proc/asound/cards"),
                    self.now,
                ),
            ),
            _ => Value::inferred(
                Transport::Other("midi".into()),
                prov(
                    LayerKind::KernelDriver,
                    Source::file("/proc/asound/cards"),
                    self.now,
                ),
            ),
        };
        let mut correlation = vec![];
        if identity {
            correlation.push(CorrelationKey {
                id: CorrelationId::AlsaRawMidi { card, device },
                confidence: Confidence::Measured,
            });
            for &(client, port) in &f.seq_ports {
                correlation.push(CorrelationKey {
                    id: CorrelationId::AlsaSeqPort { client, port },
                    confidence: Confidence::Inferred,
                });
            }
        }
        let mut layers = vec![];
        if self.layer(LayerKind::KernelDriver) {
            layers.push(self.driver_layer(data, card, None));
        }
        if self.layer(LayerKind::AlsaUserspace) {
            let owner = raw.and_then(|r| {
                r.outputs
                    .iter()
                    .chain(r.inputs.iter())
                    .find_map(|s| s.owner_pid)
            });
            let detail = self.field(FieldGroup::DirectAccess).then(|| {
                LayerDetail::Device(DeviceDetail {
                    device: format!("hw:CARD={},DEV={device}", self.card_id(card)),
                    state: raw.map(|_| {
                        Value::measured(
                            match owner {
                                Some(pid) => DeviceState::InUse {
                                    owner_pid: Some(pid),
                                },
                                None => DeviceState::Idle,
                            },
                            prov(
                                LayerKind::AlsaUserspace,
                                Source::file(file.clone()),
                                self.now,
                            ),
                        )
                    }),
                    subdevices: None,
                    current: None,
                    period_us: None,
                    iec958: None,
                })
            });
            layers.push(LayerCapabilities {
                layer: LayerKind::AlsaUserspace,
                availability: if raw.is_some() {
                    Availability::Read
                } else if self.res.sysfs_timed_out {
                    unknown_layer(UnknownReason::Timeout, "sysfs backend deadline")
                } else {
                    unknown_layer(UnknownReason::NotExposed, file.clone())
                },
                caps: CapabilitySet::default(),
                detail,
            });
        }
        let name = raw
            .map(|r| r.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| f.name.clone());
        Endpoint {
            id: f.id.clone(),
            class: f.class,
            direction: Direction::Midi,
            transport,
            name: identity.then(|| {
                Value::measured(
                    EndpointName::Plain(name),
                    prov(
                        LayerKind::AlsaUserspace,
                        Source::file(file.clone()),
                        self.now,
                    ),
                )
            }),
            correlation,
            sink: None,
            midi: identity.then(|| MidiPort {
                kind: MidiKind::Raw,
                input: raw.is_some_and(|r| !r.inputs.is_empty()),
                output: raw.is_some_and(|r| !r.outputs.is_empty()),
                subdevices: raw.map(|r| r.inputs.len().max(r.outputs.len()) as u32),
                protocol: raw
                    .and_then(|r| r.kind.as_deref())
                    .map(|k| if k == "UMP" { "ump" } else { "midi1" }.to_string()),
            }),
            layers,
            effective: vec![],
            paths: vec![],
        }
    }

    fn seq_endpoint(&self, f: &Found) -> Endpoint {
        let Kind::Seq {
            client,
            kernel,
            midi_version,
            port,
        } = &f.kind
        else {
            unreachable!()
        };
        let identity = self.field(FieldGroup::Identity);
        let src = || {
            prov(
                LayerKind::AlsaUserspace,
                Source::file(crate::probe::PROC_SEQ),
                self.now,
            )
        };
        let mut layers = vec![];
        if self.layer(LayerKind::AlsaUserspace) {
            layers.push(LayerCapabilities {
                layer: LayerKind::AlsaUserspace,
                availability: Availability::Read,
                caps: CapabilitySet::default(),
                detail: self.field(FieldGroup::DirectAccess).then(|| {
                    LayerDetail::Device(DeviceDetail {
                        device: format!("seq:{client}:{}", port.number),
                        state: None,
                        subdevices: None,
                        current: None,
                        period_us: None,
                        iec958: None,
                    })
                }),
            });
        }
        Endpoint {
            id: f.id.clone(),
            class: EndpointClass::Virtual,
            direction: Direction::Midi,
            transport: Value::measured(
                Transport::Virtual(
                    if *kernel {
                        "alsa-seq-kernel"
                    } else {
                        "alsa-seq"
                    }
                    .into(),
                ),
                src(),
            ),
            name: identity.then(|| Value::measured(EndpointName::Plain(f.name.clone()), src())),
            correlation: if identity {
                vec![CorrelationKey {
                    id: CorrelationId::AlsaSeqPort {
                        client: *client,
                        port: port.number,
                    },
                    confidence: Confidence::Measured,
                }]
            } else {
                vec![]
            },
            sink: None,
            midi: identity.then(|| MidiPort {
                kind: MidiKind::Sequencer,
                input: port.readable,
                output: port.writable,
                subdevices: None,
                protocol: midi_version
                    .as_deref()
                    .map(|v| if v.starts_with("UMP") { "ump" } else { "midi1" }.to_string()),
            }),
            layers,
            effective: vec![],
            paths: vec![],
        }
    }
}

fn plain(layer: LayerKind, availability: Availability) -> LayerCapabilities {
    LayerCapabilities {
        layer,
        availability,
        caps: CapabilitySet::default(),
        detail: None,
    }
}

fn io_unknown(e: &std::io::Error, path: &str) -> Availability {
    let reason = match e.kind() {
        std::io::ErrorKind::NotFound => UnknownReason::NotExposed,
        std::io::ErrorKind::PermissionDenied => UnknownReason::NotGranted,
        _ => UnknownReason::ReadError,
    };
    Availability::Unknown {
        reason,
        detail: Some(format!("{path}: {e}")),
    }
}

fn errno_reason(errno: Option<i32>) -> UnknownReason {
    match errno {
        Some(1 | 13) => UnknownReason::NotGranted,
        Some(2 | 6 | 19) => UnknownReason::NotExposed,
        Some(16) => UnknownReason::Busy,
        _ => UnknownReason::ReadError,
    }
}

fn control_reason(message: &str) -> UnknownReason {
    let m = message.to_ascii_lowercase();
    if m.contains("permission") || m.contains("eacces") || m.contains("eperm") {
        UnknownReason::NotGranted
    } else if m.contains("no such") || m.contains("enoent") {
        UnknownReason::NotExposed
    } else {
        UnknownReason::ReadError
    }
}

fn jack_matches(device: DefaultDevice, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    match device {
        DefaultDevice::HeadphoneOut => n.contains("headphone"),
        DefaultDevice::LineOut => n.contains("line out"),
        DefaultDevice::Speaker => n.contains("speaker"),
        DefaultDevice::Mic => n.contains("mic"),
        DefaultDevice::LineIn => n.contains("line") && !n.contains("out"),
        DefaultDevice::SpdifOut | DefaultDevice::DigitalOut => {
            n.contains("spdif") || n.contains("digital")
        }
        _ => false,
    }
}

/// Maps an ALSA format name to the model.
pub(crate) fn sample_format(name: &str) -> SampleFormat {
    match name {
        "U8" => SampleFormat::U8,
        "S16_LE" => SampleFormat::S16Le,
        "S16_BE" => SampleFormat::S16Be,
        "S24_LE" => SampleFormat::S24Le,
        "S24_3LE" => SampleFormat::S24_3Le,
        "S32_LE" => SampleFormat::S32Le,
        "FLOAT_LE" => SampleFormat::F32Le,
        "FLOAT64_LE" => SampleFormat::F64Le,
        "IEC958_SUBFRAME_LE" => SampleFormat::Iec958SubframeLe,
        other => SampleFormat::Other(other.to_string()),
    }
}

/// Maps an ALSA channel position name to the model.
fn speaker(name: &str) -> Speaker {
    match name {
        "FL" => Speaker::Fl,
        "FR" => Speaker::Fr,
        "FC" => Speaker::Fc,
        "LFE" => Speaker::Lfe,
        "RL" => Speaker::Rl,
        "RR" => Speaker::Rr,
        "SL" => Speaker::Sl,
        "SR" => Speaker::Sr,
        "RC" => Speaker::Rc,
        "FLC" => Speaker::Flc,
        "FRC" => Speaker::Frc,
        "RLC" => Speaker::Rlc,
        "RRC" => Speaker::Rrc,
        "TFL" => Speaker::Tfl,
        "TFR" => Speaker::Tfr,
        "TFC" => Speaker::Tfc,
        "TC" => Speaker::Tc,
        other => Speaker::Other(other.to_ascii_lowercase()),
    }
}

fn rate_set(r: &HwRanges) -> RateSet {
    if r.rate_continuous || (r.rates.is_empty() && r.rate.1 > 0) {
        RateSet::Continuous {
            min_hz: r.rate.0,
            max_hz: r.rate.1,
        }
    } else {
        RateSet::Discrete {
            rates_hz: r.rates.clone(),
        }
    }
}

/// Heuristic transport from names, used when neither a codec dump nor a USB stream file says.
fn infer_transport(pcm_name: &str, driver: Option<&str>, now: u64) -> Value<Transport> {
    let from = |what: &str| {
        prov(
            LayerKind::AlsaUserspace,
            Source::derived(what.to_string()),
            now,
        )
    };
    let n = pcm_name.to_ascii_lowercase();
    if driver == Some("USB-Audio") {
        return Value::inferred(Transport::Usb, from("card driver in /proc/asound/cards"));
    }
    let t = if n.contains("hdmi") || n.contains("displayport") || n.split(' ').any(|w| w == "dp") {
        Some(Transport::HdmiDp)
    } else if n.contains("s/pdif")
        || n.contains("spdif")
        || n.contains("iec958")
        || n.contains("digital")
    {
        Some(Transport::Spdif)
    } else if n.contains("analog") {
        Some(Transport::Analog)
    } else {
        None
    };
    match t {
        Some(t) => Value::inferred(t, from("PCM name in /proc/asound/pcm")),
        None => Value::unknown(
            UnknownReason::NotExposed,
            Some("no codec dump or USB stream file names the transport".into()),
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// host hardware view of one PCM

/// The codec converters and pins, or the USB stream, behind one PCM.
struct HostView<'a> {
    data: &'a CardData,
    card: u32,
    device: u32,
    /// (codec file, codec, converter nodes) for HDA cards.
    hda: Vec<(&'a str, &'a HdaCodec, Vec<&'a Node>)>,
    usb: Option<&'a UsbStream>,
}

impl<'a> HostView<'a> {
    fn new(data: &'a CardData, card: u32, device: u32, dir: Dir) -> Self {
        let output = dir == Dir::Playback;
        let hda = data
            .codecs
            .iter()
            .map(|(f, c)| {
                (
                    f.as_str(),
                    c,
                    c.converters_for(device, output).collect::<Vec<_>>(),
                )
            })
            .collect();
        Self {
            data,
            card,
            device,
            hda,
            usb: data.usb_streams.get(&device),
        }
    }

    fn output(&self, dir: Dir) -> bool {
        dir == Dir::Playback
    }

    /// Pins reached by the bound converters (connected pins only).
    fn pins(&self, dir: Dir) -> Vec<(&'a str, &'a Node)> {
        let mut out: Vec<(&str, &Node)> = Vec::new();
        for (file, codec, convs) in &self.hda {
            for conv in convs {
                for pin in codec.pins_for(conv.nid, self.output(dir)) {
                    let connected = pin
                        .pin_default
                        .is_some_and(|c| c.connectivity() != PortConnectivity::None);
                    if connected && !out.iter().any(|(_, p)| p.nid == pin.nid) {
                        out.push((file, pin));
                    }
                }
            }
        }
        out
    }

    fn pins_devices(&self) -> Vec<DefaultDevice> {
        let mut v: Vec<DefaultDevice> = self
            .pins(Dir::Playback)
            .into_iter()
            .chain(self.pins(Dir::Capture))
            .filter_map(|(_, p)| p.pin_default.map(|c| c.device()))
            .collect();
        v.dedup();
        v
    }

    /// An HDMI-only codec (generic HDMI driver): PCMs are bound to pins at runtime.
    fn hdmi_codec(&self) -> Option<&'a str> {
        self.hda.iter().find_map(|(f, c, _)| {
            let mut connected = c
                .pins()
                .filter(|p| {
                    p.pin_default
                        .is_some_and(|d| d.connectivity() != PortConnectivity::None)
                })
                .peekable();
            connected.peek()?;
            connected
                .all(|p| p.hdmi() || p.display_port())
                .then_some(*f)
        })
    }

    fn transport(&self, now: u64) -> Option<Value<Transport>> {
        if self.data.is_usb() {
            let path = if self.data.usb_id.is_some() {
                format!("/proc/asound/card{}/usbid", self.card)
            } else {
                "/proc/asound/cards".into()
            };
            return Some(Value::measured(
                Transport::Usb,
                prov(LayerKind::HostHardware, Source::file(path), now),
            ));
        }
        let mut pins = self.pins(Dir::Playback);
        pins.extend(self.pins(Dir::Capture));
        if let Some((file, _)) = pins.first() {
            let path = format!("/proc/asound/card{}/{file}", self.card);
            let t = if pins.iter().any(|(_, p)| p.hdmi() || p.display_port()) {
                Transport::HdmiDp
            } else if pins.iter().all(|(_, p)| {
                p.pin_default.is_some_and(|c| {
                    matches!(
                        c.device(),
                        DefaultDevice::SpdifOut
                            | DefaultDevice::SpdifIn
                            | DefaultDevice::DigitalOut
                            | DefaultDevice::DigitalIn
                    )
                })
            }) {
                Transport::Spdif
            } else {
                Transport::Analog
            };
            return Some(Value::measured(
                t,
                prov(LayerKind::HostHardware, Source::file(path), now),
            ));
        }
        self.hdmi_codec().map(|file| {
            Value::measured(
                Transport::HdmiDp,
                prov(
                    LayerKind::HostHardware,
                    Source::file(format!("/proc/asound/card{}/{file}", self.card)),
                    now,
                ),
            )
        })
    }

    /// Capabilities the hardware itself states: converter PCM caps, or USB alt-settings.
    fn caps(&self, ctx: &Ctx<'_>, dir: Dir) -> (CapabilitySet, Availability) {
        let layer = LayerKind::HostHardware;
        let mut c = CapabilitySet::default();
        if let Some(stream) = self.usb {
            let want = match dir {
                Dir::Playback => StreamDirection::Playback,
                Dir::Capture => StreamDirection::Capture,
            };
            let alts: Vec<_> = stream
                .dirs
                .iter()
                .filter(|d| d.direction == want)
                .flat_map(|d| d.formats.iter())
                .collect();
            if alts.is_empty() {
                return (
                    c,
                    unknown_layer(UnknownReason::NotExposed, "no alt-settings"),
                );
            }
            let p = prov(
                layer,
                Source::file(format!(
                    "/proc/asound/card{}/stream{}",
                    self.card, self.device
                )),
                ctx.now,
            );
            let mut formats: Vec<SampleFormat> = Vec::new();
            for a in &alts {
                let f = sample_format(&a.format);
                if !formats.contains(&f) {
                    formats.push(f);
                }
            }
            c.sample_formats = Some(Value::measured(formats, p.clone()));
            let min = alts
                .iter()
                .map(|a| u32::from(a.channels))
                .min()
                .unwrap_or(0);
            let max = alts
                .iter()
                .map(|a| u32::from(a.channels))
                .max()
                .unwrap_or(0);
            c.channels = Some(Value::measured(Range { min, max }, p.clone()));
            let cont = alts.iter().find_map(|a| a.rate_range);
            c.rates = Some(Value::measured(
                match cont {
                    Some((lo, hi)) => RateSet::Continuous {
                        min_hz: alts
                            .iter()
                            .filter_map(|a| a.rate_range.map(|r| r.0))
                            .min()
                            .unwrap_or(lo),
                        max_hz: alts
                            .iter()
                            .filter_map(|a| a.rate_range.map(|r| r.1))
                            .max()
                            .unwrap_or(hi),
                    },
                    None => {
                        let mut r: Vec<u32> = alts
                            .iter()
                            .flat_map(|a| a.rates_hz.iter().copied())
                            .collect();
                        r.sort_unstable();
                        r.dedup();
                        RateSet::Discrete { rates_hz: r }
                    }
                },
                p,
            ));
            return (c, Availability::Read);
        }
        let convs: Vec<(&str, &HdaCodec, &Node)> = self
            .hda
            .iter()
            .flat_map(|(f, codec, cs)| cs.iter().map(move |n| (*f, *codec, *n)))
            .collect();
        let Some((file, codec, node)) = convs.first().copied() else {
            let why = if self.hdmi_codec().is_some() {
                "HDMI PCMs are bound to converters at runtime; per-pin caps arrive with the sink chain in phase 6"
            } else if self.data.codecs.is_empty() {
                "no codec dump or USB stream file for this card"
            } else {
                "no converter in the codec dump is bound to this PCM device"
            };
            return (c, unknown_layer(UnknownReason::NotExposed, why));
        };
        let p = prov(
            layer,
            Source::file(format!("/proc/asound/card{}/{file}", self.card)),
            ctx.now,
        );
        let pcm = node.pcm.as_ref().or(codec.default_pcm.as_ref());
        if let Some(pcm) = pcm {
            c.rates = Some(Value::measured(
                RateSet::Discrete {
                    rates_hz: pcm.rates_hz.clone(),
                },
                p.clone(),
            ));
            // HDA sample widths map to ALSA containers: 16 bits → S16_LE, wider → S32_LE.
            let mut f = vec![];
            if pcm.bits.contains(&16) {
                f.push(SampleFormat::S16Le);
            }
            if pcm.bits.iter().any(|b| *b > 16) {
                f.push(SampleFormat::S32Le);
            }
            c.sample_formats = Some(Value::inferred(
                f,
                prov(
                    layer,
                    Source::derived(format!(
                        "converter bits in /proc/asound/card{}/{file}",
                        self.card
                    )),
                    ctx.now,
                ),
            ));
        }
        // Output converters of one PCM drive channel pairs together (front, surround, …); capture
        // PCMs use one converter at a time.
        let per = convs.iter().map(|(_, _, n)| u32::from(n.channels()));
        let max = if self.output(dir) {
            per.sum::<u32>()
        } else {
            per.max().unwrap_or(0)
        };
        c.channels = Some(Value::measured(
            Range {
                min: max.min(2),
                max,
            },
            p,
        ));
        (c, Availability::Read)
    }

    fn detail(&self, ctx: &Ctx<'_>, dir: Dir) -> HardwareDetail {
        let layer = LayerKind::HostHardware;
        let card = self.card;
        let p = |path: String| prov(layer, Source::file(path), ctx.now);
        let mut d = HardwareDetail::default();
        if let Some(id) = &self.data.usb_id {
            d.usb_id = Some(Value::measured(
                id.clone(),
                p(format!("/proc/asound/card{card}/usbid")),
            ));
        }
        if let Some(id) = &self.data.pci_id {
            d.pci_id = Some(Value::measured(
                id.clone(),
                p(format!("/sys/class/sound/card{card}/device/vendor")),
            ));
        }
        if let Some(s) = self.usb {
            d.usb_speed = Some(Value::measured(
                s.speed.clone(),
                p(format!("/proc/asound/card{card}/stream{}", self.device)),
            ));
            let want = match dir {
                Dir::Playback => StreamDirection::Playback,
                Dir::Capture => StreamDirection::Capture,
            };
            let alts = s
                .dirs
                .iter()
                .filter(|x| x.direction == want)
                .flat_map(|x| x.formats.iter())
                .map(|a| UsbAltSetting {
                    interface: u32::from(a.interface),
                    alt_setting: u32::from(a.altset),
                    formats: vec![sample_format(&a.format)],
                    channels: u32::from(a.channels),
                    rates: Some(match a.rate_range {
                        Some((lo, hi)) => RateSet::Continuous {
                            min_hz: lo,
                            max_hz: hi,
                        },
                        None => RateSet::Discrete {
                            rates_hz: a.rates_hz.clone(),
                        },
                    }),
                    sync: a.sync.as_ref().map(|s| s.to_ascii_lowercase()),
                })
                .collect();
            d.usb_alt_settings = Some(Value::measured(
                alts,
                p(format!("/proc/asound/card{card}/stream{}", self.device)),
            ));
        }
        if let Some((file, codec, _)) = self.hda.first() {
            d.codec = Some(Value::measured(
                codec.name.clone(),
                p(format!("/proc/asound/card{card}/{file}")),
            ));
            let pins = self.pins(dir);
            d.pins = Some(if pins.is_empty() && self.hdmi_codec().is_some() {
                Value::unknown(
                    UnknownReason::NotExposed,
                    Some(
                        "HDMI PCMs are bound to pins at runtime; mapped from ELD in phase 6".into(),
                    ),
                )
            } else {
                Value::measured(
                    pins.iter().map(|(_, n)| pin_info(n)).collect(),
                    p(format!("/proc/asound/card{card}/{file}")),
                )
            });
        }
        d
    }
}

fn pin_info(n: &Node) -> PinInfo {
    let cfg = n.pin_default;
    PinInfo {
        node: n.nid,
        device: cfg
            .map(|c| device_name(c.device()))
            .unwrap_or("unknown")
            .into(),
        connectivity: cfg
            .map(|c| match c.connectivity() {
                PortConnectivity::Jack => "jack",
                PortConnectivity::None => "none",
                PortConnectivity::Fixed => "fixed",
                PortConnectivity::Both => "both",
            })
            .unwrap_or("unknown")
            .into(),
        max_channels: n.channels(),
        hdmi: n.hdmi(),
        display_port: n.display_port(),
        hbr: n.hbr(),
        present: n.devices.iter().find(|d| d.current).map(|d| d.present),
    }
}

fn device_name(d: DefaultDevice) -> &'static str {
    match d {
        DefaultDevice::LineOut => "line_out",
        DefaultDevice::Speaker => "speaker",
        DefaultDevice::HeadphoneOut => "hp_out",
        DefaultDevice::Cd => "cd",
        DefaultDevice::SpdifOut => "spdif_out",
        DefaultDevice::DigitalOut => "digital_out",
        DefaultDevice::ModemLine => "modem_line",
        DefaultDevice::ModemHandset => "modem_handset",
        DefaultDevice::LineIn => "line_in",
        DefaultDevice::Aux => "aux",
        DefaultDevice::Mic => "mic_in",
        DefaultDevice::Telephony => "telephony",
        DefaultDevice::SpdifIn => "spdif_in",
        DefaultDevice::DigitalIn => "digital_in",
        DefaultDevice::Reserved => "reserved",
        DefaultDevice::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_heuristic_is_inferred() {
        assert!(matches!(
            infer_transport("HDMI 0", None, 0),
            Value::Known {
                value: Transport::HdmiDp,
                confidence: Confidence::Inferred,
                ..
            }
        ));
        assert!(matches!(
            infer_transport("USB Audio", Some("USB-Audio"), 0),
            Value::Known {
                value: Transport::Usb,
                ..
            }
        ));
        assert!(matches!(
            infer_transport("Something", None, 0),
            Value::Unknown { .. }
        ));
    }

    #[test]
    fn formats_and_speakers_map() {
        assert_eq!(sample_format("S24_3LE"), SampleFormat::S24_3Le);
        assert_eq!(
            sample_format("DSD_U8"),
            SampleFormat::Other("DSD_U8".into())
        );
        assert_eq!(speaker("LFE"), Speaker::Lfe);
        assert_eq!(speaker("NA"), Speaker::Other("na".into()));
    }

    #[test]
    fn continuous_rates() {
        let r = HwRanges {
            rate: (8000, 192000),
            rate_continuous: true,
            ..HwRanges::default()
        };
        assert_eq!(
            rate_set(&r),
            RateSet::Continuous {
                min_hz: 8000,
                max_hz: 192000
            }
        );
    }
}
