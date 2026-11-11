// SPDX-License-Identifier: LGPL-2.1-or-later
//! procfs and sysfs reads per card: HDA codecs, USB stream files, bus IDs, driver and module
//! state, PCM sub-stream state and raw MIDI devices. Only regular files are read, never `/dev`.
//! What is read follows [`Wants`], which the probe derives from the scope.

use std::collections::BTreeMap;

use audiocaps_model::Parameter;
use audiocaps_parse::hda::{self, HdaCodec};
use audiocaps_parse::procfs::{self, CardEntry, HwParams, PcmInfo, PcmStatus, RawMidi, UsbStream};

use crate::root::{EntryKind, SystemRoot};

/// What to read for each card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Wants {
    /// Codec dumps and USB stream files (transport, host caps, pins).
    pub hardware: bool,
    /// Bus IDs (PCI/USB).
    pub bus_ids: bool,
    /// Driver, module parameters, power.
    pub driver: bool,
    /// PCM sub-stream state (`info`, `status`, `hw_params`).
    pub pcm_state: bool,
    /// Raw MIDI devices.
    pub midi: bool,
}

/// PCM direction key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Dir {
    Playback,
    Capture,
}

impl Dir {
    pub fn suffix(self) -> char {
        match self {
            Dir::Playback => 'p',
            Dir::Capture => 'c',
        }
    }
}

/// procfs state of one PCM.
#[derive(Debug, Clone, Default)]
pub(crate) struct PcmProc {
    pub info: Option<PcmInfo>,
    /// Status of each sub-stream.
    pub status: Vec<PcmStatus>,
    /// Current parameters of the first open sub-stream.
    pub current: Option<HwParams>,
}

impl PcmProc {
    /// Owner of the first open sub-stream, if any is open.
    pub fn in_use(&self) -> Option<Option<u32>> {
        self.status.iter().find_map(|s| match s {
            PcmStatus::Open { owner_pid, .. } => Some(*owner_pid),
            PcmStatus::Closed => None,
        })
    }
}

/// Everything read for one card.
#[derive(Debug, Clone, Default)]
pub(crate) struct CardData {
    pub entry: Option<CardEntry>,
    pub codecs: Vec<(String, HdaCodec)>,
    pub usb_id: Option<String>,
    pub usb_streams: BTreeMap<u32, UsbStream>,
    pub pci_id: Option<String>,
    pub driver: Option<String>,
    pub module: Option<String>,
    pub params: Option<Vec<Parameter>>,
    pub power: Option<String>,
    pub pcms: BTreeMap<(u32, Dir), PcmProc>,
    pub midi: BTreeMap<u32, RawMidi>,
}

impl CardData {
    /// Whether this is a USB Audio card.
    pub fn is_usb(&self) -> bool {
        self.usb_id.is_some() || self.entry.as_ref().is_some_and(|e| e.driver == "USB-Audio")
    }
}

type CodecKey = (u32, String, String);
type Codecs = Vec<(String, HdaCodec)>;

/// Parsed codec dumps by card (index, id, name). Reading `codec#N` makes the HDA driver query the
/// codec over its bus (slow, and it can wake a suspended codec); the topology doesn't change while
/// the card exists, so each dump is read once. Cleared on kernel hotplug events.
static CODEC_CACHE: std::sync::Mutex<Vec<(CodecKey, Codecs)>> = std::sync::Mutex::new(Vec::new());

/// Forgets cached hardware data (after a hotplug event).
pub(crate) fn invalidate_cache() {
    CODEC_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Reads one card. `pcms` lists the PCM devices of the card that are in scope.
pub(crate) fn read_card(
    root: &SystemRoot,
    entry: Option<CardEntry>,
    index: u32,
    pcms: &[(u32, Dir)],
    want: Wants,
) -> CardData {
    let base = format!("/proc/asound/card{index}");
    let mut d = CardData {
        entry,
        ..CardData::default()
    };
    let listing = root.read_dir(&base).unwrap_or_default();
    if want.hardware || want.bus_ids {
        d.usb_id = root
            .read_string(&format!("{base}/usbid"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    if want.hardware {
        let key = d
            .entry
            .as_ref()
            .filter(|_| matches!(root, SystemRoot::Live))
            .map(|e| (index, e.id.clone(), e.name.clone()));
        let cached = key.as_ref().and_then(|k| {
            CODEC_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(ck, _)| ck == k)
                .map(|(_, v)| v.clone())
        });
        match cached {
            Some(c) => d.codecs = c,
            None => {
                for e in &listing {
                    if e.name.starts_with("codec#")
                        && e.kind == EntryKind::File
                        && let Ok(t) = root.read_string(&format!("{base}/{}", e.name))
                    {
                        d.codecs.push((e.name.clone(), hda::parse_codec(&t)));
                    }
                }
                if let Some(k) = key {
                    let mut cache = CODEC_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                    cache.retain(|(ck, _)| ck.0 != k.0);
                    cache.push((k, d.codecs.clone()));
                }
            }
        }
        for e in &listing {
            if let Some(n) = e.name.strip_prefix("stream")
                && let Ok(dev) = n.parse::<u32>()
                && let Ok(t) = root.read_string(&format!("{base}/{}", e.name))
            {
                d.usb_streams.insert(dev, procfs::parse_usb_stream(&t));
            }
        }
    }
    let dev = format!("/sys/class/sound/card{index}/device");
    if want.bus_ids && d.usb_id.is_none() {
        let id = |a: &str| {
            root.read_string(&format!("{dev}/{a}"))
                .ok()
                .map(|v| v.trim().trim_start_matches("0x").to_string())
        };
        if let (Some(v), Some(p)) = (id("vendor"), id("device")) {
            d.pci_id = Some(format!("{v}:{p}"));
        }
    }
    if want.driver {
        d.driver = root
            .read_string(&format!("{dev}/uevent"))
            .ok()
            .and_then(|u| {
                u.lines()
                    .find_map(|l| l.strip_prefix("DRIVER=").map(|v| v.trim().to_string()))
            });
        d.module = d.driver.as_ref().map(|m| m.replace('-', "_"));
        if let Some(m) = &d.module {
            let pdir = format!("/sys/module/{m}/parameters");
            if let Ok(entries) = root.read_dir(&pdir) {
                let mut params = Vec::new();
                for e in entries
                    .iter()
                    .filter(|e| e.kind == EntryKind::File)
                    .take(64)
                {
                    if let Ok(v) = root.read_string(&format!("{pdir}/{}", e.name)) {
                        params.push(Parameter {
                            name: e.name.clone(),
                            value: v.trim().to_string(),
                        });
                    }
                }
                d.params = Some(params);
            }
        }
        d.power = root
            .read_string(&format!("{dev}/power/runtime_status"))
            .ok()
            .map(|s| s.trim().to_string());
    }
    if want.pcm_state {
        for &(device, dir) in pcms {
            let pdir = format!("{base}/pcm{device}{}", dir.suffix());
            let mut p = PcmProc {
                info: root
                    .read_string(&format!("{pdir}/info"))
                    .ok()
                    .map(|t| procfs::parse_pcm_info(&t)),
                ..PcmProc::default()
            };
            let subs = root.read_dir(&pdir).unwrap_or_default();
            for s in subs.iter().filter(|s| s.name.starts_with("sub")).take(32) {
                let st = root
                    .read_string(&format!("{pdir}/{}/status", s.name))
                    .map(|t| procfs::parse_pcm_status(&t))
                    .unwrap_or(PcmStatus::Closed);
                if p.current.is_none() && matches!(st, PcmStatus::Open { .. }) {
                    p.current = root
                        .read_string(&format!("{pdir}/{}/hw_params", s.name))
                        .ok()
                        .and_then(|t| procfs::parse_hw_params(&t));
                }
                p.status.push(st);
            }
            d.pcms.insert((device, dir), p);
        }
    }
    if want.midi {
        for e in &listing {
            if let Some(n) = e.name.strip_prefix("midi")
                && let Ok(dev) = n.parse::<u32>()
                && let Ok(t) = root.read_string(&format!("{base}/{}", e.name))
            {
                d.midi.insert(dev, procfs::parse_rawmidi(&t));
            }
        }
    }
    d
}

/// Lists raw MIDI devices of a card without reading them (for discovery).
pub(crate) fn midi_devices(root: &SystemRoot, index: u32) -> Vec<u32> {
    root.read_dir(&format!("/proc/asound/card{index}"))
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.name.strip_prefix("midi").and_then(|n| n.parse().ok()))
        .collect()
}
