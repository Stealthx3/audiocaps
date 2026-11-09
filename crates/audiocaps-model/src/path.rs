// SPDX-License-Identifier: LGPL-2.1-or-later
//! Access paths, effective sets and path reports.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caps::{CapabilitySet, Codec, SampleFormat};
use crate::endpoint::Availability;
use crate::finding::FindingCode;
use crate::scope::{FieldGroup, LayerKind};
use crate::value::Value;

/// The API an application uses to reach an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccessPath {
    /// alsa-lib directly (`hw:`, `plughw:` or a named PCM).
    AlsaDirect,
    /// PulseAudio API (native server or pipewire-pulse).
    PulseAudio,
    /// Native PipeWire.
    PipeWire,
    /// JACK (jack2 or pipewire-jack).
    Jack,
}

/// What survives every layer for one access path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectiveSet {
    /// The access path.
    pub access_path: AccessPath,
    /// Whether the path could be evaluated.
    pub availability: Availability,
    /// The effective capabilities.
    pub caps: CapabilitySet,
    /// Which layer limits which field group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limited_by: Vec<Limit>,
}

/// One layer limiting one field group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Limit {
    /// The limiting layer.
    pub layer: LayerKind,
    /// The limited field group.
    pub field: FieldGroup,
}

/// The hops from the application to the device for one access path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathReport {
    /// The access path.
    pub access_path: AccessPath,
    /// Hops in order, application first.
    pub hops: Vec<Hop>,
    /// Index into `hops` of the bottleneck, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottleneck: Option<u16>,
    /// End-to-end latency estimate in microseconds.
    pub latency_us: Value<u32>,
}

/// Kinds of hop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HopKind {
    /// The application's requested format.
    Application,
    /// The sound server graph.
    SoundServer,
    /// ALSA plugin chain.
    Alsa,
    /// Kernel driver and host hardware (pin, USB interface).
    DriverHost,
    /// HDMI/DP link (video timing, TMDS/FRL, adapters).
    Link,
    /// Bluetooth link (codec, profile).
    BluetoothLink,
    /// The sink.
    Sink,
    /// Devices behind the sink (ARC/eARC).
    SinkChain,
    /// The whole path (for findings that are not about one hop).
    WholePath,
}

/// One hop in a path report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hop {
    /// Kind of hop.
    pub kind: HopKind,
    /// Short label, e.g. `alsa hw:1,3`.
    pub label: String,
    /// Format carried at this hop.
    pub format: Value<StreamFormat>,
    /// Bit rate carried, in bit/s.
    pub bitrate_bps: Value<u64>,
    /// Maximum bit rate this hop can carry, in bit/s.
    pub ceiling_bps: Value<u64>,
    /// Latency added by this hop, in microseconds.
    pub latency_us: Value<u32>,
    /// What is lost at this hop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub losses: Vec<Loss>,
}

/// A stream format at a hop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamFormat {
    /// Codec (LPCM or compressed).
    pub codec: Codec,
    /// PCM sample format (for LPCM, or the IEC 61937 carrier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_format: Option<SampleFormat>,
    /// Sample rate in Hz.
    pub rate_hz: u32,
    /// Channels.
    pub channels: u8,
    /// IEC 61937 carrier layout, for compressed passthrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iec61937: Option<Iec61937Layout>,
}

/// IEC 61937 carrier layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Iec61937Layout {
    /// 2 channels at 48 kHz (AC-3, DTS), about 1.5 Mbit/s.
    Stereo48k,
    /// 2 channels at 192 kHz (E-AC-3), about 6.1 Mbit/s.
    Stereo192k,
    /// HBR: 8 channels at 192 kHz (TrueHD, DTS-HD MA, MAT), about 24.6 Mbit/s.
    Hbr,
}

/// Something lost at a hop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Loss {
    /// Kind of loss.
    pub kind: LossKind,
    /// Detail, e.g. `96000 → 48000 Hz`.
    pub detail: String,
    /// The finding that explains it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding: Option<FindingCode>,
}

/// Kinds of loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LossKind {
    /// Sample-rate conversion.
    Resample,
    /// Channel downmix.
    Downmix,
    /// Bit depth reduced.
    BitDepth,
    /// Software volume alters samples.
    Volume,
    /// Filter or EQ alters samples.
    Filter,
    /// Compressed format dropped or decoded to PCM.
    CodecDropped,
    /// Re-encoded to another codec.
    Transcode,
}
