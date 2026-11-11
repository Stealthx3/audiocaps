// SPDX-License-Identifier: LGPL-2.1-or-later
//! Per-layer details that a [`CapabilitySet`](crate::CapabilitySet) can't express: host hardware
//! (codec pins, USB interfaces), driver state (module parameters, power, jack), the state of the
//! device in the platform audio API (in use, managed by a server, current parameters), routing of
//! the default device, and MIDI ports.
//!
//! These are OS-neutral shapes; platform names (HDA, ALSA plugin types) appear only as data.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caps::{Range, RateSet, SampleFormat};
use crate::value::Value;

/// Extra detail for one layer of one endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayerDetail {
    /// Host hardware: bus IDs, codec, pins, USB interfaces.
    HostHardware(HardwareDetail),
    /// Kernel driver: module, parameters, power state, jack.
    KernelDriver(DriverDetail),
    /// Platform audio API device state (ALSA on Linux).
    Device(DeviceDetail),
}

/// Host hardware behind an endpoint.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HardwareDetail {
    /// PCI vendor:device, e.g. `8086:293e`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci_id: Option<Value<String>>,
    /// USB vendor:product, e.g. `0d8c:0014`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_id: Option<Value<String>>,
    /// USB bus speed, e.g. `full`, `high`, `super`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_speed: Option<Value<String>>,
    /// Codec name, e.g. `Realtek ALC897`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<Value<String>>,
    /// Codec pins this endpoint reaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pins: Option<Value<Vec<PinInfo>>>,
    /// USB Audio streaming alternate settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_alt_settings: Option<Value<Vec<UsbAltSetting>>>,
}

/// One codec pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PinInfo {
    /// Node id.
    pub node: u16,
    /// Default device, e.g. `line_out`, `speaker`, `hp_out`, `spdif_out`, `digital_out`,
    /// `mic_in`, `line_in`.
    pub device: String,
    /// Port connectivity: `jack`, `none`, `fixed` or `both`.
    pub connectivity: String,
    /// Maximum channels of the pin widget.
    pub max_channels: u8,
    /// HDMI pin.
    pub hdmi: bool,
    /// DisplayPort pin.
    pub display_port: bool,
    /// High bit-rate audio (HBR) supported on the pin.
    pub hbr: bool,
    /// Presence detected (something plugged in), when the codec reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<bool>,
}

/// One USB Audio streaming alternate setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UsbAltSetting {
    /// Interface number.
    pub interface: u32,
    /// Alternate setting.
    pub alt_setting: u32,
    /// Sample formats.
    pub formats: Vec<SampleFormat>,
    /// Channel count.
    pub channels: u32,
    /// Sample rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<RateSet>,
    /// Endpoint synchronisation type, e.g. `async`, `adaptive`, `sync`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<String>,
}

/// Kernel driver state for an endpoint's device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DriverDetail {
    /// Driver name as the audio subsystem reports it, e.g. `HDA-Intel`.
    pub driver: Value<String>,
    /// Kernel module, e.g. `snd_hda_intel`.
    pub module: Value<String>,
    /// Module parameters.
    pub parameters: Value<Vec<Parameter>>,
    /// Runtime power state of the device, e.g. `active`, `suspended`.
    pub power: Value<String>,
    /// Jack state for this endpoint, when the driver exposes a jack control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jack: Option<Value<JackState>>,
}

/// A named parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Parameter {
    /// Name.
    pub name: String,
    /// Value as text.
    pub value: String,
}

/// A jack's plug state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JackState {
    /// The control that reports it, e.g. `HDMI/DP,pcm=3 Jack`.
    pub control: String,
    /// Something is plugged in.
    pub plugged: bool,
}

/// The device as the platform audio API sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceDetail {
    /// Device name an application opens directly, e.g. `hw:CARD=PCH,DEV=0`.
    pub device: String,
    /// Whether the device is free, in use, or managed by a sound server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value<DeviceState>>,
    /// Sub-devices (hardware streams): total and free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdevices: Option<Value<Subdevices>>,
    /// Parameters of the stream currently running, when the device is in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Value<CurrentParams>>,
    /// Period time range in microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_us: Option<Value<Range>>,
    /// IEC 958 (S/PDIF, HDMI) channel status, for digital outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iec958: Option<Value<Iec958State>>,
}

/// Device state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeviceState {
    /// Nobody has it open.
    Idle,
    /// A process has it open.
    InUse {
        /// The owning process, when visible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_pid: Option<u32>,
    },
    /// A sound server manages it; AudioCaps reads it through the server and never opens it.
    ManagedByServer {
        /// Server name, e.g. `pipewire`.
        server: String,
    },
}

/// Sub-device counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subdevices {
    /// Total.
    pub total: u32,
    /// Free (not opened by anyone).
    pub available: u32,
}

/// Parameters of a running stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CurrentParams {
    /// Sample format.
    pub format: SampleFormat,
    /// Channels.
    pub channels: u32,
    /// Rate in Hz.
    pub rate_hz: u32,
    /// Period size in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_frames: Option<u32>,
    /// Buffer size in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_frames: Option<u32>,
}

/// IEC 958 output state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Iec958State {
    /// The output is switched on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Channel status says "non-audio": compressed passthrough is set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_audio: Option<bool>,
    /// Sample rate in the channel status, in Hz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
}

/// How the platform's default device is routed (system-wide, per direction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Routing {
    /// The default playback device, stage by stage from the application side.
    pub default_playback: Value<Vec<RouteStage>>,
    /// The default capture device, stage by stage from the application side.
    pub default_capture: Value<Vec<RouteStage>>,
}

/// One stage of a route, e.g. one ALSA plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteStage {
    /// Name of the stage as configured, e.g. `default`, `dmix:CARD=PCH`.
    pub name: String,
    /// Stage type, e.g. ALSA plugin types `plug`, `dmix`, `softvol`, `hw`, `pulse`, `pipewire`.
    pub kind: String,
    /// Card the stage targets, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// Device the stage targets, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u32>,
    /// Fixed rate the stage forces, in Hz (e.g. a dmix rate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
}

/// A MIDI port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MidiPort {
    /// Raw device or sequencer port.
    pub kind: MidiKind,
    /// The port can be read (it sends MIDI to the app).
    pub input: bool,
    /// The port can be written (the app sends MIDI to it).
    pub output: bool,
    /// Sub-devices (raw MIDI only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdevices: Option<u32>,
    /// Protocol, e.g. `midi1`, `ump`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Kinds of MIDI port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MidiKind {
    /// A raw MIDI device of a sound card.
    Raw,
    /// A sequencer port (kernel or application client).
    Sequencer,
}
