// SPDX-License-Identifier: LGPL-2.1-or-later
//! The environment layer: os-release, kernel, sandbox and sound-server presence.
//!
//! Server presence is read from captured command output only; live server probing (native client
//! APIs, never auto-spawning a server) arrives in phase 4.

use audiocaps_model::{
    DistroFamily, EnvironmentInfo, LayerKind, OsInfo, Provenance, Sandbox, ServerInfo,
    ServerPresence, SoundServer, Source, UnknownReason, Value,
};
use audiocaps_parse::os_release;

use crate::root::SystemRoot;

fn prov(layer: LayerKind, source: Source, at: u64) -> Provenance {
    Provenance {
        layer,
        source,
        read_at_ms: at,
    }
}

/// Reads the environment layer.
pub(crate) fn environment(root: &SystemRoot, now: u64) -> EnvironmentInfo {
    let os = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|p| root.read_string(p).ok().map(|t| (p, t)))
        .map(|(p, text)| {
            let kv = os_release::parse(&text);
            let id = os_release::get(&kv, "ID").unwrap_or("linux").to_string();
            let like = os_release::get(&kv, "ID_LIKE").unwrap_or("");
            let family = family_of(&id, like);
            Value::measured(
                OsInfo {
                    id,
                    version_id: os_release::get(&kv, "VERSION_ID").map(String::from),
                    family,
                },
                prov(LayerKind::Environment, Source::file(*p), now),
            )
        })
        .unwrap_or_else(|| Value::unknown(UnknownReason::NotExposed, Some("no os-release".into())));

    let kernel = match root.read_string("/proc/sys/kernel/osrelease") {
        Ok(k) => Value::measured(
            k.trim().to_string(),
            prov(
                LayerKind::Environment,
                Source::file("/proc/sys/kernel/osrelease"),
                now,
            ),
        ),
        Err(e) => Value::unknown(UnknownReason::ReadError, Some(e.to_string())),
    };

    let sandbox = sandbox(root, now);
    EnvironmentInfo {
        os,
        kernel,
        sandbox,
        servers: servers(root, now),
    }
}

fn family_of(id: &str, like: &str) -> DistroFamily {
    let all: Vec<&str> = std::iter::once(id).chain(like.split_whitespace()).collect();
    let has = |x: &str| all.contains(&x);
    if has("debian") || has("ubuntu") {
        DistroFamily::Debian
    } else if has("fedora") || has("rhel") || has("centos") {
        DistroFamily::Fedora
    } else if has("arch") {
        DistroFamily::Arch
    } else if all.iter().any(|x| x.contains("suse")) {
        DistroFamily::Suse
    } else {
        DistroFamily::Other
    }
}

fn sandbox(root: &SystemRoot, now: u64) -> Value<Sandbox> {
    let p = prov(LayerKind::Environment, Source::file("/.flatpak-info"), now);
    if let Ok(info) = root.read_string("/.flatpak-info") {
        let (granted, missing) = crate::sandbox::flatpak(&info);
        return Value::measured(Sandbox::Flatpak { granted, missing }, p);
    }
    let snap = match root {
        SystemRoot::Live => std::env::var_os("SNAP_NAME").is_some(),
        SystemRoot::Fixture(f) => f.manifest().sandbox.snap,
    };
    if snap {
        let (connected, missing) = crate::sandbox::snap(root);
        return Value::inferred(
            Sandbox::Snap { connected, missing },
            prov(
                LayerKind::Environment,
                Source::derived("SNAP_NAME and the sockets, devices and files the snap can reach"),
                now,
            ),
        );
    }
    Value::measured(Sandbox::None, p)
}

/// Phase 1 reads server presence only from captured command output. Live server probing (native
/// client APIs, never auto-spawning a server) arrives in phase 4.
fn servers(root: &SystemRoot, now: u64) -> Vec<ServerPresence> {
    if matches!(root, SystemRoot::Live) {
        return vec![];
    }
    let mut out = vec![];
    let src = |name: &str| prov(LayerKind::SoundServer, Source::command(name), now);
    if let Some((cmd, out_bytes)) = root.captured_command("pw-dump") {
        let info = match (cmd.exit_code, out_bytes) {
            (Some(0), Some(b)) => pipewire_core(&b)
                .map(|(name, version)| {
                    Value::measured(ServerInfo { name, version }, src("pw-dump"))
                })
                .unwrap_or_else(|| Value::unknown(UnknownReason::ParseError, None)),
            _ => Value::unknown(skip_reason(cmd.skipped.as_deref()), cmd.skipped.clone()),
        };
        out.push(ServerPresence {
            server: SoundServer::PipeWire,
            info,
        });
    }
    if let Some((cmd, out_bytes)) = root.captured_command("pactl-info") {
        let info = match (cmd.exit_code, out_bytes) {
            (Some(0), Some(b)) => pulse_info(&b)
                .map(|(name, version)| {
                    Value::measured(ServerInfo { name, version }, src("pactl-info"))
                })
                .unwrap_or_else(|| Value::unknown(UnknownReason::ParseError, None)),
            _ => Value::unknown(skip_reason(cmd.skipped.as_deref()), cmd.skipped.clone()),
        };
        out.push(ServerPresence {
            server: SoundServer::PulseAudio,
            info,
        });
    }
    out
}

/// Maps a capture skip note to an unknown reason.
fn skip_reason(skipped: Option<&str>) -> UnknownReason {
    match skipped {
        Some(s) if s.contains("not installed") => UnknownReason::LibraryMissing,
        Some(s) if s.contains("timed out") => UnknownReason::Timeout,
        _ => UnknownReason::ServiceNotRunning,
    }
}

fn pipewire_core(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let core = v
        .as_array()?
        .iter()
        .find(|o| o["type"] == "PipeWire:Interface:Core")?;
    let info = &core["info"];
    Some((
        info["name"].as_str().unwrap_or("pipewire").to_string(),
        info["version"].as_str().map(String::from),
    ))
}

fn pulse_info(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some((
        v["server_name"].as_str()?.to_string(),
        v["server_version"].as_str().map(String::from),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(family_of("ubuntu", "debian"), DistroFamily::Debian);
        assert_eq!(
            family_of("rocky", "rhel centos fedora"),
            DistroFamily::Fedora
        );
        assert_eq!(family_of("opensuse-tumbleweed", "suse"), DistroFamily::Suse);
        assert_eq!(family_of("nixos", ""), DistroFamily::Other);
    }
}
