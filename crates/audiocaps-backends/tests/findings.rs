// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 7 gate, end to end: on every captured and synthetic system, exactly the expected starter
//! rules fire (and nothing else), and path reports hold together.

use std::collections::BTreeMap;
use std::path::Path;

use audiocaps_backends::{ProbeOptions, SystemRoot, probe_with};
use audiocaps_model::{AccessPath, AppRequest, ProbeScope, Snapshot};

fn snap(name: &str, opts: ProbeOptions) -> Snapshot {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    probe_with(
        &SystemRoot::fixture(dir).unwrap(),
        &ProbeScope::everything(),
        &opts,
    )
}

fn codes(s: &Snapshot) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for f in &s.findings {
        *m.entry(f.code.0.clone()).or_default() += 1;
    }
    m
}

fn expect(name: &str, opts: ProbeOptions, want: &[(&str, usize)]) {
    let got = codes(&snap(name, opts));
    let want: BTreeMap<String, usize> = want.iter().map(|(c, n)| (c.to_string(), *n)).collect();
    assert_eq!(got, want, "{name}");
}

#[test]
fn every_fixture_fires_exactly_what_it_should() {
    let o = ProbeOptions::unbounded;
    // Production rig: 5.1 profile on an 8-channel sink, PipeWire at 48 kHz, receiver behind the TV
    // unidentifiable without CEC, three unused HDMI ports, HDA power saving.
    expect(
        "htpc",
        o(),
        &[
            ("CHANNELS_DOWNMIXED", 1),
            ("POWER_SAVE_ACTIVE", 1),
            ("RATE_CAPPED_BY_SERVER", 1),
            ("SINK_DOWNSTREAM_UNKNOWN", 1),
            ("SINK_NOT_PRESENT", 3),
        ],
    );
    expect(
        "lab-alsa-only",
        o(),
        &[("ALSA_PLUGIN_CONVERSION", 1), ("POWER_SAVE_ACTIVE", 1)],
    );
    expect(
        "lab-pipewire",
        o(),
        &[("POWER_SAVE_ACTIVE", 1), ("RATE_CAPPED_BY_SERVER", 4)],
    );
    expect("lab-pulseaudio", o(), &[("POWER_SAVE_ACTIVE", 1)]);
    expect("lab-usb-uac1", o(), &[("POWER_SAVE_ACTIVE", 1)]);
    expect(
        "workstation",
        o(),
        &[("POWER_SAVE_ACTIVE", 1), ("RATE_CAPPED_BY_SERVER", 2)],
    );
    expect(
        "synthetic-bt-ldac-gap",
        o(),
        &[("BT_BITRATE_LIMITED", 1), ("BT_CODEC_UNAVAILABLE", 1)],
    );
    expect(
        "synthetic-flatpak-minimal",
        o(),
        &[("SANDBOX_PERMISSION_MISSING", 1)],
    );
    expect(
        "synthetic-flatpak-no-audio",
        o(),
        &[("SANDBOX_PERMISSION_MISSING", 1)],
    );
    expect(
        "synthetic-snap-jack",
        o(),
        &[("SANDBOX_PERMISSION_MISSING", 1)],
    );
    expect("synthetic-hdmi-desktop", o(), &[("SINK_NOT_PRESENT", 2)]);
    // Without CEC the receiver can't be seen; with it, the TV is shown to drop TrueHD.
    expect(
        "synthetic-hdmi-earc-cec",
        o(),
        &[("SINK_DOWNSTREAM_UNKNOWN", 1)],
    );
    expect(
        "synthetic-hdmi-earc-cec",
        o().with_cec(),
        &[("SINK_ARC_LIMITED", 1)],
    );
}

#[test]
fn app_request_rules() {
    let ep = Some(audiocaps_model::EndpointId("alsa:card1/pcm9p".into()));
    let app = AppRequest {
        endpoint: ep.clone(),
        channels: Some(2),
        rate_hz: Some(48000),
        latency_target_us: Some(10_000),
        ..AppRequest::default()
    };
    let s = snap("htpc", ProbeOptions::unbounded().with_app(app));
    let c = codes(&s);
    // 2 ch on a 6-channel PipeWire path; PipeWire's 21 ms quantum misses a 10 ms target.
    assert!(c.contains_key("APP_FORMAT_BELOW_SINK"), "{c:?}");
    assert!(c.contains_key("LATENCY_BUDGET_EXCEEDED"), "{c:?}");
    assert_eq!(c["APP_FORMAT_BELOW_SINK"], 1);
    let ok = AppRequest {
        endpoint: ep,
        channels: Some(6),
        rate_hz: Some(48000),
        latency_target_us: Some(50_000),
        ..AppRequest::default()
    };
    let c = codes(&snap("htpc", ProbeOptions::unbounded().with_app(ok)));
    assert!(
        !c.contains_key("APP_FORMAT_BELOW_SINK") && !c.contains_key("LATENCY_BUDGET_EXCEEDED"),
        "{c:?}"
    );
}

#[test]
fn findings_carry_evidence_and_hints() {
    let s = snap("htpc", ProbeOptions::unbounded());
    for f in &s.findings {
        assert!(!f.evidence.is_empty(), "{}", f.code.0);
        assert!(
            f.hint.as_ref().is_some_and(|h| !h.text.contains('{')),
            "{}: {:?}",
            f.code.0,
            f.hint
        );
    }
    let rate = s
        .findings
        .iter()
        .find(|f| f.code.0 == "RATE_CAPPED_BY_SERVER")
        .unwrap();
    assert!(rate.hint.as_ref().unwrap().text.contains("192000"));
}

#[test]
fn htpc_paths() {
    let s = snap("htpc", ProbeOptions::unbounded());
    let e = s
        .endpoints
        .iter()
        .find(|e| e.id.0 == "alsa:card1/pcm9p")
        .unwrap();
    let pw = e
        .paths
        .iter()
        .find(|p| p.access_path == AccessPath::PipeWire)
        .unwrap();
    // The server hop limits channels and rate.
    let b = pw.bottleneck.unwrap() as usize;
    assert_eq!(pw.hops[b].kind, audiocaps_model::HopKind::SoundServer);
    assert_eq!(pw.latency_us.known(), Some(&21_333));
    let direct = e
        .effective
        .iter()
        .find(|x| x.access_path == AccessPath::AlsaDirect)
        .unwrap();
    let pt = direct.caps.passthrough.as_ref().unwrap().known().unwrap();
    assert!(
        pt.iter().any(|c| c.codec == audiocaps_model::Codec::DtsHd),
        "AMD pins carry HBR"
    );
}

#[test]
fn scope_without_findings_computes_none() {
    let scope = ProbeScope::everything();
    let mut scope = scope;
    scope.fields = audiocaps_model::Selection::Only(vec![audiocaps_model::FieldGroup::Identity]);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/htpc");
    let s = probe_with(
        &SystemRoot::fixture(dir).unwrap(),
        &scope,
        &ProbeOptions::unbounded(),
    );
    assert!(s.findings.is_empty());
    assert!(
        s.endpoints
            .iter()
            .all(|e| e.effective.is_empty() && e.paths.is_empty())
    );
}
