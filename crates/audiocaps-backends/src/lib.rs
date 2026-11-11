// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps system access.
//!
//! - [`root::SystemRoot`]: every reader works on the live system or a captured fixture tree.
//! - [`manifest`]: the capture manifest shared with the capture tool.
//! - [`runtime::Runtime`]: the probe thread with per-backend deadlines and snapshot generations.
//! - [`probe::probe`]: one scoped probe without deadlines (fixtures, tests, tools).
//! - [`alsa`]: the ALSA backend (alsa-lib reader and its serializable dump).
//!
//! PipeWire, PulseAudio, BlueZ and JACK backends arrive in phases 4–5.

pub mod alsa;
mod assemble;
mod environment;
pub mod manifest;
pub mod probe;
pub mod root;
pub mod runtime;
pub mod servers;
mod sysfs;

pub use probe::{probe, probe_with};
pub use root::SystemRoot;
pub use runtime::{Backend, ProbeOptions, Runtime};
