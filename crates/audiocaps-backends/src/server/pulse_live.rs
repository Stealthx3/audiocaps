// SPDX-License-Identifier: LGPL-2.1-or-later
//! Live PulseAudio client → [`ServerDump`]. Works against native PulseAudio and pipewire-pulse.
//!
//! Connects with `PA_CONTEXT_NOAUTOSPAWN`, then runs four introspection calls (server, sinks,
//! sources, cards). Nothing is written and no stream is created. Driven in steps by the runtime.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libpulse_binding as pa;
use pa::callbacks::ListResult;
use pa::context::{Context, FlagSet, State as CtxState};
use pa::mainloop::standard::{IterateResult, Mainloop};
use pa::operation::{Operation, State as OpState};
use pa::proplist::Proplist;
use pa::volume::{ChannelVolumes, Volume};

use super::{
    Device, Format, FormatRange, Node, NodeDir, Profile, Route, ServerDump, ServerKind, kept,
};

#[derive(Default)]
struct Acc {
    name: Option<String>,
    version: Option<String>,
    default_sink: Option<String>,
    default_source: Option<String>,
    nodes: Vec<Node>,
    devices: Vec<Device>,
    active_ports: Vec<(u32, String)>,
}

/// A live connection being read.
pub struct PulseSession {
    mainloop: Mainloop,
    context: Context,
    acc: Rc<RefCell<Acc>>,
    ops: Vec<Box<dyn OpDone>>,
    started: bool,
    changed: Rc<RefCell<bool>>,
    subscribed: bool,
}

trait OpDone {
    fn done(&self) -> bool;
}

impl<F: ?Sized> OpDone for Operation<F> {
    fn done(&self) -> bool {
        !matches!(self.get_state(), OpState::Running)
    }
}

impl PulseSession {
    /// Starts connecting. Call only after [`super::presence::pulse`] allowed it.
    pub fn connect() -> Result<Self, String> {
        let mainloop = Mainloop::new().ok_or("pa_mainloop_new failed")?;
        let mut props = Proplist::new().ok_or("pa_proplist_new failed")?;
        let _ = props.set_str(pa::proplist::properties::APPLICATION_NAME, "audiocaps");
        let mut context = Context::new_with_proplist(&mainloop, "audiocaps", &props)
            .ok_or("pa_context_new failed")?;
        context
            .connect(None, FlagSet::NOAUTOSPAWN, None)
            .map_err(|e| e.to_string().unwrap_or_else(|| "connect failed".into()))?;
        Ok(Self {
            mainloop,
            context,
            acc: Rc::new(RefCell::new(Acc::default())),
            ops: vec![],
            started: false,
            changed: Rc::new(RefCell::new(false)),
            subscribed: false,
        })
    }

    /// Runs the loop for up to `slice`. Returns `true` when every query has answered.
    pub fn step(&mut self, slice: Duration) -> Result<bool, String> {
        let until = Instant::now() + slice;
        loop {
            match self.mainloop.iterate(false) {
                IterateResult::Quit(_) | IterateResult::Err(_) => {
                    return Err("PulseAudio main loop failed".into());
                }
                IterateResult::Success(0) if Instant::now() >= until => break,
                IterateResult::Success(0) => std::thread::sleep(Duration::from_micros(200)),
                IterateResult::Success(_) => {}
            }
            match self.context.get_state() {
                CtxState::Failed | CtxState::Terminated => {
                    return Err(self.context.errno().to_string().unwrap_or_default());
                }
                CtxState::Ready if !self.started => self.start(),
                _ => {}
            }
            if self.started && self.ops.iter().all(|o| o.done()) {
                return Ok(true);
            }
            if Instant::now() >= until {
                break;
            }
        }
        Ok(false)
    }

    fn start(&mut self) {
        self.started = true;
        let intro = self.context.introspect();
        let a = self.acc.clone();
        self.ops.push(Box::new(intro.get_server_info(move |i| {
            let mut a = a.borrow_mut();
            a.name = i.server_name.as_ref().map(|s| s.to_string());
            a.version = i.server_version.as_ref().map(|s| s.to_string());
            a.default_sink = i.default_sink_name.as_ref().map(|s| s.to_string());
            a.default_source = i.default_source_name.as_ref().map(|s| s.to_string());
        })));
        let a = self.acc.clone();
        self.ops.push(Box::new(intro.get_sink_info_list(move |r| {
            if let ListResult::Item(s) = r {
                let mut n = Node {
                    id: s.index,
                    name: s.name.as_deref().unwrap_or("").to_string(),
                    description: s.description.as_deref().unwrap_or("").to_string(),
                    media_class: "Audio/Sink".into(),
                    dir: Some(NodeDir::Sink),
                    device_id: s.card,
                    factory: s.driver.as_deref().map(String::from),
                    state: Some(format!("{:?}", s.state).to_ascii_lowercase()),
                    muted: Some(s.mute),
                    volume_percent: percent(&s.volume),
                    ..Node::default()
                };
                fill(&mut n, &s.proplist, &s.sample_spec, &s.channel_map);
                n.iec958_codecs = Some(
                    s.formats
                        .iter()
                        .filter_map(|f| {
                            pa::format::Encoding::to_string(f.get_encoding()).map(|c| c.to_string())
                        })
                        .collect(),
                );
                let mut a = a.borrow_mut();
                if let (Some(c), Some(p)) = (s.card, s.active_port.as_ref()) {
                    a.active_ports
                        .push((c, p.name.as_deref().unwrap_or("").to_string()));
                }
                a.nodes.push(n);
            }
        })));
        let a = self.acc.clone();
        self.ops.push(Box::new(intro.get_source_info_list(move |r| {
            if let ListResult::Item(s) = r {
                if s.monitor_of_sink.is_some()
                    || s.proplist.get_str("device.class").as_deref() == Some("monitor")
                {
                    return;
                }
                let mut n = Node {
                    id: s.index,
                    name: s.name.as_deref().unwrap_or("").to_string(),
                    description: s.description.as_deref().unwrap_or("").to_string(),
                    media_class: "Audio/Source".into(),
                    dir: Some(NodeDir::Source),
                    device_id: s.card,
                    factory: s.driver.as_deref().map(String::from),
                    state: Some(format!("{:?}", s.state).to_ascii_lowercase()),
                    muted: Some(s.mute),
                    volume_percent: percent(&s.volume),
                    ..Node::default()
                };
                fill(&mut n, &s.proplist, &s.sample_spec, &s.channel_map);
                n.iec958_codecs = Some(
                    s.formats
                        .iter()
                        .filter_map(|f| {
                            pa::format::Encoding::to_string(f.get_encoding()).map(|c| c.to_string())
                        })
                        .collect(),
                );
                let mut a = a.borrow_mut();
                if let (Some(c), Some(p)) = (s.card, s.active_port.as_ref()) {
                    a.active_ports
                        .push((c, p.name.as_deref().unwrap_or("").to_string()));
                }
                a.nodes.push(n);
            }
        })));
        let a = self.acc.clone();
        self.ops.push(Box::new(intro.get_card_info_list(move |r| {
            if let ListResult::Item(c) = r {
                let p = &c.proplist;
                let mut d = Device {
                    id: c.index,
                    name: c.name.as_deref().unwrap_or("").to_string(),
                    // Native PulseAudio cards carry `alsa.card` but no `device.api`.
                    api: p
                        .get_str("device.api")
                        .or_else(|| p.get_str("alsa.card").map(|_| "alsa".to_string())),
                    alsa_card: p.get_str("alsa.card").and_then(|v| v.parse().ok()),
                    active_profile: c
                        .active_profile
                        .as_ref()
                        .and_then(|x| x.name.as_deref().map(String::from)),
                    ..Device::default()
                };
                for pr in &c.profiles {
                    d.profiles.push(Profile {
                        name: pr.name.as_deref().unwrap_or("").to_string(),
                        description: pr.description.as_deref().unwrap_or("").to_string(),
                        available: Some(pr.available),
                    });
                }
                for pt in &c.ports {
                    let dir = if pt.direction.contains(pa::direction::FlagSet::OUTPUT) {
                        "output"
                    } else if pt.direction.contains(pa::direction::FlagSet::INPUT) {
                        "input"
                    } else {
                        ""
                    };
                    d.routes.push(Route {
                        name: pt.name.as_deref().unwrap_or("").to_string(),
                        description: pt.description.as_deref().unwrap_or("").to_string(),
                        direction: dir.into(),
                        available: match pt.available {
                            pa::def::PortAvailable::Yes => Some(true),
                            pa::def::PortAvailable::No => Some(false),
                            _ => None,
                        },
                        active: false,
                        devices: vec![],
                        detected_codecs: None,
                    });
                }
                for k in super::KEPT_PROPS {
                    if let Some(v) = p.get_str(k) {
                        d.props.insert((*k).to_string(), v);
                    }
                }
                a.borrow_mut().devices.push(d);
            }
        })));
    }

    /// What was read.
    pub fn dump(&self) -> ServerDump {
        let a = self.acc.borrow();
        let mut devices = a.devices.clone();
        for d in &mut devices {
            for r in &mut d.routes {
                r.active = a
                    .active_ports
                    .iter()
                    .any(|(c, p)| *c == d.id && *p == r.name);
            }
        }
        devices.sort_by_key(|d| d.id);
        let mut nodes = a.nodes.clone();
        nodes.sort_by_key(|n| (n.dir == Some(NodeDir::Source), n.id));
        ServerDump {
            kind: ServerKind::PulseAudio,
            name: a.name.clone().unwrap_or_else(|| "pulseaudio".into()),
            version: a.version.clone(),
            clock: None,
            default_sink: a.default_sink.clone(),
            default_source: a.default_source.clone(),
            policy: Default::default(),
            session_manager: None,
            devices,
            nodes,
        }
    }
}

impl PulseSession {
    /// Subscribes to sink, source, card and server events (call once the read is done).
    pub fn subscribe(&mut self) {
        if self.subscribed {
            return;
        }
        self.subscribed = true;
        let c = self.changed.clone();
        self.context
            .set_subscribe_callback(Some(Box::new(move |_f, _op, _idx| *c.borrow_mut() = true)));
        use pa::context::subscribe::InterestMaskSet as M;
        let _op = self
            .context
            .subscribe(M::SINK | M::SOURCE | M::CARD | M::SERVER, |_| {});
    }

    /// Pumps events without blocking. Fails when the connection is gone.
    pub fn pump(&mut self) -> Result<(), String> {
        loop {
            match self.mainloop.iterate(false) {
                IterateResult::Success(0) => return Ok(()),
                IterateResult::Success(_) => {}
                _ => return Err("PulseAudio main loop failed".into()),
            }
            if matches!(
                self.context.get_state(),
                CtxState::Failed | CtxState::Terminated
            ) {
                return Err("PulseAudio connection lost".into());
            }
        }
    }

    /// Re-runs the introspection queries on the open connection (after an event).
    pub fn refresh(&mut self) {
        *self.acc.borrow_mut() = Acc::default();
        self.ops.clear();
        self.started = false;
    }

    /// Whether a subscribed event arrived since the last call.
    pub fn take_changed(&self) -> bool {
        std::mem::take(&mut *self.changed.borrow_mut())
    }
}

impl Drop for PulseSession {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

/// Per-channel percentages rounded like `pactl`, then averaged.
fn percent(v: &ChannelVolumes) -> Option<u32> {
    let norm = u64::from(Volume::NORMAL.0);
    let p: Vec<u32> = v
        .get()
        .iter()
        .map(|c| ((u64::from(c.0) * 100 + norm / 2) / norm) as u32)
        .collect();
    (!p.is_empty()).then(|| p.iter().sum::<u32>() / p.len() as u32)
}

fn fill(n: &mut Node, p: &Proplist, spec: &pa::sample::Spec, map: &pa::channelmap::Map) {
    n.api = p.get_str("device.api");
    n.alsa_card = p.get_str("alsa.card").and_then(|v| v.parse().ok());
    n.alsa_device = p.get_str("alsa.device").and_then(|v| v.parse().ok());
    n.profile = p.get_str("device.profile.name");
    for k in super::KEPT_PROPS {
        if kept(k)
            && let Some(v) = p.get_str(k)
        {
            n.props.insert((*k).to_string(), v);
        }
    }
    let f = Format {
        subtype: "raw".into(),
        format: spec.format.to_string().unwrap_or_default().to_string(),
        rate: spec.rate,
        channels: u32::from(spec.channels),
    };
    let positions: Vec<String> = map
        .get()
        .iter()
        .take(usize::from(map.len()))
        .map(|p| {
            pa::channelmap::Position::to_string(*p)
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
        .collect();
    n.enum_formats = vec![FormatRange {
        subtype: "raw".into(),
        formats: vec![f.format.clone()],
        rates: vec![f.rate],
        rate_range: None,
        channels: Some((f.channels, f.channels)),
        positions,
    }];
    n.format = Some(f);
}
