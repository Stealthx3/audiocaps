// SPDX-License-Identifier: LGPL-2.1-or-later
//! JSON schema generation. The committed schema (`schema/audiocaps-snapshot-v1.schema.json`) is
//! checked against this in tests; run with `UPDATE_EXPECT=1` to regenerate after a model change.

use crate::snapshot::Snapshot;

/// Stable `$id` of schema major version 1.
pub const SCHEMA_ID_V1: &str =
    "https://stealthx3.github.io/audiocaps/schema/audiocaps-snapshot-v1.schema.json";

/// The JSON schema for [`Snapshot`], as pretty-printed JSON with a trailing newline.
pub fn snapshot_schema() -> String {
    let mut schema = schemars::schema_for!(Snapshot);
    schema.insert("$id".into(), SCHEMA_ID_V1.into());
    schema.insert(
        "x-audiocaps-schema-version".into(),
        crate::SCHEMA_VERSION.into(),
    );
    let mut out = serde_json::to_string_pretty(&schema).expect("schema serializes");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn committed_schema_is_current() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schema/audiocaps-snapshot-v1.schema.json"
        );
        expect_test::expect_file![path].assert_eq(&super::snapshot_schema());
    }
}
