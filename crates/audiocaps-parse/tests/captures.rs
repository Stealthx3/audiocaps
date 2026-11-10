// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 2 gate: parser output matches the captures in `tests/fixtures/`.
//!
//! - every EDID parses; the SADs, name and identity in each valid kernel ELD text match the EDID
//!   of the same sink, and an ELD rebuilt from that EDID the way the kernel's DRM helper does
//!   decodes to the same values;
//! - every HDA codec dump parses with one node per `Node` line, and every HDMI/DP pin has caps;
//! - every USB `stream#` file parses with one format per `Altset` line, and where the capture has
//!   the device's raw `descriptors`, every alternate setting matches them;
//! - every `hw_params` file parses (closed or open).

use std::fs;
use std::path::{Path, PathBuf};

use audiocaps_parse::edid::{Edid, parse_edid};
use audiocaps_parse::eld::{parse_eld, parse_eld_text};
use audiocaps_parse::hda::parse_codec;
use audiocaps_parse::procfs::{StreamDirection, parse_hw_params, parse_usb_stream};
use audiocaps_parse::usb::{UsbRates, parse_descriptors};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// All regular files under `dir` (symlinks are not followed) whose name satisfies `pred`.
fn files(dir: &Path, pred: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            files(&p, pred, out);
        } else if ft.is_file() && pred(&e.file_name().to_string_lossy()) {
            out.push(p);
        }
    }
}

fn find(pred: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut v = Vec::new();
    files(&fixtures(), pred, &mut v);
    v
}

fn fixture_of(p: &Path) -> String {
    p.strip_prefix(fixtures())
        .ok()
        .and_then(|r| r.iter().next())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn edids() -> Vec<(String, PathBuf, Edid)> {
    find(&|n| n == "edid")
        .into_iter()
        .filter_map(|p| {
            let b = fs::read(&p).ok()?;
            if b.is_empty() {
                return None;
            }
            let e = parse_edid(&b).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
            Some((fixture_of(&p), p, e))
        })
        .collect()
}

#[test]
fn every_edid_parses() {
    let all = edids();
    assert!(
        all.len() >= 6,
        "expected the captured EDIDs, found {}",
        all.len()
    );
    for (_, p, e) in &all {
        assert_eq!(e.missing_extensions, 0, "{}", p.display());
        for c in &e.cta {
            assert!(
                !c.truncated,
                "{}: CTA block {} truncated",
                p.display(),
                c.block
            );
        }
    }
}

/// Builds ELD bytes from an EDID the way the kernel's `drm_edid_to_eld` does.
fn eld_from_edid(e: &Edid, raw: &[u8], dp: bool) -> Vec<u8> {
    let mut eld = vec![0u8; 20];
    eld[0] = 2 << 3;
    let name = e.name.clone().unwrap_or_default();
    let name = &name.as_bytes()[..name.len().min(16)];
    let cta_rev = e.cta.first().map_or(0, |c| c.revision);
    eld[4] = (cta_rev << 5) | name.len() as u8;
    let sads: Vec<_> = e.sads().take(15).collect();
    eld[5] = ((sads.len() as u8) << 4) | if dp { 1 << 2 } else { 0 };
    eld[7] = e.speaker_allocation().map_or(0, |s| s[0]);
    eld[16..20].copy_from_slice(&raw[8..12]);
    eld.extend_from_slice(name);
    for s in sads {
        eld.extend_from_slice(&s.raw);
    }
    while !(eld.len() - 4).is_multiple_of(4) {
        eld.push(0);
    }
    eld[2] = ((eld.len() - 4) / 4) as u8;
    eld
}

#[test]
fn eld_text_matches_edid_and_raw_eld() {
    let all = edids();
    let mut checked = 0;
    for p in find(&|n| n.starts_with("eld#")) {
        let text = fs::read_to_string(&p).unwrap();
        let t = parse_eld_text(&text);
        if !t.eld_valid {
            assert!(t.sads.is_empty(), "{}: invalid ELD with SADs", p.display());
            continue;
        }
        let fx = fixture_of(&p);
        let (mfg, prod) = (t.manufacture_id.unwrap(), t.product_id.unwrap());
        let sink = all.iter().find(|(f, _, e)| {
            *f == fx && e.manufacturer_id.swap_bytes() == mfg && e.product_code == prod
        });
        let Some((_, edid_path, e)) = sink else {
            panic!("{}: no EDID in {fx} for {mfg:#x}/{prod:#x}", p.display());
        };
        let ctx = format!("{} vs {}", p.display(), edid_path.display());
        assert_eq!(t.monitor_name, e.name, "{ctx}: name");
        let edid_sads: Vec<_> = e.sads().collect();
        assert_eq!(t.sads.len(), edid_sads.len(), "{ctx}: SAD count");
        for (ts, es) in t.sads.iter().zip(&edid_sads) {
            assert_eq!(ts.coding_type, es.format_code(), "{ctx}: coding type");
            assert_eq!(ts.channels, es.channels(), "{ctx}: channels");
            assert_eq!(ts.rates_hz, es.rates_hz(), "{ctx}: rates");
            assert_eq!(ts.bits, es.lpcm_bit_depths(), "{ctx}: bits");
            assert_eq!(
                ts.max_bitrate_bps,
                es.max_bitrate_kbps().map(|k| k * 1000),
                "{ctx}: bit rate"
            );
        }
        // Raw ELD path: rebuild the bytes from the EDID and decode them.
        let raw = fs::read(edid_path).unwrap();
        let dp = t.connection_type.as_deref() == Some("DisplayPort");
        let eld = parse_eld(&eld_from_edid(e, &raw, dp)).unwrap();
        assert_eq!(
            Some(eld.monitor_name.clone()),
            t.monitor_name,
            "{ctx}: raw name"
        );
        assert_eq!(eld.manufacturer_id, mfg, "{ctx}: raw manufacturer");
        assert_eq!(eld.product_code, prod, "{ctx}: raw product");
        assert_eq!(eld.sads.len(), t.sads.len(), "{ctx}: raw SAD count");
        for (ts, rs) in t.sads.iter().zip(&eld.sads) {
            assert_eq!(
                (ts.coding_type, ts.channels),
                (rs.format_code(), rs.channels())
            );
            assert_eq!(ts.rates_hz, rs.rates_hz());
        }
        assert_eq!(Some(eld.version), t.eld_version, "{ctx}: ELD version");
        checked += 1;
    }
    assert!(
        checked >= 3,
        "expected the valid ELDs from workstation and HTPC, checked {checked}"
    );
}

#[test]
fn every_codec_dump_parses() {
    let dumps = find(&|n| n.starts_with("codec#"));
    assert!(dumps.len() >= 8, "found {} codec dumps", dumps.len());
    for p in dumps {
        let text = fs::read_to_string(&p).unwrap();
        let c = parse_codec(&text);
        let ctx = p.display();
        assert!(!c.name.is_empty(), "{ctx}: name");
        assert!(c.vendor_id.is_some(), "{ctx}: vendor id");
        let node_lines = text.lines().filter(|l| l.starts_with("Node 0x")).count();
        assert_eq!(c.nodes.len(), node_lines, "{ctx}: node count");
        for pin in c.pins() {
            assert!(
                pin.pincap.is_some(),
                "{ctx}: pin {:#x} without Pincap",
                pin.nid
            );
            assert!(
                pin.pin_default.is_some(),
                "{ctx}: pin {:#x} without default",
                pin.nid
            );
        }
        let conn_lines = text
            .lines()
            .filter(|l| l.trim_start().starts_with("Connection: "))
            .count();
        let with_conn = c.nodes.iter().filter(|n| !n.connections.is_empty()).count();
        let empty_conn = text
            .lines()
            .filter(|l| l.trim_start().starts_with("Connection: 0"))
            .count();
        assert_eq!(
            with_conn + empty_conn,
            conn_lines,
            "{ctx}: connection lists"
        );
        for n in &c.nodes {
            if let Some(sel) = n.selected {
                assert!(n.connections.contains(&sel), "{ctx}: selected not in list");
            }
        }
    }
}

#[test]
fn hdmi_codecs_have_digital_pins() {
    for p in find(&|n| n.starts_with("codec#")) {
        let c = parse_codec(&fs::read_to_string(&p).unwrap());
        if !c.name.contains("HDMI") {
            continue;
        }
        let pins: Vec<_> = c.pins().collect();
        assert!(!pins.is_empty(), "{}", p.display());
        assert!(
            pins.iter().all(|n| n.hdmi() && n.digital()),
            "{}",
            p.display()
        );
    }
}

#[test]
fn usb_streams_match_descriptors() {
    let streams = find(&|n| n.starts_with("stream") && n[6..].parse::<u32>().is_ok());
    assert!(streams.len() >= 3, "found {} stream files", streams.len());
    let mut matched = 0;
    for p in streams {
        let text = fs::read_to_string(&p).unwrap();
        let s = parse_usb_stream(&text);
        let ctx = p.display().to_string();
        assert!(
            !s.name.is_empty() && !s.location.is_empty(),
            "{ctx}: header"
        );
        let alts = text
            .lines()
            .filter(|l| l.trim_start().starts_with("Altset "))
            .count();
        let parsed: usize = s.dirs.iter().map(|d| d.formats.len()).sum();
        assert_eq!(parsed, alts, "{ctx}: formats");
        for f in s.dirs.iter().flat_map(|d| &d.formats) {
            assert!(
                f.channels > 0 && !f.format.is_empty(),
                "{ctx}: format fields"
            );
            assert!(
                !f.rates_hz.is_empty() || f.rate_range.is_some(),
                "{ctx}: rates"
            );
        }
        // Raw descriptors of the same device: sys/class/sound/cardN -> USB interface -> device.
        let card = p
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let root = fixtures().join(fixture_of(&p)).join("root");
        let Some(desc) = descriptors_for(&root, &card) else {
            continue;
        };
        let u = parse_descriptors(&desc);
        matched += 1;
        assert!(!u.truncated, "{ctx}: descriptors truncated");
        for d in &s.dirs {
            for f in &d.formats {
                let alt = u
                    .streaming
                    .iter()
                    .find(|a| a.interface == f.interface && a.alt == f.altset)
                    .unwrap_or_else(|| {
                        panic!("{ctx}: no descriptor for {}/{}", f.interface, f.altset)
                    });
                assert_eq!(alt.channels, Some(f.channels), "{ctx}: channels");
                assert_eq!(alt.bit_resolution, f.bits, "{ctx}: bits");
                let ep = alt.data_endpoint().expect("data endpoint");
                assert_eq!(Some(ep.address), f.endpoint, "{ctx}: endpoint");
                assert_eq!(
                    ep.is_in(),
                    d.direction == StreamDirection::Capture,
                    "{ctx}: direction"
                );
                if let Some(UsbRates::Discrete(r)) = &alt.rates {
                    assert_eq!(r, &f.rates_hz, "{ctx}: rates");
                }
            }
        }
    }
    assert!(matched >= 1, "no capture had USB descriptors to compare");
}

/// Follows the card's `device` link up to the USB device directory and reads `descriptors`.
fn descriptors_for(root: &Path, card: &str) -> Option<Vec<u8>> {
    let class = root.join("sys/class/sound").join(card);
    let target = fs::read_link(&class).ok()?;
    let card_dir = class.parent()?.join(target);
    let card_dir = normalize(&card_dir);
    // card dir: …/<usb-device>/<interface>/sound/cardN
    let dev = card_dir.parent()?.parent()?.parent()?;
    let p = dev.join("descriptors");
    fs::read(p).ok().filter(|b| !b.is_empty())
}

/// Resolves `..` lexically (fixture trees contain relative sysfs links).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[test]
fn hw_params_parse() {
    let all = find(&|n| n == "hw_params");
    assert!(!all.is_empty());
    let mut open = 0;
    for p in all {
        let text = fs::read_to_string(&p).unwrap();
        match parse_hw_params(&text) {
            None => assert_eq!(text.trim(), "closed", "{}", p.display()),
            Some(h) => {
                assert!(h.format.is_some() && h.rate_hz.is_some(), "{}", p.display());
                open += 1;
            }
        }
    }
    assert!(
        open >= 1,
        "expected at least one open substream in the captures"
    );
}
