// SPDX-License-Identifier: LGPL-2.1-or-later
//! Change events: a watch keeps its own event sources open and re-probes the watched scope when
//! one fires, then delivers the new snapshot with its diff.
//!
//! Sources, all passive:
//! - the kernel: an inotify watch on `/dev/snd` for device nodes appearing or disappearing
//!   (nothing under `/dev/snd` is opened; only the directory is watched);
//! - PipeWire: the watch's own session, subscribed to node, device and metadata changes;
//! - PulseAudio: a subscription to sink, source, card and server events.
//!
//! The re-probe reuses the sessions' current server state instead of reconnecting, which keeps
//! event-to-snapshot latency low. Events whose re-probe changes nothing in scope are dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use audiocaps_model::{LayerKind, ProbeScope, Snapshot, SnapshotDiff};
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};

use crate::probe::{Prefilled, probe_inner};
use crate::root::SystemRoot;
use crate::runtime::{ProbeOptions, Shared};
use crate::server::presence;
use crate::server::pulse_live::PulseSession;
use crate::server::pw_live::PwSession;

/// What triggered an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The first snapshot of the watch.
    Initial,
    /// Sound device nodes appeared or disappeared (hotplug).
    Kernel,
    /// A PipeWire node, device or setting changed.
    PipeWire,
    /// A PulseAudio sink, source, card or server setting changed.
    PulseAudio,
}

/// One update.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// The new snapshot (also published as the runtime's latest).
    pub snapshot: Arc<Snapshot>,
    /// Changes against the previous snapshot of this watch.
    pub diff: SnapshotDiff,
    /// What triggered it.
    pub causes: Vec<Cause>,
    /// Time from noticing the event to the new snapshot.
    pub latency: Duration,
}

/// A running watch. Dropping it stops the watch thread.
pub struct Watch {
    rx: mpsc::Receiver<WatchEvent>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Watch {
    pub(crate) fn start(
        root: SystemRoot,
        scope: ProbeScope,
        opts: ProbeOptions,
        shared: Arc<Shared>,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let handle = thread::Builder::new()
            .name("audiocaps-watch".into())
            .spawn(move || run(root, scope, opts, shared, tx, s))?;
        Ok(Self {
            rx,
            stop,
            handle: Some(handle),
        })
    }

    /// Waits for the next update.
    pub fn recv(&self) -> Option<WatchEvent> {
        self.rx.recv().ok()
    }

    /// Waits up to `timeout` for the next update.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<WatchEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// The next update, if one is waiting.
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

const TICK: Duration = Duration::from_millis(1);
const SETUP: Duration = Duration::from_millis(500);

struct Sources {
    inotify: Option<Inotify>,
    pw: Option<PwSession>,
    pulse: Option<PulseSession>,
    next_retry: Instant,
    /// Servers a snap's confinement refused (PipeWire, PulseAudio): not retried, since every try
    /// logs an AppArmor denial.
    refused: (bool, bool),
}

impl Sources {
    fn open(root: &SystemRoot, scope: &ProbeScope) -> Self {
        let live = matches!(root, SystemRoot::Live);
        let endpoints = !scope.endpoint_classes.is_empty() && !scope.directions.is_empty();
        let inotify = (live && endpoints)
            .then(|| {
                let i = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC).ok()?;
                i.add_watch(
                    "/dev/snd",
                    AddWatchFlags::IN_CREATE | AddWatchFlags::IN_DELETE,
                )
                .ok()?;
                Some(i)
            })
            .flatten();
        let mut s = Self {
            inotify,
            pw: None,
            pulse: None,
            next_retry: Instant::now(),
            refused: (false, false),
        };
        if live && scope.layers.includes(&LayerKind::SoundServer) {
            s.connect_servers(root);
        }
        s
    }

    /// (Re)connects server sessions that aren't open, at most once a second.
    fn connect_servers(&mut self, root: &SystemRoot) {
        if Instant::now() < self.next_retry {
            return;
        }
        self.next_retry = Instant::now() + Duration::from_secs(1);
        let snap = std::env::var_os("SNAP_NAME").is_some();
        if self.pw.is_none()
            && !self.refused.0
            && let Ok(socket) = presence::pipewire(root)
        {
            let connected = PwSession::connect(&socket);
            self.refused.0 = snap && connected.is_err();
            self.pw = connected.ok().and_then(|mut s| {
                let until = Instant::now() + SETUP;
                loop {
                    match s.step(Duration::from_millis(5)) {
                        Ok(true) => return Some(s),
                        Ok(false) if Instant::now() < until => {}
                        _ => return None,
                    }
                }
            });
        }
        if self.pulse.is_none()
            && !self.refused.1
            && let Ok(server) = presence::pulse(root)
        {
            self.pulse = PulseSession::connect(server.as_deref())
                .ok()
                .and_then(|mut s| {
                    if read_pulse(&mut s) {
                        s.subscribe();
                        Some(s)
                    } else {
                        None
                    }
                });
            // libpulse connects asynchronously: a refused connection shows as a failed read.
            self.refused.1 = snap && self.pulse.is_none();
        }
    }

    /// Waits about one tick and reports what changed.
    fn poll(&mut self, root: &SystemRoot, want_servers: bool) -> Vec<Cause> {
        let mut causes = vec![];
        match &mut self.pw {
            Some(s) => match s.step(TICK) {
                Ok(_) => {
                    if s.take_changed() {
                        causes.push(Cause::PipeWire);
                    }
                }
                Err(_) => {
                    self.pw = None;
                    causes.push(Cause::PipeWire);
                }
            },
            None => thread::sleep(TICK),
        }
        if let Some(s) = &mut self.pulse {
            match s.pump() {
                Ok(()) if s.take_changed() => causes.push(Cause::PulseAudio),
                Ok(()) => {}
                Err(_) => {
                    self.pulse = None;
                    causes.push(Cause::PulseAudio);
                }
            }
        }
        if let Some(i) = &self.inotify
            && let Ok(ev) = i.read_events()
            && !ev.is_empty()
        {
            causes.push(Cause::Kernel);
        }
        if want_servers && (self.pw.is_none() || self.pulse.is_none()) {
            self.connect_servers(root);
        }
        causes
    }

    fn prefilled(&mut self, causes: &[Cause]) -> Prefilled<'_> {
        let requery = causes.contains(&Cause::PulseAudio);
        let pulse = self.pulse.as_ref().map(PulseSession::dump);
        Prefilled {
            pipewire: self.pw.as_ref().map(PwSession::dump),
            pulse: if requery { None } else { pulse },
            pulse_session: if requery { self.pulse.as_mut() } else { None },
        }
    }
}

fn read_pulse(s: &mut PulseSession) -> bool {
    let until = Instant::now() + SETUP;
    loop {
        match s.step(Duration::from_millis(2)) {
            Ok(true) => return true,
            Ok(false) if Instant::now() < until => {}
            _ => return false,
        }
    }
}

fn run(
    root: SystemRoot,
    scope: ProbeScope,
    opts: ProbeOptions,
    shared: Arc<Shared>,
    tx: mpsc::Sender<WatchEvent>,
    stop: Arc<AtomicBool>,
) {
    let trace = std::env::var_os("AUDIOCAPS_WATCH_TRACE").is_some();
    let mut src = Sources::open(&root, &scope);
    let want_servers =
        matches!(root, SystemRoot::Live) && scope.layers.includes(&LayerKind::SoundServer);
    let probe = |src: &mut Sources, causes: &[Cause]| {
        let pre = src.prefilled(causes);
        let previous = shared.latest();
        let mut snap = probe_inner(&root, &scope, &opts, pre, previous.as_deref());
        snap.generation = shared.next_generation();
        shared.publish(snap)
    };
    let t0 = Instant::now();
    let mut prev = probe(&mut src, &[Cause::Initial]);
    let first = WatchEvent {
        snapshot: prev.clone(),
        diff: SnapshotDiff {
            from_generation: 0,
            to_generation: prev.generation,
            added: prev.endpoints.iter().map(|e| e.id.clone()).collect(),
            ..SnapshotDiff::default()
        },
        causes: vec![Cause::Initial],
        latency: t0.elapsed(),
    };
    if tx.send(first).is_err() {
        return;
    }
    while !stop.load(Ordering::SeqCst) {
        let mut causes = src.poll(&root, want_servers);
        if causes.is_empty() {
            continue;
        }
        let noticed = Instant::now();
        // Coalesce a burst (a new node arrives as several events within a millisecond).
        for c in src.poll(&root, want_servers) {
            if !causes.contains(&c) {
                causes.push(c);
            }
        }
        if causes.contains(&Cause::Kernel) {
            crate::sysfs::invalidate_cache();
            crate::alsa::live::invalidate_cache();
        }
        let coalesced = noticed.elapsed();
        let snap = probe(&mut src, &causes);
        let diff = snap.diff(&prev);
        let latency = noticed.elapsed();
        if trace {
            eprintln!(
                "watch: coalesce {:.2} ms, probe {:.2} ms, causes {causes:?}",
                coalesced.as_secs_f64() * 1e3,
                (latency - coalesced).as_secs_f64() * 1e3
            );
        }
        prev = snap.clone();
        if diff.is_empty() {
            continue;
        }
        let ev = WatchEvent {
            snapshot: snap,
            diff,
            causes,
            latency,
        };
        if tx.send(ev).is_err() {
            return;
        }
    }
}
