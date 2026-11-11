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

/// The PipeWire remote to connect to, if the passive rules allow it.
pub fn pipewire(root: &SystemRoot) -> Result<(), Skip> {
    let explicit = std::env::var("PIPEWIRE_REMOTE").ok();
    let socket = match (&explicit, runtime_dir()) {
        (Some(r), _) if r.starts_with('/') => Some(r.clone()),
        (Some(r), Some(d)) => Some(format!("{d}/{r}")),
        (None, Some(d)) => Some(format!("{d}/pipewire-0")),
        _ => None,
    };
    let Some(socket) = socket.filter(|s| root.exists(s)) else {
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
        Ok(())
    } else {
        Err(Skip {
            reason: UnknownReason::ServiceNotRunning,
            detail: format!("{socket} exists but no pipewire process runs; not starting it"),
        })
    }
}

/// Whether the PulseAudio client may connect.
pub fn pulse(root: &SystemRoot) -> Result<(), Skip> {
    if std::env::var_os("PULSE_SERVER").is_some() {
        return Ok(());
    }
    let socket = runtime_dir()
        .map(|d| format!("{d}/pulse/native"))
        .filter(|s| root.exists(s));
    let Some(socket) = socket else {
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
        Ok(())
    } else {
        Err(Skip {
            reason: UnknownReason::ServiceNotRunning,
            detail: format!(
                "{socket} exists but no pulseaudio or pipewire-pulse process runs; not starting it"
            ),
        })
    }
}
