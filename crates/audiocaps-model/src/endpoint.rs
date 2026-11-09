// SPDX-License-Identifier: LGPL-2.1-or-later
//! Endpoints: the things an application plays to or records from.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caps::{CapabilitySet, CodecCapability, Speaker};
use crate::path::{EffectiveSet, PathReport};
use crate::redact::{Redact, RedactionPolicy, Sensitive};
use crate::scope::LayerKind;
use crate::value::{Confidence, UnknownReason, Value};

/// Stable endpoint identifier, e.g. `alsa:card1/pcm3p`. Stable across probes on the same system
/// while the hardware and configuration stay the same.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct EndpointId(pub String);

/// Endpoint class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointClass {
    /// Wired hardware: HDMI/DP, analog, S/PDIF, USB, internal.
    Hardware,
    /// Bluetooth.
    Bluetooth,
    /// Network sinks: RAOP/AirPlay, RTP, Snapcast.
    Network,
    /// Virtual: filter chains, combined sinks, loopbacks.
    Virtual,
}

/// Direction of an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Audio output.
    Playback,
    /// Audio input.
    Capture,
    /// A MIDI port (input, output or both).
    Midi,
}

/// The physical or logical transport of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// HDMI or DisplayPort audio.
    HdmiDp,
    /// Analog line, headphone, speaker or microphone.
    Analog,
    /// S/PDIF (coaxial or optical).
    Spdif,
    /// USB Audio Class device.
    Usb,
    /// Bluetooth (classic or LE Audio).
    Bluetooth,
    /// Network protocol, e.g. `raop`, `rtp`, `snapcast`.
    Network(String),
    /// Server-side virtual device, e.g. `filter-chain`, `combine`, `loopback`.
    Virtual(String),
    /// Something else, named.
    Other(String),
}

/// One endpoint with everything reported about it (within the probe scope).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Endpoint {
    /// Stable identifier.
    pub id: EndpointId,
    /// Class.
    pub class: EndpointClass,
    /// Direction.
    pub direction: Direction,
    /// Transport.
    pub transport: Value<Transport>,
    /// Display name. Redacted only when it is a user-chosen name (Bluetooth aliases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Value<EndpointName>>,
    /// Keys that join this endpoint across layers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub correlation: Vec<CorrelationKey>,
    /// The connected sink, for HDMI/DP and similar links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sink: Option<SinkRecord>,
    /// Capabilities as each layer reports them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerCapabilities>,
    /// What survives every layer, per access path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective: Vec<EffectiveSet>,
    /// Path reports, per access path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<PathReport>,
}

/// An endpoint name: a product/driver name in the clear, or a user-chosen name that is sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointName {
    /// A product or driver name, not personal.
    Plain(String),
    /// A user-chosen name (e.g. a Bluetooth alias).
    Personal(Sensitive),
}

/// A key that identifies this endpoint in one layer, with how sure the join is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CorrelationKey {
    /// The key.
    pub id: CorrelationId,
    /// Measured when the layer states the key directly; inferred when matched heuristically.
    pub confidence: Confidence,
}

/// Identifiers used to join an endpoint across layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorrelationId {
    /// ALSA card.
    AlsaCard {
        /// Card index.
        card: u32,
        /// Card id string (e.g. `HDMI`, `PCH`).
        id: String,
    },
    /// ALSA PCM device.
    AlsaPcm {
        /// Card index.
        card: u32,
        /// Device index.
        device: u32,
    },
    /// PipeWire node.
    PipewireNode {
        /// `node.name`.
        name: String,
    },
    /// PulseAudio sink or source.
    PulseDevice {
        /// Sink or source name.
        name: String,
    },
    /// DRM connector, e.g. `card1-HDMI-A-1`.
    DrmConnector {
        /// Connector name.
        name: String,
    },
    /// BlueZ device.
    BluezDevice {
        /// Device address.
        address: Sensitive,
    },
}

/// What the sink at the far end of an HDMI/DP link (or a USB/Bluetooth device) says it is and can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SinkRecord {
    /// Identity of the sink.
    pub identity: SinkIdentity,
    /// Short audio descriptors: formats the sink accepts.
    pub audio_descriptors: Value<Vec<CodecCapability>>,
    /// Speaker allocation.
    pub speaker_allocation: Value<Vec<Speaker>>,
    /// Audio sync delay reported by the sink, in milliseconds.
    pub sync_delay_ms: Value<u16>,
    /// Devices behind the sink (ARC/eARC receiver, TV behind a receiver), when known.
    pub downstream: Value<Vec<SinkIdentity>>,
}

/// Identity of a sink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SinkIdentity {
    /// Three-letter PNP manufacturer ID, e.g. `DON`.
    pub manufacturer_id: Value<String>,
    /// Manufacturer name resolved from the PNP ID.
    pub manufacturer_name: Value<String>,
    /// Product code.
    pub product_code: Value<u16>,
    /// Monitor or OSD name, e.g. `AVR-S660H`.
    pub model_name: Value<String>,
    /// Serial number (redacted on export by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<Sensitive>,
    /// Stable key apps can store settings under; derived from manufacturer, product and serial hash.
    pub sink_key: Value<String>,
}

/// One layer's view of an endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerCapabilities {
    /// The layer.
    pub layer: LayerKind,
    /// Whether the layer could be read.
    pub availability: Availability,
    /// What the layer reports. Fields outside the probe scope are omitted.
    pub caps: CapabilitySet,
}

/// Whether a layer could be read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability {
    /// Fully read.
    Read,
    /// Partly read; see the unknown fields.
    Partial,
    /// Not readable.
    Unknown {
        /// Why.
        reason: UnknownReason,
        /// Detail, e.g. the missing permission.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl Redact for Endpoint {
    fn redact(&mut self, policy: &RedactionPolicy) {
        self.name.redact(policy);
        self.correlation.redact(policy);
        self.sink.redact(policy);
    }
}

impl Redact for EndpointName {
    fn redact(&mut self, policy: &RedactionPolicy) {
        if let Self::Personal(s) = self {
            s.redact(policy);
        }
    }
}

impl Redact for CorrelationKey {
    fn redact(&mut self, policy: &RedactionPolicy) {
        if let CorrelationId::BluezDevice { address } = &mut self.id {
            address.redact(policy);
        }
    }
}

impl Redact for SinkRecord {
    fn redact(&mut self, policy: &RedactionPolicy) {
        self.identity.redact(policy);
        self.downstream.redact(policy);
    }
}

impl Redact for SinkIdentity {
    fn redact(&mut self, policy: &RedactionPolicy) {
        self.serial.redact(policy);
    }
}
