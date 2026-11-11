// SPDX-License-Identifier: LGPL-2.1-or-later
//! The ALSA dump: everything the ALSA backend reads through alsa-lib, in one serializable record.

use serde::{Deserialize, Serialize};

/// Current dump format.
pub const DUMP_FORMAT: u32 = 1;

/// Name of the captured command entry and its output file (`commands/alsa.json`).
pub const CAPTURE_NAME: &str = "alsa";

/// What the ALSA backend read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlsaDump {
    /// Dump format.
    pub format: u32,
    /// The sound server that manages the cards, when one was found (PCMs were not opened).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managing_server: Option<String>,
    /// The global configuration as alsa-lib loaded it (`snd_config_save` output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Why the configuration couldn't be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    /// Cards.
    #[serde(default)]
    pub cards: Vec<CardDump>,
}

/// One card.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CardDump {
    /// Card index.
    pub index: u32,
    /// Selected controls (jacks, IEC958, ELD), when read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls: Option<Vec<ControlDump>>,
    /// Why the controls couldn't be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls_error: Option<String>,
    /// PCM devices.
    #[serde(default)]
    pub pcms: Vec<PcmDump>,
}

/// One control element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlDump {
    /// Interface: `card`, `mixer`, `pcm`, …
    pub iface: String,
    /// Name, e.g. `HDMI/DP,pcm=3 Jack`.
    pub name: String,
    /// Device.
    pub device: u32,
    /// Index.
    pub index: u32,
    /// Type: `boolean`, `integer`, `integer64`, `enumerated`, `bytes`, `iec958`.
    pub kind: String,
    /// Values (bytes as 0–255; empty for IEC958, which has no safe reader).
    #[serde(default)]
    pub values: Vec<i64>,
}

/// Stream direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamDir {
    /// Playback.
    Playback,
    /// Capture.
    Capture,
}

/// One PCM device and direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcmDump {
    /// Device number.
    pub device: u32,
    /// Direction.
    pub stream: StreamDir,
    /// What happened.
    pub result: PcmResult,
}

/// Outcome of reading one PCM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PcmResult {
    /// Opened (non-blocking), ranges read, closed.
    Ranges(HwRanges),
    /// Not opened, with the reason.
    NotOpened {
        /// Why.
        reason: NotOpened,
        /// Detail.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// The open or a query failed.
    Error {
        /// errno, when alsa-lib gave one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        errno: Option<i32>,
        /// Message.
        message: String,
    },
}

/// Why a PCM was not opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotOpened {
    /// No free sub-device: another process has it open.
    Busy,
    /// A sound server manages the card.
    ManagedByServer,
}

/// Hardware parameter ranges of an open, unconfigured PCM.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwRanges {
    /// Access types, e.g. `MMAP_INTERLEAVED`.
    pub access: Vec<String>,
    /// Sample formats, ALSA names (`S16_LE` …).
    pub formats: Vec<String>,
    /// Channels min/max.
    pub channels: (u32, u32),
    /// Rate min/max in Hz.
    pub rate: (u32, u32),
    /// Standard rates the device accepts.
    pub rates: Vec<u32>,
    /// Non-standard rates inside the range are accepted too (a continuous clock).
    pub rate_continuous: bool,
    /// Period time min/max in µs.
    pub period_us: (u32, u32),
    /// Buffer time min/max in µs.
    pub buffer_us: (u32, u32),
    /// Channel maps the driver offers: type (`fixed`, `var`, `paired`) and positions.
    #[serde(default)]
    pub chmaps: Vec<(String, Vec<String>)>,
}

impl AlsaDump {
    /// The card with this index.
    pub fn card(&self, index: u32) -> Option<&CardDump> {
        self.cards.iter().find(|c| c.index == index)
    }
}

impl CardDump {
    /// The PCM with this device and direction.
    pub fn pcm(&self, device: u32, stream: StreamDir) -> Option<&PcmDump> {
        self.pcms
            .iter()
            .find(|p| p.device == device && p.stream == stream)
    }
}
