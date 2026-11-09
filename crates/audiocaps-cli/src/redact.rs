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
    Regex::new(r"(?i)\b[0-9a-f]{2}([:_-])[0-9a-f]{2}(?:[:_-][0-9a-f]{2}){4}\b").expect("regex")
});
static IPV4: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}\b")
        .expect("regex")
});
static IPV6: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:[0-9a-f]{1,4}:){1,7}:(?:[0-9a-f]{1,4}(?::[0-9a-f]{1,4}){0,6})?\b|(?i)\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b")
        .expect("regex")
});

/// Identity of the capturing machine, replaced wherever it appears.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    host: Option<Regex>,
    user: Option<Regex>,
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
        }
    }

    /// Redacts free text.
    pub fn text(&self, s: &str) -> String {
        let mut out = MAC
            .replace_all(s, |c: &regex::Captures<'_>| {
                let sep = &c[1];
                let h = Sha256::digest(
                    c[0].replace(['_', '-'], ":")
                        .to_ascii_lowercase()
                        .as_bytes(),
                );
                let parts: Vec<String> = std::iter::once("02".to_string())
                    .chain(h.iter().take(5).map(|b| format!("{b:02X}")))
                    .collect();
                parts.join(sep)
            })
            .into_owned();
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
                for (k, val) in map.iter_mut() {
                    let kl = k.to_ascii_lowercase();
                    if kl.contains("serial")
                        && let Json::String(s) = val
                    {
                        *s = format!("redacted-serial-{}", short_hash(s));
                        continue;
                    }
                    if kl.contains("machine-id")
                        || kl.contains("session-id")
                        || kl == "host_name"
                        || kl == "user_name"
                    {
                        *val = Json::String("redacted".into());
                        continue;
                    }
                    let name_key = matches!(
                        k.as_str(),
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
                        if let Json::String(_) = val {
                            *val = Json::String("redacted-name".into());
                            continue;
                        }
                        if let Json::Object(o) = val {
                            // busctl --json: {"type":"s","data":"..."}
                            if let Some(d) = o.get_mut("data") {
                                *d = Json::String("redacted-name".into());
                                continue;
                            }
                        }
                    }
                    self.walk(val, bt);
                }
            }
            _ => {}
        }
    }
}

/// Patterns that must never appear in a redacted capture. Returns the offending matches.
pub fn leaks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = MAC
        .find_iter(text)
        .map(|m| m.as_str())
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
        let a = r.text("dev 00:1A:7D:DA:71:13 and bluez_card.00_1A_7D_DA_71_13");
        assert!(!a.contains("00:1A:7D"));
        assert!(!a.contains("00_1A_7D"));
        let colon = a.split_whitespace().nth(1).unwrap();
        assert!(colon.starts_with("02:"));
        // Same address, different separators, same pseudo address.
        assert!(a.contains(&colon.replace(':', "_")));
        assert!(leaks(&a).is_empty());
    }

    #[test]
    fn ips_host_user() {
        let r = Redactor::new(Some("micah-desk"), Some("alex"));
        let t =
            r.text("raop 10.0.0.12 on micah-desk by alex; lo 127.0.0.1; fe80::1c2b:3aff:fe4d:1");
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
            {"info": {"props": {"api.bluez5.address": "00:1A:7D:DA:71:13", "device.description": "Alex's WH-1000XM4"}}},
            {"/org/bluez/hci0/dev_00_1A_7D_DA_71_13": {"org.bluez.Device1": {"Name": {"type": "s", "data": "Alex phone"}}}}
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
        assert_eq!(leaks("x 00:1a:7d:da:71:13 y 192.168.1.4").len(), 2);
        assert!(leaks("version 0000:00:1f.3 1.4.2").is_empty());
    }
}
