// SPDX-License-Identifier: LGPL-2.1-or-later
//! USB Audio Class descriptors.
#![no_main]

use audiocaps_parse::usb::parse_descriptors;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let u = parse_descriptors(data);
    for a in &u.streaming {
        let _ = a.data_endpoint().map(|e| e.is_in());
    }
});
