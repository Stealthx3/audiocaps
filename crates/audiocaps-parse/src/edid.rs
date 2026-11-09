// SPDX-License-Identifier: LGPL-2.1-or-later
//! EDID helpers needed by the capture tool: serial redaction. Full EDID / CTA-861 parsing is
//! phase 2.

/// Size of one EDID block.
pub const BLOCK: usize = 128;
const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Result of [`redact_serials`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdidRedaction {
    /// Not an EDID base block (empty, too short or bad header); bytes left untouched.
    NotEdid,
    /// Serial fields cleared and the base-block checksum fixed.
    Redacted {
        /// Whether a numeric serial (bytes 12–15) was non-zero.
        numeric_serial: bool,
        /// Whether a serial-number descriptor (tag 0xFF) was present.
        serial_descriptor: bool,
    },
}

/// Clears the numeric serial (bytes 12–15) and any serial-number display descriptor (tag 0xFF)
/// in the EDID base block, then fixes the base-block checksum. Manufacturer, product code, name
/// and extension blocks (CTA-861 audio data) are kept.
pub fn redact_serials(edid: &mut [u8]) -> EdidRedaction {
    if edid.len() < BLOCK || edid[..8] != HEADER {
        return EdidRedaction::NotEdid;
    }
    let numeric_serial = edid[12..16].iter().any(|&b| b != 0);
    edid[12..16].fill(0);
    let mut serial_descriptor = false;
    for off in [54, 72, 90, 108] {
        let d = &mut edid[off..off + 18];
        if d[0] == 0 && d[1] == 0 && d[2] == 0 && d[3] == 0xFF {
            serial_descriptor = true;
            let text = b"REDACTED\n    ";
            d[5..18].copy_from_slice(text);
        }
    }
    let sum: u32 = edid[..BLOCK - 1].iter().map(|&b| u32::from(b)).sum();
    edid[BLOCK - 1] = ((256 - (sum % 256)) % 256) as u8;
    EdidRedaction::Redacted {
        numeric_serial,
        serial_descriptor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut e = vec![0u8; 256];
        e[..8].copy_from_slice(&HEADER);
        e[12..16].copy_from_slice(&[1, 2, 3, 4]);
        e[72..77].copy_from_slice(&[0, 0, 0, 0xFF, 0]);
        e[77..90].copy_from_slice(b"SN1234567\n   ");
        e[90..95].copy_from_slice(&[0, 0, 0, 0xFC, 0]);
        e[95..108].copy_from_slice(b"AVR-S660H\n   ");
        e[128] = 0x02; // CTA extension tag
        e
    }

    #[test]
    fn clears_serials_keeps_name_and_fixes_checksum() {
        let mut e = sample();
        let r = redact_serials(&mut e);
        assert_eq!(
            r,
            EdidRedaction::Redacted {
                numeric_serial: true,
                serial_descriptor: true
            }
        );
        assert_eq!(&e[12..16], &[0, 0, 0, 0]);
        assert_eq!(&e[77..86], b"REDACTED\n");
        assert_eq!(&e[95..104], b"AVR-S660H");
        let sum: u32 = e[..128].iter().map(|&b| u32::from(b)).sum();
        assert_eq!(sum % 256, 0);
        assert_eq!(e[128], 0x02);
    }

    #[test]
    fn not_edid_untouched() {
        let mut e = vec![1u8; 10];
        assert_eq!(redact_serials(&mut e), EdidRedaction::NotEdid);
        assert_eq!(e, vec![1u8; 10]);
        let mut empty: Vec<u8> = vec![];
        assert_eq!(redact_serials(&mut empty), EdidRedaction::NotEdid);
    }
}
