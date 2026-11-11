// SPDX-License-Identifier: LGPL-2.1-or-later
//! Bluetooth codec identification: first byte is the codec id, the rest the capabilities.
#![no_main]

use audiocaps_parse::bt;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&codec, caps)) = data.split_first() else {
        return;
    };
    let _ = bt::a2dp_codec(codec, caps);
    for f in [bt::Family::A2dp, bt::Family::Hfp, bt::Family::LeAudio] {
        let _ = bt::transport_codec(f, codec, caps);
    }
    let s = String::from_utf8_lossy(caps);
    let _ = (bt::family(&s), bt::profile_name(&s));
});
