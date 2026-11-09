// SPDX-License-Identifier: LGPL-2.1-or-later
//! CTA-861 Short Audio Descriptors (SADs), shared by the ELD and EDID parsers.
//!
//! A SAD is 3 bytes. The raw bytes are kept, so flags that the kernel's ELD text drops (E-AC-3 JOC,
//! MAT Atmos, extension type codes) survive.

use audiocaps_model::{Codec, CodecCapability, CodecFlag};

/// Sample rates in SAD byte 2, bit 0 first.
pub const SAD_RATES_HZ: [u32; 7] = [32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// One Short Audio Descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sad {
    /// The 3 raw bytes.
    pub raw: [u8; 3],
}

impl Sad {
    /// Wraps 3 raw bytes.
    pub fn from_bytes(b: [u8; 3]) -> Self {
        Self { raw: b }
    }

    /// Audio Format Code (1 = LPCM … 15 = extension).
    pub fn format_code(&self) -> u8 {
        (self.raw[0] >> 3) & 0x0F
    }

    /// Maximum channel count.
    pub fn channels(&self) -> u8 {
        (self.raw[0] & 0x07) + 1
    }

    /// Rate bitmask (byte 2, bits 0–6).
    pub fn rate_mask(&self) -> u8 {
        self.raw[1] & 0x7F
    }

    /// Supported sample rates in Hz, ascending.
    pub fn rates_hz(&self) -> Vec<u32> {
        (0..7)
            .filter(|i| self.raw[1] & (1 << i) != 0)
            .map(|i| SAD_RATES_HZ[i])
            .collect()
    }

    /// LPCM bit depths (format code 1 only).
    pub fn lpcm_bit_depths(&self) -> Vec<u8> {
        if self.format_code() != 1 {
            return Vec::new();
        }
        [16u8, 20, 24]
            .into_iter()
            .enumerate()
            .filter(|(i, _)| self.raw[2] & (1 << i) != 0)
            .map(|(_, d)| d)
            .collect()
    }

    /// Maximum bit rate in kbit/s (format codes 2–8: byte 3 × 8 kbit/s).
    pub fn max_bitrate_kbps(&self) -> Option<u32> {
        match self.format_code() {
            2..=8 if self.raw[2] != 0 => Some(u32::from(self.raw[2]) * 8),
            _ => None,
        }
    }

    /// Extension type code (format code 15 only).
    pub fn extension_code(&self) -> Option<u8> {
        (self.format_code() == 15).then_some(self.raw[2] >> 3)
    }

    /// The codec this SAD describes.
    pub fn codec(&self) -> Codec {
        match self.format_code() {
            1 => Codec::Lpcm,
            2 => Codec::Ac3,
            3 => Codec::Mpeg1,
            4 => Codec::Mp3,
            5 => Codec::Mpeg2,
            6 => Codec::Aac,
            7 => Codec::Dts,
            8 => Codec::Other("atrac".into()),
            9 => Codec::OneBitAudio,
            10 => Codec::Eac3,
            11 => Codec::DtsHd,
            // CTA-861 "MAT (MLP)": how sinks advertise Dolby TrueHD.
            12 => Codec::TrueHd,
            13 => Codec::Dst,
            14 => Codec::Other("wma_pro".into()),
            15 => Codec::Other(
                match self.raw[2] >> 3 {
                    4 => "mpeg4_he_aac",
                    5 => "mpeg4_he_aac_v2",
                    6 => "mpeg4_aac_lc",
                    7 => "dra",
                    8 => "mpeg4_he_aac_mps",
                    10 => "mpeg4_aac_lc_mps",
                    11 => "mpeg_h_3d_audio",
                    12 => "ac4",
                    13 => "lpcm_3d",
                    n => return Codec::Other(format!("cta_extension_{n}")),
                }
                .into(),
            ),
            n => Codec::Other(format!("cta_format_{n}")),
        }
    }

    /// Extra flags carried in byte 3.
    pub fn flags(&self) -> Vec<CodecFlag> {
        let mut f = Vec::new();
        match self.format_code() {
            10 if self.raw[2] & 0x01 != 0 => f.push(CodecFlag::Joc),
            11 => f.push(CodecFlag::NeedsHbr),
            12 => {
                f.push(CodecFlag::NeedsHbr);
                if self.raw[2] & 0x01 != 0 {
                    f.push(CodecFlag::Atmos);
                }
            }
            _ => {}
        }
        f
    }

    /// The SAD as a model capability.
    pub fn to_capability(&self) -> CodecCapability {
        CodecCapability {
            codec: self.codec(),
            max_channels: Some(self.channels()),
            rates_hz: self.rates_hz(),
            bit_depths: self.lpcm_bit_depths(),
            max_bitrate_kbps: self.max_bitrate_kbps(),
            flags: self.flags(),
        }
    }
}

/// Splits a byte run into SADs; a trailing partial SAD is ignored.
pub fn parse_sads(bytes: &[u8]) -> Vec<Sad> {
    let (chunks, _) = bytes.as_chunks::<3>();
    chunks.iter().map(|c| Sad::from_bytes(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lpcm_8ch() {
        let s = Sad::from_bytes([0x0F, 0x7F, 0x07]);
        assert_eq!(s.codec(), Codec::Lpcm);
        assert_eq!(s.channels(), 8);
        assert_eq!(s.rates_hz(), SAD_RATES_HZ.to_vec());
        assert_eq!(s.lpcm_bit_depths(), vec![16, 20, 24]);
        assert_eq!(s.max_bitrate_kbps(), None);
    }

    #[test]
    fn ac3_bitrate_and_eac3_joc() {
        let ac3 = Sad::from_bytes([0x15, 0x07, 0x50]);
        assert_eq!(ac3.codec(), Codec::Ac3);
        assert_eq!(ac3.channels(), 6);
        assert_eq!(ac3.max_bitrate_kbps(), Some(640));
        let eac3 = Sad::from_bytes([0x57, 0x06, 0x01]);
        assert_eq!(eac3.codec(), Codec::Eac3);
        assert_eq!(eac3.flags(), vec![CodecFlag::Joc]);
    }

    #[test]
    fn mat_atmos_and_extension() {
        let mat = Sad::from_bytes([0x67, 0x54, 0x01]);
        assert_eq!(mat.codec(), Codec::TrueHd);
        assert_eq!(mat.flags(), vec![CodecFlag::NeedsHbr, CodecFlag::Atmos]);
        let ac4 = Sad::from_bytes([0x7F, 0x06, 12 << 3]);
        assert_eq!(ac4.extension_code(), Some(12));
        assert_eq!(ac4.codec(), Codec::Other("ac4".into()));
    }

    #[test]
    fn partial_tail_ignored() {
        assert_eq!(parse_sads(&[1, 2, 3, 4, 5]).len(), 1);
    }
}
