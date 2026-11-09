// SPDX-License-Identifier: LGPL-2.1-or-later
//! HDA codec dumps: `/proc/asound/cardN/codec#M`.
//!
//! Decodes the codec identity, the audio function group's default PCM caps and every widget: type,
//! capability bits, channel count, PCM rates/bits/formats, pin capabilities (HDMI, DP, HBR), pin
//! default configuration, connection list and per-device (DP-MST) presence. Lines that are not
//! recognised are skipped, so new kernel output never breaks parsing.

/// A decoded codec dump.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HdaCodec {
    /// Codec name, e.g. `Realtek ALC892`.
    pub name: String,
    /// Codec address on the link.
    pub address: Option<u8>,
    /// Vendor id (vendor << 16 | device).
    pub vendor_id: Option<u32>,
    /// Subsystem id.
    pub subsystem_id: Option<u32>,
    /// Revision id.
    pub revision_id: Option<u32>,
    /// The audio function group's default PCM caps.
    pub default_pcm: Option<PcmCaps>,
    /// Widgets in dump order.
    pub nodes: Vec<Node>,
}

impl HdaCodec {
    /// Pin widgets.
    pub fn pins(&self) -> impl Iterator<Item = &Node> {
        self.nodes
            .iter()
            .filter(|n| n.kind == WidgetKind::PinComplex)
    }

    /// Looks up a widget by node id.
    pub fn node(&self, nid: u16) -> Option<&Node> {
        self.nodes.iter().find(|n| n.nid == nid)
    }
}

/// PCM capabilities of a converter or the function group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcmCaps {
    /// Rate bitmask as printed.
    pub rates_mask: u32,
    /// Rates in Hz.
    pub rates_hz: Vec<u32>,
    /// Bit-depth bitmask as printed.
    pub bits_mask: u32,
    /// Bit depths.
    pub bits: Vec<u8>,
    /// Stream format bitmask as printed (bit 0 PCM, 1 FLOAT32, 2 AC3).
    pub formats_mask: u32,
    /// Stream formats by name (`PCM`, `FLOAT`, `AC3`).
    pub formats: Vec<String>,
}

/// Widget types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetKind {
    /// Audio output converter (DAC / digital out).
    AudioOutput,
    /// Audio input converter (ADC / digital in).
    AudioInput,
    /// Mixer.
    AudioMixer,
    /// Selector.
    AudioSelector,
    /// Pin complex.
    PinComplex,
    /// Power widget.
    PowerWidget,
    /// Volume knob.
    VolumeKnob,
    /// Beep generator.
    BeepGenerator,
    /// Vendor-defined widget.
    VendorDefined,
    /// Another label, verbatim.
    Other(String),
}

impl WidgetKind {
    fn from_label(s: &str) -> Self {
        match s {
            "Audio Output" => Self::AudioOutput,
            "Audio Input" => Self::AudioInput,
            "Audio Mixer" => Self::AudioMixer,
            "Audio Selector" => Self::AudioSelector,
            "Pin Complex" => Self::PinComplex,
            "Power Widget" => Self::PowerWidget,
            "Volume Knob Widget" => Self::VolumeKnob,
            "Beep Generator Widget" => Self::BeepGenerator,
            "Vendor Defined Widget" => Self::VendorDefined,
            o => Self::Other(o.to_string()),
        }
    }
}

/// One widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Node id.
    pub nid: u16,
    /// Widget type.
    pub kind: WidgetKind,
    /// Audio widget capabilities.
    pub wcaps: u32,
    /// PCM caps of a converter (absent: uses the function group default).
    pub pcm: Option<PcmCaps>,
    /// Pin capabilities.
    pub pincap: Option<u32>,
    /// Pin default configuration.
    pub pin_default: Option<PinConfig>,
    /// Connection list.
    pub connections: Vec<u16>,
    /// Currently selected connection.
    pub selected: Option<u16>,
    /// Per-device state for DP-MST capable pins.
    pub devices: Vec<PinDevice>,
    /// Mixer control names bound to this widget.
    pub controls: Vec<String>,
}

impl Node {
    /// Channel count from wcaps (bits 15:13 extension and bit 0 stereo).
    pub fn channels(&self) -> u8 {
        let n = (((self.wcaps >> 13) & 0x7) << 1) | (self.wcaps & 1);
        (n + 1) as u8
    }

    /// Digital widget.
    pub fn digital(&self) -> bool {
        self.wcaps & (1 << 9) != 0
    }

    /// Pin can output.
    pub fn output_capable(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 4) != 0)
    }

    /// Pin can input.
    pub fn input_capable(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 5) != 0)
    }

    /// Pin supports HDMI.
    pub fn hdmi(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 7) != 0)
    }

    /// Pin supports DisplayPort.
    pub fn display_port(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 24) != 0)
    }

    /// Pin supports high-bit-rate (HBR) packets, needed for TrueHD and DTS-HD MA passthrough.
    pub fn hbr(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 27) != 0)
    }

    /// Pin supports presence detection.
    pub fn presence_detect(&self) -> bool {
        self.pincap.is_some_and(|p| p & (1 << 2) != 0)
    }
}

/// Device entry of a DP-MST capable pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinDevice {
    /// Device index.
    pub index: u8,
    /// Current device for the pin.
    pub current: bool,
    /// Presence detect.
    pub present: bool,
    /// ELD valid.
    pub eld_valid: bool,
}

/// Port connectivity in the pin default configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortConnectivity {
    /// Jack.
    Jack,
    /// No physical connection (pin unused).
    None,
    /// Fixed function device (built-in speaker, mic).
    Fixed,
    /// Both a jack and an internal device.
    Both,
}

/// Default device in the pin default configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultDevice {
    /// Line out.
    LineOut,
    /// Speaker.
    Speaker,
    /// Headphone out.
    HeadphoneOut,
    /// CD.
    Cd,
    /// S/PDIF out.
    SpdifOut,
    /// Digital other out (HDMI/DP).
    DigitalOut,
    /// Modem line side.
    ModemLine,
    /// Modem handset side.
    ModemHandset,
    /// Line in.
    LineIn,
    /// Aux.
    Aux,
    /// Microphone.
    Mic,
    /// Telephony.
    Telephony,
    /// S/PDIF in.
    SpdifIn,
    /// Digital other in.
    DigitalIn,
    /// Reserved (0xE).
    Reserved,
    /// Other (0xF).
    Other,
}

/// A decoded pin default configuration (the 32-bit `Pin Default` value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinConfig {
    /// Raw value.
    pub raw: u32,
}

impl PinConfig {
    /// Port connectivity (bits 31:30).
    pub fn connectivity(&self) -> PortConnectivity {
        match self.raw >> 30 {
            0 => PortConnectivity::Jack,
            1 => PortConnectivity::None,
            2 => PortConnectivity::Fixed,
            _ => PortConnectivity::Both,
        }
    }

    /// Location (bits 29:24).
    pub fn location(&self) -> u8 {
        ((self.raw >> 24) & 0x3F) as u8
    }

    /// Default device (bits 23:20).
    pub fn device(&self) -> DefaultDevice {
        use DefaultDevice::*;
        [
            LineOut,
            Speaker,
            HeadphoneOut,
            Cd,
            SpdifOut,
            DigitalOut,
            ModemLine,
            ModemHandset,
            LineIn,
            Aux,
            Mic,
            Telephony,
            SpdifIn,
            DigitalIn,
            Reserved,
            Other,
        ][((self.raw >> 20) & 0xF) as usize]
    }

    /// Connection type (bits 19:16).
    pub fn connection_type(&self) -> u8 {
        ((self.raw >> 16) & 0xF) as u8
    }

    /// Color (bits 15:12).
    pub fn color(&self) -> u8 {
        ((self.raw >> 12) & 0xF) as u8
    }

    /// Jack detect override (misc bit 0).
    pub fn no_jack_detect(&self) -> bool {
        self.raw & (1 << 8) != 0
    }

    /// Default association (bits 7:4).
    pub fn association(&self) -> u8 {
        ((self.raw >> 4) & 0xF) as u8
    }

    /// Sequence (bits 3:0).
    pub fn sequence(&self) -> u8 {
        (self.raw & 0xF) as u8
    }
}

fn hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_end_matches(['*', ',', ':']);
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

/// Parses `[0x560]: 44100 48000` into (mask, values).
fn mask_list(rest: &str) -> (u32, &str) {
    let rest = rest.trim();
    if let Some(r) = rest.strip_prefix('[')
        && let Some((m, tail)) = r.split_once(']')
    {
        return (hex(m).unwrap_or(0), tail.trim_start_matches(':').trim());
    }
    (0, rest)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    DefaultPcm,
    NodePcm,
    Connections,
}

/// Parses a codec dump.
pub fn parse_codec(text: &str) -> HdaCodec {
    let mut c = HdaCodec::default();
    let mut pending = Pending::None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Node ") {
            pending = Pending::None;
            // "Node 0x02 [Audio Output] wcaps 0x41d: Stereo Amp-Out"
            let Some((nid, rest)) = rest.split_once(' ') else {
                continue;
            };
            let Some(nid) = hex(nid).and_then(|n| u16::try_from(n).ok()) else {
                continue;
            };
            let (label, rest) = match rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
                Some((l, r)) => (l, r),
                None => ("", rest),
            };
            let wcaps = rest
                .trim()
                .strip_prefix("wcaps ")
                .and_then(|w| w.split(':').next())
                .and_then(hex)
                .unwrap_or(0);
            c.nodes.push(Node {
                nid,
                kind: WidgetKind::from_label(label),
                wcaps,
                pcm: None,
                pincap: None,
                pin_default: None,
                connections: Vec::new(),
                selected: None,
                devices: Vec::new(),
                controls: Vec::new(),
            });
            continue;
        }
        if c.nodes.is_empty() {
            if let Some(v) = t.strip_prefix("Codec: ") {
                c.name = v.to_string();
            } else if let Some(v) = t.strip_prefix("Address: ") {
                c.address = v.parse().ok();
            } else if let Some(v) = t.strip_prefix("Vendor Id: ") {
                c.vendor_id = hex(v);
            } else if let Some(v) = t.strip_prefix("Subsystem Id: ") {
                c.subsystem_id = hex(v);
            } else if let Some(v) = t.strip_prefix("Revision Id: ") {
                c.revision_id = hex(v);
            } else if t == "Default PCM:" {
                c.default_pcm = Some(PcmCaps::default());
                pending = Pending::DefaultPcm;
                continue;
            }
        }
        let node = c.nodes.last_mut();
        // PCM sub-lines.
        if matches!(pending, Pending::DefaultPcm | Pending::NodePcm) {
            let pcm = match (pending, node) {
                (Pending::DefaultPcm, _) => c.default_pcm.as_mut(),
                (_, Some(n)) => n.pcm.as_mut(),
                _ => None,
            };
            if let Some(pcm) = pcm {
                if let Some(r) = t.strip_prefix("rates ") {
                    let (m, list) = mask_list(r);
                    pcm.rates_mask = m;
                    pcm.rates_hz = list
                        .split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    continue;
                } else if let Some(r) = t.strip_prefix("bits ") {
                    let (m, list) = mask_list(r);
                    pcm.bits_mask = m;
                    pcm.bits = list
                        .split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    continue;
                } else if let Some(r) = t.strip_prefix("formats ") {
                    let (m, list) = mask_list(r);
                    pcm.formats_mask = m;
                    pcm.formats = list.split_whitespace().map(str::to_string).collect();
                    continue;
                }
            }
            pending = Pending::None;
        }
        let Some(node) = c.nodes.last_mut() else {
            continue;
        };
        if pending == Pending::Connections {
            pending = Pending::None;
            let toks: Vec<&str> = t.split_whitespace().collect();
            if !toks.is_empty() && toks.iter().all(|s| s.starts_with("0x")) {
                for tok in toks {
                    if let Some(n) = hex(tok).and_then(|n| u16::try_from(n).ok()) {
                        node.connections.push(n);
                        if tok.ends_with('*') {
                            node.selected = Some(n);
                        }
                    }
                }
                continue;
            }
        }
        if t == "PCM:" {
            node.pcm = Some(PcmCaps::default());
            pending = Pending::NodePcm;
        } else if let Some(v) = t.strip_prefix("Pincap ") {
            node.pincap = v.split(':').next().and_then(hex);
        } else if let Some(v) = t.strip_prefix("Pin Default ") {
            node.pin_default = v
                .split(':')
                .next()
                .and_then(hex)
                .map(|raw| PinConfig { raw });
        } else if let Some(v) = t.strip_prefix("Connection: ") {
            if v.trim().parse::<u32>().is_ok_and(|n| n > 0) {
                pending = Pending::Connections;
            }
        } else if let Some(v) = t.strip_prefix("Control: name=\"") {
            if let Some((name, _)) = v.split_once('"') {
                node.controls.push(name.to_string());
            }
        } else if let Some(d) = parse_dev_line(t) {
            node.devices.push(d);
        }
    }
    c
}

/// Parses `*Dev 00: PD = 1, ELDV = 1, IA = 0, Connections [ … ]`.
fn parse_dev_line(t: &str) -> Option<PinDevice> {
    let (current, t) = match t.strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let rest = t.strip_prefix("Dev ")?;
    let (idx, rest) = rest.split_once(':')?;
    let flag = |key: &str| {
        rest.split(',')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .is_some_and(|(_, v)| v.trim() == "1")
    };
    Some(PinDevice {
        index: idx.trim().parse().ok()?,
        current,
        present: flag("PD"),
        eld_valid: flag("ELDV"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = "Codec: Nvidia GPU a3 HDMI/DP
Address: 0
AFG Function Id: 0x1 (unsol 0)
Vendor Id: 0x10de00a3
Subsystem Id: 0x10de0000
Revision Id: 0x100100
Default PCM:
    rates [0x0]:
    bits [0x0]:
    formats [0x0]:
Default Amp-In caps: N/A
Node 0x05 [Pin Complex] wcaps 0x407381: 8-Channels Digital CP
  Control: name=\"IEC958 Playback Con Mask\", index=1, device=0
  Pincap 0x09000094: OUT Detect HBR HDMI DP
  Pin Default 0x185600f0: [Jack] Digital Out at Int HDMI
    Conn = Digital, Color = Unknown
    DefAssociation = 0xf, Sequence = 0x0
  Pin-ctls: 0x40: OUT
  Devices: 4
    *Dev 00: PD = 1, ELDV = 1, IA = 0, Connections [ 0x08* 0x09 0x0a 0x0b ]
     Dev 01: PD = 0, ELDV = 0, IA = 0, Connections [ 0x08* 0x09 0x0a 0x0b ]
  Connection: 4
     0x08 0x09* 0x0a 0x0b
Node 0x08 [Audio Output] wcaps 0x62b1: 8-Channels Digital Stripe
  Converter: stream=0, channel=0
  Digital: Enabled
  PCM:
    rates [0x7f0]: 32000 44100 48000 88200 96000 176400 192000
    bits [0xe]: 16 20 24
    formats [0x5]: PCM AC3
  Power states:  D0 D3 EPSS
";

    #[test]
    fn nvidia_hdmi() {
        let c = parse_codec(DUMP);
        assert_eq!(c.name, "Nvidia GPU a3 HDMI/DP");
        assert_eq!(c.vendor_id, Some(0x10de00a3));
        assert_eq!(c.address, Some(0));
        assert_eq!(c.default_pcm.as_ref().unwrap().rates_hz, Vec::<u32>::new());
        assert_eq!(c.nodes.len(), 2);
        let pin = c.node(5).unwrap();
        assert_eq!(pin.kind, WidgetKind::PinComplex);
        assert_eq!(pin.channels(), 8);
        assert!(pin.digital() && pin.hdmi() && pin.display_port() && pin.hbr());
        assert!(pin.output_capable() && !pin.input_capable());
        let cfg = pin.pin_default.unwrap();
        assert_eq!(cfg.device(), DefaultDevice::DigitalOut);
        assert_eq!(cfg.connectivity(), PortConnectivity::Jack);
        assert_eq!(cfg.association(), 0xF);
        assert_eq!(pin.connections, vec![8, 9, 10, 11]);
        assert_eq!(pin.selected, Some(9));
        assert_eq!(pin.devices.len(), 2);
        assert!(pin.devices[0].current && pin.devices[0].present && pin.devices[0].eld_valid);
        assert!(!pin.devices[1].present);
        assert_eq!(pin.controls, vec!["IEC958 Playback Con Mask".to_string()]);
        let cvt = c.node(8).unwrap();
        let pcm = cvt.pcm.as_ref().unwrap();
        assert_eq!(pcm.rates_hz.len(), 7);
        assert_eq!(pcm.bits, vec![16, 20, 24]);
        assert_eq!(pcm.formats, vec!["PCM".to_string(), "AC3".to_string()]);
        assert_eq!(cvt.channels(), 8);
    }

    #[test]
    fn junk() {
        for s in [
            "",
            "Node",
            "Node 0x",
            "Node 0xzz [",
            "Connection: 3\n",
            "PCM:\n rates [",
            "*Dev",
        ] {
            let _ = parse_codec(s);
        }
    }
}
