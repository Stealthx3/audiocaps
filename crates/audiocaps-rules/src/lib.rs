// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps rule engine.
//!
//! Diagnostics are declarative rule packs: match an endpoint, compare fields across layers, emit a
//! [`audiocaps_model::Finding`] with a code and a distro-aware hint. Packs are bundled with each
//! release and can be overridden from a system directory or the app's bundle; nothing is fetched
//! from the network. The engine arrives in phase 7; this crate fixes the boundary now.

/// Version of the rule-pack format this build reads.
pub const RULE_PACK_FORMAT: u32 = 1;
