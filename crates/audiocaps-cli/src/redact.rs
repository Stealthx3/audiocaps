// SPDX-License-Identifier: LGPL-2.1-or-later
//! Redaction of captured text and JSON before anything is written to disk.
//!
//! Rules (also recorded in each capture's manifest):
//! - MAC / Bluetooth addresses (`:`, `_` or `-` separated) → stable pseudo address `02:..` from a hash
//! - IPv4 addresses other than loopback and 0.0.0.0 → `redacted-ipv4`; IPv6 with `::` → `redacted-ipv6`
//! - The host name and user name of the capturing machine → `redacted-host`, `redacted-user`
//! - JSON keys containing `serial` → `redacted-serial-<hash>`; machine/session ids → `redacted`
//! - Bluetooth device names and aliases → `redacted-name`
//! - EDID serial number fields (see `audiocaps_parse::edid`)

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

static MAC: LazyLock<Regex> = LazyLock::new(|| {
    // No \b: addresses often follow `_` (BlueZ `dev_AA_BB_…`), which is a word character.
    // Boundaries are checked by hand in `mac_spans`.
    Regex::new(r"(?i)[0-9a-f]{2}([:_-])[0-9a-f]{2}(?:[:_-][0-9a-f]{2}){4}").expect("regex")
});
static IPV4: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}\b")
        .expect("regex")
});
static IPV6: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:[0-9a-f]{1,4}:){1,7}:(?:[0-9a-f]{1,4}(?::[0-9a-f]{1,4}){0,6})?\b|(?i)\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b")
        .expect("regex")
});

/// Byte spans of MAC-like addresses that are not part of a longer hex/colon run.
fn mac_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let edge = |c: u8| c.is_ascii_hexdigit() || c == b':';
    MAC.find_iter(s)
        .filter(|m| {
            let before = m.start().checked_sub(1).map(|i| b[i]);
            let after = b.get(m.end()).copied();
            !before.is_some_and(edge) && !after.is_some_and(edge)
        })
        .map(|m| (m.start(), m.end()))
        .collect()
}

/// Identity of the capturing machine, replaced wherever it appears.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    host: Option<Regex>,
    user: Option<Regex>,
    /// Leave `02:…` pseudo addresses alone (when re-redacting an already redacted fixture).
    keep_pseudo: bool,
}

fn word(s: &str) -> Option<Regex> {
    let s = s.trim();
    (s.len() >= 2).then(|| Regex::new(&format!(r"\b{}\b", regex::escape(s))).expect("regex"))
}

fn short_hash(s: &str) -> String {
    let d = Sha256::digest(s.to_ascii_lowercase().as_bytes());
    d.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// The rules, one line each, for the manifest.
pub const RULES: &[&str] = &[
    "MAC and Bluetooth addresses replaced by stable pseudo addresses (02:..)",
    "IPv4 (except loopback) and IPv6 addresses replaced",
    "host name and user name of the capturing machine replaced",
    "JSON values under keys containing 'serial' hashed; machine and session ids removed",
    "Bluetooth device names and aliases replaced",
    "EDID numeric serial and serial descriptor cleared, checksum fixed",
    "DMI serials, product UUID and USB serial attributes are never collected",
];

impl Redactor {
    /// A redactor for this host and user.
    pub fn new(host: Option<&str>, user: Option<&str>) -> Self {
        Self {
            host: host.and_then(word),
            user: user.and_then(word),
            keep_pseudo: false,
        }
    }

    /// A redactor for fixtures that were already redacted: pseudo addresses stay as they are.
    pub fn for_redacted_fixture() -> Self {
        Self {
            keep_pseudo: true,
            ..Self::default()
        }
    }

    fn macs(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for (a, z) in mac_spans(s) {
            let m = &s[a..z];
            out.push_str(&s[last..a]);
            if self.keep_pseudo && m.starts_with("02") {
                out.push_str(m);
            } else {
                let sep = &m[2..3];
                let h = Sha256::digest(m.replace(['_', '-'], ":").to_ascii_lowercase().as_bytes());
                let parts: Vec<String> = std::iter::once("02".to_string())
                    .chain(h.iter().take(5).map(|b| format!("{b:02X}")))
                    .collect();
                out.push_str(&parts.join(sep));
            }
            last = z;
        }
        out.push_str(&s[last..]);
        out
    }

    /// Redacts free text.
    pub fn text(&self, s: &str) -> String {
        let mut out = self.macs(s);
        out = IPV4
            .replace_all(&out, |c: &regex::Captures<'_>| match &c[0] {
                "127.0.0.1" | "0.0.0.0" => c[0].to_string(),
                _ => "redacted-ipv4".to_string(),
            })
            .into_owned();
        out = IPV6
            .replace_all(&out, |c: &regex::Captures<'_>| match &c[0] {
                "::1" | "::" => c[0].to_string(),
                _ => "redacted-ipv6".to_string(),
            })
            .into_owned();
        if let Some(h) = &self.host {
            out = h.replace_all(&out, "redacted-host").into_owned();
        }
        if let Some(u) = &self.user {
            out = u.replace_all(&out, "redacted-user").into_owned();
        }
        out
    }

    /// Redacts a JSON document in place.
    pub fn json(&self, v: &mut Json) {
        self.walk(v, false);
    }

    fn walk(&self, v: &mut Json, bluetooth: bool) {
        match v {
            Json::String(s) => *s = self.text(s),
            Json::Array(a) => a.iter_mut().for_each(|x| self.walk(x, bluetooth)),
            Json::Object(map) => {
                let bt = bluetooth
                    || map
                        .keys()
                        .any(|k| k.starts_with("api.bluez5.") || k == "org.bluez.Device1");
                // Keys can carry addresses too (BlueZ object paths), so the map is rebuilt.
                let entries = std::mem::take(map);
                for (k, mut val) in entries {
                    self.entry(&k, &mut val, bt);
                    map.insert(self.text(&k), val);
                }
            }
            _ => {}
        }
    }

    fn entry(&self, k: &str, val: &mut Json, bt: bool) {
        let kl = k.to_ascii_lowercase();
        if kl.contains("serial")
            && let Json::String(s) = val
        {
            *s = format!("redacted-serial-{}", short_hash(s));
            return;
        }
        if kl.contains("machine-id")
            || kl.contains("session-id")
            || kl == "host_name"
            || kl == "user_name"
        {
            *val = Json::String("redacted".into());
            return;
        }
        let name_key = matches!(
            k,
            "Name"
                | "Alias"
                | "device.description"
                | "device.alias"
                | "device.nick"
                | "node.description"
                | "node.nick"
                | "media.name"
                | "description"
        );
        if bt && name_key {
            match val {
                Json::String(_) => {
                    *val = Json::String("redacted-name".into());
                    return;
                }
                // busctl --json: {"type":"s","data":"..."}
                Json::Object(o) if o.contains_key("data") => {
                    o.insert("data".into(), Json::String("redacted-name".into()));
                    return;
                }
                _ => {}
            }
        }
        self.walk(val, bt);
    }
}

/// Patterns that must never appear in a redacted capture. Returns the offending matches.
pub fn leaks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = mac_spans(text)
        .into_iter()
        .map(|(a, z)| &text[a..z])
        .filter(|m| !m.starts_with("02"))
        .map(|m| format!("mac {m}"))
        .collect();
    out.extend(
        IPV4.find_iter(text)
            .map(|m| m.as_str())
            .filter(|m| *m != "127.0.0.1" && *m != "0.0.0.0")
            .map(|m| format!("ipv4 {m}")),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_are_stable_pseudo_addresses() {
        let r = Redactor::default();
        let a = r.text("dev 00:00:5E:00:53:13 and bluez_card.00_00_5E_00_53_13");
        assert!(!a.contains("00:00:5E"));
        assert!(!a.contains("00_00_5E"));
        let colon = a.split_whitespace().nth(1).unwrap();
        assert!(colon.starts_with("02:"));
        // Same address, different separators, same pseudo address.
        assert!(a.contains(&colon.replace(':', "_")));
        assert!(leaks(&a).is_empty());
    }

    #[test]
    fn addresses_after_underscores_and_in_keys() {
        let r = Redactor::default();
        let mut v: Json = serde_json::json!({"/org/bluez/hci0/dev_00_00_5E_00_53_13": {"x": "dev_00_00_5E_00_53_13"}});
        r.json(&mut v);
        let s = v.to_string();
        assert!(!s.contains("00_00_5E"), "{s}");
        assert!(leaks(&s).is_empty());
        assert_eq!(leaks("path dev_00_00_5E_00_53_13").len(), 1);
        // Not an address: part of a longer hex/colon run.
        assert!(leaks("00:00:5e:00:53:13:77").is_empty());
    }

    #[test]
    fn re_redaction_keeps_pseudo_addresses() {
        let once = Redactor::default().text("a 00:00:5E:00:53:13");
        let twice = Redactor::for_redacted_fixture().text(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn ips_host_user() {
        let r = Redactor::new(Some("micah-desk"), Some("alex"));
        let t =
            r.text("raop 192.0.2.12 on micah-desk by alex; lo 127.0.0.1; fe80::1c2b:3aff:fe4d:1");
        assert_eq!(
            t,
            "raop redacted-ipv4 on redacted-host by redacted-user; lo 127.0.0.1; redacted-ipv6"
        );
    }

    #[test]
    fn json_serials_and_bt_names() {
        let r = Redactor::default();
        let mut v: Json = serde_json::json!([
            {"info": {"props": {"device.serial": "ABC123", "device.name": "alsa_card.pci"}}},
            {"info": {"props": {"api.bluez5.address": "00:00:5E:00:53:13", "device.description": "Alex's WH-1000XM4"}}},
            {"/org/bluez/hci0/dev_00_00_5E_00_53_13": {"org.bluez.Device1": {"Name": {"type": "s", "data": "Alex phone"}}}}
        ]);
        r.json(&mut v);
        let s = v.to_string();
        assert!(!s.contains("ABC123"));
        assert!(s.contains("redacted-serial-"));
        assert!(!s.contains("Alex"));
        assert!(s.contains("alsa_card.pci"));
        assert!(leaks(&s).is_empty());
    }

    #[test]
    fn leak_scan_flags_real_addresses() {
        assert_eq!(leaks("x 00:00:5e:00:53:13 y 198.51.100.4").len(), 2);
        assert!(leaks("version 0000:00:1f.3 1.4.2").is_empty());
    }
}
