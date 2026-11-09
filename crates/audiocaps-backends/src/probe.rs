// SPDX-License-Identifier: LGPL-2.1-or-later
//! The scoped probe. Phase 1 reads only the ALSA procfs PCM list and the environment basics; every
//! other layer is reported as `unknown` with reason `not_implemented` and the phase that adds it.
//! Nothing outside the scope is read.

use audiocaps_model::{
    Availability, CapabilitySet, CorrelationId, CorrelationKey, Direction, DistroFamily, Endpoint,
    EndpointClass, EndpointId, EndpointName, EnvironmentInfo, FieldGroup, LayerCapabilities,
    LayerKind, OsInfo, ProbeScope, Provenance, RedactionPolicy, SCHEMA_VERSION, Sandbox,
    ServerInfo, ServerPresence, Snapshot, SnapshotSource, SoundServer, Source, SystemLayer,
    Transport, UnknownReason, Value,
};
use audiocaps_parse::{os_release, procfs};

use crate::root::SystemRoot;

const PROC_PCM: &str = "/proc/asound/pcm";
const PROC_CARDS: &str = "/proc/asound/cards";

/// Probes `root` within `scope`. The result is redacted with the default policy (an opt-out
/// option arrives with the C API in phase 9).
pub fn probe(root: &SystemRoot, scope: &ProbeScope) -> Snapshot {
    let now = root.now_ms();
    let source = match root {
        SystemRoot::Live => SnapshotSource::Live,
        SystemRoot::Fixture(f) => SnapshotSource::Fixture {
            name: f.name().to_string(),
        },
    };

    let layers = LayerKind::ALL
        .iter()
        .filter(|l| scope.layers.includes(l))
        .map(|&layer| SystemLayer {
            layer,
            availability: system_availability(root, layer),
        })
        .collect();

    let environment = scope
        .layers
        .includes(&LayerKind::Environment)
        .then(|| environment(root, now));

    let mut snapshot = Snapshot {
        schema_version: SCHEMA_VERSION.to_string(),
        generation: 1,
        taken_at_ms: now,
        source,
        scope: scope.clone(),
        redaction: RedactionPolicy::keep_all(),
        layers,
        environment,
        endpoints: alsa_endpoints(root, scope, now),
        findings: vec![],
    };
    snapshot.endpoints.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.redact(&RedactionPolicy::default());
    snapshot
}

fn not_implemented(phase: u8) -> Availability {
    Availability::Unknown {
        reason: UnknownReason::NotImplemented,
        detail: Some(format!("read from phase {phase} of the roadmap")),
    }
}

fn phase_for(layer: LayerKind) -> u8 {
    match layer {
        LayerKind::SinkChain => 6,
        LayerKind::HostHardware | LayerKind::KernelDriver | LayerKind::AlsaUserspace => 3,
        LayerKind::SoundServer => 4,
        LayerKind::Wireless => 5,
        LayerKind::Environment => 5,
    }
}

fn system_availability(root: &SystemRoot, layer: LayerKind) -> Availability {
    match layer {
        LayerKind::AlsaUserspace => match root.read(PROC_PCM) {
            Ok(_) => Availability::Partial,
            Err(e) => io_unknown(&e, PROC_PCM),
        },
        LayerKind::Environment => Availability::Partial,
        other => not_implemented(phase_for(other)),
    }
}

fn io_unknown(e: &std::io::Error, path: &str) -> Availability {
    let reason = match e.kind() {
        std::io::ErrorKind::NotFound => UnknownReason::NotExposed,
        std::io::ErrorKind::PermissionDenied => UnknownReason::NotGranted,
        _ => UnknownReason::ReadError,
    };
    Availability::Unknown {
        reason,
        detail: Some(format!("{path}: {e}")),
    }
}

fn prov(layer: LayerKind, source: Source, at: u64) -> Provenance {
    Provenance {
        layer,
        source,
        read_at_ms: at,
    }
}

fn alsa_endpoints(root: &SystemRoot, scope: &ProbeScope, now: u64) -> Vec<Endpoint> {
    // Nothing to do unless hardware endpoints and at least one direction are in scope.
    if !scope.endpoint_classes.includes(&EndpointClass::Hardware) || scope.directions.is_empty() {
        return vec![];
    }
    let Ok(pcm_text) = root.read_string(PROC_PCM) else {
        return vec![];
    };
    let cards = root
        .read_string(PROC_CARDS)
        .map(|t| procfs::parse_cards(&t))
        .unwrap_or_default();
    let identity = scope.fields.includes(&FieldGroup::Identity);
    let mut out = Vec::new();
    for pcm in procfs::parse_pcm(&pcm_text) {
        for (dir, count, suffix) in [
            (Direction::Playback, pcm.playback, 'p'),
            (Direction::Capture, pcm.capture, 'c'),
        ] {
            if count == 0 {
                continue;
            }
            let id = EndpointId(format!("alsa:card{}/pcm{}{}", pcm.card, pcm.device, suffix));
            if !scope.includes_endpoint(EndpointClass::Hardware, dir, &id) {
                continue;
            }
            let card = cards.iter().find(|c| c.index == pcm.card);
            let mut correlation = vec![];
            if identity {
                if let Some(c) = card {
                    correlation.push(CorrelationKey {
                        id: CorrelationId::AlsaCard {
                            card: c.index,
                            id: c.id.clone(),
                        },
                        confidence: audiocaps_model::Confidence::Measured,
                    });
                }
                correlation.push(CorrelationKey {
                    id: CorrelationId::AlsaPcm {
                        card: pcm.card,
                        device: pcm.device,
                    },
                    confidence: audiocaps_model::Confidence::Measured,
                });
            }
            let layers = LayerKind::ALL
                .iter()
                .filter(|l| scope.layers.includes(l) && **l != LayerKind::Environment)
                .map(|&layer| LayerCapabilities {
                    layer,
                    availability: not_implemented(phase_for(layer)),
                    caps: CapabilitySet::default(),
                })
                .collect();
            out.push(Endpoint {
                id,
                class: EndpointClass::Hardware,
                direction: dir,
                transport: infer_transport(&pcm.name, card.map(|c| c.driver.as_str()), now),
                name: identity.then(|| {
                    Value::measured(
                        EndpointName::Plain(pcm.name.clone()),
                        prov(LayerKind::AlsaUserspace, Source::file(PROC_PCM), now),
                    )
                }),
                correlation,
                sink: None,
                layers,
                effective: vec![],
                paths: vec![],
            });
        }
    }
    out
}

/// Phase 1 heuristic from names; phase 3 replaces it with pin configs and USB descriptors.
fn infer_transport(pcm_name: &str, driver: Option<&str>, now: u64) -> Value<Transport> {
    let from = |what: &str| {
        prov(
            LayerKind::AlsaUserspace,
            Source::derived(what.to_string()),
            now,
        )
    };
    let n = pcm_name.to_ascii_lowercase();
    if driver == Some("USB-Audio") {
        return Value::inferred(Transport::Usb, from("card driver in /proc/asound/cards"));
    }
    let t = if n.contains("hdmi") || n.contains("displayport") || n.split(' ').any(|w| w == "dp") {
        Some(Transport::HdmiDp)
    } else if n.contains("s/pdif")
        || n.contains("spdif")
        || n.contains("iec958")
        || n.contains("digital")
    {
        Some(Transport::Spdif)
    } else if n.contains("analog") {
        Some(Transport::Analog)
    } else {
        None
    };
    match t {
        Some(t) => Value::inferred(t, from("PCM name in /proc/asound/pcm")),
        None => Value::unknown(
            UnknownReason::NotImplemented,
            Some("transport is read from HDA pin configs and USB descriptors in phase 3".into()),
        ),
    }
}

fn environment(root: &SystemRoot, now: u64) -> EnvironmentInfo {
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
        let granted = info
            .lines()
            .filter(|l| {
                ["sockets=", "devices=", "system-talk-name", "filesystems="]
                    .iter()
                    .any(|k| l.starts_with(k))
            })
            .map(|l| l.trim().to_string())
            .collect();
        return Value::measured(Sandbox::Flatpak { granted }, p);
    }
    let snap = match root {
        SystemRoot::Live => std::env::var_os("SNAP_NAME").is_some(),
        SystemRoot::Fixture(f) => f.manifest().sandbox.snap,
    };
    if snap {
        return Value::measured(
            Sandbox::Snap { connected: vec![] },
            prov(
                LayerKind::Environment,
                Source::derived("SNAP_NAME environment variable"),
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

    #[test]
    fn transport_heuristic_is_inferred() {
        assert!(matches!(
            infer_transport("HDMI 0", None, 0),
            Value::Known {
                value: Transport::HdmiDp,
                confidence: audiocaps_model::Confidence::Inferred,
                ..
            }
        ));
        assert!(matches!(
            infer_transport("USB Audio", Some("USB-Audio"), 0),
            Value::Known {
                value: Transport::Usb,
                ..
            }
        ));
        assert!(matches!(
            infer_transport("Something", None, 0),
            Value::Unknown { .. }
        ));
    }

    #[test]
    fn empty_scope_reads_nothing() {
        let s = probe(&SystemRoot::Live, &ProbeScope::nothing());
        assert!(s.endpoints.is_empty());
        assert!(s.layers.is_empty());
        assert!(s.environment.is_none());
    }
}
