// SPDX-License-Identifier: LGPL-2.1-or-later
//! Raw ELD bytes and the kernel's ELD text.
#![no_main]

use audiocaps_parse::eld::{parse_eld, parse_eld_text};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(e) = parse_eld(data) {
        assert!(e.sads.len() <= 15);
        assert!(e.monitor_name.len() <= 16 * 3);
        for s in &e.sads {
            let _ = s.to_capability();
        }
        let _ = e.manufacturer_pnp();
    }
    let _ = parse_eld_text(&String::from_utf8_lossy(data));
});
