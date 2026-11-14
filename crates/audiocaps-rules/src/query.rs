// SPDX-License-Identifier: LGPL-2.1-or-later
//! Queries on a snapshot: "can this endpoint play format X via access path Y?", answered from the
//! effective set with the reason when it can't. Pure, no I/O, no locks.

use audiocaps_model::{AccessPath, AppRequest, Codec, EndpointId, RateSet, Snapshot};

use crate::facts::{codec_name, format_bits};

/// The name a value has in the JSON schema (`pulse_audio`, `alsa_kernel`).
fn wire_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The path carries the format.
    Yes,
    /// It doesn't, with the reason.
    No(String),
    /// Not enough is known, with what's missing.
    Unknown(String),
}

/// Whether `endpoint` can play `format` via `path`. Needs effective sets in the snapshot.
pub fn can_play(
    s: &Snapshot,
    endpoint: &EndpointId,
    path: AccessPath,
    format: &AppRequest,
) -> Answer {
    let Some(e) = s.endpoints.iter().find(|e| &e.id == endpoint) else {
        return Answer::Unknown(format!("no endpoint {} in the snapshot", endpoint.0));
    };
    let Some(eff) = e.effective.iter().find(|x| x.access_path == path) else {
        return Answer::Unknown(format!(
            "no effective set for {} (path not available, or `effective` not in the scope)",
            wire_name(&path)
        ));
    };
    let limited = |field: audiocaps_model::FieldGroup| {
        eff.limited_by
            .iter()
            .filter(|l| l.field == field)
            .map(|l| wire_name(&l.layer))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let codec = format.codec.clone().unwrap_or(Codec::Lpcm);
    if codec != Codec::Lpcm {
        return match eff.caps.passthrough.as_ref().and_then(|v| v.known()) {
            Some(list) if list.iter().any(|c| c.codec == codec) => Answer::Yes,
            Some(_) => Answer::No(format!(
                "{} doesn't pass through on this path{}",
                codec_name(&codec),
                match limited(audiocaps_model::FieldGroup::Passthrough) {
                    l if l.is_empty() => String::new(),
                    l => format!(" (limited by {l})"),
                }
            )),
            None => Answer::Unknown("passthrough formats for this path aren't known".into()),
        };
    }
    let mut missing = vec![];
    if let Some(ch) = format.channels {
        match eff.caps.channels.as_ref().and_then(|v| v.known()) {
            Some(r) if u32::from(ch) > r.max || u32::from(ch) < r.min => {
                return Answer::No(format!(
                    "{ch} channels; the path carries {}–{}{}",
                    r.min,
                    r.max,
                    match limited(audiocaps_model::FieldGroup::Channels) {
                        l if l.is_empty() => String::new(),
                        l => format!(" (limited by {l})"),
                    }
                ));
            }
            Some(_) => {}
            None => missing.push("channels"),
        }
    }
    if let Some(rate) = format.rate_hz {
        match eff.caps.rates.as_ref().and_then(|v| v.known()) {
            Some(RateSet::Discrete { rates_hz }) if !rates_hz.contains(&rate) => {
                return Answer::No(format!(
                    "{rate} Hz; the path carries {rates_hz:?}{}",
                    match limited(audiocaps_model::FieldGroup::Rates) {
                        l if l.is_empty() => String::new(),
                        l => format!(" (limited by {l})"),
                    }
                ));
            }
            Some(RateSet::Continuous { min_hz, max_hz }) if rate < *min_hz || rate > *max_hz => {
                return Answer::No(format!("{rate} Hz; the path carries {min_hz}–{max_hz} Hz"));
            }
            Some(_) => {}
            None => missing.push("rates"),
        }
    }
    if let Some(bits) = format.bits {
        match eff.caps.sample_formats.as_ref().and_then(|v| v.known()) {
            Some(f) => {
                let max = f.iter().filter_map(format_bits).max().unwrap_or(0);
                if bits > max {
                    return Answer::No(format!("{bits}-bit; the path carries at most {max}-bit"));
                }
            }
            None => missing.push("bit depth"),
        }
    }
    if !missing.is_empty() {
        return Answer::Unknown(format!("not known for this path: {}", missing.join(", ")));
    }
    // Every asked field is known for the path. A `partial` path (some layers unread) still
    // answers yes: the unread layers are named in the path's availability and findings.
    Answer::Yes
}
