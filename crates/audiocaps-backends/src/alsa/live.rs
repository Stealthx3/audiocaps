// SPDX-License-Identifier: LGPL-2.1-or-later
//! The alsa-lib reader. Passive rules, all enforced here:
//!
//! - A PCM is opened only with `SND_PCM_NONBLOCK`, only when no sound server manages the cards,
//!   and only when `/proc` shows a free sub-device. The open reads the parameter *ranges*
//!   (`snd_pcm_hw_params_any`) and channel maps; `hw_params` is never installed, nothing is
//!   prepared, started or written, and the handle is closed straight away.
//! - Control devices are opened read-only in effect: elements are listed and read, never written,
//!   locked or subscribed.
//! - The configuration is read with `snd_config_update`, which loads files only; no PCM plugin
//!   is instantiated, so nothing connects to PulseAudio, PipeWire or JACK.

use alsa::hctl::HCtl;
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, Output};

use super::dump::{
    AlsaDump, CardDump, ControlDump, DUMP_FORMAT, HwRanges, NotOpened, PcmDump, PcmResult,
    StreamDir,
};
use crate::root::SystemRoot;
use audiocaps_parse::procfs;

/// Standard rates tested against a device's rate range.
pub const STANDARD_RATES: [u32; 17] = [
    5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176400, 192000,
    352800, 384000, 705600, 768000,
];

const EBUSY: i32 = 16;

/// Whether a control is one the backend reads: jacks, IEC958 switches/status, ELD.
pub fn wanted_control(name: &str) -> bool {
    name.ends_with(" Jack") || name.starts_with("IEC958") || name == "ELD"
}

/// Reads the global configuration as alsa-lib loads it.
pub fn read_config() -> Result<String, String> {
    let cfg = alsa::config::update_ref().map_err(|e| e.to_string())?;
    let mut out = Output::buffer_open().map_err(|e| e.to_string())?;
    cfg.save(&mut out).map_err(|e| e.to_string())?;
    Ok(out.buffer_string(|b| String::from_utf8_lossy(b).into_owned()))
}

/// Reads the wanted controls of one card.
pub fn read_controls(card: u32) -> Result<Vec<ControlDump>, String> {
    let h = HCtl::new(&format!("hw:{card}"), true).map_err(|e| e.to_string())?;
    h.load().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for elem in h.elem_iter() {
        let Ok(id) = elem.get_id() else { continue };
        let Ok(name) = id.get_name() else { continue };
        if !wanted_control(name) {
            continue;
        }
        let Ok(info) = elem.info() else { continue };
        let count = info.get_count();
        let ty = info.get_type();
        let kind = match ty {
            alsa::ctl::ElemType::Boolean => "boolean",
            alsa::ctl::ElemType::Integer => "integer",
            alsa::ctl::ElemType::Integer64 => "integer64",
            alsa::ctl::ElemType::Enumerated => "enumerated",
            alsa::ctl::ElemType::Bytes => "bytes",
            alsa::ctl::ElemType::IEC958 => "iec958",
            alsa::ctl::ElemType::None => "none",
        };
        let mut values = Vec::new();
        if let Ok(v) = elem.read() {
            match ty {
                alsa::ctl::ElemType::Bytes => {
                    if let Some(b) = v.get_bytes() {
                        values = b
                            .iter()
                            .take(count as usize)
                            .map(|&x| i64::from(x))
                            .collect();
                    }
                }
                _ => {
                    for i in 0..count.min(128) {
                        let x = match ty {
                            alsa::ctl::ElemType::Boolean => v.get_boolean(i).map(i64::from),
                            alsa::ctl::ElemType::Integer => v.get_integer(i).map(i64::from),
                            alsa::ctl::ElemType::Integer64 => v.get_integer64(i),
                            alsa::ctl::ElemType::Enumerated => v.get_enumerated(i).map(i64::from),
                            _ => None,
                        };
                        if let Some(x) = x {
                            values.push(x);
                        }
                    }
                }
            }
        }
        out.push(ControlDump {
            iface: format!("{:?}", id.get_interface()).to_ascii_lowercase(),
            name: name.to_string(),
            device: id.get_device(),
            index: id.get_index(),
            kind: kind.to_string(),
            values,
        });
    }
    Ok(out)
}

/// Whether `/proc` shows the PCM with no free sub-device, and the owner if known.
pub fn busy(root: &SystemRoot, card: u32, device: u32, stream: StreamDir) -> Option<Option<u32>> {
    let s = match stream {
        StreamDir::Playback => 'p',
        StreamDir::Capture => 'c',
    };
    let base = format!("/proc/asound/card{card}/pcm{device}{s}");
    let info = procfs::parse_pcm_info(&root.read_string(&format!("{base}/info")).ok()?);
    if info.subdevices_avail != Some(0) {
        return None;
    }
    let owner = root
        .read_string(&format!("{base}/sub0/status"))
        .ok()
        .and_then(|t| match procfs::parse_pcm_status(&t) {
            procfs::PcmStatus::Open { owner_pid, .. } => owner_pid,
            procfs::PcmStatus::Closed => None,
        });
    Some(owner)
}

/// Reads one PCM's hardware parameter ranges, if the passive rules allow opening it.
pub fn read_pcm(
    root: &SystemRoot,
    card: u32,
    device: u32,
    stream: StreamDir,
    managing_server: Option<&str>,
) -> PcmResult {
    if let Some(owner) = busy(root, card, device, stream) {
        return PcmResult::NotOpened {
            reason: NotOpened::Busy,
            detail: owner.map(|p| format!("owner pid {p}")),
        };
    }
    if let Some(server) = managing_server {
        return PcmResult::NotOpened {
            reason: NotOpened::ManagedByServer,
            detail: Some(server.to_string()),
        };
    }
    let dir = match stream {
        StreamDir::Playback => Direction::Playback,
        StreamDir::Capture => Direction::Capture,
    };
    let err = |e: alsa::Error| {
        let errno = e.errno().abs();
        if errno == EBUSY {
            PcmResult::NotOpened {
                reason: NotOpened::Busy,
                detail: Some("open returned EBUSY".into()),
            }
        } else {
            PcmResult::Error {
                errno: Some(errno),
                message: e.to_string(),
            }
        }
    };
    let pcm = match PCM::new(&format!("hw:{card},{device}"), dir, true) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let ranges = match HwParams::any(&pcm) {
        Ok(hw) => ranges(&pcm, &hw),
        Err(e) => return err(e),
    };
    drop(pcm);
    PcmResult::Ranges(ranges)
}

fn ranges(pcm: &PCM, hw: &HwParams<'_>) -> HwRanges {
    let access = Access::all()
        .iter()
        .filter(|a| hw.test_access(**a).is_ok())
        .map(|a| {
            match a {
                Access::MMapInterleaved => "MMAP_INTERLEAVED",
                Access::MMapNonInterleaved => "MMAP_NONINTERLEAVED",
                Access::MMapComplex => "MMAP_COMPLEX",
                Access::RWInterleaved => "RW_INTERLEAVED",
                Access::RWNonInterleaved => "RW_NONINTERLEAVED",
            }
            .to_string()
        })
        .collect();
    // Only real format codes: alsa-lib asserts (aborts) on SND_PCM_FORMAT_UNKNOWN (-1) and on
    // codes past its mask size.
    let formats = Format::all()
        .iter()
        .filter(|f| (0..64).contains(&(**f as i32)))
        .filter(|f| hw.test_format(**f).is_ok())
        .map(|f| f.to_string())
        .collect();
    let rmin = hw.get_rate_min().unwrap_or(0);
    let rmax = hw.get_rate_max().unwrap_or(0);
    let rates: Vec<u32> = STANDARD_RATES
        .iter()
        .copied()
        .filter(|r| (rmin..=rmax).contains(r) && hw.test_rate(*r).is_ok())
        .collect();
    // A continuous clock accepts odd rates inside the range too.
    let rate_continuous = rmax > rmin + 2
        && [rmin + 1, (rmin + rmax) / 2 + 1]
            .iter()
            .all(|r| !STANDARD_RATES.contains(r) && hw.test_rate(*r).is_ok());
    let chmaps = pcm
        .query_chmaps()
        .take(32)
        .map(|(t, m)| {
            let kind = format!("{t:?}").to_ascii_lowercase();
            let positions = m.to_string();
            (
                kind,
                positions.split_whitespace().map(str::to_string).collect(),
            )
        })
        .collect();
    HwRanges {
        access,
        formats,
        channels: (
            hw.get_channels_min().unwrap_or(0),
            hw.get_channels_max().unwrap_or(0),
        ),
        rate: (rmin, rmax),
        rates,
        rate_continuous,
        period_us: (
            hw.get_period_time_min().unwrap_or(0),
            hw.get_period_time_max().unwrap_or(0),
        ),
        buffer_us: (
            hw.get_buffer_time_min().unwrap_or(0),
            hw.get_buffer_time_max().unwrap_or(0),
        ),
        chmaps,
    }
}

/// A full dump of every card and PCM, for the capture tool. Same passive rules as the probe.
pub fn dump_all(root: &SystemRoot) -> AlsaDump {
    let managing = crate::servers::managing_server(root);
    let (config, config_error) = match read_config() {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e)),
    };
    let pcms = root
        .read_string("/proc/asound/pcm")
        .map(|t| procfs::parse_pcm(&t))
        .unwrap_or_default();
    let cards = root
        .read_string("/proc/asound/cards")
        .map(|t| procfs::parse_cards(&t))
        .unwrap_or_default();
    let mut out = AlsaDump {
        format: DUMP_FORMAT,
        managing_server: managing.clone(),
        config,
        config_error,
        cards: vec![],
    };
    for card in &cards {
        let (controls, controls_error) = match read_controls(card.index) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e)),
        };
        let mut cd = CardDump {
            index: card.index,
            controls,
            controls_error,
            pcms: vec![],
        };
        for p in pcms.iter().filter(|p| p.card == card.index) {
            for (n, stream) in [
                (p.playback, StreamDir::Playback),
                (p.capture, StreamDir::Capture),
            ] {
                if n == 0 {
                    continue;
                }
                cd.pcms.push(PcmDump {
                    device: p.device,
                    stream,
                    result: read_pcm(root, card.index, p.device, stream, managing.as_deref()),
                });
            }
        }
        out.cards.push(cd);
    }
    out
}
