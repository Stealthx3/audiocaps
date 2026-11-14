// SPDX-License-Identifier: LGPL-2.1-or-later
//! Active probe (opt-in): play a short silence to one idle device to confirm the host path takes a
//! format.
//!
//! Runs only when the application names the endpoint and the formats. Never on a device that is
//! open, never while a sound server runs (it would hold or reopen the device), never on the
//! application's behalf for anything not asked. For LPCM it plays digital silence in the requested
//! layout on `hw:`; for compressed codecs it plays IEC 61937 *pause* bursts (silence to a decoder)
//! in the codec's carrier layout through alsa-lib's `hdmi:` or `iec958:` device, whose hooks set the
//! IEC 958 "non-audio" status and restore the previous status when the device is closed. The sink
//! can't report back, so an accepted result confirms the host side, not the receiver.

use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use audiocaps_model::{
    ActiveOutcome, ActiveProbe, ActiveProbeRequest, ActiveResult, Codec, UnknownReason,
};
use audiocaps_parse::procfs;

use super::dump::StreamDir;
use super::live::busy;
use crate::root::SystemRoot;

/// Milliseconds of silence per format.
const PLAY_MS: u32 = 100;

/// Carrier layout: (channels, rate, IEC 958 AES3 rate code).
fn carrier(c: &Codec) -> Option<(u32, u32, u8)> {
    Some(match c {
        Codec::Ac3 | Codec::Dts | Codec::Mpeg1 | Codec::Mp3 | Codec::Mpeg2 | Codec::Aac => {
            (2, 48_000, 0x02)
        }
        Codec::Eac3 => (2, 192_000, 0x0e),
        // High bit rate: 8 channels at 192 kHz (IEC 958 rate field: 768 kHz).
        Codec::TrueHd | Codec::DtsHd | Codec::Mat => (8, 192_000, 0x09),
        _ => return None,
    })
}

fn parse_id(id: &str) -> Option<(u32, u32)> {
    let rest = id.strip_prefix("alsa:card")?;
    let (c, d) = rest.split_once("/pcm")?;
    let d = d.strip_suffix('p')?;
    Some((c.parse().ok()?, d.parse().ok()?))
}

/// IEC 61937 pause bursts (Pa, Pb, Pc = pause, Pd = 32 bits) every 1536 frames, zeros elsewhere.
fn pause_bursts(frames: usize, channels: usize) -> Vec<i16> {
    let mut v = vec![0i16; frames * channels];
    let words = [0xF872u16, 0x4E1F, 0x0003, 0x0020];
    let mut f = 0;
    while f < frames {
        for (i, w) in words.iter().enumerate() {
            let idx = f * channels + i;
            if idx < v.len() {
                v[idx] = *w as i16;
            }
        }
        f += 1536;
    }
    v
}

/// Plays `samples` (interleaved S16) on an opened PCM. Returns the error stage on failure.
fn play(pcm: &PCM, channels: u32, rate: u32, samples: &[i16]) -> Result<(), (String, String)> {
    let e = |stage: &'static str| move |x: alsa::Error| (stage.to_string(), x.to_string());
    {
        let hw = HwParams::any(pcm).map_err(e("hw_params"))?;
        hw.set_access(Access::RWInterleaved)
            .map_err(e("hw_params"))?;
        hw.set_format(Format::s16()).map_err(e("hw_params"))?;
        hw.set_channels(channels).map_err(e("hw_params"))?;
        hw.set_rate(rate, ValueOr::Nearest)
            .map_err(e("hw_params"))?;
        pcm.hw_params(&hw).map_err(e("hw_params"))?;
    }
    let io = pcm.io_i16().map_err(e("write"))?;
    let mut off = 0;
    let step = channels as usize;
    while off < samples.len() {
        match io.writei(&samples[off..]) {
            Ok(n) => off += n * step,
            Err(err) if err.errno() == 11 => {
                let _ = pcm.wait(Some(50));
            }
            Err(err) => return Err(("write".into(), err.to_string())),
        }
    }
    std::thread::sleep(Duration::from_millis(u64::from(PLAY_MS) + 20));
    let st = pcm.state();
    let _ = pcm.drop();
    if st == alsa::pcm::State::XRun {
        return Err(("write".into(), "underrun while playing".into()));
    }
    Ok(())
}

/// Runs the probe the application asked for.
pub fn run(
    root: &SystemRoot,
    req: &ActiveProbeRequest,
) -> Result<ActiveProbe, (UnknownReason, String)> {
    if !matches!(root, SystemRoot::Live) {
        return Err((
            UnknownReason::NotExposed,
            "active probes run on the live system only".into(),
        ));
    }
    let (card, device) = parse_id(&req.endpoint.0).ok_or((
        UnknownReason::NotExposed,
        "active probes take an ALSA playback endpoint (alsa:cardN/pcmDp)".to_string(),
    ))?;
    if let Some(server) = crate::servers::managing_server(root) {
        return Err((
            UnknownReason::ManagedByServer,
            format!(
                "{server} manages the device; an active probe would disturb it, so it was not run"
            ),
        ));
    }
    if let Some(owner) = busy(root, card, device, StreamDir::Playback) {
        return Err((
            UnknownReason::Busy,
            format!(
                "the device is in use{}; not touched",
                owner.map(|p| format!(" (pid {p})")).unwrap_or_default()
            ),
        ));
    }
    let cards = root
        .read_string("/proc/asound/cards")
        .map(|t| procfs::parse_cards(&t))
        .unwrap_or_default();
    let id = cards
        .iter()
        .find(|c| c.index == card)
        .map(|c| c.id.clone())
        .unwrap_or_else(|| card.to_string());
    let pcms = root
        .read_string("/proc/asound/pcm")
        .map(|t| procfs::parse_pcm(&t))
        .unwrap_or_default();
    let name = pcms
        .iter()
        .find(|p| p.card == card && p.device == device)
        .map(|p| p.name.to_ascii_lowercase())
        .unwrap_or_default();
    let hdmi = name.contains("hdmi") || name.contains("displayport");
    // The `hdmi:` device's DEV is the ordinal among the card's HDMI PCMs.
    let hdmi_index = pcms
        .iter()
        .filter(|p| {
            p.card == card && p.playback > 0 && p.name.to_ascii_lowercase().contains("hdmi")
        })
        .position(|p| p.device == device);
    let spdif = !hdmi
        && (name.contains("digital")
            || name.contains("iec958")
            || name.contains("s/pdif")
            || name.contains("spdif"));

    let hw_name = format!("hw:CARD={id},DEV={device}");
    let mut device_used = hw_name.clone();
    let mut results = vec![];
    for codec in &req.codecs {
        let (channels, rate, samples, name) = if *codec == Codec::Lpcm {
            let ch = req.channels.unwrap_or(2);
            let rate = req.rate_hz.unwrap_or(48_000);
            let frames = (rate as usize) * PLAY_MS as usize / 1000;
            (ch, rate, vec![0i16; frames * ch as usize], hw_name.clone())
        } else {
            let Some((ch, rate, aes3)) = carrier(codec) else {
                results.push(ActiveResult {
                    codec: codec.clone(),
                    channels: 0,
                    rate_hz: 0,
                    outcome: ActiveOutcome::Skipped {
                        reason: "no IEC 61937 carrier for this codec".into(),
                    },
                });
                continue;
            };
            let aes = format!("AES0=0x06,AES1=0x82,AES2=0x00,AES3=0x{aes3:02x}");
            let name = match (hdmi, hdmi_index, spdif) {
                (true, Some(i), _) => format!("hdmi:CARD={id},DEV={i},{aes}"),
                (_, _, true) => format!("iec958:CARD={id},{aes}"),
                _ => {
                    results.push(ActiveResult {
                        codec: codec.clone(),
                        channels: ch,
                        rate_hz: rate,
                        outcome: ActiveOutcome::Skipped {
                            reason: "not an HDMI/DP or S/PDIF output".into(),
                        },
                    });
                    continue;
                }
            };
            let frames = (rate as usize) * PLAY_MS as usize / 1000;
            (ch, rate, pause_bursts(frames, ch as usize), name)
        };
        device_used = name.clone();
        let outcome = match PCM::new(&name, Direction::Playback, true) {
            Err(e) => ActiveOutcome::Refused {
                stage: "open".into(),
                error: e.to_string(),
            },
            Ok(pcm) => match play(&pcm, channels, rate, &samples) {
                Ok(()) => ActiveOutcome::Accepted { played_ms: PLAY_MS },
                Err((stage, error)) => ActiveOutcome::Refused { stage, error },
            },
        };
        results.push(ActiveResult {
            codec: codec.clone(),
            channels,
            rate_hz: rate,
            outcome,
        });
    }
    Ok(ActiveProbe {
        device: device_used,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_and_carriers() {
        let v = pause_bursts(3072, 2);
        assert_eq!(v[0] as u16, 0xF872);
        assert_eq!(v[3] as u16, 0x0020);
        assert_eq!(v[1536 * 2] as u16, 0xF872);
        assert_eq!(v[4], 0);
        assert_eq!(carrier(&Codec::TrueHd), Some((8, 192_000, 0x09)));
        assert_eq!(carrier(&Codec::Lpcm), None);
        assert_eq!(parse_id("alsa:card1/pcm3p"), Some((1, 3)));
        assert_eq!(parse_id("alsa:card1/pcm3c"), None);
    }
}
