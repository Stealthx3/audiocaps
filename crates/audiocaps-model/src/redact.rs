// SPDX-License-Identifier: LGPL-2.1-or-later
//! Redaction of sensitive values: serial numbers, Bluetooth addresses and device names.
//!
//! Sensitive strings are wrapped in [`Sensitive`]. Exports are redacted by default
//! ([`RedactionPolicy::default`]); an application can opt out per kind.
//!
//! | Kind | Default | Result |
//! |---|---|---|
//! | `serial` | hash | `sha256:<16 hex>` — stable, so sink keys still match across runs |
//! | `bluetooth_address` | hash | `sha256:<16 hex>` — stable, so correlation still works |
//! | `device_name` | mask | `redacted` |

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What kind of sensitive data a [`Sensitive`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveKind {
    /// A hardware serial number (EDID, USB).
    Serial,
    /// A Bluetooth device address.
    BluetoothAddress,
    /// A user-visible device name that may identify a person (e.g. "Alex's headphones").
    DeviceName,
}

/// The value of a [`Sensitive`]: in the clear, or redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveValue {
    /// The original value (only when the application opted out of redaction).
    Clear(String),
    /// A stable hash (`sha256:<16 hex>`) or the literal `redacted`.
    Redacted(String),
}

/// A sensitive string, redacted on export according to its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-audiocaps-redaction" = "serial and bluetooth_address: sha256 hash prefix; device_name: masked as 'redacted'; applied unless the application opts out"))]
pub struct Sensitive {
    /// What kind of data this is; decides how it is redacted.
    pub kind: SensitiveKind,
    /// The value.
    #[serde(flatten)]
    pub value: SensitiveValue,
}

impl Sensitive {
    /// A sensitive value in the clear.
    pub fn new(kind: SensitiveKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: SensitiveValue::Clear(value.into()),
        }
    }

    /// Applies `policy` in place. Already redacted values are left alone.
    pub fn redact(&mut self, policy: &RedactionPolicy) {
        if let SensitiveValue::Clear(v) = &self.value {
            match policy.mode_for(self.kind) {
                RedactionMode::Keep => {}
                RedactionMode::Hash => self.value = SensitiveValue::Redacted(stable_hash(v)),
                RedactionMode::Mask => self.value = SensitiveValue::Redacted("redacted".into()),
            }
        }
    }
}

/// How one kind of sensitive value is exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactionMode {
    /// Export in the clear.
    Keep,
    /// Replace with a stable hash.
    Hash,
    /// Replace with `redacted`.
    Mask,
}

/// Redaction settings for an export. The default redacts everything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RedactionPolicy {
    /// Serial numbers.
    pub serials: RedactionMode,
    /// Bluetooth addresses.
    pub bluetooth_addresses: RedactionMode,
    /// Device names.
    pub device_names: RedactionMode,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            serials: RedactionMode::Hash,
            bluetooth_addresses: RedactionMode::Hash,
            device_names: RedactionMode::Mask,
        }
    }
}

impl RedactionPolicy {
    /// Exports everything in the clear (application opt-out).
    pub fn keep_all() -> Self {
        Self {
            serials: RedactionMode::Keep,
            bluetooth_addresses: RedactionMode::Keep,
            device_names: RedactionMode::Keep,
        }
    }

    /// The mode for a kind.
    pub fn mode_for(&self, kind: SensitiveKind) -> RedactionMode {
        match kind {
            SensitiveKind::Serial => self.serials,
            SensitiveKind::BluetoothAddress => self.bluetooth_addresses,
            SensitiveKind::DeviceName => self.device_names,
        }
    }
}

/// `sha256:` plus the first 16 hex digits of the SHA-256 of `value`.
pub fn stable_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Types that contain [`Sensitive`] values.
pub trait Redact {
    /// Applies `policy` to every sensitive value inside.
    fn redact(&mut self, policy: &RedactionPolicy);
}

impl Redact for Sensitive {
    fn redact(&mut self, policy: &RedactionPolicy) {
        Sensitive::redact(self, policy);
    }
}

impl<T: Redact> Redact for Vec<T> {
    fn redact(&mut self, policy: &RedactionPolicy) {
        self.iter_mut().for_each(|x| x.redact(policy));
    }
}

impl<T: Redact> Redact for Option<T> {
    fn redact(&mut self, policy: &RedactionPolicy) {
        if let Some(x) = self {
            x.redact(policy);
        }
    }
}

impl<T: Redact> Redact for crate::value::Value<T> {
    fn redact(&mut self, policy: &RedactionPolicy) {
        if let Some(x) = self.known_mut() {
            x.redact(policy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_hashes_and_masks() {
        let p = RedactionPolicy::default();
        let mut serial = Sensitive::new(SensitiveKind::Serial, "ABC123");
        let mut addr = Sensitive::new(SensitiveKind::BluetoothAddress, "00:11:22:33:44:55");
        let mut name = Sensitive::new(SensitiveKind::DeviceName, "Alex's headphones");
        serial.redact(&p);
        addr.redact(&p);
        name.redact(&p);
        assert_eq!(
            serial.value,
            SensitiveValue::Redacted(stable_hash("ABC123"))
        );
        assert!(matches!(&addr.value, SensitiveValue::Redacted(h) if h.starts_with("sha256:")));
        assert_eq!(name.value, SensitiveValue::Redacted("redacted".into()));
    }

    #[test]
    fn hash_is_stable_and_short() {
        assert_eq!(stable_hash("x"), stable_hash("x"));
        assert_eq!(stable_hash("x").len(), "sha256:".len() + 16);
    }

    #[test]
    fn keep_all_leaves_values() {
        let mut s = Sensitive::new(SensitiveKind::Serial, "ABC");
        s.redact(&RedactionPolicy::keep_all());
        assert_eq!(s.value, SensitiveValue::Clear("ABC".into()));
    }

    #[test]
    fn serializes_flat() {
        let s = Sensitive::new(SensitiveKind::DeviceName, "x");
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"kind":"device_name","clear":"x"}"#
        );
    }
}
