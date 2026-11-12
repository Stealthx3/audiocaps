// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps declarative data.
//!
//! - [`quirks`]: sink quirk data, matched by manufacturer, product and model name (phase 6).
//! - Rule packs: match an endpoint, compare fields across layers, emit a
//!   [`audiocaps_model::Finding`] with a code and a distro-aware hint (phase 7).
//!
//! Packs are bundled with each release and can be overridden from a system directory or the app's
//! bundle; nothing is fetched from the network. This crate does no I/O: callers pass pack text in.

pub mod quirks;

/// Version of the rule-pack format this build reads.
pub const RULE_PACK_FORMAT: u32 = 1;
