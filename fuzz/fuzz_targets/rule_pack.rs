// SPDX-License-Identifier: LGPL-2.1-or-later
//! Rule and quirk packs: arbitrary pack text loads without panicking, and every loaded rule
//! evaluates on arbitrary facts. Input: pack text, a NUL byte, facts JSON.
#![no_main]

use audiocaps_rules::RuleSet;
use audiocaps_rules::engine::fired;
use audiocaps_rules::facts::Facts;
use audiocaps_rules::quirks::QuirkSet;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (pack, facts) = match data.iter().position(|b| *b == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &b"{}"[..]),
    };
    let pack = String::from_utf8_lossy(pack);
    let set = RuleSet::load([("fuzz".to_string(), pack.as_ref())]);
    let _ = QuirkSet::load([("fuzz".to_string(), pack.as_ref())]);
    if let Ok(j) = serde_json::from_slice::<serde_json::Value>(facts) {
        let f = Facts::from_json(&j);
        let _ = fired(&set, &f);
        let _ = fired(&RuleSet::bundled(), &f);
    }
});
