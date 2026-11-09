// SPDX-License-Identifier: LGPL-2.1-or-later
//! Every committed fixture passes the redaction scan (no MAC or IP addresses).

use std::process::Command;

#[test]
fn committed_fixtures_contain_no_addresses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if !e.path().join("manifest.json").is_file() {
            continue;
        }
        n += 1;
        let out = Command::new(env!("CARGO_BIN_EXE_audiocaps"))
            .arg("check-redaction")
            .arg(e.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}: {}",
            e.path().display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(n > 0);
}
