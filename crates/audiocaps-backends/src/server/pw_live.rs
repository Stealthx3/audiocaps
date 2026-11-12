// SPDX-License-Identifier: LGPL-2.1-or-later
//! Live PipeWire client → [`ServerDump`].
//!
//! Connects to the running daemon, lists globals, binds audio nodes, devices, metadata and the
//! WirePlumber client, and asks for their parameters (`EnumFormat`, `Format`, `Props`,
//! `EnumProfile`, `Profile`, `EnumRoute`, `Route`). Two core round trips: the first ends the
//! global list, the second the replies of the bound objects. Nothing is created, set or linked.
//! The session is driven in steps ([`PwSession::step`]) so the runtime can enforce a deadline.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Once;
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::{Value, ValueArray};
use pw::spa::sys as ss;
use pw::types::ObjectType;

use super::{
    Clock, Device, Format, FormatRange, Node, NodeDir, Profile, Route, ServerDump, ServerKind,
    int_list, kept,
};
use crate::server::pwdump::linear_to_percent;

static INIT: Once = Once::new();

#[derive(Default)]
struct State {
    core_name: Option<String>,
    core_version: Option<String>,
    core_props: BTreeMap<String, String>,
    session_manager: Option<String>,
    nodes: BTreeMap<u32, Node>,
    devices: BTreeMap<u32, Device>,
    /// (metadata name, key) → value.
    metadata: BTreeMap<(String, String), String>,
    /// Route params of devices: (device id, name, direction) of active routes.
    active_routes: Vec<(u32, String, String)>,
    /// Keeps bound proxies and listeners alive.
    keep: Vec<Box<dyn std::any::Any>>,
    done_seq: Vec<i32>,
    error: Option<String>,
    /// Set once the initial read is complete; later events then mark a change.
    settled: bool,
    /// Something relevant changed after the initial read.
    changed: bool,
}

impl State {
    fn touch(&mut self) {
        if self.settled {
            self.changed = true;
        }
    }
}

/// A live connection being read.
pub struct PwSession {
    mainloop: pw::main_loop::MainLoopRc,
    _context: pw::context::ContextRc,
    core: pw::core::CoreRc,
    _registry: pw::registry::RegistryRc,
    _listeners: (pw::core::Listener, pw::registry::Listener),
    state: Rc<RefCell<State>>,
    phase: u8,
    pending: i32,
}

impl PwSession {
    /// Connects to the daemon. Call only after [`super::presence::pipewire`] allowed it.
    pub fn connect() -> Result<Self, String> {
        INIT.call_once(pw::init);
        let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
        let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| e.to_string())?;
        let core = context.connect_rc(None).map_err(|e| e.to_string())?;
        let registry = core.get_registry_rc().map_err(|e| e.to_string())?;
        let state = Rc::new(RefCell::new(State::default()));

        let st = state.clone();
        let st_err = state.clone();
        let core_listener = core
            .add_listener_local()
            .info(move |info| {
                let mut s = st.borrow_mut();
                s.core_name = Some(info.name().to_string());
                s.core_version = Some(info.version().to_string());
                if let Some(p) = info.props() {
                    for (k, v) in p.iter() {
                        if k.starts_with("default.clock.") {
                            s.core_props.insert(k.to_string(), v.to_string());
                        }
                    }
                }
            })
            .done({
                let st = state.clone();
                move |_id, seq| st.borrow_mut().done_seq.push(seq.seq())
            })
            .error(move |id, _seq, _res, msg| {
                // Errors on a bound object (e.g. "enum params … Format failed" for a suspended
                // node, PipeWire 1.6) concern that object only; only core errors end the session.
                if id == pw::core::PW_ID_CORE {
                    st_err.borrow_mut().error = Some(msg.to_string());
                }
            })
            .register();

        let st = state.clone();
        let reg_weak = registry.downgrade();
        let reg_listener = registry
            .add_listener_local()
            .global(move |g| {
                let Some(registry) = reg_weak.upgrade() else {
                    return;
                };
                let props = g.props;
                let get = |k: &str| props.and_then(|p| p.get(k)).map(str::to_string);
                match g.type_ {
                    ObjectType::Node => {
                        let class = get("media.class").unwrap_or_default();
                        if !class.starts_with("Audio/") {
                            return;
                        }
                        bind_node(&registry, g, &st);
                    }
                    ObjectType::Device => {
                        if get("media.class").as_deref() != Some("Audio/Device") {
                            return;
                        }
                        bind_device(&registry, g, &st);
                    }
                    ObjectType::Metadata => {
                        let name = get("metadata.name").unwrap_or_default();
                        if ["settings", "default", "sm-settings"].contains(&name.as_str()) {
                            bind_metadata(&registry, g, name, &st);
                        }
                    }
                    ObjectType::Client
                        if get("application.name").as_deref() == Some("WirePlumber") =>
                    {
                        st.borrow_mut().session_manager = Some("WirePlumber".into());
                    }
                    _ => {}
                }
            })
            .global_remove({
                let st = state.clone();
                move |id| {
                    let mut s = st.borrow_mut();
                    let known = s.nodes.remove(&id).is_some() | s.devices.remove(&id).is_some();
                    if known {
                        s.touch();
                    }
                }
            })
            .register();

        let pending = core.sync(0).map_err(|e| e.to_string())?.seq();
        Ok(Self {
            mainloop,
            _context: context,
            core,
            _registry: registry,
            _listeners: (core_listener, reg_listener),
            state,
            phase: 0,
            pending,
        })
    }

    /// Runs the loop for up to `slice`. Returns `true` once both round trips are done.
    pub fn step(&mut self, slice: Duration) -> Result<bool, String> {
        self.mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(slice));
        if let Some(e) = self.state.borrow_mut().error.take() {
            return Err(e);
        }
        let done = self.state.borrow().done_seq.contains(&self.pending);
        if self.phase >= 2 {
            return Ok(true);
        }
        if done {
            self.phase += 1;
            if self.phase >= 2 {
                self.state.borrow_mut().settled = true;
                return Ok(true);
            }
            self.pending = self.core.sync(0).map_err(|e| e.to_string())?.seq();
        }
        Ok(false)
    }

    /// Whether anything relevant changed since the last call (after the initial read).
    pub fn take_changed(&self) -> bool {
        std::mem::take(&mut self.state.borrow_mut().changed)
    }

    /// What was read so far.
    pub fn dump(&self) -> ServerDump {
        let s = self.state.borrow();
        let meta = |m: &str, k: &str| s.metadata.get(&(m.to_string(), k.to_string()));
        let num = |m: &str, k: &str| meta(m, k).and_then(|v| v.trim().parse::<u32>().ok());
        let core_num = |k: &str| s.core_props.get(k).and_then(|v| v.parse::<u32>().ok());
        let clock = Clock {
            rate: num("settings", "clock.rate").or(core_num("default.clock.rate")),
            allowed_rates: meta("settings", "clock.allowed-rates")
                .or(s.core_props.get("default.clock.allowed-rates"))
                .map(|v| int_list(v))
                .unwrap_or_default(),
            quantum: num("settings", "clock.quantum").or(core_num("default.clock.quantum")),
            min_quantum: num("settings", "clock.min-quantum")
                .or(core_num("default.clock.min-quantum")),
            max_quantum: num("settings", "clock.max-quantum")
                .or(core_num("default.clock.max-quantum")),
            force_rate: num("settings", "clock.force-rate"),
            force_quantum: num("settings", "clock.force-quantum"),
        };
        let default_name = |k: &str| {
            meta("default", k).and_then(|v| {
                serde_json::from_str::<serde_json::Value>(v)
                    .ok()
                    .and_then(|j| j["name"].as_str().map(String::from))
            })
        };
        let mut devices: Vec<Device> = s.devices.values().cloned().collect();
        for d in &mut devices {
            for r in &mut d.routes {
                r.active = s
                    .active_routes
                    .iter()
                    .any(|(id, n, dir)| *id == d.id && *n == r.name && *dir == r.direction);
            }
        }
        ServerDump {
            kind: ServerKind::PipeWire,
            name: s.core_name.clone().unwrap_or_else(|| "pipewire".into()),
            version: s.core_version.clone(),
            clock: (clock != Clock::default()).then_some(clock),
            default_sink: default_name("default.audio.sink"),
            default_source: default_name("default.audio.source"),
            policy: s
                .metadata
                .iter()
                .filter(|((m, _), _)| m == "sm-settings")
                .map(|((_, k), v)| (k.clone(), v.clone()))
                .collect(),
            session_manager: s.session_manager.clone(),
            devices,
            nodes: s.nodes.values().cloned().collect(),
        }
    }
}

fn dict_props(p: Option<&spa::utils::dict::DictRef>) -> BTreeMap<String, String> {
    p.map(|p| {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    })
    .unwrap_or_default()
}

fn bind_node(
    registry: &pw::registry::RegistryRc,
    g: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>,
    st: &Rc<RefCell<State>>,
) {
    let Ok(node) = registry.bind::<pw::node::Node, _>(g) else {
        return;
    };
    let id = g.id;
    let s1 = st.clone();
    let s2 = st.clone();
    let listener = node
        .add_listener_local()
        .info(move |info| {
            let p = dict_props(info.props());
            let mut s = s1.borrow_mut();
            let n = s.nodes.entry(id).or_insert_with(|| Node {
                id,
                ..Node::default()
            });
            let get = |k: &str| p.get(k).cloned();
            let num = |k: &str| p.get(k).and_then(|v| v.parse::<u32>().ok());
            n.name = get("node.name").unwrap_or_default();
            n.description = get("node.description").unwrap_or_default();
            n.media_class = get("media.class").unwrap_or_default();
            n.dir = if n.media_class.contains("Sink") {
                Some(NodeDir::Sink)
            } else if n.media_class.contains("Source") {
                Some(NodeDir::Source)
            } else {
                None
            };
            n.device_id = num("device.id");
            n.api = get("device.api");
            n.factory = get("factory.name");
            n.alsa_card = num("api.alsa.pcm.card").or(num("alsa.card"));
            n.alsa_device = num("api.alsa.pcm.device").or(num("alsa.device"));
            n.profile = get("device.profile.name");
            n.state = Some(
                match info.state() {
                    pw::node::NodeState::Running => "running",
                    pw::node::NodeState::Idle => "idle",
                    pw::node::NodeState::Suspended => "suspended",
                    pw::node::NodeState::Creating => "creating",
                    pw::node::NodeState::Error(_) => "error",
                }
                .to_string(),
            );
            n.props = p.into_iter().filter(|(k, _)| kept(k)).collect();
            s.touch();
        })
        .param(move |_seq, ty, index, _next, pod| {
            let mut s = s2.borrow_mut();
            s.touch();
            let n = s.nodes.entry(id).or_insert_with(|| Node {
                id,
                ..Node::default()
            });
            // A new enumeration starts at index 0: replace what an earlier one gave.
            if index == 0 {
                match ty {
                    spa::param::ParamType::EnumFormat => n.enum_formats.clear(),
                    spa::param::ParamType::Format => n.format = None,
                    _ => {}
                }
            }
            let Some(pod) = pod else { return };
            let Ok((_, v)) = PodDeserializer::deserialize_any_from(pod.as_bytes()) else {
                return;
            };
            match ty {
                spa::param::ParamType::EnumFormat => {
                    if let Some(r) = format_range(&v) {
                        n.enum_formats.push(r);
                    }
                }
                spa::param::ParamType::Format => n.format = format(&v),
                spa::param::ParamType::Props => props(&v, n),
                _ => {}
            }
        })
        .register();
    // Subscribing delivers the current values now and again whenever they change.
    node.subscribe_params(&[
        spa::param::ParamType::EnumFormat,
        spa::param::ParamType::Format,
        spa::param::ParamType::Props,
    ]);
    let mut s = st.borrow_mut();
    s.keep.push(Box::new(node));
    s.keep.push(Box::new(listener));
    s.touch();
}

fn bind_device(
    registry: &pw::registry::RegistryRc,
    g: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>,
    st: &Rc<RefCell<State>>,
) {
    let Ok(device) = registry.bind::<pw::device::Device, _>(g) else {
        return;
    };
    let id = g.id;
    let s1 = st.clone();
    let s2 = st.clone();
    let listener = device
        .add_listener_local()
        .info(move |info| {
            let p = dict_props(info.props());
            let mut s = s1.borrow_mut();
            let d = s.devices.entry(id).or_insert_with(|| Device {
                id,
                ..Device::default()
            });
            d.name = p.get("device.name").cloned().unwrap_or_default();
            d.api = p.get("device.api").cloned();
            d.alsa_card = p
                .get("api.alsa.card")
                .or(p.get("alsa.card"))
                .and_then(|v| v.parse().ok());
            d.props = p.into_iter().filter(|(k, _)| kept(k)).collect();
            s.touch();
        })
        .param(move |_seq, ty, index, _next, pod| {
            let mut s = s2.borrow_mut();
            s.touch();
            if index == 0 {
                match ty {
                    spa::param::ParamType::EnumProfile => dev(&mut s, id).profiles.clear(),
                    spa::param::ParamType::Profile => dev(&mut s, id).active_profile = None,
                    spa::param::ParamType::EnumRoute => dev(&mut s, id).routes.clear(),
                    spa::param::ParamType::Route => s.active_routes.retain(|r| r.0 != id),
                    _ => {}
                }
            }
            let Some(pod) = pod else { return };
            let Ok((_, v)) = PodDeserializer::deserialize_any_from(pod.as_bytes()) else {
                return;
            };
            match ty {
                spa::param::ParamType::EnumProfile => {
                    if let Some(p) = profile(&v) {
                        dev(&mut s, id).profiles.push(p);
                    }
                }
                spa::param::ParamType::Profile => {
                    if let Some(p) = profile(&v) {
                        dev(&mut s, id).active_profile = Some(p.name);
                    }
                }
                spa::param::ParamType::EnumRoute => {
                    if let Some(r) = route(&v) {
                        dev(&mut s, id).routes.push(r);
                    }
                }
                spa::param::ParamType::Route => {
                    if let Some(r) = route(&v) {
                        s.active_routes.push((id, r.name, r.direction));
                    }
                }
                _ => {}
            }
        })
        .register();
    device.subscribe_params(&[
        spa::param::ParamType::EnumProfile,
        spa::param::ParamType::Profile,
        spa::param::ParamType::EnumRoute,
        spa::param::ParamType::Route,
    ]);
    let mut s = st.borrow_mut();
    s.keep.push(Box::new(device));
    s.keep.push(Box::new(listener));
    s.touch();
}

fn dev(s: &mut State, id: u32) -> &mut Device {
    s.devices.entry(id).or_insert_with(|| Device {
        id,
        ..Device::default()
    })
}

fn bind_metadata(
    registry: &pw::registry::RegistryRc,
    g: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>,
    name: String,
    st: &Rc<RefCell<State>>,
) {
    let Ok(md) = registry.bind::<pw::metadata::Metadata, _>(g) else {
        return;
    };
    let s1 = st.clone();
    let listener = md
        .add_listener_local()
        .property(move |subject, key, _ty, value| {
            if subject == 0
                && let (Some(k), Some(v)) = (key, value)
            {
                let mut s = s1.borrow_mut();
                s.metadata
                    .insert((name.clone(), k.to_string()), v.to_string());
                s.touch();
            }
            0
        })
        .register();
    let mut s = st.borrow_mut();
    s.keep.push(Box::new(md));
    s.keep.push(Box::new(listener));
}

// ---------------------------------------------------------------------------------------------
// SPA pod decoding

fn object(v: &Value) -> Option<&spa::pod::Object> {
    match v {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

fn prop(o: &spa::pod::Object, key: u32) -> Option<&Value> {
    o.properties.iter().find(|p| p.key == key).map(|p| &p.value)
}

/// All values a pod value allows: a scalar, an enum choice's values, or a range's bounds
/// (returned separately).
fn ints(v: &Value) -> (Vec<u32>, Option<(u32, u32)>) {
    use spa::utils::ChoiceEnum;
    let u = |x: i32| u32::try_from(x).ok();
    match v {
        Value::Int(i) => (u(*i).into_iter().collect(), None),
        Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(_, c))) => match c {
            ChoiceEnum::None(x) => (u(*x).into_iter().collect(), None),
            ChoiceEnum::Range { min, max, .. } | ChoiceEnum::Step { min, max, .. } => {
                (vec![], u(*min).zip(u(*max)))
            }
            ChoiceEnum::Enum {
                default,
                alternatives,
            } => {
                let mut out: Vec<u32> = Vec::new();
                for x in std::iter::once(default).chain(alternatives.iter()) {
                    if let Some(x) = u(*x)
                        && !out.contains(&x)
                    {
                        out.push(x);
                    }
                }
                (out, None)
            }
            ChoiceEnum::Flags { default, .. } => (u(*default).into_iter().collect(), None),
        },
        _ => (vec![], None),
    }
}

fn ids(v: &Value) -> Vec<u32> {
    use spa::utils::ChoiceEnum;
    match v {
        Value::Id(i) => vec![i.0],
        Value::Choice(spa::pod::ChoiceValue::Id(spa::utils::Choice(_, c))) => match c {
            ChoiceEnum::None(x) => vec![x.0],
            ChoiceEnum::Enum {
                default,
                alternatives,
            } => {
                let mut out: Vec<u32> = Vec::new();
                for x in std::iter::once(default).chain(alternatives.iter()) {
                    if !out.contains(&x.0) {
                        out.push(x.0);
                    }
                }
                out
            }
            _ => vec![],
        },
        Value::ValueArray(ValueArray::Id(a)) => a.iter().map(|i| i.0).collect(),
        _ => vec![],
    }
}

fn subtype_name(id: u32) -> &'static str {
    match id {
        ss::SPA_MEDIA_SUBTYPE_raw => "raw",
        ss::SPA_MEDIA_SUBTYPE_dsp => "dsp",
        ss::SPA_MEDIA_SUBTYPE_iec958 => "iec958",
        ss::SPA_MEDIA_SUBTYPE_dsd => "dsd",
        _ => "other",
    }
}

fn audio_object(v: &Value) -> Option<(&spa::pod::Object, &'static str)> {
    let o = object(v)?;
    if ids(prop(o, ss::SPA_FORMAT_mediaType)?).first() != Some(&ss::SPA_MEDIA_TYPE_audio) {
        return None;
    }
    let sub = ids(prop(o, ss::SPA_FORMAT_mediaSubtype)?)
        .first()
        .map(|i| subtype_name(*i))
        .unwrap_or("other");
    Some((o, sub))
}

fn format(v: &Value) -> Option<Format> {
    let (o, sub) = audio_object(v)?;
    let names = if sub == "iec958" {
        prop(o, ss::SPA_FORMAT_AUDIO_iec958Codec).map(|x| codec_names(&ids(x)))
    } else {
        prop(o, ss::SPA_FORMAT_AUDIO_format).map(|x| format_names(&ids(x)))
    };
    let first = |k| prop(o, k).and_then(|x| ints(x).0.first().copied());
    Some(Format {
        subtype: sub.into(),
        format: names.and_then(|n| n.into_iter().next()).unwrap_or_default(),
        rate: first(ss::SPA_FORMAT_AUDIO_rate).unwrap_or(0),
        channels: first(ss::SPA_FORMAT_AUDIO_channels).unwrap_or(0),
    })
}

fn format_range(v: &Value) -> Option<FormatRange> {
    let (o, sub) = audio_object(v)?;
    let formats = if sub == "iec958" {
        prop(o, ss::SPA_FORMAT_AUDIO_iec958Codec).map(|x| codec_names(&ids(x)))
    } else {
        prop(o, ss::SPA_FORMAT_AUDIO_format).map(|x| format_names(&ids(x)))
    }
    .unwrap_or_default();
    let (rates, rate_range) = prop(o, ss::SPA_FORMAT_AUDIO_rate)
        .map(ints)
        .unwrap_or_default();
    let channels = prop(o, ss::SPA_FORMAT_AUDIO_channels).and_then(|x| match ints(x) {
        (_, Some(r)) => Some(r),
        (v, None) => Some((*v.iter().min()?, *v.iter().max()?)),
    });
    let positions = prop(o, ss::SPA_FORMAT_AUDIO_position)
        .map(|x| ids(x).into_iter().map(channel_name).collect())
        .unwrap_or_default();
    Some(FormatRange {
        subtype: sub.into(),
        formats,
        rates,
        rate_range,
        channels,
        positions,
    })
}

fn props(v: &Value, n: &mut Node) {
    let Some(o) = object(v) else { return };
    if let Some(x) = prop(o, ss::SPA_PROP_iec958Codecs) {
        n.iec958_codecs = Some(codec_names(&ids(x)));
    }
    if let Some(Value::ValueArray(ValueArray::Float(vols))) = prop(o, ss::SPA_PROP_channelVolumes)
        && !vols.is_empty()
    {
        let avg = vols.iter().map(|x| f64::from(*x)).sum::<f64>() / vols.len() as f64;
        n.volume_percent = Some(linear_to_percent(avg));
    }
    if let Some(Value::Bool(m)) = prop(o, ss::SPA_PROP_mute) {
        n.muted = Some(*m);
    }
}

fn string(o: &spa::pod::Object, key: u32) -> String {
    match prop(o, key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn availability(o: &spa::pod::Object, key: u32) -> Option<bool> {
    match prop(o, key).map(ids).and_then(|v| v.first().copied()) {
        Some(ss::SPA_PARAM_AVAILABILITY_yes) => Some(true),
        Some(ss::SPA_PARAM_AVAILABILITY_no) => Some(false),
        _ => None,
    }
}

fn profile(v: &Value) -> Option<Profile> {
    let o = object(v)?;
    Some(Profile {
        name: string(o, ss::SPA_PARAM_PROFILE_name),
        description: string(o, ss::SPA_PARAM_PROFILE_description),
        available: availability(o, ss::SPA_PARAM_PROFILE_available),
    })
}

fn route(v: &Value) -> Option<Route> {
    let o = object(v)?;
    let direction = match prop(o, ss::SPA_PARAM_ROUTE_direction)
        .map(ids)
        .and_then(|v| v.first().copied())
    {
        Some(ss::SPA_DIRECTION_INPUT) => "input",
        Some(ss::SPA_DIRECTION_OUTPUT) => "output",
        _ => "",
    };
    let devices = match prop(o, ss::SPA_PARAM_ROUTE_devices) {
        Some(Value::ValueArray(ValueArray::Int(a))) => {
            a.iter().filter_map(|x| u32::try_from(*x).ok()).collect()
        }
        _ => vec![],
    };
    Some(Route {
        name: string(o, ss::SPA_PARAM_ROUTE_name),
        description: string(o, ss::SPA_PARAM_ROUTE_description),
        direction: direction.into(),
        available: availability(o, ss::SPA_PARAM_ROUTE_available),
        active: false,
        devices,
    })
}

/// pw-dump names for SPA audio formats.
fn format_names(ids: &[u32]) -> Vec<String> {
    ids.iter()
        .map(|i| {
            match *i {
                ss::SPA_AUDIO_FORMAT_S8 => "S8",
                ss::SPA_AUDIO_FORMAT_U8 => "U8",
                ss::SPA_AUDIO_FORMAT_S16_LE => "S16LE",
                ss::SPA_AUDIO_FORMAT_S16_BE => "S16BE",
                ss::SPA_AUDIO_FORMAT_U16_LE => "U16LE",
                ss::SPA_AUDIO_FORMAT_S24_32_LE => "S24_32LE",
                ss::SPA_AUDIO_FORMAT_S24_32_BE => "S24_32BE",
                ss::SPA_AUDIO_FORMAT_S32_LE => "S32LE",
                ss::SPA_AUDIO_FORMAT_S32_BE => "S32BE",
                ss::SPA_AUDIO_FORMAT_U32_LE => "U32LE",
                ss::SPA_AUDIO_FORMAT_S24_LE => "S24LE",
                ss::SPA_AUDIO_FORMAT_S24_BE => "S24BE",
                ss::SPA_AUDIO_FORMAT_S20_LE => "S20LE",
                ss::SPA_AUDIO_FORMAT_S18_LE => "S18LE",
                ss::SPA_AUDIO_FORMAT_F32_LE => "F32LE",
                ss::SPA_AUDIO_FORMAT_F32_BE => "F32BE",
                ss::SPA_AUDIO_FORMAT_F64_LE => "F64LE",
                ss::SPA_AUDIO_FORMAT_F64_BE => "F64BE",
                ss::SPA_AUDIO_FORMAT_U8P => "U8P",
                ss::SPA_AUDIO_FORMAT_S16P => "S16P",
                ss::SPA_AUDIO_FORMAT_S24_32P => "S24_32P",
                ss::SPA_AUDIO_FORMAT_S32P => "S32P",
                ss::SPA_AUDIO_FORMAT_S24P => "S24P",
                ss::SPA_AUDIO_FORMAT_F32P => "F32P",
                ss::SPA_AUDIO_FORMAT_F64P => "F64P",
                ss::SPA_AUDIO_FORMAT_S8P => "S8P",
                ss::SPA_AUDIO_FORMAT_ENCODED => "ENCODED",
                _ => "",
            }
            .to_string()
        })
        .map(|s| {
            if s.is_empty() {
                "UNKNOWN".to_string()
            } else {
                s
            }
        })
        .collect()
}

/// pw-dump names for IEC 958 codecs.
fn codec_names(ids: &[u32]) -> Vec<String> {
    ids.iter()
        .map(|i| {
            match *i {
                ss::SPA_AUDIO_IEC958_CODEC_PCM => "PCM",
                ss::SPA_AUDIO_IEC958_CODEC_DTS => "DTS",
                ss::SPA_AUDIO_IEC958_CODEC_AC3 => "AC3",
                ss::SPA_AUDIO_IEC958_CODEC_MPEG => "MPEG",
                ss::SPA_AUDIO_IEC958_CODEC_MPEG2_AAC => "MPEG2-AAC",
                ss::SPA_AUDIO_IEC958_CODEC_EAC3 => "EAC3",
                ss::SPA_AUDIO_IEC958_CODEC_TRUEHD => "TrueHD",
                ss::SPA_AUDIO_IEC958_CODEC_DTSHD => "DTS-HD",
                _ => "UNKNOWN",
            }
            .to_string()
        })
        .collect()
}

/// pw-dump names for channel positions.
fn channel_name(id: u32) -> String {
    match id {
        ss::SPA_AUDIO_CHANNEL_MONO => "MONO",
        ss::SPA_AUDIO_CHANNEL_FL => "FL",
        ss::SPA_AUDIO_CHANNEL_FR => "FR",
        ss::SPA_AUDIO_CHANNEL_FC => "FC",
        ss::SPA_AUDIO_CHANNEL_LFE => "LFE",
        ss::SPA_AUDIO_CHANNEL_SL => "SL",
        ss::SPA_AUDIO_CHANNEL_SR => "SR",
        ss::SPA_AUDIO_CHANNEL_FLC => "FLC",
        ss::SPA_AUDIO_CHANNEL_FRC => "FRC",
        ss::SPA_AUDIO_CHANNEL_RC => "RC",
        ss::SPA_AUDIO_CHANNEL_RL => "RL",
        ss::SPA_AUDIO_CHANNEL_RR => "RR",
        ss::SPA_AUDIO_CHANNEL_TC => "TC",
        ss::SPA_AUDIO_CHANNEL_TFL => "TFL",
        ss::SPA_AUDIO_CHANNEL_TFC => "TFC",
        ss::SPA_AUDIO_CHANNEL_TFR => "TFR",
        ss::SPA_AUDIO_CHANNEL_TRL => "TRL",
        ss::SPA_AUDIO_CHANNEL_TRC => "TRC",
        ss::SPA_AUDIO_CHANNEL_TRR => "TRR",
        ss::SPA_AUDIO_CHANNEL_RLC => "RLC",
        ss::SPA_AUDIO_CHANNEL_RRC => "RRC",
        ss::SPA_AUDIO_CHANNEL_FLW => "FLW",
        ss::SPA_AUDIO_CHANNEL_FRW => "FRW",
        ss::SPA_AUDIO_CHANNEL_LFE2 => "LFE2",
        ss::SPA_AUDIO_CHANNEL_FLH => "FLH",
        ss::SPA_AUDIO_CHANNEL_FCH => "FCH",
        ss::SPA_AUDIO_CHANNEL_FRH => "FRH",
        ss::SPA_AUDIO_CHANNEL_TFLC => "TFLC",
        ss::SPA_AUDIO_CHANNEL_TFRC => "TFRC",
        ss::SPA_AUDIO_CHANNEL_TSL => "TSL",
        ss::SPA_AUDIO_CHANNEL_TSR => "TSR",
        ss::SPA_AUDIO_CHANNEL_LLFE => "LLFE",
        ss::SPA_AUDIO_CHANNEL_RLFE => "RLFE",
        ss::SPA_AUDIO_CHANNEL_BC => "BC",
        ss::SPA_AUDIO_CHANNEL_BLC => "BLC",
        ss::SPA_AUDIO_CHANNEL_BRC => "BRC",
        ss::SPA_AUDIO_CHANNEL_NA => "NA",
        _ => "UNK",
    }
    .to_string()
}
