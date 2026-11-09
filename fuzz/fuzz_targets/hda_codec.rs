// SPDX-License-Identifier: LGPL-2.1-or-later
//! HDA codec dumps.
#![no_main]

use audiocaps_parse::hda::parse_codec;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let c = parse_codec(&String::from_utf8_lossy(data));
    for n in &c.nodes {
        let _ = (n.channels(), n.hdmi(), n.hbr(), n.display_port());
        if let Some(p) = n.pin_default {
            let _ = (p.device(), p.connectivity(), p.location());
        }
    }
});
