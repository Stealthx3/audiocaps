// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps system access.
//!
//! - [`root::SystemRoot`]: every reader works on the live system or a captured fixture tree.
//! - [`manifest`]: the capture manifest shared with the capture tool.
//! - [`probe::probe`]: the scoped probe that builds a [`audiocaps_model::Snapshot`].
//!
//! Subsystem backends (ALSA, PipeWire, PulseAudio, BlueZ, JACK) arrive in phases 3–5, each as its
//! own module reading through `SystemRoot` or its native client library.

pub mod manifest;
pub mod probe;
pub mod root;

pub use probe::probe;
pub use root::SystemRoot;
