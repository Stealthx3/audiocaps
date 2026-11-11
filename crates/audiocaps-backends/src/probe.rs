// SPDX-License-Identifier: LGPL-2.1-or-later
//! The scoped probe: discovery, the backend tasks, and assembly into a [`Snapshot`].
//!
//! 1. **Discovery** reads the ALSA card and PCM lists (and, when MIDI is in scope, the raw MIDI
//!    devices and sequencer clients) and keeps only endpoints in scope.
//! 2. **Backends** run as tasks on the runtime's executor, each with its own deadline:
//!    `sysfs` (codecs, USB streams, driver state, PCM state, MIDI), `alsa` (alsa-lib: PCM ranges,
//!    controls, configuration; or the captured dump for a fixture) and `environment`.
//! 3. **Assembly** ([`crate::assemble`]) turns what was read into the model, with provenance, and
//!    reports anything not reached as `unknown` with the reason.
//!
//! Nothing outside the scope is read: each backend only gets work for in-scope endpoints, layers
//! and field groups.

use std::collections::{BTreeMap, VecDeque};

use audiocaps_model::{
    Direction, EndpointClass, EndpointId, EnvironmentInfo, FieldGroup, LayerKind, ProbeScope,
    Snapshot, UnknownReason,
};
use audiocaps_parse::procfs::{self, CardEntry, SeqPort};

use crate::alsa::dump::{self, AlsaDump, CardDump, DUMP_FORMAT, PcmDump, StreamDir};
use crate::alsa::live;
use crate::root::SystemRoot;
use crate::runtime::{self, Backend, ProbeOptions, Task};
use crate::server::pulse_live::PulseSession;
use crate::server::pw_live::PwSession;
use crate::server::{ServerDump, ServerKind, pactl, presence, pwdump};
use crate::sysfs::{self, CardData, Dir, Wants};

pub(crate) const PROC_PCM: &str = "/proc/asound/pcm";
pub(crate) const PROC_CARDS: &str = "/proc/asound/cards";
pub(crate) const PROC_SEQ: &str = "/proc/asound/seq/clients";

/// Probes `root` within `scope` with no deadlines (fixtures, tests, tools). Applications use
/// [`crate::Runtime`], which applies per-backend deadlines.
pub fn probe(root: &SystemRoot, scope: &ProbeScope) -> Snapshot {
    probe_with(root, scope, &ProbeOptions::unbounded())
}

/// Probes `root` within `scope`, running the backends with the deadlines in `opts`.
pub fn probe_with(root: &SystemRoot, scope: &ProbeScope, opts: &ProbeOptions) -> Snapshot {
    probe_inner(root, scope, opts, Prefilled::default())
}

/// Server state a caller already holds (the watch keeps live sessions): those backends don't
/// run again.
#[derive(Default)]
pub(crate) struct Prefilled<'a> {
    pub pipewire: Option<ServerDump>,
    pub pulse: Option<ServerDump>,
    /// An open PulseAudio session to re-query as a task (concurrently with the other backends).
    pub pulse_session: Option<&'a mut PulseSession>,
}

pub(crate) fn probe_inner<'a>(
    root: &'a SystemRoot,
    scope: &ProbeScope,
    opts: &ProbeOptions,
    prefilled: Prefilled<'a>,
) -> Snapshot {
    let now = root.now_ms();
    let needs_cards =
        !scope.endpoint_classes.is_empty() && !scope.directions.is_empty() || routing_wanted(scope);
    let cards = if needs_cards {
        root.read_string(PROC_CARDS)
            .map(|t| procfs::parse_cards(&t))
            .unwrap_or_default()
    } else {
        vec![]
    };
    let found = discover(root, scope, &cards);
    let mut res = Results::default();
    let mut tasks: Vec<Box<dyn Task<Results> + 'a>> = Vec::new();

    let want = wants(scope, &found);
    let mut card_work: BTreeMap<u32, Vec<(u32, Dir)>> = BTreeMap::new();
    for f in &found {
        match f.kind {
            Kind::Pcm { card, device, dir } => {
                card_work.entry(card).or_default().push((device, dir))
            }
            Kind::RawMidi { card, .. } => {
                card_work.entry(card).or_default();
            }
            Kind::Seq { .. } => {}
        }
    }
    if !card_work.is_empty() {
        tasks.push(Box::new(SysfsTask {
            root,
            cards: cards.clone(),
            todo: card_work.into_iter().collect(),
            want,
            out: BTreeMap::new(),
        }));
    }

    let alsa = alsa_work(scope, &found);
    if alsa.any() {
        tasks.push(Box::new(AlsaTask::new(root, alsa)));
    }
    if scope.layers.includes(&LayerKind::SoundServer) {
        match prefilled.pipewire {
            Some(d) => {
                res.pipewire = ServerResult::Read {
                    dump: Box::new(d),
                    live: true,
                }
            }
            None => tasks.push(Box::new(ServerTask::new(root, ServerKind::PipeWire))),
        }
        match (prefilled.pulse_session, prefilled.pulse) {
            (Some(session), _) => {
                session.refresh();
                tasks.push(Box::new(ServerTask {
                    root,
                    kind: ServerKind::PulseAudio,
                    session: Session::PulseRef(session),
                    result: None,
                }));
            }
            (None, Some(d)) => {
                res.pulse = ServerResult::Read {
                    dump: Box::new(d),
                    live: true,
                }
            }
            (None, None) => tasks.push(Box::new(ServerTask::new(root, ServerKind::PulseAudio))),
        }
    }
    if scope.layers.includes(&LayerKind::Environment) {
        tasks.push(Box::new(EnvTask {
            root,
            now,
            out: None,
        }));
    }
    runtime::run(tasks, opts, &mut res);
    crate::assemble::snapshot(root, scope, now, &cards, found, res)
}

/// Results the backend tasks hand over.
#[derive(Debug, Default)]
pub(crate) struct Results {
    pub cards: BTreeMap<u32, CardData>,
    pub sysfs_timed_out: bool,
    pub alsa: Option<AlsaDump>,
    /// Why there is no ALSA dump (fixture without one).
    pub alsa_missing: Option<String>,
    pub alsa_live: bool,
    pub alsa_timed_out: bool,
    pub environment: Option<EnvironmentInfo>,
    pub env_timed_out: bool,
    pub pipewire: ServerResult,
    pub pulse: ServerResult,
}

/// What a sound-server backend produced.
#[derive(Debug, Default)]
pub(crate) enum ServerResult {
    /// Not in scope.
    #[default]
    NotRun,
    /// Read, live or from captured output.
    Read { dump: Box<ServerDump>, live: bool },
    /// Not connected, with the reason.
    Skipped {
        reason: UnknownReason,
        detail: String,
    },
    /// Connected but failed.
    Failed(String),
    /// Past the deadline.
    TimedOut,
}

/// A discovered endpoint.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub id: EndpointId,
    pub class: EndpointClass,
    pub direction: Direction,
    pub kind: Kind,
    /// Name from the list it was found in.
    pub name: String,
    /// Sequencer ports that belong to a raw MIDI device.
    pub seq_ports: Vec<(u32, u32)>,
}

/// What a discovered endpoint is.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Pcm {
        card: u32,
        device: u32,
        dir: Dir,
    },
    RawMidi {
        card: u32,
        device: u32,
    },
    Seq {
        client: u32,
        kernel: bool,
        midi_version: Option<String>,
        port: SeqPort,
    },
}

fn routing_wanted(scope: &ProbeScope) -> bool {
    scope.fields.includes(&FieldGroup::Routing) && scope.layers.includes(&LayerKind::AlsaUserspace)
}

fn wants(scope: &ProbeScope, found: &[Found]) -> Wants {
    let layer = |l| scope.layers.includes(&l);
    let field = |f| scope.fields.includes(&f);
    let pcm = found.iter().any(|f| matches!(f.kind, Kind::Pcm { .. }));
    Wants {
        // Codecs and USB stream files give the transport, which every endpoint reports.
        hardware: pcm,
        bus_ids: layer(LayerKind::HostHardware) && field(FieldGroup::Hardware),
        driver: layer(LayerKind::KernelDriver) && field(FieldGroup::Driver),
        pcm_state: pcm && layer(LayerKind::AlsaUserspace) && field(FieldGroup::DirectAccess),
        midi: found.iter().any(|f| matches!(f.kind, Kind::RawMidi { .. })),
    }
}

/// Discovers endpoints in scope.
fn discover(root: &SystemRoot, scope: &ProbeScope, cards: &[CardEntry]) -> Vec<Found> {
    let mut out = Vec::new();
    let hw = scope.endpoint_classes.includes(&EndpointClass::Hardware);
    let pcm_dirs = scope.directions.includes(&Direction::Playback)
        || scope.directions.includes(&Direction::Capture);
    if hw
        && pcm_dirs
        && let Ok(text) = root.read_string(PROC_PCM)
    {
        for pcm in procfs::parse_pcm(&text) {
            for (dir, count, d) in [
                (Direction::Playback, pcm.playback, Dir::Playback),
                (Direction::Capture, pcm.capture, Dir::Capture),
            ] {
                if count == 0 {
                    continue;
                }
                let id = EndpointId(format!(
                    "alsa:card{}/pcm{}{}",
                    pcm.card,
                    pcm.device,
                    d.suffix()
                ));
                if scope.includes_endpoint(EndpointClass::Hardware, dir, &id) {
                    out.push(Found {
                        id,
                        class: EndpointClass::Hardware,
                        direction: dir,
                        kind: Kind::Pcm {
                            card: pcm.card,
                            device: pcm.device,
                            dir: d,
                        },
                        name: pcm.name.clone(),
                        seq_ports: vec![],
                    });
                }
            }
        }
    }
    if scope.directions.includes(&Direction::Midi) {
        discover_midi(root, scope, cards, &mut out);
    }
    out
}

/// Card classes that are virtual MIDI devices rather than hardware.
fn midi_card_class(card: Option<&CardEntry>) -> EndpointClass {
    match card.map(|c| c.driver.as_str()) {
        Some("VirMIDI") => EndpointClass::Virtual,
        _ => EndpointClass::Hardware,
    }
}

fn discover_midi(root: &SystemRoot, scope: &ProbeScope, cards: &[CardEntry], out: &mut Vec<Found>) {
    let mut raw: Vec<Found> = Vec::new();
    for c in cards {
        let class = midi_card_class(Some(c));
        for device in sysfs::midi_devices(root, c.index) {
            let id = EndpointId(format!("alsa:card{}/midi{device}", c.index));
            if scope.includes_endpoint(class, Direction::Midi, &id) {
                // The device's own name (first line of its proc file) matches its sequencer port.
                let name = root
                    .read_string(&format!("/proc/asound/card{}/midi{device}", c.index))
                    .ok()
                    .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("{} MIDI {device}", c.name));
                raw.push(Found {
                    id,
                    class,
                    direction: Direction::Midi,
                    kind: Kind::RawMidi {
                        card: c.index,
                        device,
                    },
                    name,
                    seq_ports: vec![],
                });
            }
        }
    }
    let clients = root
        .read_string(PROC_SEQ)
        .map(|t| procfs::parse_seq_clients(&t))
        .unwrap_or_default();
    for client in clients {
        if client.number == 0 {
            continue; // System: timer and announce ports.
        }
        // A kernel client or port named after a raw MIDI device belongs to it: the device's
        // name, or `<name> <card>-<device>` (VirMIDI: client "Virtual Raw MIDI 3-0").
        if client.kernel {
            let mut matched = false;
            for p in &client.ports {
                let owner = raw.iter_mut().find(|f| {
                    let Kind::RawMidi { card, device } = f.kind else {
                        return false;
                    };
                    let tag = format!(" {card}-{device}");
                    f.name == p.name
                        || client.name == format!("{}{tag}", f.name)
                        || p.name.ends_with(&tag)
                });
                if let Some(r) = owner {
                    r.seq_ports.push((client.number, p.number));
                    matched = true;
                }
            }
            if matched {
                continue;
            }
        }
        // A kernel client named after a card belongs to that card's raw MIDI devices.
        let card = client
            .kernel
            .then(|| cards.iter().find(|c| c.name == client.name))
            .flatten();
        if let Some(card) = card {
            let on_card: Vec<usize> = raw
                .iter()
                .enumerate()
                .filter(|(_, f)| matches!(f.kind, Kind::RawMidi { card: c, .. } if c == card.index))
                .map(|(i, _)| i)
                .collect();
            for p in &client.ports {
                let target = if on_card.len() == 1 {
                    on_card.first().copied()
                } else {
                    on_card.get(p.number as usize).copied()
                };
                if let Some(i) = target {
                    raw[i].seq_ports.push((client.number, p.number));
                }
            }
            continue;
        }
        for port in client.ports {
            if !port.readable && !port.writable {
                continue;
            }
            let id = EndpointId(format!(
                "alsa:seq/{}/{}",
                client.name.replace('/', "_"),
                port.name.replace('/', "_")
            ));
            if !scope.includes_endpoint(EndpointClass::Virtual, Direction::Midi, &id) {
                continue;
            }
            out.push(Found {
                id,
                class: EndpointClass::Virtual,
                direction: Direction::Midi,
                name: format!("{}: {}", client.name, port.name),
                kind: Kind::Seq {
                    client: client.number,
                    kernel: client.kernel,
                    midi_version: client.midi_version.clone(),
                    port,
                },
                seq_ports: vec![],
            });
        }
    }
    out.extend(raw);
}

// ---------------------------------------------------------------------------------------------
// sysfs task

struct SysfsTask<'a> {
    root: &'a SystemRoot,
    cards: Vec<CardEntry>,
    todo: VecDeque<(u32, Vec<(u32, Dir)>)>,
    want: Wants,
    out: BTreeMap<u32, CardData>,
}

impl Task<Results> for SysfsTask<'_> {
    fn backend(&self) -> Backend {
        Backend::Sysfs
    }

    fn step(&mut self) -> bool {
        if let Some((index, pcms)) = self.todo.pop_front() {
            let entry = self.cards.iter().find(|c| c.index == index).cloned();
            let data = sysfs::read_card(self.root, entry, index, &pcms, self.want);
            self.out.insert(index, data);
        }
        self.todo.is_empty()
    }

    fn finish(self: Box<Self>, out: &mut Results, timed_out: bool) {
        out.cards = self.out;
        out.sysfs_timed_out = timed_out;
    }
}

// ---------------------------------------------------------------------------------------------
// ALSA task

/// What the ALSA backend must read.
#[derive(Debug, Default)]
pub(crate) struct AlsaWork {
    pub pcms: Vec<(u32, u32, StreamDir)>,
    pub controls: Vec<u32>,
    pub config: bool,
}

impl AlsaWork {
    fn any(&self) -> bool {
        !self.pcms.is_empty() || !self.controls.is_empty() || self.config
    }
}

fn alsa_work(scope: &ProbeScope, found: &[Found]) -> AlsaWork {
    let layer = |l| scope.layers.includes(&l);
    let field = |f| scope.fields.includes(&f);
    let ranges = layer(LayerKind::AlsaUserspace)
        && [
            FieldGroup::Formats,
            FieldGroup::Rates,
            FieldGroup::Channels,
            FieldGroup::ChannelMaps,
            FieldGroup::Latency,
            FieldGroup::DirectAccess,
        ]
        .iter()
        .any(|f| field(*f));
    let controls = (layer(LayerKind::KernelDriver) && field(FieldGroup::Driver))
        || (layer(LayerKind::AlsaUserspace) && field(FieldGroup::Passthrough));
    let mut w = AlsaWork {
        config: routing_wanted(scope),
        ..AlsaWork::default()
    };
    for f in found {
        if let Kind::Pcm { card, device, dir } = f.kind {
            if ranges {
                let s = match dir {
                    Dir::Playback => StreamDir::Playback,
                    Dir::Capture => StreamDir::Capture,
                };
                w.pcms.push((card, device, s));
            }
            if controls && !w.controls.contains(&card) {
                w.controls.push(card);
            }
        }
    }
    w
}

enum AlsaItem {
    LoadFixture,
    Managing,
    Config,
    Controls(u32),
    Pcm(u32, u32, StreamDir),
}

struct AlsaTask<'a> {
    root: &'a SystemRoot,
    items: VecDeque<AlsaItem>,
    managing: Option<String>,
    dump: AlsaDump,
    missing: Option<String>,
    live: bool,
}

impl<'a> AlsaTask<'a> {
    fn new(root: &'a SystemRoot, work: AlsaWork) -> Self {
        let live = matches!(root, SystemRoot::Live);
        let mut items = VecDeque::new();
        if live {
            if !work.pcms.is_empty() {
                items.push_back(AlsaItem::Managing);
            }
            if work.config {
                items.push_back(AlsaItem::Config);
            }
            items.extend(work.controls.into_iter().map(AlsaItem::Controls));
            items.extend(
                work.pcms
                    .into_iter()
                    .map(|(c, d, s)| AlsaItem::Pcm(c, d, s)),
            );
        } else {
            items.push_back(AlsaItem::LoadFixture);
        }
        Self {
            root,
            items,
            managing: None,
            dump: AlsaDump {
                format: DUMP_FORMAT,
                ..AlsaDump::default()
            },
            missing: None,
            live,
        }
    }

    fn card(&mut self, index: u32) -> &mut CardDump {
        if let Some(i) = self.dump.cards.iter().position(|c| c.index == index) {
            &mut self.dump.cards[i]
        } else {
            self.dump.cards.push(CardDump {
                index,
                ..CardDump::default()
            });
            self.dump.cards.last_mut().expect("just pushed")
        }
    }
}

impl Task<Results> for AlsaTask<'_> {
    fn backend(&self) -> Backend {
        Backend::Alsa
    }

    fn step(&mut self) -> bool {
        match self.items.pop_front() {
            Some(AlsaItem::LoadFixture) => match self.root.captured_command(dump::CAPTURE_NAME) {
                Some((_, Some(bytes))) => match serde_json::from_slice::<AlsaDump>(&bytes) {
                    Ok(d) => self.dump = d,
                    Err(e) => self.missing = Some(format!("alsa.json: {e}")),
                },
                Some((cmd, None)) => {
                    self.missing = Some(
                        cmd.skipped
                            .unwrap_or_else(|| "the capture has no ALSA output".into()),
                    )
                }
                None => {
                    self.missing =
                        Some("the capture predates ALSA dumps (capture format 1, phase 2)".into())
                }
            },
            Some(AlsaItem::Managing) => {
                self.managing = crate::servers::managing_server(self.root);
                self.dump.managing_server = self.managing.clone();
            }
            Some(AlsaItem::Config) => match live::read_config() {
                Ok(c) => self.dump.config = Some(c.to_string()),
                Err(e) => self.dump.config_error = Some(e),
            },
            Some(AlsaItem::Controls(card)) => {
                let r = live::read_controls(card);
                let c = self.card(card);
                match r {
                    Ok(v) => c.controls = Some(v),
                    Err(e) => c.controls_error = Some(e),
                }
            }
            Some(AlsaItem::Pcm(card, device, stream)) => {
                let result =
                    live::read_pcm(self.root, card, device, stream, self.managing.as_deref());
                self.card(card).pcms.push(PcmDump {
                    device,
                    stream,
                    result,
                });
            }
            None => {}
        }
        self.items.is_empty()
    }

    fn finish(self: Box<Self>, out: &mut Results, timed_out: bool) {
        out.alsa = if self.missing.is_some() {
            None
        } else {
            Some(self.dump)
        };
        out.alsa_missing = self.missing;
        out.alsa_live = self.live;
        out.alsa_timed_out = timed_out;
    }
}

// ---------------------------------------------------------------------------------------------
// sound-server tasks

enum Session<'a> {
    None,
    Pw(PwSession),
    Pulse(PulseSession),
    PulseRef(&'a mut PulseSession),
}

struct ServerTask<'a> {
    root: &'a SystemRoot,
    kind: ServerKind,
    session: Session<'a>,
    result: Option<ServerResult>,
}

impl<'a> ServerTask<'a> {
    fn new(root: &'a SystemRoot, kind: ServerKind) -> Self {
        Self {
            root,
            kind,
            session: Session::None,
            result: None,
        }
    }

    fn fixture(&self) -> ServerResult {
        let cmd = |name: &str| -> Result<serde_json::Value, (UnknownReason, String)> {
            match self.root.captured_command(name) {
                Some((_, Some(b))) => serde_json::from_slice(&b)
                    .map_err(|e| (UnknownReason::ParseError, format!("{name}: {e}"))),
                Some((c, None)) => Err((
                    UnknownReason::ServiceNotRunning,
                    c.skipped
                        .unwrap_or_else(|| format!("{name} gave no output")),
                )),
                None => Err((UnknownReason::NotExposed, format!("{name} not captured"))),
            }
        };
        let parsed = match self.kind {
            ServerKind::PipeWire => cmd("pw-dump").map(|j| pwdump::parse(&j)),
            ServerKind::PulseAudio => {
                cmd("pactl-info").map(|i| pactl::parse(&i, cmd("pactl-list").ok().as_ref()))
            }
        };
        match parsed {
            Ok(Some(dump)) => ServerResult::Read {
                dump: Box::new(dump),
                live: false,
            },
            Ok(None) => ServerResult::Skipped {
                reason: UnknownReason::ParseError,
                detail: "unexpected output shape".into(),
            },
            Err((reason, detail)) => ServerResult::Skipped { reason, detail },
        }
    }
}

impl Task<Results> for ServerTask<'_> {
    fn backend(&self) -> Backend {
        match self.kind {
            ServerKind::PipeWire => Backend::PipeWire,
            ServerKind::PulseAudio => Backend::PulseAudio,
        }
    }

    fn step(&mut self) -> bool {
        const SLICE: std::time::Duration = std::time::Duration::from_millis(2);
        if self.result.is_some() {
            return true;
        }
        if !matches!(self.root, SystemRoot::Live) {
            self.result = Some(self.fixture());
            return true;
        }
        let step = match &mut self.session {
            Session::None => {
                let allowed = match self.kind {
                    ServerKind::PipeWire => presence::pipewire(self.root),
                    ServerKind::PulseAudio => presence::pulse(self.root),
                };
                if let Err(skip) = allowed {
                    self.result = Some(ServerResult::Skipped {
                        reason: skip.reason,
                        detail: skip.detail,
                    });
                    return true;
                }
                let connected = match self.kind {
                    ServerKind::PipeWire => PwSession::connect().map(Session::Pw),
                    ServerKind::PulseAudio => PulseSession::connect().map(Session::Pulse),
                };
                match connected {
                    Ok(s) => self.session = s,
                    Err(e) => self.result = Some(ServerResult::Failed(e)),
                }
                return self.result.is_some();
            }
            Session::Pw(s) => s.step(SLICE).map(|done| done.then(|| s.dump())),
            Session::Pulse(s) => s.step(SLICE).map(|done| done.then(|| s.dump())),
            Session::PulseRef(s) => s.step(SLICE).map(|done| done.then(|| s.dump())),
        };
        match step {
            Ok(Some(dump)) => {
                self.result = Some(ServerResult::Read {
                    dump: Box::new(dump),
                    live: true,
                })
            }
            Ok(None) => {}
            Err(e) => self.result = Some(ServerResult::Failed(e)),
        }
        self.result.is_some()
    }

    fn finish(self: Box<Self>, out: &mut Results, timed_out: bool) {
        let r = match self.result {
            Some(r) => r,
            None if timed_out => ServerResult::TimedOut,
            None => ServerResult::Failed("ended without a result".into()),
        };
        match self.kind {
            ServerKind::PipeWire => out.pipewire = r,
            ServerKind::PulseAudio => out.pulse = r,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// environment task

struct EnvTask<'a> {
    root: &'a SystemRoot,
    now: u64,
    out: Option<EnvironmentInfo>,
}

impl Task<Results> for EnvTask<'_> {
    fn backend(&self) -> Backend {
        Backend::Environment
    }

    fn step(&mut self) -> bool {
        self.out = Some(crate::environment::environment(self.root, self.now));
        true
    }

    fn finish(self: Box<Self>, out: &mut Results, timed_out: bool) {
        out.environment = self.out;
        out.env_timed_out = timed_out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scope_reads_nothing() {
        let s = probe(&SystemRoot::Live, &ProbeScope::nothing());
        assert!(s.endpoints.is_empty());
        assert!(s.layers.is_empty());
        assert!(s.environment.is_none());
        assert!(s.routing.is_none());
    }

    #[test]
    fn alsa_work_follows_scope() {
        let f = vec![Found {
            id: EndpointId("alsa:card0/pcm0p".into()),
            class: EndpointClass::Hardware,
            direction: Direction::Playback,
            kind: Kind::Pcm {
                card: 0,
                device: 0,
                dir: Dir::Playback,
            },
            name: "x".into(),
            seq_ports: vec![],
        }];
        let only_identity = ProbeScope::nothing()
            .with_layer(LayerKind::AlsaUserspace)
            .with_field(FieldGroup::Identity);
        assert!(!alsa_work(&only_identity, &f).any());
        let rates = only_identity.clone().with_field(FieldGroup::Rates);
        let w = alsa_work(&rates, &f);
        assert_eq!(w.pcms.len(), 1);
        assert!(w.controls.is_empty() && !w.config);
        let wrong_layer = ProbeScope::nothing()
            .with_layer(LayerKind::SinkChain)
            .with_field(FieldGroup::Rates);
        assert!(!alsa_work(&wrong_layer, &f).any());
    }
}
