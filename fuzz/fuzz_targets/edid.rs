// SPDX-License-Identifier: LGPL-2.1-or-later
//! EDID + CTA-861, and serial redaction (which must keep a parsable EDID parsable).
#![no_main]

use audiocaps_parse::edid::{parse_edid, redact_serials};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parsed = parse_edid(data);
    if let Ok(e) = &parsed {
        for s in e.sads() {
            let _ = s.to_capability();
        }
        let _ = (e.hdmi(), e.hdmi_forum().map(|f| f.max_frl_gbps()));
    }
    let mut copy = data.to_vec();
    let _ = redact_serials(&mut copy);
    assert_eq!(copy.len(), data.len());
    if let Ok(before) = parsed {
        let after = parse_edid(&copy).expect("redaction keeps an EDID parsable");
        assert_eq!(before.cta, after.cta, "redaction must not touch extension blocks");
    }
});
