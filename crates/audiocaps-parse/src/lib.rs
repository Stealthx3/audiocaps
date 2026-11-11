// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps parsers: pure functions over bytes and text. No I/O, so every parser can be fuzzed
//! directly (see `fuzz/`).
//!
//! - [`eld`] — raw ELD bytes and the kernel's `eld#X.Y` text.
//! - [`edid`] — EDID base block and CTA-861 extensions; serial redaction for captures.
//! - [`sad`] — CTA-861 Short Audio Descriptors, shared by ELD and EDID.
//! - [`hda`] — HDA codec dumps (`codec#N`).
//! - [`usb`] — USB Audio Class descriptors (sysfs `descriptors`).
//! - [`procfs`] — ALSA card and PCM lists, PCM `info`/`status`, USB `stream#` files, `hw_params`,
//!   raw MIDI devices, sequencer clients.
//! - [`bt`] — Bluetooth audio codec and profile identification from BlueZ data.
//! - [`alsa_conf`] — alsa-lib configuration syntax and PCM plugin-chain resolution.
//! - [`os_release`] — `/etc/os-release`.
//!
//! Parsers never panic on any input; malformed input yields an error, `None`, or a partial result
//! with a `truncated` flag.

pub mod alsa_conf;
pub mod bt;
pub mod edid;
pub mod eld;
pub mod hda;
pub mod os_release;
pub mod procfs;
pub mod sad;
pub mod usb;
