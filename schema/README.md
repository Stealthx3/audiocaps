<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# AudioCaps JSON schema

`audiocaps-snapshot-v1.schema.json` describes the JSON export of a snapshot (JSON Schema 2020-12).
It is generated from the Rust model (`crates/audiocaps-model`) and checked by a test, so it can't
drift from the code. Print it with `audiocaps schema`.

## Versioning

- The file name carries the **major** version; `x-audiocaps-schema-version` and each snapshot's
  `schema_version` carry the full version (`1.0.0`).
- Within v1, changes are **additive only**: new optional fields, new enum variants, new finding
  codes. Consumers must ignore unknown fields and treat unknown enum values as "other".
- Removing or renaming a field, or changing its meaning, needs v2 (a new file next to this one).
- Regenerate after a model change: `UPDATE_EXPECT=1 cargo test -p audiocaps-model` in the lab VM.

## Redaction rules

Sensitive strings are `Sensitive` objects: `{"kind": "...", "clear": "..."}` or
`{"kind": "...", "redacted": "..."}`. Exports are redacted unless the application opts out.

| Kind | Default | Exported as |
|---|---|---|
| `serial` | hash | `sha256:` + 16 hex digits (stable, so sink keys still match) |
| `bluetooth_address` | hash | `sha256:` + 16 hex digits (stable, so correlation still works) |
| `device_name` | mask | `redacted` (only user-chosen names such as Bluetooth aliases) |

Each snapshot records the policy used in `redaction`. Captures made by `audiocaps capture` are
redacted further before they reach disk: MAC/Bluetooth addresses, IP addresses, the host and user
name, JSON values under keys containing `serial`, machine and session ids, Bluetooth device names,
and EDID serial fields. DMI serials, product UUIDs and USB serial attributes are never collected.
