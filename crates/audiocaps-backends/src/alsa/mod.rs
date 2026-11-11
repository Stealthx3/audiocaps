// SPDX-License-Identifier: LGPL-2.1-or-later
//! ALSA backend.
//!
//! - [`dump`]: what the backend reads through alsa-lib, as a serializable record. The live probe
//!   produces it for the scoped devices; the capture tool saves a full one (`commands/alsa.json`)
//!   so fixtures replay the same path.
//! - [`live`]: the alsa-lib reader. It opens a PCM (non-blocking) only when no sound server
//!   manages the cards and the device has a free sub-device, reads the hardware parameter ranges
//!   and channel maps, and closes it again without installing any parameters. Control devices are
//!   read, never written.

pub mod dump;
pub mod live;
