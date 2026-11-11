// SPDX-License-Identifier: LGPL-2.1-or-later
//! alsa-lib configuration text: parse, then resolve `default` (both directions) and a `plughw`
//! name. Resolution must terminate and never panic on any tree.
#![no_main]

use audiocaps_parse::alsa_conf::{self, CardRef, Stream};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let Ok(root) = alsa_conf::parse(&text) else {
        return;
    };
    let cards = [
        CardRef {
            index: 0,
            id: "PCH".into(),
            driver: "HDA-Intel".into(),
            name: "PCH".into(),
        },
        CardRef {
            index: 1,
            id: "Device".into(),
            driver: "USB-Audio".into(),
            name: "Device".into(),
        },
    ];
    for (name, stream) in [
        ("default", Stream::Playback),
        ("default", Stream::Capture),
        ("plughw:1,0", Stream::Playback),
    ] {
        let chain = alsa_conf::resolve_pcm(&root, name, stream, &cards);
        assert!(chain.stages.len() <= 16);
    }
});
