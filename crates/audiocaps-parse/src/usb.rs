// SPDX-License-Identifier: LGPL-2.1-or-later
//! USB Audio Class descriptors, from the sysfs `descriptors` file of a USB device.
//!
//! The file holds the device descriptor followed by the raw configuration descriptors. This parser
//! walks every descriptor and decodes what matters for audio: the UAC version of each audio
//! function, terminals, UAC2 clock sources, and every streaming alternate setting with its format,
//! channels, bit resolution, UAC1 sample rates and endpoint (sync type, packet size, interval).
//! UAC2 sample rates are not in the descriptors (the device reports them on request), so they
//! come from the driver's `stream#` file instead. UAC3 functions are recognised but their class
//! descriptors are not decoded.

/// USB Audio Class version of an interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UacVersion {
    /// UAC 1.0 (protocol 0x00).
    Uac1,
    /// UAC 2.0 (protocol 0x20).
    Uac2,
    /// UAC 3.0 (protocol 0x30).
    Uac3,
    /// Another protocol code.
    Unknown(u8),
}

impl UacVersion {
    fn from_protocol(p: u8) -> Self {
        match p {
            0x00 => Self::Uac1,
            0x20 => Self::Uac2,
            0x30 => Self::Uac3,
            n => Self::Unknown(n),
        }
    }
}

/// The device descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// USB spec release (BCD), e.g. 0x0200.
    pub usb_bcd: u16,
    /// Device class.
    pub class: u8,
    /// Vendor id.
    pub vendor_id: u16,
    /// Product id.
    pub product_id: u16,
    /// Device release (BCD).
    pub device_bcd: u16,
    /// Number of configurations.
    pub num_configurations: u8,
}

/// A terminal of an audio control interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terminal {
    /// Terminal id.
    pub id: u8,
    /// Input (true) or output terminal.
    pub input: bool,
    /// Terminal type, e.g. 0x0101 USB streaming, 0x0301 speaker, 0x0201 microphone.
    pub terminal_type: u16,
    /// Channels (input terminals only).
    pub channels: Option<u8>,
    /// Clock source id (UAC2 only).
    pub clock_id: Option<u8>,
}

/// Clock source type (UAC2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockType {
    /// External clock.
    External,
    /// Internal fixed clock.
    InternalFixed,
    /// Internal variable clock.
    InternalVariable,
    /// Internal programmable clock.
    InternalProgrammable,
}

/// A UAC2 clock source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSource {
    /// Clock id.
    pub id: u8,
    /// Clock type.
    pub kind: ClockType,
    /// Synchronised to the USB start-of-frame.
    pub sof_synced: bool,
    /// Frequency control: 0 none, 1 read-only, 3 host-programmable.
    pub frequency_control: u8,
}

/// An audio function (one audio control interface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioControl {
    /// Configuration value.
    pub configuration: u8,
    /// Interface number.
    pub interface: u8,
    /// UAC version.
    pub uac: UacVersion,
    /// Audio Device Class release (BCD) from the header.
    pub adc_bcd: Option<u16>,
    /// Terminals.
    pub terminals: Vec<Terminal>,
    /// Clock sources (UAC2).
    pub clocks: Vec<ClockSource>,
    /// Clock selector ids (UAC2).
    pub clock_selectors: Vec<u8>,
}

/// Endpoint synchronisation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncType {
    /// No synchronisation.
    None,
    /// Asynchronous (device clock, feedback to host).
    Async,
    /// Adaptive (device follows the host).
    Adaptive,
    /// Synchronous (locked to SOF).
    Sync,
}

/// An endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    /// Endpoint address (bit 7 = IN).
    pub address: u8,
    /// Transfer type: 0 control, 1 isochronous, 2 bulk, 3 interrupt.
    pub transfer: u8,
    /// Sync type (isochronous only).
    pub sync: SyncType,
    /// Usage: 0 data, 1 feedback, 2 implicit feedback data.
    pub usage: u8,
    /// Max packet size in bytes.
    pub max_packet: u16,
    /// Additional transactions per microframe (high-bandwidth).
    pub extra_transactions: u8,
    /// bInterval.
    pub interval: u8,
}

impl Endpoint {
    /// Device-to-host.
    pub fn is_in(&self) -> bool {
        self.address & 0x80 != 0
    }
}

/// UAC1 sample rates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbRates {
    /// Discrete rates in Hz.
    Discrete(Vec<u32>),
    /// Continuous range in Hz.
    Continuous {
        /// Lower bound.
        min: u32,
        /// Upper bound.
        max: u32,
    },
}

/// One audio streaming alternate setting with endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingAlt {
    /// Configuration value.
    pub configuration: u8,
    /// Interface number.
    pub interface: u8,
    /// Alternate setting.
    pub alt: u8,
    /// UAC version.
    pub uac: UacVersion,
    /// Linked terminal id.
    pub terminal_link: Option<u8>,
    /// UAC1 wFormatTag (0x0001 PCM, 0x1001 AC-3 …).
    pub format_tag: Option<u16>,
    /// UAC2 bmFormats bitmap (bit 0 PCM, bit 1 PCM8, bit 2 IEEE float …).
    pub formats: Option<u32>,
    /// Format type (1 = type I PCM, 2 = type II compressed, 3 = type III IEC 61937).
    pub format_type: Option<u8>,
    /// Channels.
    pub channels: Option<u8>,
    /// Bytes per sample slot.
    pub subslot_bytes: Option<u8>,
    /// Valid bits per sample.
    pub bit_resolution: Option<u8>,
    /// Sample rates (UAC1 only).
    pub rates: Option<UsbRates>,
    /// Endpoints.
    pub endpoints: Vec<Endpoint>,
}

impl StreamingAlt {
    /// The data endpoint (not a feedback endpoint).
    pub fn data_endpoint(&self) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.usage != 1)
    }
}

/// Everything decoded from one `descriptors` file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsbDescriptors {
    /// Device descriptor.
    pub device: Option<DeviceDescriptor>,
    /// Audio control interfaces.
    pub controls: Vec<AudioControl>,
    /// Audio streaming alternate settings that have endpoints.
    pub streaming: Vec<StreamingAlt>,
    /// The descriptor chain ended with a malformed or cut-off descriptor.
    pub truncated: bool,
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn le24(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(i)?,
        *b.get(i + 1)?,
        *b.get(i + 2)?,
        0,
    ]))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(i)?,
        *b.get(i + 1)?,
        *b.get(i + 2)?,
        *b.get(i + 3)?,
    ]))
}

#[derive(Clone, Copy)]
struct Iface {
    number: u8,
    alt: u8,
    class: u8,
    subclass: u8,
    uac: UacVersion,
}

const AUDIO: u8 = 0x01;
const SUB_CONTROL: u8 = 0x01;
const SUB_STREAMING: u8 = 0x02;
const CS_INTERFACE: u8 = 0x24;

/// Parses a sysfs `descriptors` file.
pub fn parse_descriptors(b: &[u8]) -> UsbDescriptors {
    let mut out = UsbDescriptors::default();
    let mut config = 0u8;
    let mut iface: Option<Iface> = None;
    // Index into `out.streaming` of the current alt setting, if it is audio streaming.
    let mut alt_idx: Option<usize> = None;
    let mut i = 0usize;
    while i < b.len() {
        let len = usize::from(b[i]);
        if len < 2 || i + len > b.len() {
            out.truncated = true;
            break;
        }
        let d = &b[i..i + len];
        i += len;
        match d[1] {
            0x01 if len >= 18 => {
                out.device = Some(DeviceDescriptor {
                    usb_bcd: le16(d, 2).unwrap_or(0),
                    class: d[4],
                    vendor_id: le16(d, 8).unwrap_or(0),
                    product_id: le16(d, 10).unwrap_or(0),
                    device_bcd: le16(d, 12).unwrap_or(0),
                    num_configurations: d[17],
                });
            }
            0x02 if len >= 9 => {
                config = d[5];
                iface = None;
                alt_idx = None;
            }
            0x04 if len >= 9 => {
                let f = Iface {
                    number: d[2],
                    alt: d[3],
                    class: d[5],
                    subclass: d[6],
                    uac: UacVersion::from_protocol(d[7]),
                };
                iface = Some(f);
                alt_idx = None;
                if f.class == AUDIO && f.subclass == SUB_CONTROL {
                    out.controls.push(AudioControl {
                        configuration: config,
                        interface: f.number,
                        uac: f.uac,
                        adc_bcd: None,
                        terminals: Vec::new(),
                        clocks: Vec::new(),
                        clock_selectors: Vec::new(),
                    });
                } else if f.class == AUDIO && f.subclass == SUB_STREAMING && d[4] > 0 {
                    out.streaming.push(StreamingAlt {
                        configuration: config,
                        interface: f.number,
                        alt: f.alt,
                        uac: f.uac,
                        terminal_link: None,
                        format_tag: None,
                        formats: None,
                        format_type: None,
                        channels: None,
                        subslot_bytes: None,
                        bit_resolution: None,
                        rates: None,
                        endpoints: Vec::new(),
                    });
                    alt_idx = Some(out.streaming.len() - 1);
                }
            }
            CS_INTERFACE if len >= 3 => {
                let Some(f) = iface else { continue };
                if f.class != AUDIO {
                    continue;
                }
                if f.subclass == SUB_CONTROL {
                    if let Some(ac) = out.controls.last_mut() {
                        class_control(ac, d);
                    }
                } else if f.subclass == SUB_STREAMING
                    && let Some(k) = alt_idx
                {
                    class_streaming(&mut out.streaming[k], d);
                }
            }
            0x05 if len >= 7 => {
                if let Some(k) = alt_idx {
                    let attr = d[3];
                    let mps = le16(d, 4).unwrap_or(0);
                    out.streaming[k].endpoints.push(Endpoint {
                        address: d[2],
                        transfer: attr & 0x03,
                        sync: match (attr >> 2) & 0x03 {
                            0 => SyncType::None,
                            1 => SyncType::Async,
                            2 => SyncType::Adaptive,
                            _ => SyncType::Sync,
                        },
                        usage: (attr >> 4) & 0x03,
                        max_packet: mps & 0x07FF,
                        extra_transactions: ((mps >> 11) & 0x03) as u8,
                        interval: d[6],
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn class_control(ac: &mut AudioControl, d: &[u8]) {
    let uac2 = ac.uac == UacVersion::Uac2;
    if ac.uac == UacVersion::Uac3 {
        return;
    }
    match d[2] {
        0x01 => ac.adc_bcd = le16(d, 3),
        0x02 if d.len() >= 8 => ac.terminals.push(Terminal {
            id: d[3],
            input: true,
            terminal_type: le16(d, 4).unwrap_or(0),
            channels: if uac2 { d.get(8) } else { d.get(7) }.copied(),
            clock_id: if uac2 { d.get(7).copied() } else { None },
        }),
        0x03 if d.len() >= 8 => ac.terminals.push(Terminal {
            id: d[3],
            input: false,
            terminal_type: le16(d, 4).unwrap_or(0),
            channels: None,
            clock_id: if uac2 { d.get(8).copied() } else { None },
        }),
        0x0A if uac2 && d.len() >= 6 => ac.clocks.push(ClockSource {
            id: d[3],
            kind: match d[4] & 0x03 {
                0 => ClockType::External,
                1 => ClockType::InternalFixed,
                2 => ClockType::InternalVariable,
                _ => ClockType::InternalProgrammable,
            },
            sof_synced: d[4] & 0x04 != 0,
            frequency_control: d[5] & 0x03,
        }),
        0x0B if uac2 && d.len() >= 4 => ac.clock_selectors.push(d[3]),
        _ => {}
    }
}

fn class_streaming(a: &mut StreamingAlt, d: &[u8]) {
    match (a.uac, d[2]) {
        (UacVersion::Uac1, 0x01) => {
            a.terminal_link = d.get(3).copied();
            a.format_tag = le16(d, 5);
        }
        (UacVersion::Uac2, 0x01) => {
            a.terminal_link = d.get(3).copied();
            a.format_type = d.get(5).copied();
            a.formats = le32(d, 6);
            a.channels = d.get(10).copied();
        }
        (UacVersion::Uac1, 0x02) => {
            a.format_type = d.get(3).copied();
            if a.format_type == Some(1) {
                a.channels = d.get(4).copied();
                a.subslot_bytes = d.get(5).copied();
                a.bit_resolution = d.get(6).copied();
            }
            // Type I and III carry the frequency table at byte 7; type II at byte 8.
            let at = if a.format_type == Some(2) { 8 } else { 7 };
            if let Some(&n) = d.get(at) {
                a.rates = if n == 0 {
                    match (le24(d, at + 1), le24(d, at + 4)) {
                        (Some(min), Some(max)) => Some(UsbRates::Continuous { min, max }),
                        _ => None,
                    }
                } else {
                    let r: Vec<u32> = (0..usize::from(n))
                        .map_while(|k| le24(d, at + 1 + 3 * k))
                        .collect();
                    Some(UsbRates::Discrete(r))
                };
            }
        }
        (UacVersion::Uac2, 0x02) => {
            a.format_type = d.get(3).copied();
            if matches!(a.format_type, Some(1) | Some(3)) {
                a.subslot_bytes = d.get(4).copied();
                a.bit_resolution = d.get(5).copied();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Vec<u8> {
        vec![
            18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x34, 0x12, 0x78, 0x56, 0x00, 0x01, 1, 2, 3, 1,
        ]
    }

    /// UAC1 speaker, 2ch 16-bit, 44.1/48 kHz, adaptive OUT endpoint.
    pub(super) fn uac1() -> Vec<u8> {
        let mut b = device();
        b.extend([9, 2, 0, 0, 2, 1, 0, 0x80, 50]); // config 1
        b.extend([9, 4, 0, 0, 0, 1, 1, 0, 0]); // AC interface
        b.extend([9, 0x24, 1, 0x00, 0x01, 0, 0, 1, 1]); // header 1.00
        b.extend([12, 0x24, 2, 1, 0x01, 0x01, 0, 2, 3, 0, 0, 0]); // IT USB streaming, 2ch
        b.extend([9, 0x24, 3, 3, 0x01, 0x03, 0, 1, 0]); // OT speaker
        b.extend([9, 4, 1, 0, 0, 1, 2, 0, 0]); // AS alt 0, no endpoints
        b.extend([9, 4, 1, 1, 1, 1, 2, 0, 0]); // AS alt 1
        b.extend([7, 0x24, 1, 1, 1, 0x01, 0x00]); // AS general: PCM
        b.extend([14, 0x24, 2, 1, 2, 2, 16, 2, 0x44, 0xAC, 0, 0x80, 0xBB, 0]); // type I
        b.extend([9, 5, 0x01, 0x09, 0xC4, 0x00, 1, 0, 0]); // iso adaptive OUT, 196 bytes
        b
    }

    #[test]
    fn uac1_speaker() {
        let u = parse_descriptors(&uac1());
        assert!(!u.truncated);
        let dev = u.device.unwrap();
        assert_eq!((dev.vendor_id, dev.product_id), (0x1234, 0x5678));
        assert_eq!(u.controls.len(), 1);
        let ac = &u.controls[0];
        assert_eq!(ac.uac, UacVersion::Uac1);
        assert_eq!(ac.adc_bcd, Some(0x0100));
        assert_eq!(ac.terminals.len(), 2);
        assert_eq!(ac.terminals[0].channels, Some(2));
        assert_eq!(ac.terminals[1].terminal_type, 0x0301);
        assert_eq!(u.streaming.len(), 1);
        let a = &u.streaming[0];
        assert_eq!((a.interface, a.alt), (1, 1));
        assert_eq!(a.format_tag, Some(1));
        assert_eq!(
            (a.channels, a.subslot_bytes, a.bit_resolution),
            (Some(2), Some(2), Some(16))
        );
        assert_eq!(a.rates, Some(UsbRates::Discrete(vec![44100, 48000])));
        let ep = a.data_endpoint().unwrap();
        assert!(!ep.is_in());
        assert_eq!(
            (ep.transfer, ep.sync, ep.max_packet),
            (1, SyncType::Adaptive, 196)
        );
    }

    #[test]
    fn uac2_clock_and_async() {
        let mut b = device();
        b.extend([9, 2, 0, 0, 2, 1, 0, 0x80, 50]);
        b.extend([8, 0x0B, 0, 2, 1, 0, 0x20, 0]); // IAD
        b.extend([9, 4, 0, 0, 0, 1, 1, 0x20, 0]);
        b.extend([9, 0x24, 1, 0x00, 0x02, 1, 0, 0, 0]); // header 2.00
        b.extend([8, 0x24, 0x0A, 0x29, 0x07, 0x07, 0, 0]); // clock source: internal prog, SOF
        b.extend([
            17, 0x24, 2, 1, 0x01, 0x01, 0, 0x29, 2, 3, 0, 0, 0, 0, 0, 0, 0,
        ]); // IT
        b.extend([12, 0x24, 3, 3, 0x02, 0x03, 0, 1, 0x29, 0, 0, 0]); // OT headphones
        b.extend([9, 4, 1, 1, 2, 1, 2, 0x20, 0]);
        b.extend([16, 0x24, 1, 1, 0, 1, 1, 0, 0, 0, 2, 3, 0, 0, 0, 0]); // general
        b.extend([6, 0x24, 2, 1, 4, 24]); // type I, 4-byte slot, 24 bit
        b.extend([7, 5, 0x01, 0x05, 0x00, 0x04, 1]); // iso async OUT 1024
        b.extend([7, 5, 0x81, 0x11, 0x04, 0x00, 4]); // feedback IN
        let u = parse_descriptors(&b);
        let ac = &u.controls[0];
        assert_eq!(ac.uac, UacVersion::Uac2);
        assert_eq!(ac.clocks.len(), 1);
        let clk = ac.clocks[0];
        assert_eq!(
            (clk.id, clk.kind, clk.sof_synced, clk.frequency_control),
            (0x29, ClockType::InternalProgrammable, true, 3)
        );
        assert_eq!(ac.terminals[0].clock_id, Some(0x29));
        assert_eq!(ac.terminals[0].channels, Some(2));
        let a = &u.streaming[0];
        assert_eq!(a.formats, Some(1));
        assert_eq!(a.channels, Some(2));
        assert_eq!((a.subslot_bytes, a.bit_resolution), (Some(4), Some(24)));
        assert_eq!(a.rates, None);
        assert_eq!(a.endpoints.len(), 2);
        assert_eq!(a.data_endpoint().unwrap().sync, SyncType::Async);
    }

    #[test]
    fn truncated() {
        let mut b = uac1();
        b.truncate(b.len() - 3);
        assert!(parse_descriptors(&b).truncated);
        assert!(parse_descriptors(&[0]).truncated);
        assert_eq!(parse_descriptors(&[]), UsbDescriptors::default());
    }
}
