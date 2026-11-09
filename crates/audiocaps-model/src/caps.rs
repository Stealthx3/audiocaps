// SPDX-License-Identifier: LGPL-2.1-or-later
//! Capability sets: formats, rates, channels, codecs, latency.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::value::Value;

/// What one layer (or the effective path) allows. A field is `None` when it is outside the probe
/// scope or meaningless for the layer, and `Unknown` when it is in scope but couldn't be read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitySet {
    /// PCM sample formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_formats: Option<Value<Vec<SampleFormat>>>,
    /// Sample rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<Value<RateSet>>,
    /// Channel counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<Value<Range>>,
    /// Supported channel maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_maps: Option<Value<Vec<ChannelMap>>>,
    /// Compressed formats that can pass through (IEC 61937).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passthrough: Option<Value<Vec<CodecCapability>>>,
    /// Bluetooth codecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluetooth_codecs: Option<Value<Vec<BluetoothCodecState>>>,
    /// Latency range in microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_us: Option<Value<Range>>,
    /// Whether the app can open the device directly (exclusive/bit-perfect access).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_access: Option<Value<bool>>,
}

/// An inclusive integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Range {
    /// Minimum.
    pub min: u32,
    /// Maximum.
    pub max: u32,
}

/// A set of sample rates in Hz.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RateSet {
    /// Specific rates.
    Discrete {
        /// Rates in Hz, ascending.
        rates_hz: Vec<u32>,
    },
    /// Any rate in a continuous range.
    Continuous {
        /// Minimum rate in Hz.
        min_hz: u32,
        /// Maximum rate in Hz.
        max_hz: u32,
    },
}

/// PCM sample format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    /// Unsigned 8-bit.
    U8,
    /// Signed 16-bit little-endian.
    S16Le,
    /// Signed 16-bit big-endian.
    S16Be,
    /// Signed 24-bit in 32-bit container, little-endian.
    S24Le,
    /// Signed 24-bit packed in 3 bytes, little-endian.
    S24_3Le,
    /// Signed 32-bit little-endian.
    S32Le,
    /// 32-bit float little-endian.
    F32Le,
    /// 64-bit float little-endian.
    F64Le,
    /// IEC 958 subframe, little-endian.
    Iec958SubframeLe,
    /// Another format, by its native name.
    Other(String),
}

/// Speaker positions (CTA-861 / channel map names).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// Front left.
    Fl,
    /// Front right.
    Fr,
    /// Front center.
    Fc,
    /// Low-frequency effects.
    Lfe,
    /// Rear left.
    Rl,
    /// Rear right.
    Rr,
    /// Side left.
    Sl,
    /// Side right.
    Sr,
    /// Rear center.
    Rc,
    /// Front left of center.
    Flc,
    /// Front right of center.
    Frc,
    /// Rear left of center.
    Rlc,
    /// Rear right of center.
    Rrc,
    /// Top front left.
    Tfl,
    /// Top front right.
    Tfr,
    /// Top front center.
    Tfc,
    /// Top center.
    Tc,
    /// Another position, by name.
    Other(String),
}

/// A channel map: speaker position per channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelMap {
    /// Positions in channel order.
    pub positions: Vec<Speaker>,
}

/// Audio coding formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// Linear PCM.
    Lpcm,
    /// Dolby Digital (AC-3).
    Ac3,
    /// Dolby Digital Plus (E-AC-3).
    Eac3,
    /// DTS.
    Dts,
    /// DTS-HD (HRA or MA).
    DtsHd,
    /// Dolby TrueHD / MLP.
    TrueHd,
    /// Dolby MAT.
    Mat,
    /// MPEG-1 layers 1/2.
    Mpeg1,
    /// MP3.
    Mp3,
    /// MPEG-2 multichannel.
    Mpeg2,
    /// AAC.
    Aac,
    /// DST (SACD).
    Dst,
    /// One-bit audio (DSD).
    OneBitAudio,
    /// Another codec, by name.
    Other(String),
}

/// Extra codec flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodecFlag {
    /// E-AC-3 with Joint Object Coding (Dolby Atmos).
    Joc,
    /// TrueHD/MAT carrying Atmos.
    Atmos,
    /// DTS:X.
    DtsX,
    /// Needs high-bit-rate (HBR) transport.
    NeedsHbr,
    /// Another flag, by name.
    Other(String),
}

/// What a sink or layer accepts for one codec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CodecCapability {
    /// The codec.
    pub codec: Codec,
    /// Maximum channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_channels: Option<u8>,
    /// Sample rates in Hz.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rates_hz: Vec<u32>,
    /// LPCM bit depths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bit_depths: Vec<u8>,
    /// Maximum bit rate in kbit/s (AC-3, DTS family).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bitrate_kbps: Option<u32>,
    /// Extra flags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<CodecFlag>,
}

/// Bluetooth codecs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothCodec {
    /// SBC.
    Sbc,
    /// SBC-XQ.
    SbcXq,
    /// AAC.
    Aac,
    /// aptX.
    AptX,
    /// aptX HD.
    AptXHd,
    /// aptX Low Latency.
    AptXLl,
    /// LDAC.
    Ldac,
    /// LC3 (LE Audio).
    Lc3,
    /// Opus.
    Opus,
    /// mSBC (wideband voice).
    Msbc,
    /// CVSD (narrowband voice).
    Cvsd,
    /// Another codec, by name.
    Other(String),
}

/// One Bluetooth codec and what is known about it on this link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BluetoothCodecState {
    /// The codec.
    pub codec: BluetoothCodec,
    /// Advertised by the remote device.
    pub advertised_by_device: bool,
    /// Available on the host (plugin installed and enabled).
    pub available_on_host: bool,
    /// Currently negotiated.
    pub negotiated: bool,
    /// Quality mode, e.g. LDAC `hq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_mode: Option<String>,
}
