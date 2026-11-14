// SPDX-License-Identifier: LGPL-2.1-or-later
//! Whether a sound-server client may connect. A socket alone is not enough outside a sandbox:
//! with systemd socket activation, connecting would start a server that isn't running. So the
//! server process must be visible. Inside Flatpak or Snap processes are hidden; there the client
//! connects when the socket is granted (decision 2026-11-11).

use audiocaps_model::UnknownReason;

use crate::root::SystemRoot;
use crate::servers::{process_running, sandboxed};

/// Why a client won't connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    /// Reason.
    pub reason: UnknownReason,
    /// Detail.
    pub detail: String,
}

fn runtime_dir() -> Option<String> {
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|d| d.starts_with('/'))
}

/// Directories that may hold the user's sound-server sockets. Inside a snap `XDG_RUNTIME_DIR` is
/// the snap's private `/run/user/<uid>/snap.<name>`; the sockets the audio plugs grant are in its
/// parent.
pub(crate) fn socket_dirs() -> Vec<String> {
    let Some(d) = runtime_dir() else {
        return vec![];
    };
    let mut v = vec![d.clone()];
    if let Some((parent, last)) = d.rsplit_once('/')
        && last.starts_with("snap.")
        && !parent.is_empty()
    {
        v.push(parent.to_string());
    }
    v
}

/// The first `name` under [`socket_dirs`] that is a socket. (Flatpak leaves an empty placeholder
/// file at `pipewire-0` when the socket isn't granted, so existence isn't enough.)
pub(crate) fn find_socket(root: &SystemRoot, name: &str) -> Option<String> {
    socket_dirs()
        .into_iter()
        .map(|d| format!("{d}/{name}"))
        .find(|s| root.is_socket(s))
}

/// The PipeWire socket to connect to, if the passive rules allow it.
pub fn pipewire(root: &SystemRoot) -> Result<String, Skip> {
    let socket = match std::env::var("PIPEWIRE_REMOTE").ok() {
        Some(r) if r.starts_with('/') => root.is_socket(&r).then_some(r),
        Some(r) => find_socket(root, &r),
        None => find_socket(root, "pipewire-0"),
    };
    let Some(socket) = socket else {
        return Err(Skip {
            reason: if sandboxed(root) {
                UnknownReason::NotGranted
            } else {
                UnknownReason::ServiceNotRunning
            },
            detail: "no PipeWire socket".into(),
        });
    };
    if sandboxed(root) || process_running(root, &["pipewire"]) {
        Ok(socket)
    } else {
        Err(Skip {
            reason: UnknownReason::ServiceNotRunning,
            detail: format!("{socket} exists but no pipewire process runs; not starting it"),
        })
    }
}

/// Whether the PulseAudio client may connect, and to which server (`None`: `PULSE_SERVER`).
pub fn pulse(root: &SystemRoot) -> Result<Option<String>, Skip> {
    if std::env::var_os("PULSE_SERVER").is_some() {
        return Ok(None);
    }
    let Some(socket) = find_socket(root, "pulse/native") else {
        return Err(Skip {
            reason: if sandboxed(root) {
                UnknownReason::NotGranted
            } else {
                UnknownReason::ServiceNotRunning
            },
            detail: "no PulseAudio socket".into(),
        });
    };
    if sandboxed(root) || process_running(root, &["pulseaudio", "pipewire-pulse"]) {
        Ok(Some(format!("unix:{socket}")))
    } else {
        Err(Skip {
            reason: UnknownReason::ServiceNotRunning,
            detail: format!(
                "{socket} exists but no pulseaudio or pipewire-pulse process runs; not starting it"
            ),
        })
    }
}
