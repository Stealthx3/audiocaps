// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sandbox permissions: what a Flatpak or Snap app was granted, and which permissions it lacks,
//! with the layers each one hides and the exact fix.
//!
//! - Flatpak: read from `/.flatpak-info` (`[Context]` sockets, devices, filesystems and the
//!   `[System Bus Policy]`).
//! - Snap: a library can't run `snapctl`, so connected plugs are inferred from what the snap can
//!   reach (the PulseAudio and PipeWire sockets, `/dev/snd`, `/sys/class/sound`). They are
//!   reported with inferred confidence. The `bluez` plug is judged from the BlueZ backend's result.

use audiocaps_model::{LayerKind, PermissionGap};

use crate::root::SystemRoot;

/// Granted permission lines and missing permissions of a Flatpak app.
pub(crate) fn flatpak(info: &str) -> (Vec<String>, Vec<PermissionGap>) {
    let mut section = "";
    let mut app = "<app-id>".to_string();
    let mut sockets: Vec<String> = vec![];
    let mut filesystems: Vec<String> = vec![];
    let mut system_bus: Vec<String> = vec![];
    let mut granted = vec![];
    for line in info.lines() {
        let t = line.trim();
        if let Some(s) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = match s {
                "Application" => "app",
                "Context" => "context",
                "System Bus Policy" => "system-bus",
                _ => "",
            };
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        let list = || -> Vec<String> {
            v.split(';')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        };
        match (section, k.trim()) {
            ("app", "name") => app = v.trim().to_string(),
            ("context", "sockets") => {
                sockets = list();
                granted.push(t.to_string());
            }
            ("context", "devices") => granted.push(t.to_string()),
            ("context", "filesystems") => {
                filesystems = list();
                granted.push(t.to_string());
            }
            ("system-bus", name) => {
                system_bus.push(name.to_string());
                granted.push(format!("system-talk-name={name}"));
            }
            _ => {}
        }
    }
    let has = |l: &[String], x: &str| l.iter().any(|y| y == x || y.starts_with(&format!("{x}:")));
    let fix =
        |arg: &str| format!("flatpak override --user {arg} {app} (packagers: finish-args {arg})");
    let mut missing = vec![];
    if !has(&sockets, "pulseaudio") {
        missing.push(PermissionGap {
            permission: "--socket=pulseaudio".into(),
            layers: vec![LayerKind::SoundServer, LayerKind::AlsaUserspace],
            fix: fix("--socket=pulseaudio"),
            required: true,
        });
    }
    if !filesystems
        .iter()
        .any(|f| f.starts_with("xdg-run/pipewire-0"))
    {
        missing.push(PermissionGap {
            permission: "--filesystem=xdg-run/pipewire-0".into(),
            layers: vec![LayerKind::SoundServer],
            fix: fix("--filesystem=xdg-run/pipewire-0"),
            required: false,
        });
    }
    if !system_bus.iter().any(|n| n == "org.bluez") {
        missing.push(PermissionGap {
            permission: "--system-talk-name=org.bluez".into(),
            layers: vec![LayerKind::Wireless],
            fix: fix("--system-talk-name=org.bluez"),
            required: false,
        });
    }
    (granted, missing)
}

/// Inferred connected plugs and missing plugs of a Snap app.
pub(crate) fn snap(root: &SystemRoot) -> (Vec<String>, Vec<PermissionGap>) {
    let name = std::env::var("SNAP_NAME").unwrap_or_else(|_| "<snap>".into());
    let sock = |p: &str| crate::server::presence::find_socket(root, p).is_some();
    let fix = |plug: &str| format!("snap connect {name}:{plug}");
    let mut connected = vec![];
    let mut missing = vec![];
    let mut check = |ok: Option<bool>, plug: &str, layers: Vec<LayerKind>, required: bool| match ok
    {
        Some(true) => connected.push(plug.to_string()),
        Some(false) => missing.push(PermissionGap {
            permission: plug.into(),
            layers,
            fix: fix(plug),
            required,
        }),
        None => {}
    };
    let live = matches!(root, SystemRoot::Live);
    check(
        live.then(|| sock("pulse/native")),
        "audio-playback",
        vec![LayerKind::SoundServer],
        true,
    );
    check(
        live.then(|| sock("pipewire-0")),
        "pipewire",
        vec![LayerKind::SoundServer],
        false,
    );
    // /dev/snd is visible in every snap (the device cgroup blocks it); the alsa plug's AppArmor
    // rules are what open /proc/asound.
    check(
        live.then(|| root.read("/proc/asound/cards").is_ok()),
        "alsa",
        vec![LayerKind::AlsaUserspace],
        false,
    );
    // /sys/class/sound lists without any plug; hardware-observe is what opens device attributes
    // such as the DRM connectors' EDID (the sink chain). Unknown when there is no connector.
    let edid = root.read_dir("/sys/class/drm").ok().and_then(|es| {
        es.iter()
            .filter(|e| e.name.starts_with("card") && e.name.contains('-'))
            .map(|e| format!("/sys/class/drm/{}/edid", e.name))
            .find(|p| root.exists(p))
    });
    check(
        live.then_some(edid)
            .flatten()
            .map(|p| root.read(&p).is_ok()),
        "hardware-observe",
        vec![LayerKind::HostHardware, LayerKind::SinkChain],
        false,
    );
    // Driver parameters (power saving, model quirks) under /sys/module.
    check(
        live.then(|| root.read_dir("/sys/module").is_ok()),
        "kernel-module-observe",
        vec![LayerKind::KernelDriver],
        false,
    );
    (connected, missing)
}

/// The gap for the `bluez` plug, when the BlueZ backend was refused inside a snap.
pub(crate) fn snap_bluez_gap() -> PermissionGap {
    snap_gap("bluez", vec![LayerKind::Wireless], false)
}

/// The gap for a snap plug a backend was refused for.
pub(crate) fn snap_gap(plug: &str, layers: Vec<LayerKind>, required: bool) -> PermissionGap {
    let name = std::env::var("SNAP_NAME").unwrap_or_else(|_| "<snap>".into());
    PermissionGap {
        permission: plug.into(),
        layers,
        fix: format!("snap connect {name}:{plug}"),
        required,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_minimal_and_full() {
        let minimal = "[Application]\nname=org.example.Player\n\n[Context]\nshared=network;ipc;\nsockets=x11;wayland;pulseaudio;\n";
        let (g, m) = flatpak(minimal);
        assert!(g.iter().any(|x| x.contains("pulseaudio")));
        let perms: Vec<_> = m.iter().map(|x| x.permission.as_str()).collect();
        assert_eq!(
            perms,
            [
                "--filesystem=xdg-run/pipewire-0",
                "--system-talk-name=org.bluez"
            ]
        );
        assert!(m.iter().all(|x| !x.required));
        assert!(m[0].fix.contains("org.example.Player"));
        let full = "[Application]\nname=a.b\n[Context]\nsockets=pulseaudio;\nfilesystems=xdg-run/pipewire-0:ro;\n[System Bus Policy]\norg.bluez=talk\n";
        assert!(flatpak(full).1.is_empty());
        let none = "[Application]\nname=a.b\n[Context]\nsockets=wayland;\n";
        let m = flatpak(none).1;
        assert!(m[0].required && m[0].permission == "--socket=pulseaudio");
        assert!(m[0].layers.contains(&LayerKind::AlsaUserspace));
    }
}
