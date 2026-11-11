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
}

/// Probe options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    /// Deadline for backends without their own; `None` waits for completion.
    pub default_deadline: Option<Duration>,
    /// Per-backend deadlines.
    pub deadlines: Vec<(Backend, Duration)>,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            default_deadline: Some(DEFAULT_DEADLINE),
            deadlines: vec![],
        }
    }
}

impl ProbeOptions {
    /// No deadlines: every backend runs to completion (fixtures, tests, the capture tool).
    pub fn unbounded() -> Self {
        Self {
            default_deadline: None,
            deadlines: vec![],
        }
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
        [Backend::Alsa, Backend::Sysfs, Backend::Environment]
            .iter()
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

/// A probe context: the probe thread, the generation counter and the latest snapshot.
pub struct Runtime {
    tx: Option<mpsc::Sender<Request>>,
    latest: Mutex<Option<Arc<Snapshot>>>,
    generation: AtomicU64,
    worker: Option<thread::JoinHandle<()>>,
}

impl Runtime {
    /// Starts the probe thread for `root`.
    pub fn new(root: SystemRoot) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Request>();
        let worker = thread::Builder::new()
            .name("audiocaps-probe".into())
            .spawn(move || {
                for req in rx {
                    let mut snap = crate::probe::probe_with(&root, &req.scope, &req.opts);
                    snap.generation = req.generation;
                    // The caller may have given up; a late result is dropped.
                    let _ = req.reply.send(snap);
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            latest: Mutex::new(None),
            generation: AtomicU64::new(0),
            worker: Some(worker),
        })
    }

    /// Probes `scope` and publishes the result as the latest snapshot. Returns within the longest
    /// backend deadline plus a short grace period, even if a backend call blocks.
    pub fn probe(&self, scope: &ProbeScope, opts: &ProbeOptions) -> Arc<Snapshot> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
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
            Ok(snap) => {
                let snap = Arc::new(snap);
                let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
                // Never replace a newer generation with an older one.
                if latest
                    .as_ref()
                    .is_none_or(|l| l.generation < snap.generation)
                {
                    *latest = Some(snap.clone());
                }
                snap
            }
            Err(why) => Arc::new(timed_out(scope, generation, why)),
        }
    }

    /// The latest published snapshot.
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
        endpoints: vec![],
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
