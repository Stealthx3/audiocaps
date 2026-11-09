// SPDX-License-Identifier: LGPL-2.1-or-later
//! ALSA procfs lists: `/proc/asound/cards` and `/proc/asound/pcm`.

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
    fn junk_does_not_panic() {
        for s in ["", ":", " : : ", "x-y: a", "[", " 0 [", " 0 [a]"] {
            let _ = parse_cards(s);
            let _ = parse_pcm(s);
        }
    }
}
