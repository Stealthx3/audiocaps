// SPDX-License-Identifier: LGPL-2.1-or-later
//! Per-layer details that a [`CapabilitySet`](crate::CapabilitySet) can't express: host hardware
//! (codec pins, USB interfaces), driver state (module parameters, power, jack), the state of the
//! device in the platform audio API (in use, managed by a server, current parameters), routing of
//! the default device, and MIDI ports.
//!
//! These are OS-neutral shapes; platform names (HDA, ALSA plugin types) appear only as data.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caps::{CodecCapability, Range, RateSet, SampleFormat};
use crate::snapshot::SoundServer;
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
    /// Sound-server views of the endpoint, one per server API.
    SoundServer(ServerDetail),
    /// Wireless link (Bluetooth device and transport).
    Wireless(BluetoothDetail),
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

/// How sound servers present an endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ServerDetail {
    /// One view per server API (native PipeWire, PulseAudio API).
    pub views: Vec<ServerView>,
}

/// One server's view of an endpoint (a PipeWire node, a PulseAudio sink or source).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ServerView {
    /// Server API.
    pub server: SoundServer,
    /// Node, sink or source name (Bluetooth addresses hashed).
    pub object: String,
    /// Media class, e.g. `Audio/Sink`.
    pub media_class: String,
    /// State: `running`, `idle`, `suspended`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value<String>>,
    /// Format the server runs the device at now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Value<CurrentParams>>,
    /// Profiles of the owning device, with the active one marked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Value<Vec<ProfileState>>>,
    /// Routes (ports) of the owning device in this direction, with the active one marked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routes: Option<Value<Vec<RouteState>>>,
    /// Compressed formats the server is set to pass through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passthrough: Option<Value<Vec<CodecCapability>>>,
    /// Volume and mute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Value<VolumeState>>,
    /// The endpoint is the server's default sink or source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// A device profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileState {
    /// Name, e.g. `output:hdmi-surround`.
    pub name: String,
    /// Description.
    pub description: String,
    /// Availability, when the server knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// The active profile.
    pub active: bool,
}

/// A route (port).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteState {
    /// Name, e.g. `hdmi-output-0`.
    pub name: String,
    /// Description.
    pub description: String,
    /// Availability (plugged), when the server knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// The active route.
    pub active: bool,
    /// Compressed formats the server detected on this route from the sink (PipeWire
    /// `iec958.codecs.detected`), when it publishes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_passthrough: Option<Vec<crate::caps::Codec>>,
}

/// Volume and mute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VolumeState {
    /// Volume in percent as mixers show it (100 = unity).
    pub percent: u32,
    /// Muted.
    pub muted: bool,
}

/// A sound server's global state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ServerState {
    /// Server API.
    pub server: SoundServer,
    /// Whether the server could be read.
    pub availability: crate::endpoint::Availability,
    /// Name and version as the server reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Value<crate::snapshot::ServerInfo>>,
    /// Graph clock (PipeWire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<Value<ClockSettings>>,
    /// Default sink (endpoint id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_sink: Option<Value<String>>,
    /// Default source (endpoint id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<Value<String>>,
    /// Session manager and its policy settings (WirePlumber `sm-settings`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<Value<Policy>>,
    /// Physical ports (JACK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<Value<Vec<PortGroup>>>,
}

/// Ports of one client and direction (JACK).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PortGroup {
    /// Client name, e.g. `system`.
    pub client: String,
    /// `playback` (the app writes to it) or `capture`.
    pub direction: String,
    /// Number of ports (channels).
    pub channels: u32,
    /// Latency range in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_frames: Option<Range>,
}

/// A Bluetooth device and its audio transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BluetoothDetail {
    /// Connected.
    pub connected: Value<bool>,
    /// Audio profiles the device offers (`a2dp_sink`, `hfp_hf`, `pacs`, …).
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The active audio transport, when one is set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Value<BluetoothTransport>>,
}

/// An active Bluetooth audio transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BluetoothTransport {
    /// Profile, e.g. `a2dp_sink`, `hfp_hf`.
    pub profile: String,
    /// Codec in use.
    pub codec: crate::caps::BluetoothCodec,
    /// Transport state: `idle`, `pending`, `active`.
    pub state: String,
}

/// A sandbox permission the app lacks, with the layers it hides and the exact fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionGap {
    /// The permission, e.g. `--socket=pulseaudio`, `alsa` (Snap plug).
    pub permission: String,
    /// Layers it hides.
    pub layers: Vec<crate::scope::LayerKind>,
    /// What the user or packager runs or adds, e.g. `flatpak override --user --socket=pulseaudio <app>`.
    pub fix: String,
    /// Needed for basic operation (else it only adds detail).
    pub required: bool,
}

/// PipeWire graph clock settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClockSettings {
    /// Graph rate in Hz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
    /// Rates the graph may switch to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_rates_hz: Vec<u32>,
    /// Quantum in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantum: Option<u32>,
    /// Minimum quantum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_quantum: Option<u32>,
    /// Maximum quantum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_quantum: Option<u32>,
    /// Forced rate (0 = not forced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_rate_hz: Option<u32>,
    /// Forced quantum (0 = not forced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_quantum: Option<u32>,
}

/// Session manager policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Policy {
    /// Session manager, e.g. `WirePlumber`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager: Option<String>,
    /// Settings, e.g. `bluetooth.autoswitch-to-headset-profile = true`.
    pub settings: Vec<Parameter>,
}

/// HDMI link limits and latency stated by a sink.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HdmiLink {
    /// The GPU connector, e.g. `card1-HDMI-A-2`, `card0-DP-1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,
    /// What the sink says the link is (`hdmi`, `displayport`), from the ELD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// CEC physical address as a 16-bit number (`0x1000` is input 1 of the root display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_address: Option<u16>,
    /// Maximum TMDS character rate in MHz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tmds_mhz: Option<u32>,
    /// Maximum FRL rate in Gbit/s (HDMI 2.1), when advertised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frl_gbps: Option<u32>,
    /// Audio latency in milliseconds (progressive video), when stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_latency_ms: Option<u16>,
    /// Video latency in milliseconds (progressive video), when stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_latency_ms: Option<u16>,
    /// The sink accepts ACP/ISRC packets (Supports_AI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_ai: Option<bool>,
}

/// A CEC bus as seen from one adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CecBus {
    /// Adapter driver, e.g. `pulse8-cec`, `amdgpu`.
    pub adapter: String,
    /// The adapter's own physical address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_address: Option<u16>,
    /// The adapter's claimed logical addresses.
    #[serde(default)]
    pub own_logical_addresses: Vec<u8>,
    /// Devices that answered.
    pub devices: Vec<CecDevice>,
}

/// One device on the CEC bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CecDevice {
    /// Logical address (0 TV, 5 audio system, 4/8/11 playback, …).
    pub logical_address: u8,
    /// Device type from the logical address, e.g. `tv`, `audio_system`.
    pub device_type: String,
    /// Physical address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_address: Option<u16>,
    /// OSD name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osd_name: Option<String>,
    /// IEEE OUI vendor id, e.g. `0005cd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<String>,
    /// Vendor name resolved from the OUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Short audio descriptors the device reported (audio systems).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_descriptors: Vec<CodecCapability>,
}

/// A quirk-data entry applied to a sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppliedQuirk {
    /// Entry id, e.g. `denon-avr-s660h`.
    pub id: String,
    /// What the entry corrects or adds.
    pub description: String,
    /// The pack it came from: `bundled`, or the directory that overrode it.
    pub origin: String,
    /// Where the facts come from (spec sheet, measurement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Effects.
    pub effects: Vec<QuirkEffect>,
}

/// One effect of a quirk entry. Effects are reported, and values they change are marked
/// `inferred` with a `quirk` source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum QuirkEffect {
    /// A capability no EDID field carries, e.g. `dolby_pro_logic_ii`.
    Feature {
        /// Feature name.
        name: String,
    },
    /// The sink accepts more channels than it processes (e.g. 8-channel PCM, 5.1 processing).
    ProcessingChannels {
        /// Channels the sink actually renders.
        max: u8,
    },
    /// A codec the sink accepts but doesn't advertise.
    AddCodec {
        /// The codec.
        codec: CodecCapability,
    },
    /// A codec the sink advertises but can't play.
    RemoveCodec {
        /// The codec.
        codec: crate::caps::Codec,
    },
    /// A note for diagnostics.
    Note {
        /// Text.
        text: String,
    },
}

/// What the application intends to play, for findings that compare against it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppRequest {
    /// The endpoint the app plays to; app findings are about it alone. Without it they apply to
    /// every endpoint in scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<crate::endpoint::EndpointId>,
    /// Codec the app plays (LPCM or a compressed format for passthrough).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<crate::caps::Codec>,
    /// Channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u8>,
    /// Sample rate in Hz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
    /// Bits per sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<u8>,
    /// End-to-end latency target in microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_target_us: Option<u32>,
}

/// Software codecs the application could load (opt-in check). Presence is read from the
/// libraries on disk; nothing is loaded, so what each provider decodes or encodes is `inferred`
/// from what its standard builds include.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SoftwareCodecs {
    /// Providers found.
    pub providers: Vec<CodecProvider>,
    /// Codecs some provider decodes.
    pub decoders: Vec<crate::caps::Codec>,
    /// Codecs some provider encodes.
    pub encoders: Vec<crate::caps::Codec>,
}

/// A library that provides codecs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CodecProvider {
    /// Name, e.g. `ffmpeg`, `alsa-a52`, `pipewire-bluez5-ldac`.
    pub name: String,
    /// Library path as found.
    pub library: String,
    /// Version from the soname, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Codecs it decodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decodes: Vec<crate::caps::Codec>,
    /// Codecs it encodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encodes: Vec<crate::caps::Codec>,
    /// Bluetooth codecs it adds to the host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bluetooth: Vec<crate::caps::BluetoothCodec>,
    /// Use: e.g. `alsa-plugin` (open as an ALSA PCM), `library`, `gstreamer`, `pipewire`.
    pub kind: String,
}

/// Result of an active probe of one endpoint (opt-in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActiveProbe {
    /// The device name used, e.g. `hdmi:CARD=HDMI,DEV=0`.
    pub device: String,
    /// One result per requested format.
    pub results: Vec<ActiveResult>,
}

/// One format tried by an active probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActiveResult {
    /// The codec tried.
    pub codec: crate::caps::Codec,
    /// Carrier: channels.
    pub channels: u32,
    /// Carrier: rate in Hz.
    pub rate_hz: u32,
    /// What happened.
    pub outcome: ActiveOutcome,
}

/// Outcome of one active test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ActiveOutcome {
    /// The host path accepted the format and played silence without error. (The sink can't
    /// report back, so this confirms the host side.)
    Accepted {
        /// Milliseconds of silence played.
        played_ms: u32,
    },
    /// The device refused it.
    Refused {
        /// Stage: `open`, `hw_params`, `write`.
        stage: String,
        /// Error.
        error: String,
    },
    /// Not tried.
    Skipped {
        /// Why.
        reason: String,
    },
}

/// What the application asked an active probe to try.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActiveProbeRequest {
    /// The endpoint (an idle ALSA playback device).
    pub endpoint: crate::endpoint::EndpointId,
    /// Codecs to try: `lpcm` (silent PCM at the given layout) or compressed codecs (silent
    /// IEC 61937 pause bursts in their carrier layout).
    pub codecs: Vec<crate::caps::Codec>,
    /// LPCM layout: channels (default 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    /// LPCM layout: rate (default 48000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
}
