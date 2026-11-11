// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps data model.
//!
//! OS-neutral types for everything AudioCaps reports: the probe [`ProbeScope`], endpoints and their
//! per-layer capability sets, effective sets per access path, path reports with hops, findings,
//! provenance and confidence, and explicit unknowns. The JSON schema (`schema/`) is generated from
//! these types; see [`schema::snapshot_schema`].
//!
//! Nothing here assumes Linux: Linux-specific identifiers appear only as data inside
//! [`endpoint::CorrelationId`] variants and [`value::Source`] locators.

pub mod caps;
pub mod detail;
pub mod endpoint;
pub mod finding;
pub mod path;
pub mod redact;
pub mod schema;
pub mod scope;
pub mod snapshot;
pub mod value;

pub use caps::*;
pub use detail::*;
pub use endpoint::*;
pub use finding::*;
pub use path::*;
pub use redact::{RedactionMode, RedactionPolicy, Sensitive, SensitiveKind, SensitiveValue};
pub use scope::*;
pub use snapshot::*;
pub use value::*;

/// Version of the JSON schema this crate produces. Within major version 1, changes are additive
/// only (new optional fields, new enum variants); see `schema/README.md`.
pub const SCHEMA_VERSION: &str = "1.1.0";
