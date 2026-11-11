// SPDX-License-Identifier: LGPL-2.1-or-later
//! Phase 5 gate: codec gaps and permission gaps are reported correctly (synthetic fixtures with a
//! known answer).

use std::path::Path;

use audiocaps_backends::{SystemRoot, probe};
use audiocaps_model::{
    Availability, BluetoothCodec, CorrelationId, EndpointClass, LayerDetail, LayerKind, ProbeScope,
    Sandbox, SoundServer, UnknownReason,
};

fn snap(name: &str) -> audiocaps_model::Snapshot {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    probe(
        &SystemRoot::fixture(dir).expect("fixture"),
        &ProbeScope::everything(),
    )
}

fn layer(e: &audiocaps_model::Endpoint, k: LayerKind) -> &audiocaps_model::LayerCapabilities {
    e.layers.iter().find(|l| l.layer == k).expect("layer")
}

#[test]
fn bluetooth_ldac_advertised_but_missing_on_host() {
    let s = snap("synthetic-bt-ldac-gap");
    let e = s
        .endpoints
        .iter()
        .find(|e| e.class == EndpointClass::Bluetooth)
        .expect("Bluetooth endpoint");
    assert!(e.id.0.starts_with("node:bluez_output.bt-"), "{}", e.id.0);
    // The address appears only hashed.
    let key = e
        .correlation
        .iter()
        .find_map(|k| match &k.id {
            CorrelationId::BluezDevice { address } => Some(address),
            _ => None,
        })
        .expect("BlueZ key");
    let json = serde_json::to_string(key).unwrap();
    assert!(
        json.contains("sha256:") && !json.contains("02:5A"),
        "{json}"
    );

    let w = layer(e, LayerKind::Wireless);
    assert_eq!(w.availability, Availability::Read);
    let codecs = w.caps.bluetooth_codecs.as_ref().unwrap().known().unwrap();
    let get = |c: BluetoothCodec| codecs.iter().find(|x| x.codec == c).unwrap();
    let ldac = get(BluetoothCodec::Ldac);
    assert!(ldac.advertised_by_device && !ldac.available_on_host && !ldac.negotiated);
    let aac = get(BluetoothCodec::Aac);
    assert!(aac.advertised_by_device && aac.available_on_host && !aac.negotiated);
    let sbc = get(BluetoothCodec::Sbc);
    assert!(sbc.advertised_by_device && sbc.available_on_host && sbc.negotiated);
    let Some(LayerDetail::Wireless(d)) = &w.detail else {
        panic!("wireless detail")
    };
    assert_eq!(d.connected.known(), Some(&true));
    assert_eq!(d.profiles, vec!["a2dp_sink", "hfp_hf"]);
    let t = d.transport.as_ref().unwrap().known().unwrap();
    assert_eq!(
        (t.profile.as_str(), &t.codec),
        ("a2dp_sink", &BluetoothCodec::Sbc)
    );
    // The alias is user-chosen: masked on export.
    let name = serde_json::to_string(&e.name).unwrap();
    assert!(
        !name.contains("Headphones") && name.contains("personal"),
        "{name}"
    );
}

fn sandbox(s: &audiocaps_model::Snapshot) -> &Sandbox {
    s.environment.as_ref().unwrap().sandbox.known().unwrap()
}

#[test]
fn flatpak_minimal_lacks_only_optional_permissions() {
    let s = snap("synthetic-flatpak-minimal");
    let Sandbox::Flatpak { granted, missing } = sandbox(&s) else {
        panic!("flatpak")
    };
    assert!(granted.iter().any(|g| g.contains("pulseaudio")));
    let perms: Vec<_> = missing.iter().map(|g| g.permission.as_str()).collect();
    assert_eq!(
        perms,
        [
            "--filesystem=xdg-run/pipewire-0",
            "--system-talk-name=org.bluez"
        ]
    );
    assert!(missing.iter().all(|g| !g.required));
    assert!(missing[1].layers.contains(&LayerKind::Wireless));
    assert!(missing[1].fix.contains("org.example.Player"));
}

#[test]
fn flatpak_without_audio_names_the_required_fix() {
    let s = snap("synthetic-flatpak-no-audio");
    let Sandbox::Flatpak { missing, .. } = sandbox(&s) else {
        panic!("flatpak")
    };
    let pulse = missing
        .iter()
        .find(|g| g.permission == "--socket=pulseaudio")
        .expect("pulseaudio gap");
    assert!(pulse.required);
    assert!(
        pulse
            .fix
            .starts_with("flatpak override --user --socket=pulseaudio org.example.Viewer")
    );
    // The layer the gap hides says so, with the fix.
    let ss = s
        .layers
        .iter()
        .find(|l| l.layer == LayerKind::SoundServer)
        .unwrap();
    let Availability::Unknown { detail, .. } = &ss.availability else {
        panic!("sound server should be unknown")
    };
    assert!(
        detail.as_deref().unwrap().contains("--socket=pulseaudio"),
        "{detail:?}"
    );
}

#[test]
fn snap_without_hardware_observe_and_jack_state() {
    let s = snap("synthetic-snap-jack");
    let Sandbox::Snap { missing, .. } = sandbox(&s) else {
        panic!("snap")
    };
    let hw = missing
        .iter()
        .find(|g| g.permission == "hardware-observe")
        .expect("hardware-observe gap");
    assert!(hw.layers.contains(&LayerKind::HostHardware));
    assert!(hw.fix.contains("snap connect") && hw.fix.ends_with(":hardware-observe"));
    let jack = s
        .servers
        .iter()
        .find(|x| x.server == SoundServer::Jack)
        .expect("JACK state");
    assert_eq!(jack.availability, Availability::Read);
    let c = jack.clock.as_ref().unwrap().known().unwrap();
    assert_eq!((c.rate_hz, c.quantum), (Some(48000), Some(256)));
    let ports = jack.ports.as_ref().unwrap().known().unwrap();
    assert_eq!(ports.len(), 2);
    assert!(
        ports
            .iter()
            .any(|p| p.direction == "playback" && p.channels == 2)
    );
}

#[test]
fn jack_absent_is_unknown_not_unsupported() {
    let s = snap("synthetic-bt-ldac-gap");
    let jack = s
        .servers
        .iter()
        .find(|x| x.server == SoundServer::Jack)
        .unwrap();
    assert!(matches!(
        jack.availability,
        Availability::Unknown {
            reason: UnknownReason::ServiceNotRunning,
            ..
        }
    ));
}
