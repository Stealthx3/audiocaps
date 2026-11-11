// SPDX-License-Identifier: LGPL-2.1-or-later
//! ALSA procfs text: `/proc/asound/cards`, `/proc/asound/pcm`, USB `stream#` files and PCM
//! `hw_params`.

/// One line group of `/proc/asound/cards`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardEntry {
    /// Card index.
    pub index: u32,
    /// Card id (the text in brackets), e.g. `PCH`.
    pub id: String,
    /// Driver name, e.g. `HDA-Intel`.
    pub driver: String,
    /// Short name, e.g. `HDA Intel PCH`.
    pub name: String,
}

/// Parses `/proc/asound/cards`. Lines that don't match are skipped.
pub fn parse_cards(text: &str) -> Vec<CardEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        // " 0 [PCH            ]: HDA-Intel - HDA Intel PCH"
        let t = line.trim_start();
        let Some((idx, rest)) = t.split_once(' ') else {
            continue;
        };
        let Ok(index) = idx.parse::<u32>() else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('[') else {
            continue;
        };
        let Some((id, rest)) = rest.split_once(']') else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let (driver, name) = match rest.split_once(" - ") {
            Some((d, n)) => (d.trim(), n.trim()),
            None => (rest.trim(), ""),
        };
        out.push(CardEntry {
            index,
            id: id.trim().to_string(),
            driver: driver.to_string(),
            name: name.to_string(),
        });
    }
    out
}

/// One line of `/proc/asound/pcm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmEntry {
    /// Card index.
    pub card: u32,
    /// Device index.
    pub device: u32,
    /// PCM id.
    pub id: String,
    /// PCM name.
    pub name: String,
    /// Playback substream count (0 if none).
    pub playback: u32,
    /// Capture substream count (0 if none).
    pub capture: u32,
}

/// Parses `/proc/asound/pcm`. Lines that don't match are skipped.
pub fn parse_pcm(text: &str) -> Vec<PcmEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        // "01-03: HDMI 0 : HDMI 0 : playback 1"
        let mut parts = line.split(" : ");
        let Some(head) = parts.next() else { continue };
        let Some((cd, id)) = head.split_once(':') else {
            continue;
        };
        let Some((c, d)) = cd.split_once('-') else {
            continue;
        };
        let (Ok(card), Ok(device)) = (c.trim().parse::<u32>(), d.trim().parse::<u32>()) else {
            continue;
        };
        let name = parts.next().unwrap_or("").trim().to_string();
        let (mut playback, mut capture) = (0, 0);
        for p in parts {
            let p = p.trim();
            if let Some(n) = p.strip_prefix("playback ") {
                playback = n.trim().parse().unwrap_or(0);
            } else if let Some(n) = p.strip_prefix("capture ") {
                capture = n.trim().parse().unwrap_or(0);
            }
        }
        out.push(PcmEntry {
            card,
            device,
            id: id.trim().to_string(),
            name,
            playback,
            capture,
        });
    }
    out
}

/// Direction of a USB audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    /// Playback (host to device).
    Playback,
    /// Capture (device to host).
    Capture,
}

/// One alternate setting in a USB `stream#` file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsbAltFormat {
    /// USB interface number.
    pub interface: u8,
    /// Alternate setting.
    pub altset: u8,
    /// ALSA sample format name, e.g. `S24_3LE`.
    pub format: String,
    /// Channels.
    pub channels: u8,
    /// Endpoint address.
    pub endpoint: Option<u8>,
    /// Endpoint sync type as printed (`ASYNC`, `ADAPTIVE`, `SYNC`, `NONE`).
    pub sync: Option<String>,
    /// Discrete rates in Hz (empty when continuous).
    pub rates_hz: Vec<u32>,
    /// Continuous rate range in Hz.
    pub rate_range: Option<(u32, u32)>,
    /// Valid bits per sample.
    pub bits: Option<u8>,
    /// Channel map, by position name.
    pub channel_map: Vec<String>,
    /// Data packet interval in µs.
    pub packet_interval_us: Option<u32>,
}

/// One direction in a USB `stream#` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbStreamDir {
    /// Direction.
    pub direction: StreamDirection,
    /// `Running` or `Stop`.
    pub status: Option<String>,
    /// Alternate settings.
    pub formats: Vec<UsbAltFormat>,
}

/// A USB `stream#` file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsbStream {
    /// Product name as the driver prints it.
    pub name: String,
    /// Bus location, e.g. `usb-0000:07:00.3-4`.
    pub location: String,
    /// Bus speed (`full`, `high`, `super` …).
    pub speed: String,
    /// Directions.
    pub dirs: Vec<UsbStreamDir>,
}

fn after_colon(t: &str) -> &str {
    t.split_once(':').map_or("", |(_, v)| v.trim())
}

/// Parses a USB audio `stream#` file.
pub fn parse_usb_stream(text: &str) -> UsbStream {
    let mut s = UsbStream::default();
    let mut lines = text.lines();
    if let Some(head) = lines.next() {
        // "<name> at <location>, <speed>[ speed] : USB Audio"
        let head = head.rsplit_once(" : ").map_or(head, |(h, _)| h);
        if let Some((name, rest)) = head.rsplit_once(" at ") {
            s.name = name.trim().to_string();
            let (loc, speed) = rest.split_once(", ").unwrap_or((rest, ""));
            s.location = loc.trim().to_string();
            s.speed = speed.trim().trim_end_matches(" speed").to_string();
        }
    }
    for line in lines {
        let t = line.trim();
        let dir = match t {
            "Playback:" => Some(StreamDirection::Playback),
            "Capture:" => Some(StreamDirection::Capture),
            _ => None,
        };
        if let Some(direction) = dir {
            s.dirs.push(UsbStreamDir {
                direction,
                status: None,
                formats: Vec::new(),
            });
            continue;
        }
        let Some(d) = s.dirs.last_mut() else { continue };
        if let Some(v) = t.strip_prefix("Status:") {
            d.status = Some(v.trim().to_string());
            continue;
        }
        if let Some(v) = t.strip_prefix("Interface ")
            && !v.starts_with('=')
        {
            d.formats.push(UsbAltFormat {
                interface: v.trim().parse().unwrap_or(0),
                ..UsbAltFormat::default()
            });
            continue;
        }
        let Some(f) = d.formats.last_mut() else {
            continue;
        };
        if let Some(v) = t.strip_prefix("Altset ")
            && !v.starts_with('=')
        {
            f.altset = v.trim().parse().unwrap_or(0);
            continue;
        }
        let Some((key, _)) = t.split_once(':') else {
            continue;
        };
        let v = after_colon(t);
        match key {
            "Altset" => f.altset = v.parse().unwrap_or(0),
            "Format" => f.format = v.to_string(),
            "Channels" => f.channels = v.parse().unwrap_or(0),
            "Endpoint" => {
                // "0x81 (1 IN) (ASYNC)"
                let mut it = v.split_whitespace();
                f.endpoint = it
                    .next()
                    .and_then(|e| e.strip_prefix("0x"))
                    .and_then(|e| u8::from_str_radix(e, 16).ok());
                f.sync = v
                    .rsplit_once('(')
                    .map(|(_, r)| r.trim_end_matches(')').trim().to_string())
                    .filter(|r| !r.contains(' '));
            }
            "Rates" => {
                if let Some((lo, hi)) = v.split_once(" - ") {
                    let hi = hi.split_whitespace().next().unwrap_or("");
                    if let (Ok(lo), Ok(hi)) = (lo.trim().parse(), hi.parse()) {
                        f.rate_range = Some((lo, hi));
                    }
                } else {
                    f.rates_hz = v
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter_map(|r| r.parse().ok())
                        .collect();
                }
            }
            "Bits" => f.bits = v.parse().ok(),
            "Channel map" => f.channel_map = v.split_whitespace().map(str::to_string).collect(),
            "Data packet interval" => {
                f.packet_interval_us = v.split_whitespace().next().and_then(|n| n.parse().ok())
            }
            _ => {}
        }
    }
    s
}

/// Hardware parameters of an open PCM substream (`pcmXY/sub0/hw_params`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HwParams {
    /// Access mode, e.g. `MMAP_INTERLEAVED`.
    pub access: Option<String>,
    /// Sample format, e.g. `S32_LE`.
    pub format: Option<String>,
    /// Channels.
    pub channels: Option<u32>,
    /// Rate in Hz.
    pub rate_hz: Option<u32>,
    /// Period size in frames.
    pub period_size: Option<u32>,
    /// Buffer size in frames.
    pub buffer_size: Option<u32>,
}

/// Parses `hw_params`. Returns `None` when the substream is `closed`.
pub fn parse_hw_params(text: &str) -> Option<HwParams> {
    let t = text.trim();
    if t.is_empty() || t == "closed" {
        return None;
    }
    let mut h = HwParams::default();
    for line in t.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        let first = v.split_whitespace().next().unwrap_or("");
        match k.trim() {
            "access" => h.access = Some(v.to_string()),
            "format" => h.format = Some(v.to_string()),
            "channels" => h.channels = first.parse().ok(),
            "rate" => h.rate_hz = first.parse().ok(),
            "period_size" => h.period_size = first.parse().ok(),
            "buffer_size" => h.buffer_size = first.parse().ok(),
            _ => {}
        }
    }
    Some(h)
}

/// A PCM's `info` file (`pcmXY/info` or `pcmXY/subN/info`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcmInfo {
    /// Id string.
    pub id: Option<String>,
    /// Name.
    pub name: Option<String>,
    /// Total sub-devices.
    pub subdevices_count: Option<u32>,
    /// Free sub-devices.
    pub subdevices_avail: Option<u32>,
}

/// Parses a PCM `info` file.
pub fn parse_pcm_info(text: &str) -> PcmInfo {
    let mut i = PcmInfo::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "id" => i.id = Some(v.to_string()),
            "name" => i.name = Some(v.to_string()),
            "subdevices_count" => i.subdevices_count = v.parse().ok(),
            "subdevices_avail" => i.subdevices_avail = v.parse().ok(),
            _ => {}
        }
    }
    i
}

/// A PCM sub-stream `status` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcmStatus {
    /// Not open.
    Closed,
    /// Open.
    Open {
        /// State, e.g. `RUNNING`, `PREPARED`, `SETUP`.
        state: Option<String>,
        /// Owning process.
        owner_pid: Option<u32>,
    },
}

/// Parses a sub-stream `status` file. Empty input is treated as closed.
pub fn parse_pcm_status(text: &str) -> PcmStatus {
    let t = text.trim();
    if t.is_empty() || t == "closed" {
        return PcmStatus::Closed;
    }
    let mut state = None;
    let mut owner_pid = None;
    for line in t.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim() {
            "state" => state = Some(v.trim().to_string()),
            "owner_pid" => owner_pid = v.trim().parse().ok(),
            _ => {}
        }
    }
    PcmStatus::Open { state, owner_pid }
}

/// A raw MIDI device (`/proc/asound/cardN/midiD`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawMidi {
    /// Device name (first line).
    pub name: String,
    /// `Type:` line (`Legacy` or `UMP`), when present.
    pub kind: Option<String>,
    /// Output sub-streams.
    pub outputs: Vec<MidiSubstream>,
    /// Input sub-streams.
    pub inputs: Vec<MidiSubstream>,
}

/// One raw MIDI sub-stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MidiSubstream {
    /// Number.
    pub number: u32,
    /// Owning process, when open.
    pub owner_pid: Option<u32>,
}

/// Parses a raw MIDI proc file.
pub fn parse_rawmidi(text: &str) -> RawMidi {
    let mut m = RawMidi::default();
    let mut lines = text.lines();
    m.name = lines.next().unwrap_or("").trim().to_string();
    let mut cur: Option<(bool, usize)> = None;
    for line in lines {
        let t = line.trim();
        if let Some(n) = t.strip_prefix("Output ") {
            if let Ok(number) = n.trim().parse() {
                m.outputs.push(MidiSubstream {
                    number,
                    owner_pid: None,
                });
                cur = Some((true, m.outputs.len() - 1));
            }
        } else if let Some(n) = t.strip_prefix("Input ") {
            if let Ok(number) = n.trim().parse() {
                m.inputs.push(MidiSubstream {
                    number,
                    owner_pid: None,
                });
                cur = Some((false, m.inputs.len() - 1));
            }
        } else if let Some((k, v)) = t.split_once(':') {
            match k.trim() {
                "Type" => m.kind = Some(v.trim().to_string()),
                "Owner PID" => {
                    let pid = v.trim().parse().ok();
                    match cur {
                        Some((true, i)) => m.outputs[i].owner_pid = pid,
                        Some((false, i)) => m.inputs[i].owner_pid = pid,
                        None => {}
                    }
                }
                _ => {}
            }
        }
    }
    m
}

/// A sequencer client (`/proc/asound/seq/clients`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeqClient {
    /// Client number.
    pub number: u32,
    /// Name.
    pub name: String,
    /// Kernel client (else a user-space application).
    pub kernel: bool,
    /// MIDI version tag, e.g. `Legacy`, `UMP MIDI1`, `UMP MIDI2`.
    pub midi_version: Option<String>,
    /// Ports.
    pub ports: Vec<SeqPort>,
}

/// A sequencer port.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeqPort {
    /// Port number.
    pub number: u32,
    /// Name.
    pub name: String,
    /// Readable: the port sends events (an input to the app).
    pub readable: bool,
    /// Writable: the port receives events (an output from the app).
    pub writable: bool,
    /// Others may subscribe (capital `R`/`W` in the flags).
    pub subscribable: bool,
    /// Direction tag, e.g. `In`, `Out`, `In/Out`.
    pub direction: Option<String>,
}

/// Parses `/proc/asound/seq/clients`.
pub fn parse_seq_clients(text: &str) -> Vec<SeqClient> {
    let mut out: Vec<SeqClient> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("Client ") {
            let Some((num, rest)) = rest.split_once(':') else {
                continue;
            };
            let Ok(number) = num.trim().parse() else {
                continue;
            };
            let (name, tail) = quoted(rest);
            let tag = tail
                .split_once('[')
                .and_then(|(_, r)| r.split_once(']'))
                .map(|(t, _)| t.trim())
                .unwrap_or("");
            let mut words = tag.splitn(2, ' ');
            let kind = words.next().unwrap_or("");
            out.push(SeqClient {
                number,
                name,
                kernel: kind == "Kernel",
                midi_version: words.next().map(str::to_string),
                ports: vec![],
            });
        } else if let Some(rest) = t.strip_prefix("Port ") {
            let Some(client) = out.last_mut() else {
                continue;
            };
            let Some((num, rest)) = rest.split_once(':') else {
                continue;
            };
            let Ok(number) = num.trim().parse() else {
                continue;
            };
            let (name, tail) = quoted(rest);
            let flags = tail
                .split_once('(')
                .and_then(|(_, r)| r.split_once(')'))
                .map(|(f, _)| f)
                .unwrap_or("");
            let fb = flags.as_bytes();
            let direction = tail
                .rsplit_once('[')
                .and_then(|(_, r)| r.split_once(']'))
                .map(|(d, _)| d.trim().to_string());
            client.ports.push(SeqPort {
                number,
                name,
                readable: matches!(fb.first(), Some(b'R' | b'r')),
                writable: matches!(fb.get(1), Some(b'W' | b'w')),
                subscribable: matches!(fb.first(), Some(b'R')) || matches!(fb.get(1), Some(b'W')),
                direction,
            });
        }
    }
    out
}

/// Splits `"name" rest` into the unquoted name and the rest.
fn quoted(s: &str) -> (String, &str) {
    let s = s.trim_start();
    if let Some(r) = s.strip_prefix('"')
        && let Some(end) = r.find('"')
    {
        return (r[..end].to_string(), &r[end + 1..]);
    }
    (String::new(), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards() {
        let t = " 0 [PCH            ]: HDA-Intel - HDA Intel PCH\n                      HDA Intel PCH at 0xf7f10000 irq 32\n 1 [HDMI           ]: HDA-Intel - HD-Audio Generic\n";
        let c = parse_cards(t);
        assert_eq!(c.len(), 2);
        assert_eq!(
            c[0],
            CardEntry {
                index: 0,
                id: "PCH".into(),
                driver: "HDA-Intel".into(),
                name: "HDA Intel PCH".into()
            }
        );
        assert_eq!(c[1].id, "HDMI");
    }

    #[test]
    fn pcm() {
        let t = "00-00: ALC892 Analog : ALC892 Analog : playback 1 : capture 1\n01-03: HDMI 0 : HDMI 0 : playback 1\ngarbage\n";
        let p = parse_pcm(t);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].playback, p[0].capture), (1, 1));
        assert_eq!(
            (p[1].card, p[1].device, p[1].playback, p[1].capture),
            (1, 3, 1, 0)
        );
        assert_eq!(p[1].name, "HDMI 0");
    }

    #[test]
    fn usb_stream() {
        let t = "HP, Inc HyperX SoloCast at usb-0000:07:00.3-4, full speed : USB Audio

Playback:
  Status: Running
    Interface = 1
    Altset = 1
  Interface 1
    Altset 1
    Format: S24_3LE
    Channels: 2
    Endpoint: 0x01 (1 OUT) (ADAPTIVE)
    Rates: 44100, 48000
    Bits: 24
    Channel map: FL FR
    Data packet interval: 125 us

Capture:
  Status: Stop
  Interface 2
    Altset 1
    Format: S16_LE
    Channels: 1
    Endpoint: 0x82 (2 IN) (ASYNC)
    Rates: 8000 - 48000 (continuous)
    Bits: 16
";
        let s = parse_usb_stream(t);
        assert_eq!(s.name, "HP, Inc HyperX SoloCast");
        assert_eq!(s.location, "usb-0000:07:00.3-4");
        assert_eq!(s.speed, "full");
        assert_eq!(s.dirs.len(), 2);
        let p = &s.dirs[0];
        assert_eq!(p.direction, StreamDirection::Playback);
        assert_eq!(p.status.as_deref(), Some("Running"));
        assert_eq!(p.formats.len(), 1);
        let f = &p.formats[0];
        assert_eq!((f.interface, f.altset, f.channels), (1, 1, 2));
        assert_eq!(f.endpoint, Some(0x01));
        assert_eq!(f.sync.as_deref(), Some("ADAPTIVE"));
        assert_eq!(f.rates_hz, vec![44100, 48000]);
        assert_eq!(f.bits, Some(24));
        assert_eq!(f.channel_map, vec!["FL", "FR"]);
        assert_eq!(f.packet_interval_us, Some(125));
        let c = &s.dirs[1].formats[0];
        assert_eq!(c.rate_range, Some((8000, 48000)));
        assert!(c.rates_hz.is_empty());
    }

    #[test]
    fn hw_params() {
        assert_eq!(parse_hw_params("closed\n"), None);
        let h = parse_hw_params(
            "access: MMAP_INTERLEAVED\nformat: S32_LE\nsubformat: STD\nchannels: 6\nrate: 48000 (48000/1)\nperiod_size: 1024\nbuffer_size: 32768\n",
        )
        .unwrap();
        assert_eq!(h.format.as_deref(), Some("S32_LE"));
        assert_eq!(
            (h.channels, h.rate_hz, h.buffer_size),
            (Some(6), Some(48000), Some(32768))
        );
    }

    #[test]
    fn junk_does_not_panic() {
        for s in [
            "",
            ":",
            " : : ",
            "x-y: a",
            "[",
            " 0 [",
            " 0 [a]",
            " at , :",
            "Capture:\nInterface x\nRates: - ",
        ] {
            let _ = parse_cards(s);
            let _ = parse_pcm(s);
            let _ = parse_usb_stream(s);
            let _ = parse_hw_params(s);
        }
    }

    #[test]
    fn pcm_info_and_status() {
        let i = parse_pcm_info(
            "card: 1\ndevice: 0\nid: ALC Analog\nname: ALC Analog\nsubdevices_count: 1\nsubdevices_avail: 0\n",
        );
        assert_eq!(i.subdevices_count, Some(1));
        assert_eq!(i.subdevices_avail, Some(0));
        assert_eq!(parse_pcm_status("closed\n"), PcmStatus::Closed);
        assert_eq!(
            parse_pcm_status("state: RUNNING\nowner_pid   : 777833\ntrigger_time: 1.0\n"),
            PcmStatus::Open {
                state: Some("RUNNING".into()),
                owner_pid: Some(777833)
            }
        );
    }

    #[test]
    fn rawmidi() {
        let m = parse_rawmidi(
            "VirMIDI 1-0\n\nType: Legacy\nOutput 0\n  Tx bytes     : 0\n  Owner PID    : 42\n  Mode         : native\nInput 0\n  Rx bytes     : 0\n",
        );
        assert_eq!(m.name, "VirMIDI 1-0");
        assert_eq!(m.kind.as_deref(), Some("Legacy"));
        assert_eq!(m.outputs[0].owner_pid, Some(42));
        assert_eq!(m.inputs.len(), 1);
        assert_eq!(m.inputs[0].owner_pid, None);
    }

    #[test]
    fn seq_clients() {
        let t = "Client info\n  cur  clients : 4\n\nClient   0 : \"System\" [Kernel Legacy]\n  Port   0 : \"Timer\" (Rwe-) [In/Out]\n    Connecting To: 142:0\nClient  14 : \"Midi Through\" [Kernel Legacy]\n  Port   0 : \"Midi Through Port-0\" (RWe-) [In/Out]\nClient 142 : \"PipeWire-System\" [User UMP MIDI2]\n  Port   0 : \"input\" (rwe-) [In/Out]\n  Input pool :\n    Pool size          : 200\n";
        let c = parse_seq_clients(t);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].number, 14);
        assert!(c[1].kernel);
        assert_eq!(c[1].ports[0].name, "Midi Through Port-0");
        assert!(c[1].ports[0].readable && c[1].ports[0].writable && c[1].ports[0].subscribable);
        assert!(!c[2].kernel);
        assert_eq!(c[2].midi_version.as_deref(), Some("UMP MIDI2"));
        assert!(!c[2].ports[0].subscribable);
        assert_eq!(c[0].ports[0].direction.as_deref(), Some("In/Out"));
    }
}
