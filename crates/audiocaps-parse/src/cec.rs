// SPDX-License-Identifier: LGPL-2.1-or-later
//! CEC message payloads: physical addresses, vendor ids, OSD names and short audio descriptors.

use crate::sad::{Sad, parse_sads};

/// Device type named by a logical address.
pub fn device_type(logical: u8) -> &'static str {
    match logical {
        0 => "tv",
        1 | 2 | 9 => "recording",
        3 | 6 | 7 | 10 => "tuner",
        4 | 8 | 11 => "playback",
        5 => "audio_system",
        12 | 13 => "backup",
        14 => "specific",
        _ => "unregistered",
    }
}

/// `<Report Physical Address>`: physical address (2 bytes) and device type.
pub fn physical_address(payload: &[u8]) -> Option<u16> {
    let b = payload.get(..2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// `<Device Vendor ID>`: a 24-bit IEEE OUI.
pub fn vendor_id(payload: &[u8]) -> Option<u32> {
    let b = payload.get(..3)?;
    Some(u32::from_be_bytes([0, b[0], b[1], b[2]]))
}

/// `<Set OSD Name>`: up to 14 printable ASCII characters.
pub fn osd_name(payload: &[u8]) -> Option<String> {
    let s: String = payload
        .iter()
        .take(14)
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|b| *b as char)
        .collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `<Report Short Audio Descriptor>`: up to four 3-byte SADs.
pub fn short_audio_descriptors(payload: &[u8]) -> Vec<Sad> {
    parse_sads(payload)
}

/// The audio format ids to ask for with `<Request Short Audio Descriptor>`, at most four per
/// message: LPCM, AC-3, DTS, E-AC-3, DTS-HD, MAT, plus the extension type.
pub const SAD_REQUESTS: [&[u8]; 2] = [&[1, 2, 7, 10], &[11, 12, 15]];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads() {
        assert_eq!(physical_address(&[0x10, 0x00, 0x05]), Some(0x1000));
        assert_eq!(vendor_id(&[0x00, 0x05, 0xcd]), Some(0x0005cd));
        assert_eq!(osd_name(b"AVR-S660H"), Some("AVR-S660H".into()));
        assert_eq!(osd_name(&[0, 1]), None);
        let s = short_audio_descriptors(&[0x0f, 0x7f, 0x07, 0x15, 0x07, 0x50]);
        assert_eq!(s.len(), 2);
        assert_eq!(device_type(5), "audio_system");
    }
}
