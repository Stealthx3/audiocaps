// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps parsers: pure functions over bytes and text. No I/O, no allocation beyond results,
//! so every parser can be fuzzed directly.
//!
//! Phase 1 holds only what the capture tool and fixture replay need: the ALSA procfs card and
//! PCM lists, os-release, and EDID serial redaction. ELD, full EDID/CTA-861, HDA codec and USB
//! descriptor parsers arrive in phase 2.

pub mod edid;
pub mod os_release;
pub mod procfs;
