// SPDX-License-Identifier: LGPL-2.1-or-later
//! Passive detection of a sound server that manages ALSA devices.
//!
//! The ALSA backend never opens a PCM while a server manages it. In phase 3 that is decided
//! conservatively: if any PipeWire, PulseAudio or JACK server is found, every card counts as
//! managed. Nothing connects to a server here: the checks only look for server sockets in the
//! user's runtime directory and for server process names in `/proc/<pid>/comm`. Neither starts a
//! server (PulseAudio autospawn happens only on a client connect, which this never does).
//!
//! Inside Flatpak or Snap the host's processes are hidden and a server socket is visible only when
//! granted, so a server can't be ruled out: the sandbox counts as managed.

use crate::root::SystemRoot;

/// Process names that mean a server may hold or reopen ALSA devices.
const SERVER_PROCESSES: &[(&str, &str)] = &[
    ("pipewire", "pipewire"),
    ("pulseaudio", "pulseaudio"),
    ("jackd", "jack"),
    ("jackdbus", "jack"),
];

/// Upper bound on processes inspected, so a huge process table can't slow a probe.
const MAX_PIDS: usize = 8192;

/// The first server found that manages ALSA devices, by name (`pipewire`, `pulseaudio`, `jack`).
pub fn managing_server(root: &SystemRoot) -> Option<String> {
    match root {
        SystemRoot::Live => live(root),
        SystemRoot::Fixture(_) => fixture(root),
    }
}

/// Name reported when a sandbox hides whether a server runs.
pub const SANDBOX_UNKNOWN: &str = "a host sound server (hidden by the sandbox)";

fn live(root: &SystemRoot) -> Option<String> {
    let found = live_found(root);
    let sandboxed = sandboxed(root);
    match found {
        Some(s) => Some(s),
        None if sandboxed => Some(SANDBOX_UNKNOWN.into()),
        None => None,
    }
}

fn live_found(root: &SystemRoot) -> Option<String> {
    if crate::server::presence::find_socket(root, "pipewire-0").is_some() {
        return Some("pipewire".into());
    }
    if crate::server::presence::find_socket(root, "pulse/native").is_some() {
        return Some("pulseaudio".into());
    }
    let entries = root.read_dir("/proc").ok()?;
    entries
        .iter()
        .filter(|e| e.name.bytes().all(|b| b.is_ascii_digit()))
        .take(MAX_PIDS)
        .find_map(|e| {
            let comm = root.read_string(&format!("/proc/{}/comm", e.name)).ok()?;
            let comm = comm.trim();
            SERVER_PROCESSES
                .iter()
                .find(|(p, _)| *p == comm)
                .map(|(_, s)| s.to_string())
        })
}

fn fixture(root: &SystemRoot) -> Option<String> {
    let ran = |name: &str| {
        root.captured_command(name)
            .is_some_and(|(c, _)| c.exit_code == Some(0))
    };
    if ran("pw-dump") {
        Some("pipewire".into())
    } else if ran("pactl-info") {
        Some("pulseaudio".into())
    } else {
        None
    }
}

/// Whether a process with one of these names is visible in `/proc`.
pub fn process_running(root: &SystemRoot, names: &[&str]) -> bool {
    root.read_dir("/proc")
        .map(|entries| {
            entries
                .iter()
                .filter(|e| e.name.bytes().all(|b| b.is_ascii_digit()))
                .take(MAX_PIDS)
                .any(|e| {
                    root.read_string(&format!("/proc/{}/comm", e.name))
                        .is_ok_and(|c| names.contains(&c.trim()))
                })
        })
        .unwrap_or(false)
}

/// Whether the process runs inside Flatpak or Snap.
pub fn sandboxed(root: &SystemRoot) -> bool {
    root.exists("/.flatpak-info") || std::env::var_os("SNAP_NAME").is_some()
}
