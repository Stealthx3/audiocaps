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
    /// Routing of the default device, when the routing field group is in scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::detail::Routing>,
    /// Sound servers, when the sound-server layer is in scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<crate::detail::ServerState>,
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

    /// What changed from `older` to this snapshot, by endpoint. Read times are ignored, so a
    /// re-read of the same state is no change.
    pub fn diff(&self, older: &Snapshot) -> SnapshotDiff {
        use std::collections::BTreeMap;
        let old: BTreeMap<_, _> = older.endpoints.iter().map(|e| (&e.id, e)).collect();
        let new: BTreeMap<_, _> = self.endpoints.iter().map(|e| (&e.id, e)).collect();
        let strip = |e: &Endpoint| {
            let mut v = serde_json::to_value(e).unwrap_or_default();
            strip_read_times(&mut v);
            v
        };
        let mut diff = SnapshotDiff {
            from_generation: older.generation,
            to_generation: self.generation,
            ..SnapshotDiff::default()
        };
        for (id, e) in &new {
            match old.get(id) {
                None => diff.added.push((*id).clone()),
                Some(o) if strip(o) != strip(e) => diff.changed.push((*id).clone()),
                Some(_) => {}
            }
        }
        diff.removed = old
            .keys()
            .filter(|id| !new.contains_key(*id))
            .map(|id| (*id).clone())
            .collect();
        let sys = |s: &Snapshot| {
            let mut v = serde_json::json!([s.layers, s.environment, s.routing, s.servers]);
            strip_read_times(&mut v);
            v
        };
        diff.system_changed = sys(self) != sys(older);
        diff
    }
}

fn strip_read_times(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.remove("read_at_ms");
            m.values_mut().for_each(strip_read_times);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(strip_read_times),
        _ => {}
    }
}

/// Changes between two snapshots of the same probe context.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotDiff {
    /// Generation of the older snapshot.
    pub from_generation: u64,
    /// Generation of the newer snapshot.
    pub to_generation: u64,
    /// Endpoints that appeared.
    pub added: Vec<crate::endpoint::EndpointId>,
    /// Endpoints that disappeared.
    pub removed: Vec<crate::endpoint::EndpointId>,
    /// Endpoints whose reported values changed.
    pub changed: Vec<crate::endpoint::EndpointId>,
    /// System-wide parts (layers, environment, routing, servers) changed.
    pub system_changed: bool,
}

impl SnapshotDiff {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && !self.system_changed
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
        /// Permissions the app lacks, with the layers they hide and the fix.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        missing: Vec<crate::detail::PermissionGap>,
    },
    /// Snap.
    Snap {
        /// Connected plugs relevant to audio (inferred from what the snap can reach).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        connected: Vec<String>,
        /// Plugs the snap lacks, with the layers they hide and the fix.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        missing: Vec<crate::detail::PermissionGap>,
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
