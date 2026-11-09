// SPDX-License-Identifier: LGPL-2.1-or-later
//! Snapshots: the immutable result of one scoped probe, and the top-level JSON export.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::endpoint::Endpoint;
use crate::finding::Finding;
use crate::redact::{Redact, RedactionPolicy};
use crate::scope::ProbeScope;
use crate::value::Value;

/// The result of one probe. Contains only what the scope asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "AudioCaps snapshot")]
pub struct Snapshot {
    /// Schema version, e.g. `1.0.0`.
    pub schema_version: String,
    /// Generation number; increases with each new snapshot from the same probe context.
    pub generation: u64,
    /// When the probe ran, in milliseconds since the Unix epoch (capture time for fixtures).
    pub taken_at_ms: u64,
    /// What was probed.
    pub source: SnapshotSource,
    /// The scope that was asked for.
    pub scope: ProbeScope,
    /// Whether sensitive values have been redacted, and how.
    pub redaction: RedactionPolicy,
    /// System-wide availability of each layer in scope.
    pub layers: Vec<SystemLayer>,
    /// Environment layer, when in scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentInfo>,
    /// Endpoints in scope, sorted by id.
    pub endpoints: Vec<Endpoint>,
    /// Findings, when in scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

impl Snapshot {
    /// Applies `policy` to every sensitive value and records it.
    pub fn redact(&mut self, policy: &RedactionPolicy) {
        self.endpoints.redact(policy);
        self.redaction = policy.clone();
    }
}

/// Whether a layer could be read on this system at all (independent of endpoints).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemLayer {
    /// The layer.
    pub layer: crate::scope::LayerKind,
    /// Whether it could be read.
    pub availability: crate::endpoint::Availability,
}

/// What a snapshot was taken from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotSource {
    /// The running system.
    Live,
    /// A captured fixture tree.
    Fixture {
        /// Fixture name.
        name: String,
    },
}

/// The environment the library runs in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnvironmentInfo {
    /// Operating system release.
    pub os: Value<OsInfo>,
    /// Kernel version string.
    pub kernel: Value<String>,
    /// Sandbox the app runs in.
    pub sandbox: Value<Sandbox>,
    /// Sound servers found.
    pub servers: Vec<ServerPresence>,
}

/// Operating system release (from os-release on Linux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OsInfo {
    /// `ID`, e.g. `debian`.
    pub id: String,
    /// `VERSION_ID`, e.g. `13`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    /// Distro family, used to pick remediation hints.
    pub family: DistroFamily,
}

/// Distro families for remediation hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DistroFamily {
    /// Debian, Ubuntu and derivatives.
    Debian,
    /// Fedora, RHEL and derivatives.
    Fedora,
    /// Arch and derivatives.
    Arch,
    /// openSUSE and SUSE.
    Suse,
    /// Anything else (generic hints).
    Other,
}

/// Sandbox type and what it grants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Sandbox {
    /// Not sandboxed.
    None,
    /// Flatpak.
    Flatpak {
        /// Granted permissions relevant to audio, as written in `/.flatpak-info`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        granted: Vec<String>,
    },
    /// Snap.
    Snap {
        /// Connected plugs relevant to audio.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        connected: Vec<String>,
    },
}

/// Sound servers and services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SoundServer {
    /// PipeWire.
    PipeWire,
    /// WirePlumber session manager.
    WirePlumber,
    /// PulseAudio API (native server or pipewire-pulse).
    PulseAudio,
    /// JACK.
    Jack,
    /// BlueZ.
    Bluez,
}

/// Whether a server is present, and what it reports about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ServerPresence {
    /// The server.
    pub server: SoundServer,
    /// What is known about it.
    pub info: Value<ServerInfo>,
}

/// What a server reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServerInfo {
    /// Server name as reported, e.g. `PulseAudio (on PipeWire 1.4.2)`.
    pub name: String,
    /// Server version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}
