// SPDX-License-Identifier: LGPL-2.1-or-later
//! The runtime: one probe thread that owns every backend, runs the backends of a probe
//! concurrently in small steps with a deadline each, and publishes each result as a new snapshot
//! generation.
//!
//! - **Single-threaded executor.** [`run`] interleaves backend [`Task`]s round-robin on the
//!   calling thread. Each task does one bounded unit of work per step (one PCM, one card, one
//!   file group). A task that passes its backend's deadline is stopped and reports everything it
//!   didn't reach as `unknown` with reason `timeout`; the others carry on.
//! - **Probe thread.** [`Runtime`] owns the system root and runs probes on its own thread, so a
//!   backend call that blocks in the kernel can't stall the application: the caller waits at most
//!   the longest deadline plus a grace period, then gets a snapshot that says which layers timed
//!   out.
//! - **Publishing.** Each completed probe gets the next generation number and replaces the
//!   latest snapshot (an `Arc` swap under a short lock). Queries on a snapshot never lock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use audiocaps_model::{
    Availability, LayerKind, ProbeScope, SCHEMA_VERSION, Snapshot, SnapshotDiff, SystemLayer,
    UnknownReason,
};

use crate::root::SystemRoot;

/// Default deadline for each backend.
pub const DEFAULT_DEADLINE: Duration = Duration::from_millis(150);

/// Extra time the caller waits for the probe thread beyond the longest deadline.
const GRACE: Duration = Duration::from_millis(250);

/// The backends a probe runs. Deadlines are set per backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    /// alsa-lib: PCM ranges, controls, configuration.
    Alsa,
    /// procfs and sysfs: cards, codecs, USB streams, driver state, MIDI.
    Sysfs,
    /// Environment: os-release, kernel, sandbox, servers.
    Environment,
    /// Native PipeWire client.
    PipeWire,
    /// PulseAudio client (native PulseAudio or pipewire-pulse).
    PulseAudio,
    /// BlueZ over D-Bus.
    Bluez,
    /// JACK client (never starts a server).
    Jack,
    /// CEC bus (opt-in).
    Cec,
    /// Software codec check (opt-in).
    Codecs,
    /// Active probe (opt-in).
    Active,
}

/// Probe options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    /// Deadline for backends without their own; `None` waits for completion.
    pub default_deadline: Option<Duration>,
    /// Per-backend deadlines.
    pub deadlines: Vec<(Backend, Duration)>,
    /// Opt-in: query the CEC bus (sends query messages on HDMI-CEC). Off by default.
    pub cec: bool,
    /// Extra quirk-pack directories from the application's bundle; they override the bundled
    /// pack and the system directories.
    pub quirk_dirs: Vec<std::path::PathBuf>,
    /// Extra rule-pack directories from the application's bundle (override by code).
    pub rule_dirs: Vec<std::path::PathBuf>,
    /// What the application intends to play (for findings that compare against it).
    pub app: Option<audiocaps_model::AppRequest>,
    /// Opt-in: check which codec libraries the app could load.
    pub software_codecs: bool,
    /// Opt-in: play silence to one idle device to confirm formats.
    pub active_probe: Option<audiocaps_model::ActiveProbeRequest>,
    /// Redaction of sensitive values; `None` is the default policy (serials and Bluetooth
    /// addresses hashed, personal names masked).
    pub redaction: Option<audiocaps_model::RedactionPolicy>,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            default_deadline: Some(DEFAULT_DEADLINE),
            deadlines: vec![],
            cec: false,
            quirk_dirs: vec![],
            rule_dirs: vec![],
            app: None,
            software_codecs: false,
            active_probe: None,
            redaction: None,
        }
    }
}

impl ProbeOptions {
    /// No deadlines: every backend runs to completion (fixtures, tests, the capture tool).
    pub fn unbounded() -> Self {
        Self {
            default_deadline: None,
            deadlines: vec![],
            cec: false,
            quirk_dirs: vec![],
            rule_dirs: vec![],
            app: None,
            software_codecs: false,
            active_probe: None,
            redaction: None,
        }
    }

    /// Opts in to CEC queries, with a deadline that allows for slow buses (5 s unless set).
    pub fn with_cec(mut self) -> Self {
        self.cec = true;
        if self.default_deadline.is_some()
            && !self.deadlines.iter().any(|(b, _)| *b == Backend::Cec)
        {
            self.deadlines.push((Backend::Cec, Duration::from_secs(5)));
        }
        self
    }

    /// Opts in to the software codec check.
    pub fn with_software_codecs(mut self) -> Self {
        self.software_codecs = true;
        self
    }

    /// Opts in to an active probe of one endpoint (3 s deadline unless set).
    pub fn with_active_probe(mut self, req: audiocaps_model::ActiveProbeRequest) -> Self {
        self.active_probe = Some(req);
        if self.default_deadline.is_some()
            && !self.deadlines.iter().any(|(b, _)| *b == Backend::Active)
        {
            self.deadlines
                .push((Backend::Active, Duration::from_secs(3)));
        }
        self
    }

    /// Adds an application rule-pack directory.
    pub fn with_rule_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.rule_dirs.push(dir.into());
        self
    }

    /// Says what the application intends to play.
    pub fn with_app(mut self, app: audiocaps_model::AppRequest) -> Self {
        self.app = Some(app);
        self
    }

    /// Adds an application quirk-pack directory.
    pub fn with_quirk_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.quirk_dirs.push(dir.into());
        self
    }

    /// Sets one backend's deadline.
    pub fn with_deadline(mut self, backend: Backend, deadline: Duration) -> Self {
        self.deadlines.retain(|(b, _)| *b != backend);
        self.deadlines.push((backend, deadline));
        self
    }

    /// The deadline for `backend`.
    pub fn deadline(&self, backend: Backend) -> Option<Duration> {
        self.deadlines
            .iter()
            .find(|(b, _)| *b == backend)
            .map(|(_, d)| *d)
            .or(self.default_deadline)
    }

    /// The longest deadline over all backends, or `None` if any backend is unbounded.
    pub fn longest(&self) -> Option<Duration> {
        [
            Backend::Alsa,
            Backend::Sysfs,
            Backend::Environment,
            Backend::PipeWire,
            Backend::PulseAudio,
            Backend::Bluez,
            Backend::Jack,
            Backend::Cec,
            Backend::Codecs,
            Backend::Active,
        ]
        .iter()
        .filter(|b| **b != Backend::Cec || self.cec)
        .filter(|b| **b != Backend::Codecs || self.software_codecs)
        .filter(|b| **b != Backend::Active || self.active_probe.is_some())
        .map(|b| self.deadline(*b))
        .try_fold(Duration::ZERO, |acc, d| d.map(|d| acc.max(d)))
    }
}

/// One backend's work for one probe, done in steps.
pub(crate) trait Task<Out> {
    /// The backend, for its deadline.
    fn backend(&self) -> Backend;
    /// Does one bounded unit of work. Returns `true` when there is nothing left.
    fn step(&mut self) -> bool;
    /// Hands the results over. With `timed_out`, items not reached are reported as timeouts.
    fn finish(self: Box<Self>, out: &mut Out, timed_out: bool);
}

/// Runs tasks round-robin until each is done or past its deadline.
pub(crate) fn run<'a, Out>(
    tasks: Vec<Box<dyn Task<Out> + 'a>>,
    opts: &ProbeOptions,
    out: &mut Out,
) {
    let start = Instant::now();
    let trace = std::env::var_os("AUDIOCAPS_WATCH_TRACE").is_some();
    let mut live: Vec<(Box<dyn Task<Out> + 'a>, Option<Instant>)> = tasks
        .into_iter()
        .map(|t| {
            let d = opts.deadline(t.backend()).map(|d| start + d);
            (t, d)
        })
        .collect();
    while !live.is_empty() {
        let mut i = 0;
        while i < live.len() {
            let expired = live[i].1.is_some_and(|d| Instant::now() >= d);
            if expired {
                let (t, _) = live.swap_remove(i);
                t.finish(out, true);
                continue;
            }
            if live[i].0.step() {
                let (t, _) = live.swap_remove(i);
                if trace {
                    eprintln!(
                        "  task {:?} done at {:.2} ms",
                        t.backend(),
                        start.elapsed().as_secs_f64() * 1e3
                    );
                }
                t.finish(out, false);
                continue;
            }
            i += 1;
        }
    }
}

struct Request {
    generation: u64,
    scope: ProbeScope,
    opts: ProbeOptions,
    reply: mpsc::SyncSender<Snapshot>,
}

/// Generation counter and latest snapshot, shared by the probe thread and watches.
#[derive(Default)]
pub(crate) struct Shared {
    latest: Mutex<Option<Arc<Snapshot>>>,
    generation: AtomicU64,
}

impl Shared {
    pub(crate) fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Publishes `snap` unless a newer generation is already published.
    pub(crate) fn publish(&self, snap: Snapshot) -> Arc<Snapshot> {
        let snap = Arc::new(snap);
        let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        if latest
            .as_ref()
            .is_none_or(|l| l.generation < snap.generation)
        {
            *latest = Some(snap.clone());
        }
        snap
    }

    pub(crate) fn latest(&self) -> Option<Arc<Snapshot>> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// A probe context: the probe thread, the generation counter and the latest snapshot.
pub struct Runtime {
    tx: Option<mpsc::Sender<Request>>,
    shared: Arc<Shared>,
    root: SystemRoot,
    worker: Option<thread::JoinHandle<()>>,
}

impl Runtime {
    /// Starts the probe thread for `root`.
    pub fn new(root: SystemRoot) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Request>();
        let thread_root = root.clone();
        let shared = Arc::new(Shared::default());
        let thread_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("audiocaps-probe".into())
            .spawn(move || {
                let root = thread_root;
                for req in rx {
                    let previous = thread_shared.latest();
                    let mut snap = crate::probe::probe_inner(
                        &root,
                        &req.scope,
                        &req.opts,
                        crate::probe::Prefilled::default(),
                        previous.as_deref(),
                    );
                    snap.generation = req.generation;
                    // The caller may have given up; a late result is dropped.
                    let _ = req.reply.send(snap);
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            shared,
            root,
            worker: Some(worker),
        })
    }

    /// Probes `scope` and publishes the result as the latest snapshot. Returns within the longest
    /// backend deadline plus a short grace period, even if a backend call blocks.
    pub fn probe(&self, scope: &ProbeScope, opts: &ProbeOptions) -> Arc<Snapshot> {
        let generation = self.shared.next_generation();
        let (reply, rx) = mpsc::sync_channel(1);
        let sent = self.tx.as_ref().is_some_and(|tx| {
            tx.send(Request {
                generation,
                scope: scope.clone(),
                opts: opts.clone(),
                reply,
            })
            .is_ok()
        });
        let result = if !sent {
            Err("the probe thread has stopped")
        } else {
            match opts.longest() {
                Some(d) => rx
                    .recv_timeout(d + GRACE)
                    .map_err(|_| "the probe thread did not answer in time"),
                None => rx.recv().map_err(|_| "the probe thread has stopped"),
            }
        };
        match result {
            Ok(snap) => self.shared.publish(snap),
            Err(why) => Arc::new(timed_out(scope, generation, why)),
        }
    }

    /// The latest published snapshot.
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.shared.latest()
    }

    /// Watches `scope`: delivers a new snapshot and its diff whenever the kernel's sound devices
    /// or a sound server change. Only the watched scope is re-probed.
    pub fn watch(
        &self,
        scope: &ProbeScope,
        opts: &ProbeOptions,
    ) -> std::io::Result<crate::watch::Watch> {
        crate::watch::Watch::start(
            self.root.clone(),
            scope.clone(),
            opts.clone(),
            self.shared.clone(),
        )
    }

    /// Probes and returns the new snapshot with its changes against the previous latest one.
    pub fn probe_diff(
        &self,
        scope: &ProbeScope,
        opts: &ProbeOptions,
    ) -> (Arc<Snapshot>, Option<SnapshotDiff>) {
        let before = self.latest();
        let snap = self.probe(scope, opts);
        let diff = before.map(|b| snap.diff(&b));
        (snap, diff)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Closing the channel ends the thread once it finishes its current probe. It is not
        // joined: a probe stuck in the kernel must not hang the application on shutdown.
        self.tx.take();
        drop(self.worker.take());
    }
}

/// A snapshot for a probe whose thread didn't answer: every layer in scope is a timeout.
fn timed_out(scope: &ProbeScope, generation: u64, why: &str) -> Snapshot {
    let layers = LayerKind::ALL
        .iter()
        .filter(|l| scope.layers.includes(l))
        .map(|&layer| SystemLayer {
            layer,
            availability: Availability::Unknown {
                reason: UnknownReason::Timeout,
                detail: Some(why.to_string()),
            },
        })
        .collect();
    Snapshot {
        schema_version: SCHEMA_VERSION.to_string(),
        generation,
        taken_at_ms: 0,
        source: audiocaps_model::SnapshotSource::Live,
        scope: scope.clone(),
        redaction: audiocaps_model::RedactionPolicy::default(),
        layers,
        environment: None,
        routing: None,
        servers: vec![],
        endpoints: vec![],
        software_codecs: None,
        app_request: None,
        findings: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log {
        done: Vec<(&'static str, bool, usize)>,
    }

    struct Fake {
        name: &'static str,
        backend: Backend,
        steps: usize,
        did: usize,
        sleep: Duration,
    }

    impl Task<Log> for Fake {
        fn backend(&self) -> Backend {
            self.backend
        }
        fn step(&mut self) -> bool {
            thread::sleep(self.sleep);
            self.did += 1;
            self.did >= self.steps
        }
        fn finish(self: Box<Self>, out: &mut Log, timed_out: bool) {
            out.done.push((self.name, timed_out, self.did));
        }
    }

    #[test]
    fn slow_backend_times_out_others_complete() {
        let opts = ProbeOptions::default().with_deadline(Backend::Alsa, Duration::from_millis(30));
        let tasks: Vec<Box<dyn Task<Log>>> = vec![
            Box::new(Fake {
                name: "slow",
                backend: Backend::Alsa,
                steps: 1000,
                did: 0,
                sleep: Duration::from_millis(5),
            }),
            Box::new(Fake {
                name: "quick",
                backend: Backend::Sysfs,
                steps: 3,
                did: 0,
                sleep: Duration::ZERO,
            }),
        ];
        let start = Instant::now();
        let mut log = Log::default();
        run(tasks, &opts, &mut log);
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "{:?}",
            start.elapsed()
        );
        let slow = log.done.iter().find(|d| d.0 == "slow").unwrap();
        let quick = log.done.iter().find(|d| d.0 == "quick").unwrap();
        assert!(slow.1, "slow task must time out");
        assert!(slow.2 < 1000);
        assert_eq!((quick.1, quick.2), (false, 3));
    }

    #[test]
    fn unbounded_runs_to_completion() {
        let tasks: Vec<Box<dyn Task<Log>>> = vec![Box::new(Fake {
            name: "a",
            backend: Backend::Alsa,
            steps: 50,
            did: 0,
            sleep: Duration::ZERO,
        })];
        let mut log = Log::default();
        run(tasks, &ProbeOptions::unbounded(), &mut log);
        assert_eq!(log.done, vec![("a", false, 50)]);
    }

    #[test]
    fn longest_deadline() {
        assert_eq!(ProbeOptions::unbounded().longest(), None);
        let o = ProbeOptions::default().with_deadline(Backend::Sysfs, Duration::from_millis(400));
        assert_eq!(o.longest(), Some(Duration::from_millis(400)));
        let o = ProbeOptions::unbounded().with_deadline(Backend::Alsa, Duration::from_millis(5));
        assert_eq!(o.longest(), None);
    }
}
