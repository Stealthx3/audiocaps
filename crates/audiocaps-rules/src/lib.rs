// SPDX-License-Identifier: LGPL-2.1-or-later
//! AudioCaps declarative data and analysis.
//!
//! - [`quirks`]: sink quirk data, matched by manufacturer, product and model name (phase 6).
//! - [`paths`]: effective sets per access path and path reports (phase 7).
//! - [`query`]: "can this endpoint play format X via access path Y?" with the reason.
//! - [`facts`] and [`engine`]: declarative rule packs over facts derived from a snapshot; each rule
//!   that fires becomes a [`audiocaps_model::Finding`] with evidence and a distro-aware hint.
//!
//! Packs are bundled with each release and can be overridden from a system directory or the app's
//! bundle; nothing is fetched from the network. This crate does no I/O: callers pass pack text in.

pub mod engine;
pub mod facts;
pub mod paths;
pub mod query;
pub mod quirks;

pub use engine::{RULE_PACK_FORMAT, RuleSet};

use audiocaps_model::{FieldGroup, Snapshot};

use crate::engine::RuleScope;

/// Computes effective sets, path reports and findings for what the snapshot's scope asks for.
/// `previous` is the previous snapshot of the same probe context (for `SINK_CHANGED`).
pub fn analyze(s: &mut Snapshot, previous: Option<&Snapshot>, rules: &RuleSet) {
    paths::compute(s);
    if !s.scope.fields.includes(&FieldGroup::Findings) {
        return;
    }
    let g = facts::global(s);
    let family = s
        .environment
        .as_ref()
        .and_then(|e| e.os.known())
        .map(|o| o.family);
    let at = s.taken_at_ms;
    let mut findings = vec![];
    for (rule, _) in &rules.rules {
        if rule.scope == RuleScope::System {
            if let Some(f) = engine::apply(rule, &g, family, at) {
                findings.push(f);
            }
            continue;
        }
        for e in &s.endpoints {
            let prev = previous.and_then(|p| p.endpoints.iter().find(|x| x.id == e.id));
            let mut f = g.clone();
            // App facts belong to the endpoint the app plays to, when it named one.
            if f.get("app.endpoint")
                .is_some_and(|t| t.value.as_str() != Some(e.id.0.as_str()))
            {
                f = f.without_prefix("app.");
            }
            f.extend(&facts::endpoint(e, prev));
            if let Some(mut x) = engine::apply(rule, &f, family, at) {
                x.endpoint = Some(e.id.clone());
                x.access_path = Some(facts::preferred_path(e));
                findings.push(x);
                if rule.once {
                    break;
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        (std::cmp::Reverse(a.severity), &a.code.0, &a.endpoint).cmp(&(
            std::cmp::Reverse(b.severity),
            &b.code.0,
            &b.endpoint,
        ))
    });
    s.findings = findings;
}
