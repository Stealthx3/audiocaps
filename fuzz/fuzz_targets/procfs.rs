// SPDX-License-Identifier: LGPL-2.1-or-later
//! ALSA procfs text: cards, pcm, USB stream files, hw_params, and os-release.
//! The first byte picks the parser so one corpus covers all of them.
#![no_main]

use audiocaps_parse::{os_release, procfs};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    match sel % 5 {
        0 => drop(procfs::parse_cards(&text)),
        1 => drop(procfs::parse_pcm(&text)),
        2 => drop(procfs::parse_usb_stream(&text)),
        3 => drop(procfs::parse_hw_params(&text)),
        _ => drop(os_release::parse(&text)),
    }
});
