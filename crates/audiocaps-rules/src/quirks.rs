// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sink quirk data: facts about specific sinks that no EDID or CEC field carries, or that a sink
//! misreports. Packs are JSON (`packs/quirks-core.json` is bundled); later packs override entries
//! with the same id. Entries match by PNP manufacturer id plus a product code, a model name (EDID
//! monitor name or CEC OSD name) or a CEC vendor id.

use serde::Deserialize;

use audiocaps_model::{AppliedQuirk, QuirkEffect};

/// Version of the quirk-pack format this build reads.
pub const QUIRK_PACK_FORMAT: u32 = 1;

/// The bundled pack.
pub const BUNDLED: &str = include_str!("../packs/quirks-core.json");

#[derive(Debug, Deserialize)]
struct Pack {
    format: u32,
    #[serde(default)]
    entries: Vec<serde_json::Value>,
}

/// One quirk entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    /// Stable id.
    pub id: String,
    /// What the entry corrects or adds.
    pub description: String,
    /// Where the facts come from.
    #[serde(default)]
    pub reference: Option<String>,
    /// Match criteria.
    #[serde(rename = "match")]
    pub matcher: Match,
    /// Effects.
    pub effects: Vec<QuirkEffect>,
}

/// Match criteria: the manufacturer (or CEC vendor) and at least one more identifier must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Match {
    /// PNP manufacturer id, e.g. `DON`.
    #[serde(default)]
    pub manufacturer: Option<String>,
    /// EDID product code.
    #[serde(default)]
    pub product_code: Option<u16>,
    /// Model names (EDID monitor name or CEC OSD name), compared case-insensitively.
    #[serde(default)]
    pub model_names: Vec<String>,
    /// CEC vendor id (OUI, hex).
    #[serde(default)]
    pub cec_vendor: Option<String>,
}

/// What is known about a sink, for matching.
#[derive(Debug, Clone, Default)]
pub struct SinkKey<'a> {
    /// PNP manufacturer id.
    pub manufacturer: Option<&'a str>,
    /// EDID product code.
    pub product_code: Option<u16>,
    /// Model names seen (EDID name, CEC OSD name).
    pub model_names: Vec<&'a str>,
    /// CEC vendor id (hex).
    pub cec_vendor: Option<&'a str>,
}

impl Match {
    fn matches(&self, k: &SinkKey<'_>) -> bool {
        let maker = match (&self.manufacturer, k.manufacturer) {
            (Some(m), Some(km)) if m.eq_ignore_ascii_case(km) => true,
            _ => {
                matches!((&self.cec_vendor, k.cec_vendor), (Some(v), Some(kv)) if v.eq_ignore_ascii_case(kv))
            }
        };
        if !maker {
            return false;
        }
        let product = self.product_code.is_some() && self.product_code == k.product_code;
        let name = self.model_names.iter().any(|n| {
            k.model_names
                .iter()
                .any(|kn| kn.trim().eq_ignore_ascii_case(n))
        });
        product || name
    }
}

/// A problem with one pack or entry; the rest still loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadIssue {
    /// The pack's origin.
    pub origin: String,
    /// What was wrong.
    pub detail: String,
}

/// Loaded quirk entries, later origins overriding earlier ones by id.
#[derive(Debug, Clone, Default)]
pub struct QuirkSet {
    entries: Vec<(Entry, String)>,
    /// Packs or entries that were skipped.
    pub issues: Vec<LoadIssue>,
}

impl QuirkSet {
    /// Loads packs in order (bundled first, then overrides). Each is `(origin, json text)`.
    pub fn load<'a>(packs: impl IntoIterator<Item = (String, &'a str)>) -> Self {
        let mut set = QuirkSet::default();
        for (origin, text) in packs {
            let pack: Pack = match serde_json::from_str(text) {
                Ok(p) => p,
                Err(e) => {
                    set.issues.push(LoadIssue {
                        origin,
                        detail: format!("not a quirk pack: {e}"),
                    });
                    continue;
                }
            };
            if pack.format != QUIRK_PACK_FORMAT {
                set.issues.push(LoadIssue {
                    origin,
                    detail: format!(
                        "format {} (this build reads {QUIRK_PACK_FORMAT})",
                        pack.format
                    ),
                });
                continue;
            }
            for raw in pack.entries {
                match serde_json::from_value::<Entry>(raw) {
                    Ok(e) => {
                        set.entries.retain(|(x, _)| x.id != e.id);
                        set.entries.push((e, origin.clone()));
                    }
                    Err(err) => set.issues.push(LoadIssue {
                        origin: origin.clone(),
                        detail: format!("entry skipped: {err}"),
                    }),
                }
            }
        }
        set
    }

    /// The bundled pack alone.
    pub fn bundled() -> Self {
        Self::load([("bundled".to_string(), BUNDLED)])
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries matching a sink, as applied quirks.
    pub fn matching(&self, key: &SinkKey<'_>) -> Vec<AppliedQuirk> {
        self.entries
            .iter()
            .filter(|(e, _)| e.matcher.matches(key))
            .map(|(e, origin)| AppliedQuirk {
                id: e.id.clone(),
                description: e.description.clone(),
                origin: origin.clone(),
                reference: e.reference.clone(),
                effects: e.effects.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pack_loads_and_matches() {
        let q = QuirkSet::bundled();
        assert!(q.issues.is_empty(), "{:?}", q.issues);
        assert!(!q.is_empty());
        let k = SinkKey {
            manufacturer: Some("DON"),
            model_names: vec!["AVR-S660H"],
            ..SinkKey::default()
        };
        let m = q.matching(&k);
        assert_eq!(m[0].id, "denon-avr-s660h");
        assert!(
            m[0].effects
                .contains(&QuirkEffect::ProcessingChannels { max: 6 })
        );
        // Manufacturer alone is not enough.
        let k = SinkKey {
            manufacturer: Some("DON"),
            ..SinkKey::default()
        };
        assert!(q.matching(&k).is_empty());
        // CEC vendor + OSD name works without EDID.
        let k = SinkKey {
            cec_vendor: Some("0005CD"),
            model_names: vec!["avr-s660h"],
            ..SinkKey::default()
        };
        assert_eq!(q.matching(&k).len(), 1);
    }

    #[test]
    fn overrides_and_bad_packs() {
        let over = r#"{"format":1,"entries":[{"id":"denon-avr-s660h","description":"local",
            "match":{"manufacturer":"DON","product_code":4660},"effects":[]},
            {"id":"broken"}]}"#;
        let q = QuirkSet::load([
            ("bundled".to_string(), BUNDLED),
            ("/etc/audiocaps/quirks.d/x.json".to_string(), over),
            ("bad".to_string(), "not json"),
            ("future".to_string(), r#"{"format":9,"entries":[]}"#),
        ]);
        assert_eq!(q.len(), 1);
        assert_eq!(q.issues.len(), 3);
        let k = SinkKey {
            manufacturer: Some("DON"),
            product_code: Some(4660),
            ..SinkKey::default()
        };
        let m = q.matching(&k);
        assert_eq!(
            (m[0].description.as_str(), m[0].origin.as_str()),
            ("local", "/etc/audiocaps/quirks.d/x.json")
        );
    }
}
