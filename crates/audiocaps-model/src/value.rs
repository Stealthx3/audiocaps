// SPDX-License-Identifier: LGPL-2.1-or-later
//! Values with provenance and confidence, and explicit unknowns.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scope::LayerKind;

/// A reported value: either known, with where it came from and how sure we are, or explicitly
/// unknown with the reason. A layer that can't be read is never reported as "unsupported".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Value<T> {
    /// The value was read or derived.
    Known {
        /// The value itself.
        value: T,
        /// Measured (read directly) or inferred (derived from tables or quirk data).
        confidence: Confidence,
        /// Where and when the value was read.
        provenance: Provenance,
    },
    /// The value could not be determined.
    Unknown {
        /// Why the value is unknown.
        reason: UnknownReason,
        /// Human-readable detail, e.g. the missing permission or the error text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl<T> Value<T> {
    /// A value read directly from its source.
    pub fn measured(value: T, provenance: Provenance) -> Self {
        Self::Known {
            value,
            confidence: Confidence::Measured,
            provenance,
        }
    }

    /// A value derived from tables, heuristics or quirk data.
    pub fn inferred(value: T, provenance: Provenance) -> Self {
        Self::Known {
            value,
            confidence: Confidence::Inferred,
            provenance,
        }
    }

    /// An unknown value with a reason and optional detail.
    pub fn unknown(reason: UnknownReason, detail: impl Into<Option<String>>) -> Self {
        Self::Unknown {
            reason,
            detail: detail.into(),
        }
    }

    /// The value, if known.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known { value, .. } => Some(value),
            Self::Unknown { .. } => None,
        }
    }

    /// Mutable access to the value, if known.
    pub fn known_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Known { value, .. } => Some(value),
            Self::Unknown { .. } => None,
        }
    }
}

/// How a known value was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Read directly from the system.
    Measured,
    /// Derived from tables, heuristics, correlation or quirk data.
    Inferred,
}

/// Why a value or layer is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    /// A sandbox permission or Snap plug is missing.
    NotGranted,
    /// The device or resource is held by another process.
    Busy,
    /// A required client library is not installed.
    LibraryMissing,
    /// The sound server or service is not running.
    ServiceNotRunning,
    /// The driver or service does not expose this information.
    NotExposed,
    /// The backend did not answer before its deadline.
    Timeout,
    /// The source was read but could not be parsed.
    ParseError,
    /// Reading the source failed for another reason.
    ReadError,
    /// The source exists in the plan but this build does not read it yet.
    NotImplemented,
    /// The value only exists after an opt-in call the application has not made (e.g. CEC).
    NotRequested,
    /// A sound server manages the device, so it is never opened directly; the value is read
    /// through the server instead.
    ManagedByServer,
}

/// Where and when a value was read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// The layer the value belongs to.
    pub layer: LayerKind,
    /// The concrete source.
    pub source: Source,
    /// When it was read, in milliseconds since the Unix epoch.
    pub read_at_ms: u64,
}

/// The concrete source of a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A file, relative to the system root (e.g. `/proc/asound/card0/eld#0.0`).
    File {
        /// Absolute path as seen by the probed system.
        path: String,
    },
    /// Output of a command-line tool (only in captures and the CLI).
    Command {
        /// Command name, e.g. `pw-dump`.
        name: String,
    },
    /// A property or parameter reported by a sound server.
    ServerParam {
        /// Server name, e.g. `pipewire`.
        server: String,
        /// Object identifier within the server.
        object: String,
        /// Property or parameter key.
        key: String,
    },
    /// A D-Bus property.
    DbusProperty {
        /// Bus name, e.g. `org.bluez`.
        service: String,
        /// Object path.
        path: String,
        /// Interface name.
        interface: String,
        /// Property name.
        property: String,
    },
    /// A built-in table (e.g. IEC 61937 bit rates).
    Table {
        /// Table name.
        name: String,
    },
    /// A quirk-data entry.
    Quirk {
        /// Quirk identifier.
        id: String,
    },
    /// A platform audio API call.
    Api {
        /// API, e.g. `alsa-lib`.
        api: String,
        /// Call or object, e.g. `snd_pcm_hw_params_any(hw:0,3)`.
        call: String,
    },
    /// Derived from other values.
    Derived {
        /// What it was derived from.
        from: String,
    },
}

impl Source {
    /// A file source.
    pub fn file(path: impl Into<String>) -> Self {
        Self::File { path: path.into() }
    }

    /// A command source.
    pub fn command(name: impl Into<String>) -> Self {
        Self::Command { name: name.into() }
    }

    /// A platform API source.
    pub fn api(api: impl Into<String>, call: impl Into<String>) -> Self {
        Self::Api {
            api: api.into(),
            call: call.into(),
        }
    }

    /// A derived source.
    pub fn derived(from: impl Into<String>) -> Self {
        Self::Derived { from: from.into() }
    }
}
