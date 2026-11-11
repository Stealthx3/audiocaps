// SPDX-License-Identifier: LGPL-2.1-or-later
//! Bluetooth audio codec identification from what BlueZ reports: an A2DP stream endpoint's codec
//! byte and capabilities (vendor codecs carry a vendor id and codec id), HFP transport codec ids,
//! and LE Audio (LC3). Names follow PipeWire's (`sbc`, `aac`, `aptx_hd`, `ldac`, …).

/// Profile family, from a BlueZ service UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// A2DP (classic stereo).
    A2dp,
    /// HFP / HSP (classic voice).
    Hfp,
    /// LE Audio (BAP).
    LeAudio,
    /// Something else.
    Other,
}

/// The profile family of a service UUID.
pub fn family(uuid: &str) -> Family {
    let u = uuid.to_ascii_lowercase();
    let short = u.get(4..8).unwrap_or("");
    match short {
        "110a" | "110b" | "110d" => Family::A2dp,
        "111e" | "111f" | "1108" | "1112" => Family::Hfp,
        "184e" | "184f" | "1850" | "1853" | "8f0b" | "8f0c" => Family::LeAudio,
        _ => Family::Other,
    }
}

/// Short profile name for a service UUID (`a2dp_sink`, `a2dp_source`, `hfp_hf`, …).
pub fn profile_name(uuid: &str) -> Option<&'static str> {
    let u = uuid.to_ascii_lowercase();
    Some(match u.get(4..8)? {
        "110a" => "a2dp_source",
        "110b" => "a2dp_sink",
        "110d" => "a2dp",
        "111e" => "hfp_hf",
        "111f" => "hfp_ag",
        "1108" => "hsp_hs",
        "1112" => "hsp_ag",
        "184e" => "ascs",
        "184f" => "bass",
        "1850" => "pacs",
        "1853" => "bap",
        _ => return None,
    })
}

/// The codec of an A2DP stream endpoint (`codec` byte, `capabilities` bytes).
pub fn a2dp_codec(codec: u8, caps: &[u8]) -> String {
    match codec {
        0x00 => "sbc".into(),
        0x01 => "mpeg".into(),
        0x02 => "aac".into(),
        0x04 => "atrac".into(),
        0xff => {
            if caps.len() < 6 {
                return "vendor".into();
            }
            let vendor = u32::from_le_bytes([caps[0], caps[1], caps[2], caps[3]]);
            let id = u16::from_le_bytes([caps[4], caps[5]]);
            match (vendor, id) {
                (0x0000_004f, 0x0001) => "aptx".into(),
                (0x0000_00d7, 0x0024) => "aptx_hd".into(),
                (0x0000_000a, 0x0002) => "aptx_ll".into(),
                (0x0000_000a, 0x0001) => "faststream".into(),
                (0x0000_012d, 0x00aa) => "ldac".into(),
                (0x0000_08a9, 0x0001) => "lc3plus_hr".into(),
                (0x0000_05f1, _) => "opus_05".into(),
                (0x0000_0075, 0x0102) => "ssc".into(),
                (v, c) => format!("vendor_{v:08x}_{c:04x}"),
            }
        }
        other => format!("codec_{other:02x}"),
    }
}

/// The codec of a transport, given its profile family.
pub fn transport_codec(family: Family, codec: u8, config: &[u8]) -> String {
    match family {
        Family::Hfp => match codec {
            0x01 => "cvsd".into(),
            0x02 => "msbc".into(),
            0x03 => "lc3_swb".into(),
            other => format!("hfp_codec_{other:02x}"),
        },
        Family::LeAudio => match codec {
            0x06 => "lc3".into(),
            other => format!("le_codec_{other:02x}"),
        },
        _ => a2dp_codec(codec, config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codecs() {
        assert_eq!(a2dp_codec(0, &[0xff, 0xff]), "sbc");
        assert_eq!(a2dp_codec(2, &[]), "aac");
        assert_eq!(
            a2dp_codec(0xff, &[0x2d, 0x01, 0, 0, 0xaa, 0x00, 0x3c, 0x07]),
            "ldac"
        );
        assert_eq!(
            a2dp_codec(0xff, &[0xd7, 0, 0, 0, 0x24, 0, 0, 0, 0, 0]),
            "aptx_hd"
        );
        assert_eq!(a2dp_codec(0xff, &[1, 2]), "vendor");
        assert_eq!(transport_codec(Family::Hfp, 2, &[]), "msbc");
        assert_eq!(transport_codec(Family::LeAudio, 6, &[]), "lc3");
    }

    #[test]
    fn families() {
        assert_eq!(family("0000110b-0000-1000-8000-00805f9b34fb"), Family::A2dp);
        assert_eq!(family("0000111e-0000-1000-8000-00805f9b34fb"), Family::Hfp);
        assert_eq!(
            profile_name("0000110B-0000-1000-8000-00805F9B34FB"),
            Some("a2dp_sink")
        );
        assert_eq!(family("x"), Family::Other);
    }
}
