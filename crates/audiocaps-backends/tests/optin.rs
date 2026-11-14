// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 8 gate (fixture half): opt-ins run only when the app asks, and do only what it asked.

use std::path::Path;

use audiocaps_backends::{ProbeOptions, SystemRoot, probe_with};
use audiocaps_model::{
    ActiveProbeRequest, BluetoothCodec, Codec, EndpointId, ProbeScope, UnknownReason, Value,
};

fn root(name: &str) -> SystemRoot {
    SystemRoot::fixture(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn defaults_ask_for_nothing() {
    let o = ProbeOptions::default();
    assert!(!o.cec && !o.software_codecs && o.active_probe.is_none());
    let o = ProbeOptions::unbounded();
    assert!(!o.cec && !o.software_codecs && o.active_probe.is_none());
}

#[test]
fn no_opt_in_no_results() {
    for f in [
        "synthetic-software-codecs",
        "htpc",
        "synthetic-hdmi-earc-cec",
    ] {
        let s = probe_with(
            &root(f),
            &ProbeScope::everything(),
            &ProbeOptions::unbounded(),
        );
        assert!(s.software_codecs.is_none(), "{f}");
        assert!(s.endpoints.iter().all(|e| e.active_probe.is_none()), "{f}");
        assert!(
            s.endpoints
                .iter()
                .filter_map(|e| e.sink.as_ref())
                .all(|k| k.cec.is_none()),
            "{f}: CEC without the opt-in"
        );
    }
}

#[test]
fn software_codecs_when_asked() {
    let s = probe_with(
        &root("synthetic-software-codecs"),
        &ProbeScope::everything(),
        &ProbeOptions::unbounded().with_software_codecs(),
    );
    let c = s.software_codecs.as_ref().unwrap();
    assert!(matches!(
        c,
        Value::Known {
            confidence: audiocaps_model::Confidence::Inferred,
            ..
        }
    ));
    let c = c.known().unwrap();
    let names: Vec<_> = c.providers.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ffmpeg",
            "alsa-a52",
            "pipewire-bluez5-aac",
            "pipewire-bluez5-ldac"
        ]
    );
    assert_eq!(c.providers[0].version.as_deref(), Some("61"));
    assert!(c.decoders.contains(&Codec::TrueHd));
    assert!(c.encoders.contains(&Codec::Ac3));
    assert_eq!(c.providers[3].bluetooth, vec![BluetoothCodec::Ldac]);
    // A capture without library directories says so instead of "none".
    let s = probe_with(
        &root("htpc"),
        &ProbeScope::everything(),
        &ProbeOptions::unbounded().with_software_codecs(),
    );
    assert!(matches!(
        s.software_codecs,
        Some(Value::Unknown {
            reason: UnknownReason::NotExposed,
            ..
        })
    ));
}

#[test]
fn active_probe_only_on_the_named_endpoint_and_never_on_a_capture() {
    let req = ActiveProbeRequest {
        endpoint: EndpointId("alsa:card1/pcm9p".into()),
        codecs: vec![Codec::Lpcm, Codec::Ac3],
        channels: None,
        rate_hz: None,
    };
    let s = probe_with(
        &root("htpc"),
        &ProbeScope::everything(),
        &ProbeOptions::unbounded().with_active_probe(req),
    );
    for e in &s.endpoints {
        if e.id.0 == "alsa:card1/pcm9p" {
            assert!(matches!(
                e.active_probe,
                Some(Value::Unknown {
                    reason: UnknownReason::NotExposed,
                    ..
                })
            ));
        } else {
            assert!(e.active_probe.is_none(), "{}", e.id.0);
        }
    }
}
