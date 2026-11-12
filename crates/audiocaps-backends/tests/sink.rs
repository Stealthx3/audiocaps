// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 6: sink records, correlation, CEC (opt-in) and quirk data, on a hand-written fixture
//! modelled on the production rig (PC -> Hisense TV -> Denon AVR-S660H on eARC).

use std::path::Path;

use audiocaps_backends::{ProbeOptions, SystemRoot, probe_with};
use audiocaps_model::{
    Codec, CodecFlag, CorrelationId, LayerKind, ProbeScope, QuirkEffect, Snapshot, UnknownReason,
    Value,
};

fn snap(opts: ProbeOptions) -> Snapshot {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/synthetic-hdmi-earc-cec");
    probe_with(
        &SystemRoot::fixture(dir).unwrap(),
        &ProbeScope::everything(),
        &opts,
    )
}

#[test]
fn tv_identified_and_correlated() {
    let s = snap(ProbeOptions::unbounded());
    let e = s
        .endpoints
        .iter()
        .find(|e| e.id.0 == "alsa:card0/pcm3p")
        .unwrap();
    let sink = e.sink.as_ref().expect("sink record");
    assert_eq!(
        sink.identity.manufacturer_id.known().map(String::as_str),
        Some("HEC")
    );
    assert_eq!(
        sink.identity.manufacturer_name.known().map(String::as_str),
        Some("Hisense")
    );
    assert_eq!(sink.identity.product_code.known(), Some(&0xa030));
    assert_eq!(
        sink.identity.model_name.known().map(String::as_str),
        Some("HISENSE")
    );
    assert_eq!(
        sink.identity.sink_key.known().map(String::as_str),
        Some("HEC-a030")
    );
    // Raw ELD SADs from the ALSA control.
    let sads = sink.audio_descriptors.known().unwrap();
    assert_eq!(sads.len(), 5);
    assert!(
        !sads.iter().any(|c| c.codec == Codec::TrueHd),
        "the TV forwards no TrueHD"
    );
    let link = sink.link.as_ref().unwrap().known().unwrap();
    assert_eq!(link.physical_address, Some(0x2000));
    assert!(e.correlation.iter().any(|k| k.id
        == CorrelationId::DrmConnector {
            name: "card0-HDMI-A-1".into()
        }));
    // Without the opt-in, nothing behind the TV is claimed.
    assert!(matches!(
        sink.downstream,
        Value::Unknown {
            reason: UnknownReason::NotRequested,
            ..
        }
    ));
    assert!(sink.cec.is_none());
    let layer = e
        .layers
        .iter()
        .find(|l| l.layer == LayerKind::SinkChain)
        .unwrap();
    let pt = layer.caps.passthrough.as_ref().unwrap().known().unwrap();
    assert!(pt.iter().any(|c| c.codec == Codec::DtsHd));
    assert_eq!(
        layer.caps.channels.as_ref().unwrap().known().unwrap().max,
        8
    );
}

#[test]
fn receiver_behind_the_tv_matches_its_spec_sheet() {
    let s = snap(ProbeOptions::unbounded().with_cec());
    let e = s
        .endpoints
        .iter()
        .find(|e| e.id.0 == "alsa:card0/pcm3p")
        .unwrap();
    let sink = e.sink.as_ref().unwrap();
    let down = sink.downstream.known().expect("downstream over CEC");
    assert_eq!(down.len(), 1);
    assert_eq!(
        down[0].model_name.known().map(String::as_str),
        Some("AVR-S660H")
    );
    assert_eq!(
        down[0].manufacturer_name.known().map(String::as_str),
        Some("Denon")
    );
    let bus = sink.cec.as_ref().unwrap().known().unwrap();
    let avr = bus.devices.iter().find(|d| d.logical_address == 5).unwrap();
    // Spec sheet: DD, DD+, TrueHD, DTS, DTS 96/24, DTS-HD MA/HR; PCM 2-8 ch, 32-192 kHz,
    // 16/20/24-bit; no Atmos, no DTS:X.
    let get = |c: Codec| {
        avr.audio_descriptors
            .iter()
            .find(|x| x.codec == c)
            .unwrap_or_else(|| panic!("{c:?}"))
    };
    for c in [
        Codec::Ac3,
        Codec::Eac3,
        Codec::TrueHd,
        Codec::Dts,
        Codec::DtsHd,
    ] {
        get(c);
    }
    assert!(get(Codec::Dts).rates_hz.contains(&96000));
    let pcm = get(Codec::Lpcm);
    assert_eq!(pcm.max_channels, Some(8));
    assert_eq!(
        (pcm.rates_hz.first(), pcm.rates_hz.last()),
        (Some(&32000), Some(&192000))
    );
    assert_eq!(pcm.bit_depths, vec![16, 20, 24]);
    assert!(
        avr.audio_descriptors
            .iter()
            .all(|c| !c.flags.contains(&CodecFlag::Atmos) && !c.flags.contains(&CodecFlag::DtsX))
    );
    // Quirk data adds what no EDID or CEC field carries.
    let q = sink
        .quirks
        .iter()
        .find(|q| q.id == "denon-avr-s660h")
        .expect("Denon quirk");
    assert_eq!(q.origin, "bundled");
    assert!(q.effects.contains(&QuirkEffect::Feature {
        name: "dolby_pro_logic_ii".into()
    }));
    assert!(
        q.effects
            .contains(&QuirkEffect::ProcessingChannels { max: 6 })
    );
}

#[test]
fn app_quirk_dir_overrides_bundled() {
    let dir = std::env::temp_dir().join(format!("audiocaps-quirks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.json"),
        r#"{"format":1,"entries":[{"id":"denon-avr-s660h","description":"app override",
           "match":{"cec_vendor":"0005cd","model_names":["AVR-S660H"]},"effects":[]}]}"#,
    )
    .unwrap();
    let s = snap(ProbeOptions::unbounded().with_cec().with_quirk_dir(&dir));
    std::fs::remove_dir_all(&dir).ok();
    let sink = s.endpoints.iter().find_map(|e| e.sink.as_ref()).unwrap();
    let q = &sink.quirks[0];
    assert_eq!(q.description, "app override");
    assert!(q.origin.ends_with("app.json"));
}

/// Gate 6.5 on the production rig capture (HTPC, PC -> Hisense TV -> Denon AVR-S660H on eARC).
#[test]
fn production_rig_identified() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/htpc");
    let s = probe_with(
        &SystemRoot::fixture(dir).unwrap(),
        &ProbeScope::everything(),
        &ProbeOptions::unbounded().with_cec(),
    );
    let e = s
        .endpoints
        .iter()
        .find(|e| e.sink.is_some())
        .expect("an HDMI sink");
    assert_eq!(e.id.0, "alsa:card1/pcm9p");
    let sink = e.sink.as_ref().unwrap();
    // The TV: identified from the ELD control (bound to PCM 9) and its EDID on the same GPU.
    assert_eq!(
        sink.identity.manufacturer_name.known().map(String::as_str),
        Some("Hisense")
    );
    assert_eq!(sink.identity.product_code.known(), Some(&0xa030));
    assert!(matches!(
        &sink.audio_descriptors,
        Value::Known { provenance, .. } if matches!(&provenance.source, audiocaps_model::Source::Command { name } if name == "alsa")
    ));
    assert!(e.correlation.iter().any(
        |k| matches!(&k.id, CorrelationId::DrmConnector { name } if name == "card1-HDMI-A-2")
    ));
    assert!(
        e.correlation
            .iter()
            .any(|k| matches!(&k.id, CorrelationId::PipewireNode { .. }))
    );
    // No CEC adapter on this GPU: what sits behind the TV is unknown, not absent.
    assert!(matches!(
        sink.downstream,
        Value::Unknown {
            reason: UnknownReason::NotExposed,
            ..
        }
    ));
    // What the TV forwards over eARC vs the receiver's spec sheet: everything but TrueHD.
    let sads = sink.audio_descriptors.known().unwrap();
    let has = |c: Codec| sads.iter().any(|x| x.codec == c);
    for c in [
        Codec::Ac3,
        Codec::Eac3,
        Codec::Dts,
        Codec::DtsHd,
        Codec::Lpcm,
    ] {
        assert!(has(c.clone()), "{c:?}");
    }
    assert!(!has(Codec::TrueHd), "the TV does not forward TrueHD");
    let pcm = sads.iter().find(|c| c.codec == Codec::Lpcm).unwrap();
    assert_eq!(pcm.max_channels, Some(8));
    assert_eq!(pcm.bit_depths, vec![16, 20, 24]);
    assert!(
        sads.iter()
            .find(|c| c.codec == Codec::Dts)
            .unwrap()
            .rates_hz
            .contains(&96000)
    );
    assert!(sads.iter().all(|c| !c.flags.contains(&CodecFlag::Atmos)));
}
