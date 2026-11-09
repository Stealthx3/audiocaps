// SPDX-License-Identifier: LGPL-2.1-or-later
//! EDID-Like Data (ELD): the sink summary the graphics driver hands to the HDMI/DP audio codec.
//!
//! [`parse_eld`] decodes raw ELD bytes (HDA ELD version 2, as the ALSA `ELD` control returns them)
//! and keeps every SAD byte, so flags the kernel's text form drops survive. [`parse_eld_text`]
//! reads the kernel's `/proc/asound/cardN/eld#X.Y` text, used as a fallback and to cross-check.

use crate::sad::{Sad, parse_sads};

/// Size of the ELD header plus the fixed part of the baseline block.
const FIXED: usize = 20;
/// Largest ELD the HDA spec allows.
pub const MAX_ELD: usize = 256;

/// How the sink is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    /// HDMI.
    Hdmi,
    /// DisplayPort.
    DisplayPort,
    /// A reserved value.
    Reserved(u8),
}

/// A decoded ELD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eld {
    /// ELD version (2 = CEA-861-D or later).
    pub version: u8,
    /// Baseline block length in bytes (from the header).
    pub baseline_len: usize,
    /// CEA EDID version field (3 = CEA-861-B, C or D).
    pub cea_edid_version: u8,
    /// Connection type.
    pub connection: ConnectionType,
    /// Sink supports ACP, ISRC1 and ISRC2 packets (`S_AI`).
    pub supports_ai: bool,
    /// Sink supports HDCP.
    pub supports_hdcp: bool,
    /// Audio sync delay in ms (video latency minus audio latency); `None` if not given.
    pub audio_sync_delay_ms: Option<u16>,
    /// Speaker allocation byte (CTA-861 speaker allocation data block, byte 1).
    pub speaker_allocation: u8,
    /// Port id.
    pub port_id: u64,
    /// Manufacturer id as the kernel prints it (EDID bytes 8–9, little-endian).
    pub manufacturer_id: u16,
    /// Product code (EDID bytes 10–11).
    pub product_code: u16,
    /// Monitor name (up to 16 bytes, lossily decoded).
    pub monitor_name: String,
    /// Short Audio Descriptors, raw bytes kept.
    pub sads: Vec<Sad>,
    /// Bytes after the SADs inside the baseline block (vendor-specific or padding).
    pub vendor_bytes: Vec<u8>,
}

impl Eld {
    /// Three-letter PNP manufacturer code, e.g. `HEC`.
    pub fn manufacturer_pnp(&self) -> Option<String> {
        let be = self.manufacturer_id.swap_bytes();
        pnp_id(be)
    }
}

/// Decodes a big-endian EDID manufacturer id into its three letters.
pub fn pnp_id(be: u16) -> Option<String> {
    let s: Option<String> = [10u16, 5, 0]
        .into_iter()
        .map(|shift| {
            let v = ((be >> shift) & 0x1F) as u8;
            (1..=26).contains(&v).then_some((b'A' + v - 1) as char)
        })
        .collect();
    s
}

/// Why an ELD could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EldError {
    /// Empty buffer: no sink, or the driver gave no ELD.
    Empty,
    /// Shorter than the fixed header and baseline fields.
    TooShort(usize),
    /// Longer than the 256 bytes the spec allows.
    TooLong(usize),
    /// ELD version other than 2; 31 means the driver passed only a partial ELD.
    Version(u8),
    /// Monitor name length above 16.
    NameLength(u8),
    /// The fields claim more bytes than the buffer or the baseline length holds.
    Truncated {
        /// Bytes needed.
        needed: usize,
        /// Bytes available.
        available: usize,
    },
}

impl std::fmt::Display for EldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty ELD"),
            Self::TooShort(n) => write!(f, "ELD too short ({n} bytes)"),
            Self::TooLong(n) => write!(f, "ELD too long ({n} bytes)"),
            Self::Version(v) => write!(f, "unsupported ELD version {v}"),
            Self::NameLength(n) => write!(f, "monitor name length {n} exceeds 16"),
            Self::Truncated { needed, available } => {
                write!(f, "ELD truncated: needs {needed} bytes, has {available}")
            }
        }
    }
}

impl std::error::Error for EldError {}

/// Decodes raw ELD bytes. Trailing zero padding beyond the baseline block is allowed.
pub fn parse_eld(b: &[u8]) -> Result<Eld, EldError> {
    if b.is_empty() || b.iter().all(|&x| x == 0) {
        return Err(EldError::Empty);
    }
    if b.len() > MAX_ELD {
        return Err(EldError::TooLong(b.len()));
    }
    if b.len() < FIXED {
        return Err(EldError::TooShort(b.len()));
    }
    let version = b[0] >> 3;
    if version != 2 {
        return Err(EldError::Version(version));
    }
    let baseline_len = usize::from(b[2]) * 4;
    let total = 4 + baseline_len;
    let mnl = b[4] & 0x1F;
    if mnl > 16 {
        return Err(EldError::NameLength(mnl));
    }
    let sad_count = usize::from(b[5] >> 4);
    let needed = FIXED + usize::from(mnl) + 3 * sad_count;
    let available = total.min(b.len());
    if needed > available {
        return Err(EldError::Truncated { needed, available });
    }
    let name_end = FIXED + usize::from(mnl);
    let name_bytes = &b[FIXED..name_end];
    let name_bytes = match name_bytes.iter().position(|&c| c == 0) {
        Some(p) => &name_bytes[..p],
        None => name_bytes,
    };
    let sad_end = name_end + 3 * sad_count;
    let delay = b[6];
    Ok(Eld {
        version,
        baseline_len,
        cea_edid_version: b[4] >> 5,
        connection: match (b[5] >> 2) & 0x03 {
            0 => ConnectionType::Hdmi,
            1 => ConnectionType::DisplayPort,
            n => ConnectionType::Reserved(n),
        },
        supports_ai: b[5] & 0x02 != 0,
        supports_hdcp: b[5] & 0x01 != 0,
        audio_sync_delay_ms: (delay != 0).then_some(u16::from(delay) * 2),
        speaker_allocation: b[7],
        port_id: u64::from_le_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
        manufacturer_id: u16::from_le_bytes([b[16], b[17]]),
        product_code: u16::from_le_bytes([b[18], b[19]]),
        monitor_name: String::from_utf8_lossy(name_bytes).trim_end().to_string(),
        sads: parse_sads(&b[name_end..sad_end]),
        vendor_bytes: b[sad_end..available].to_vec(),
    })
}

/// Speaker names for the speaker allocation bits, as the kernel prints them.
pub const SPEAKER_BITS: [&str; 11] = [
    "FL/FR", "LFE", "FC", "RL/RR", "RC", "FLC/FRC", "RLC/RRC", "FLW/FRW", "FLH/FRH", "TC", "FCH",
];

/// One SAD from the kernel's ELD text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EldTextSad {
    /// Coding type number as printed (`[0xa]`).
    pub coding_type: u8,
    /// Coding type description.
    pub coding_name: String,
    /// Channel count.
    pub channels: u8,
    /// Rates in Hz.
    pub rates_hz: Vec<u32>,
    /// LPCM bit depths.
    pub bits: Vec<u8>,
    /// Maximum bit rate in bit/s.
    pub max_bitrate_bps: Option<u32>,
    /// Other `sadN_*` keys, verbatim.
    pub other: Vec<(String, String)>,
}

/// The kernel's ELD text (`/proc/asound/cardN/eld#X.Y`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EldText {
    /// A sink is connected (hot-plug detect).
    pub monitor_present: bool,
    /// The ELD is valid.
    pub eld_valid: bool,
    /// Codec pin widget.
    pub codec_pin_nid: Option<u16>,
    /// Codec device (MST stream) index.
    pub codec_dev_id: Option<u16>,
    /// Converter widget.
    pub codec_cvt_nid: Option<u16>,
    /// Monitor name.
    pub monitor_name: Option<String>,
    /// `HDMI` or `DisplayPort`.
    pub connection_type: Option<String>,
    /// ELD version.
    pub eld_version: Option<u8>,
    /// CEA EDID version.
    pub edid_version: Option<u8>,
    /// Manufacturer id.
    pub manufacture_id: Option<u16>,
    /// Product id.
    pub product_id: Option<u16>,
    /// Port id.
    pub port_id: Option<u64>,
    /// HDCP support.
    pub support_hdcp: Option<bool>,
    /// ACP/ISRC support.
    pub support_ai: Option<bool>,
    /// Audio sync delay in ms.
    pub audio_sync_delay_ms: Option<u16>,
    /// Speaker allocation mask.
    pub speakers: Option<u16>,
    /// SADs in order.
    pub sads: Vec<EldTextSad>,
    /// Unrecognised keys, verbatim.
    pub other: Vec<(String, String)>,
}

/// Parses `0x1f` or `31`.
fn num(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Splits `[0xa] E-AC-3` into (`0xa`, `E-AC-3`).
fn bracketed(v: &str) -> (Option<u64>, &str) {
    let v = v.trim();
    if let Some(rest) = v.strip_prefix('[')
        && let Some((n, tail)) = rest.split_once(']')
    {
        return (num(n), tail.trim());
    }
    (None, v)
}

fn numbers<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect()
}

/// Parses the kernel's ELD text. Unknown keys are kept in `other`; malformed values are `None`.
pub fn parse_eld_text(text: &str) -> EldText {
    let mut e = EldText::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        let small = |v: &str| num(v).and_then(|n| u16::try_from(n).ok());
        match key {
            "monitor_present" => e.monitor_present = value == "1",
            "eld_valid" => e.eld_valid = value == "1",
            "codec_pin_nid" => e.codec_pin_nid = small(value),
            "codec_dev_id" => e.codec_dev_id = small(value),
            "codec_cvt_nid" => e.codec_cvt_nid = small(value),
            "monitor_name" => e.monitor_name = Some(value.to_string()),
            "connection_type" => e.connection_type = Some(value.to_string()),
            "eld_version" => e.eld_version = bracketed(value).0.and_then(|n| u8::try_from(n).ok()),
            "edid_version" => {
                e.edid_version = bracketed(value).0.and_then(|n| u8::try_from(n).ok())
            }
            "manufacture_id" => e.manufacture_id = small(value),
            "product_id" => e.product_id = small(value),
            "port_id" => e.port_id = num(value),
            "support_hdcp" => e.support_hdcp = Some(value == "1"),
            "support_ai" => e.support_ai = Some(value == "1"),
            "audio_sync_delay" => e.audio_sync_delay_ms = small(value),
            "speakers" => e.speakers = bracketed(value).0.and_then(|n| u16::try_from(n).ok()),
            "sad_count" => {}
            k => {
                if let Some(rest) = k.strip_prefix("sad")
                    && let Some((idx, field)) = rest.split_once('_')
                    && let Ok(i) = idx.parse::<usize>()
                    && i < 16
                {
                    while e.sads.len() <= i {
                        e.sads.push(EldTextSad::default());
                    }
                    let s = &mut e.sads[i];
                    match field {
                        "coding_type" => {
                            let (n, name) = bracketed(value);
                            s.coding_type = n.and_then(|n| u8::try_from(n).ok()).unwrap_or(0);
                            s.coding_name = name.to_string();
                        }
                        "channels" => s.channels = value.parse().unwrap_or(0),
                        "rates" => s.rates_hz = numbers(bracketed(value).1),
                        "bits" => s.bits = numbers(bracketed(value).1),
                        "max_bitrate" => s.max_bitrate_bps = value.parse().ok(),
                        _ => s.other.push((field.to_string(), value.to_string())),
                    }
                } else {
                    e.other.push((k.to_string(), value.to_string()));
                }
            }
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiocaps_model::{Codec, CodecFlag};

    /// HISENSE-like ELD: HDMI, 2 SADs (E-AC-3 JOC 8ch, LPCM 2ch).
    fn sample() -> Vec<u8> {
        let name = b"HISENSE";
        let mut b = vec![0u8; 4];
        b[0] = 2 << 3;
        b.push((3 << 5) | name.len() as u8);
        b.push(2 << 4);
        b.push(0);
        b.push(0x7F);
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&[0x20, 0xA3, 0x30, 0xA0]);
        b.extend_from_slice(name);
        b.extend_from_slice(&[0x57, 0x06, 0x01, 0x09, 0x07, 0x07]);
        while !(b.len() - 4).is_multiple_of(4) {
            b.push(0);
        }
        b[2] = ((b.len() - 4) / 4) as u8;
        b
    }

    #[test]
    fn decodes_sample() {
        let e = parse_eld(&sample()).unwrap();
        assert_eq!(e.version, 2);
        assert_eq!(e.connection, ConnectionType::Hdmi);
        assert_eq!(e.monitor_name, "HISENSE");
        assert_eq!(e.manufacturer_id, 0xA320);
        assert_eq!(e.manufacturer_pnp().as_deref(), Some("HEC"));
        assert_eq!(e.product_code, 0xA030);
        assert_eq!(e.sads.len(), 2);
        assert_eq!(e.sads[0].codec(), Codec::Eac3);
        assert_eq!(e.sads[0].flags(), vec![CodecFlag::Joc]);
        assert_eq!(e.sads[1].channels(), 2);
        assert_eq!(e.audio_sync_delay_ms, None);
    }

    #[test]
    fn padding_allowed_and_errors() {
        let mut b = sample();
        b.resize(MAX_ELD, 0);
        assert!(parse_eld(&b).is_ok());
        assert_eq!(parse_eld(&[]), Err(EldError::Empty));
        assert_eq!(parse_eld(&[0; 64]), Err(EldError::Empty));
        assert_eq!(parse_eld(&[0x10; 8]), Err(EldError::TooShort(8)));
        let mut v = sample();
        v[0] = 31 << 3;
        assert_eq!(parse_eld(&v), Err(EldError::Version(31)));
        let mut t = sample();
        t[2] = 1;
        assert!(matches!(parse_eld(&t), Err(EldError::Truncated { .. })));
    }

    #[test]
    fn text() {
        let t = "monitor_present\t\t1\neld_valid\t\t1\ncodec_pin_nid\t\t0x9\nmonitor_name\t\tHISENSE\n\
                 connection_type\t\tHDMI\neld_version\t\t[0x2] CEA-861D or below\n\
                 manufacture_id\t\t0xa320\nspeakers\t\t[0xffff] FL/FR LFE\nsad_count\t\t1\n\
                 sad0_coding_type\t[0x2] AC-3\nsad0_channels\t\t6\nsad0_rates\t\t[0xe0] 32000 44100 48000\n\
                 sad0_max_bitrate\t640000\nsad0_profile\t\t3\n";
        let e = parse_eld_text(t);
        assert!(e.monitor_present && e.eld_valid);
        assert_eq!(e.codec_pin_nid, Some(9));
        assert_eq!(e.monitor_name.as_deref(), Some("HISENSE"));
        assert_eq!(e.eld_version, Some(2));
        assert_eq!(e.manufacture_id, Some(0xa320));
        assert_eq!(e.speakers, Some(0xffff));
        assert_eq!(e.sads.len(), 1);
        assert_eq!(e.sads[0].coding_type, 2);
        assert_eq!(e.sads[0].rates_hz, vec![32000, 44100, 48000]);
        assert_eq!(e.sads[0].max_bitrate_bps, Some(640_000));
        assert_eq!(e.sads[0].other, vec![("profile".into(), "3".into())]);
    }
}
