// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sound-server backends (PipeWire, PulseAudio).
//!
//! Each server is read into one [`ServerDump`]: from the live client library ([`pw_live`],
//! [`pulse_live`]) or, for a fixture, from the captured `pw-dump` / `pactl -f json` output
//! ([`pwdump`], [`pactl`]). Assembly turns a dump into the model the same way in both cases, and
//! the phase 4 gate compares the two sources on a running system.
//!
//! Passive rules: nothing is written (no profile, route, volume or default changes), no stream is
//! created, PulseAudio is connected with autospawn disabled, and a client connects only when the
//! server already runs (outside a sandbox) — connecting to a socket-activated server that isn't
//! running would start it.

pub mod pactl;
pub mod presence;
pub mod pulse_live;
pub mod pw_live;
pub mod pwdump;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which server a dump describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    /// Native PipeWire.
    PipeWire,
    /// The PulseAudio API (native PulseAudio or pipewire-pulse).
    PulseAudio,
}

/// One server, as read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerDump {
    /// Server kind.
    pub kind: ServerKind,
    /// Server name as reported, e.g. `pipewire-0`, `PulseAudio (on PipeWire 1.4.2)`.
    pub name: String,
    /// Version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Graph clock (PipeWire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<Clock>,
    /// Default sink (node or sink name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_sink: Option<String>,
    /// Default source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
    /// Session-manager settings (WirePlumber `sm-settings`), as text.
    #[serde(default)]
    pub policy: BTreeMap<String, String>,
    /// Session manager, e.g. `WirePlumber 0.5.8`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_manager: Option<String>,
    /// Devices (PipeWire devices, PulseAudio cards).
    #[serde(default)]
    pub devices: Vec<Device>,
    /// Audio nodes (PipeWire nodes, PulseAudio sinks and sources).
    #[serde(default)]
    pub nodes: Vec<Node>,
}

/// PipeWire graph clock settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    /// Current graph rate.
    pub rate: Option<u32>,
    /// Allowed rates.
    pub allowed_rates: Vec<u32>,
    /// Quantum.
    pub quantum: Option<u32>,
    /// Minimum quantum.
    pub min_quantum: Option<u32>,
    /// Maximum quantum.
    pub max_quantum: Option<u32>,
    /// Forced rate (0 = none).
    pub force_rate: Option<u32>,
    /// Forced quantum (0 = none).
    pub force_quantum: Option<u32>,
}

/// A device / card.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// Server object id (PipeWire id, PulseAudio card index).
    pub id: u32,
    /// Name, e.g. `alsa_card.pci-0000_00_1f.3`.
    pub name: String,
    /// Device API, e.g. `alsa`, `bluez5`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// ALSA card index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alsa_card: Option<u32>,
    /// Profiles.
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// Active profile name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    /// Routes (ports).
    #[serde(default)]
    pub routes: Vec<Route>,
    /// Selected properties (`bluez5.*`, `device.bus`, …).
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

/// A device profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Name, e.g. `output:hdmi-stereo`, `a2dp-sink-ldac`.
    pub name: String,
    /// Description.
    pub description: String,
    /// Availability: `Some(false)` when the server says unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
}

/// A route (port).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Name, e.g. `hdmi-output-0`.
    pub name: String,
    /// Description.
    pub description: String,
    /// `output` or `input`.
    pub direction: String,
    /// Availability: `Some(false)` when unplugged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// Active.
    pub active: bool,
    /// Device (node) indices the route belongs to (PipeWire).
    #[serde(default)]
    pub devices: Vec<u32>,
    /// `iec958.codecs.detected` from the route info (PipeWire), e.g. `["PCM","AC3"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_codecs: Option<Vec<String>>,
}

/// Parses a PipeWire route info list (`key, value, key, value, …`) for the detected codecs.
pub(crate) fn detected_codecs<'a>(pairs: impl Iterator<Item = &'a str>) -> Option<Vec<String>> {
    let v: Vec<&str> = pairs.collect();
    v.chunks(2)
        .find(|kv| kv.len() == 2 && kv[0] == "iec958.codecs.detected")
        .and_then(|kv| serde_json::from_str::<Vec<String>>(kv[1]).ok())
}

/// Node direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeDir {
    /// Sink.
    Sink,
    /// Source.
    Source,
}

/// An audio node (sink or source).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Server object id.
    pub id: u32,
    /// Name (`node.name` / sink name).
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Media class, e.g. `Audio/Sink`.
    #[serde(default)]
    pub media_class: String,
    /// Direction, when this is a sink or source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<NodeDir>,
    /// Owning device id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<u32>,
    /// Device API (`alsa`, `bluez5`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Factory (`api.alsa.pcm.sink`, `support.null-audio-sink`, …) or PulseAudio driver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory: Option<String>,
    /// ALSA card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alsa_card: Option<u32>,
    /// ALSA PCM device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alsa_device: Option<u32>,
    /// State: `running`, `idle`, `suspended`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Negotiated format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    /// Formats the node offers.
    #[serde(default)]
    pub enum_formats: Vec<FormatRange>,
    /// Enabled IEC 958 passthrough codecs (PipeWire `iec958Codecs`, PulseAudio sink formats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iec958_codecs: Option<Vec<String>>,
    /// Volume, percent (average over channels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_percent: Option<u32>,
    /// Muted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    /// Profile of the owning device that created this node (`device.profile.name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Selected properties (`api.bluez5.codec`, `node.virtual`, …).
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

/// A negotiated format.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Format {
    /// `raw`, `iec958`, `dsd`.
    pub subtype: String,
    /// Sample format, pw-dump style (`S16LE`) or IEC 958 codec (`AC3`).
    pub format: String,
    /// Rate.
    pub rate: u32,
    /// Channels.
    pub channels: u32,
}

/// A format range from `EnumFormat`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatRange {
    /// `raw`, `iec958`, `dsd`.
    pub subtype: String,
    /// Sample formats (raw) or IEC 958 codecs (iec958).
    pub formats: Vec<String>,
    /// Discrete rates, when listed.
    pub rates: Vec<u32>,
    /// Rate range, when given as a range.
    pub rate_range: Option<(u32, u32)>,
    /// Channel range.
    pub channels: Option<(u32, u32)>,
    /// Channel positions.
    pub positions: Vec<String>,
}

/// Property keys kept on nodes and devices.
pub(crate) const KEPT_PROPS: &[&str] = &[
    "api.bluez5.codec",
    "api.bluez5.address",
    "api.bluez5.profile",
    "bluez5.codecs",
    "bluez5.a2dp.ldac.quality",
    "bluez5.profile",
    "device.bus",
    "device.form-factor",
    "node.virtual",
    "node.driver",
    "audio.position",
    "factory.name",
    "media.name",
    "device.profile.name",
    "api.alsa.path",
];

/// Whether a property is kept.
pub(crate) fn kept(key: &str) -> bool {
    KEPT_PROPS.contains(&key)
}

/// Parses a leading integer out of strings like `"[ 44100 48000 ]"`.
pub(crate) fn int_list(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter_map(|t| t.parse().ok())
        .collect()
}

/// Reads one server live, outside the runtime (tools and the gate). Applies the same passive
/// connect rules as a probe.
pub fn read_live(
    root: &crate::SystemRoot,
    kind: ServerKind,
    timeout: std::time::Duration,
) -> Result<ServerDump, String> {
    let deadline = std::time::Instant::now() + timeout;
    let slice = std::time::Duration::from_millis(5);
    match kind {
        ServerKind::PipeWire => {
            let socket = presence::pipewire(root).map_err(|s| s.detail)?;
            let mut s = pw_live::PwSession::connect(&socket)?;
            while !s.step(slice)? {
                if std::time::Instant::now() > deadline {
                    return Err("timed out".into());
                }
            }
            Ok(s.dump())
        }
        ServerKind::PulseAudio => {
            let server = presence::pulse(root).map_err(|s| s.detail)?;
            let mut s = pulse_live::PulseSession::connect(server.as_deref())?;
            while !s.step(slice)? {
                if std::time::Instant::now() > deadline {
                    return Err("timed out".into());
                }
            }
            Ok(s.dump())
        }
    }
}
