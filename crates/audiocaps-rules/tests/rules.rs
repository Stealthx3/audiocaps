// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 7 gate: each starter rule fires only when it should. Every rule has at least one case
//! where it fires and one near miss where it must stay silent (`packs/rules-core.cases.json`).

use audiocaps_rules::RuleSet;
use audiocaps_rules::engine::{Case, fired};
use audiocaps_rules::facts::Facts;

fn cases() -> Vec<Case> {
    let j: serde_json::Value =
        serde_json::from_str(include_str!("../packs/rules-core.cases.json")).unwrap();
    serde_json::from_value(j["cases"].clone()).unwrap()
}

#[test]
fn bundled_pack_loads_cleanly() {
    let r = RuleSet::bundled();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(r.rules.len(), 29);
    for (rule, _) in &r.rules {
        assert!(
            rule.hint.contains_key("generic"),
            "{}: generic hint",
            rule.code
        );
        assert!(!rule.evidence.is_empty(), "{}: evidence", rule.code);
        assert!(!rule.description.is_empty(), "{}: description", rule.code);
    }
}

#[test]
fn every_rule_has_a_firing_case_and_a_near_miss() {
    let r = RuleSet::bundled();
    let cs = cases();
    for (rule, _) in &r.rules {
        assert!(
            cs.iter().any(|c| c.fires.contains(&rule.code)),
            "{}: no firing case",
            rule.code
        );
        assert!(
            cs.iter().any(|c| c.silent.contains(&rule.code)),
            "{}: no near miss",
            rule.code
        );
    }
}

#[test]
fn cases_hold() {
    let r = RuleSet::bundled();
    let mut bad = vec![];
    for c in cases() {
        let got = fired(&r, &Facts::from_json(&c.facts));
        for f in &c.fires {
            if !got.contains(f) {
                bad.push(format!("{}: {f} did not fire", c.name));
            }
        }
        for s in &c.silent {
            if got.contains(s) {
                bad.push(format!("{}: {s} fired", c.name));
            }
        }
        // Nothing else fires on a case built for one rule.
        for g in &got {
            if !c.fires.contains(g) {
                bad.push(format!("{}: unexpected {g}", c.name));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn empty_facts_fire_nothing() {
    assert!(fired(&RuleSet::bundled(), &Facts::default()).is_empty());
}

#[test]
fn override_disables_and_replaces() {
    let local = r#"{"format":1,"rules":[
        {"code":"POWER_SAVE_ACTIVE","severity":"info","hop":"driver_host","when":null},
        {"code":"NOT_BIT_PERFECT","severity":"warning","hop":"sound_server","when":{"known":"e.server.volume_percent"},
         "evidence":["e.server.volume_percent"],"hint":{"generic":"x"},"description":"local"}]}"#;
    let r = RuleSet::load([
        ("bundled".to_string(), audiocaps_rules::engine::BUNDLED),
        ("/etc/audiocaps/rules.d/local.json".to_string(), local),
    ]);
    assert!(r.rule("POWER_SAVE_ACTIVE").is_none());
    assert_eq!(r.rule("NOT_BIT_PERFECT").unwrap().description, "local");
    let bad = RuleSet::load([(
        "x".to_string(),
        r#"{"format":1,"rules":[{"code":"Y","severity":"info","hop":"sink","when":{"nope":1}}]}"#,
    )]);
    assert_eq!(bad.issues.len(), 1);
}
