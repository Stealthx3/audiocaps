// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps system access.
//!
//! - [`root::SystemRoot`]: every reader works on the live system or a captured fixture tree.
//! - [`manifest`]: the capture manifest shared with the capture tool.
//! - [`runtime::Runtime`]: the probe thread with per-backend deadlines and snapshot generations.
//! - [`probe::probe`]: one scoped probe without deadlines (fixtures, tests, tools).
//! - [`watch`]: change events (hotplug, server changes) for a watched scope.
//! - [`alsa`]: the ALSA backend (alsa-lib reader and its serializable dump).
//!
//! - [`server`]: PipeWire and PulseAudio backends (live clients and captured-output parsers).
//!
//! - [`bluez`], [`jack`]: BlueZ over D-Bus and JACK (loaded at run time).

pub mod alsa;
mod assemble;
pub mod bluez;
pub mod cec;
mod environment;
pub mod jack;
pub mod manifest;
pub mod probe;
pub mod root;
pub mod runtime;
mod sandbox;
pub mod server;
pub mod servers;
mod sysfs;
pub mod watch;

pub use probe::{probe, probe_with};
pub use root::SystemRoot;
pub use runtime::{Backend, ProbeOptions, Runtime};
