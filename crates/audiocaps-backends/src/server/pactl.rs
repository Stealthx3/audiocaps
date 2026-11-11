// SPDX-License-Identifier: LGPL-2.1-or-later
//! `pactl -f json info` and `pactl -f json list` → [`ServerDump`]. Used for fixtures, and by the
//! gate to compare with the live PulseAudio backend. Monitor sources are skipped.

use serde_json::Value;

use super::{
    Device, Format, FormatRange, Node, NodeDir, Profile, Route, ServerDump, ServerKind, kept,
};
use crate::server::pwdump::as_u32;

/// Parses `pactl -f json info` and `pactl -f json list`.
pub fn parse(info: &Value, list: Option<&Value>) -> Option<ServerDump> {
    let mut d = ServerDump {
        kind: ServerKind::PulseAudio,
        name: info["server_name"].as_str()?.to_string(),
        version: info["server_version"].as_str().map(String::from),
        clock: None,
        default_sink: info["default_sink_name"].as_str().map(String::from),
        default_source: info["default_source_name"].as_str().map(String::from),
        policy: Default::default(),
        session_manager: None,
        devices: vec![],
        nodes: vec![],
    };
    let Some(list) = list else {
        return Some(d);
    };
    // `pactl -f json` gives sinks and sources no card index; the card's owner module links them.
    let card_of_module: Vec<(u32, u32)> = list["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((as_u32(&c["owner_module"])?, as_u32(&c["index"])?)))
        .collect();
    // pipewire-pulse states the card in the `device.id` property instead.
    let card_of = |s: &Value| {
        as_u32(&s["card"])
            .or_else(|| as_u32(&s["properties"]["device.id"]))
            .or_else(|| {
                let m = as_u32(&s["owner_module"])?;
                card_of_module
                    .iter()
                    .find(|(om, _)| *om == m)
                    .map(|(_, c)| *c)
            })
    };
    let mut active_ports: Vec<(u32, String)> = Vec::new();
    for (key, dir) in [("sinks", NodeDir::Sink), ("sources", NodeDir::Source)] {
        for s in list[key].as_array().into_iter().flatten() {
            let p = &s["properties"];
            if p["device.class"].as_str() == Some("monitor") {
                continue;
            }
            let Some(mut n) = node(s, dir) else { continue };
            n.device_id = card_of(s);
            if let (Some(card), Some(port)) = (n.device_id, s["active_port"].as_str()) {
                active_ports.push((card, port.to_string()));
            }
            d.nodes.push(n);
        }
    }
    for c in list["cards"].as_array().into_iter().flatten() {
        let Some(id) = as_u32(&c["index"]) else {
            continue;
        };
        let p = &c["properties"];
        let mut dev = Device {
            id,
            name: c["name"].as_str().unwrap_or("").into(),
            api: p["device.api"]
                .as_str()
                .map(String::from)
                .or_else(|| p["alsa.card"].is_string().then(|| "alsa".to_string())),
            alsa_card: as_u32(&p["alsa.card"]),
            active_profile: c["active_profile"].as_str().map(String::from),
            ..Device::default()
        };
        if let Some(m) = c["profiles"].as_object() {
            for (name, pr) in m {
                dev.profiles.push(Profile {
                    name: name.clone(),
                    description: pr["description"].as_str().unwrap_or("").into(),
                    available: pr["available"].as_bool(),
                });
            }
        }
        if let Some(m) = c["ports"].as_object() {
            for (name, pt) in m {
                // Card ports don't say their direction; sinks and sources list theirs.
                let direction = port_direction(list, id, name);
                dev.routes.push(Route {
                    name: name.clone(),
                    description: pt["description"].as_str().unwrap_or("").into(),
                    direction,
                    available: port_available(pt["availability"].as_str()),
                    active: active_ports.iter().any(|(c, p)| *c == id && p == name),
                    devices: vec![],
                });
            }
        }
        if let Some(m) = p.as_object() {
            for (k, v) in m {
                if kept(k) {
                    dev.props
                        .insert(k.clone(), v.as_str().unwrap_or("").to_string());
                }
            }
        }
        d.devices.push(dev);
    }
    d.nodes
        .sort_by_key(|n| (n.dir == Some(NodeDir::Source), n.id));
    d.devices.sort_by_key(|n| n.id);
    Some(d)
}

fn port_direction(list: &Value, card: u32, port: &str) -> String {
    let module = list["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| as_u32(&c["index"]) == Some(card))
        .and_then(|c| as_u32(&c["owner_module"]));
    for (key, dir) in [("sinks", "output"), ("sources", "input")] {
        for s in list[key].as_array().into_iter().flatten() {
            let same_card = as_u32(&s["card"]) == Some(card)
                || as_u32(&s["properties"]["device.id"]) == Some(card)
                || (module.is_some() && as_u32(&s["owner_module"]) == module);
            if same_card
                && s["ports"]
                    .as_array()
                    .is_some_and(|ps| ps.iter().any(|x| x["name"].as_str() == Some(port)))
            {
                return dir.into();
            }
        }
    }
    if port.contains("output") {
        "output".into()
    } else if port.contains("input") {
        "input".into()
    } else {
        String::new()
    }
}

fn port_available(s: Option<&str>) -> Option<bool> {
    match s {
        Some("available") => Some(true),
        Some("not available") => Some(false),
        _ => None,
    }
}

fn node(s: &Value, dir: NodeDir) -> Option<Node> {
    let p = &s["properties"];
    let mut n = Node {
        id: as_u32(&s["index"])?,
        name: s["name"].as_str()?.to_string(),
        description: s["description"].as_str().unwrap_or("").into(),
        media_class: match dir {
            NodeDir::Sink => "Audio/Sink".into(),
            NodeDir::Source => "Audio/Source".into(),
        },
        dir: Some(dir),
        device_id: as_u32(&s["card"]),
        api: p["device.api"].as_str().map(String::from),
        factory: s["driver"].as_str().map(String::from),
        alsa_card: as_u32(&p["alsa.card"]),
        alsa_device: as_u32(&p["alsa.device"]),
        state: s["state"].as_str().map(|x| x.to_ascii_lowercase()),
        profile: p["device.profile.name"].as_str().map(String::from),
        ..Node::default()
    };
    if let Some(spec) = s["sample_specification"].as_str() {
        n.format = sample_spec(spec);
        if let Some(f) = &n.format {
            n.enum_formats.push(FormatRange {
                subtype: "raw".into(),
                formats: vec![f.format.clone()],
                rates: vec![f.rate],
                rate_range: None,
                channels: Some((f.channels, f.channels)),
                positions: s["channel_map"]
                    .as_str()
                    .map(|m| m.split(',').map(str::to_string).collect())
                    .unwrap_or_default(),
            });
        }
    }
    if let Some(f) = s["formats"].as_array() {
        n.iec958_codecs = Some(
            f.iter()
                .filter_map(|x| x.as_str())
                .map(|x| x.split(',').next().unwrap_or("").trim().to_string())
                .collect(),
        );
    }
    if let Some(m) = s["volume"].as_object() {
        let v: Vec<u32> = m
            .values()
            .filter_map(|c| c["value_percent"].as_str())
            .filter_map(|p| p.trim_end_matches('%').trim().parse().ok())
            .collect();
        if !v.is_empty() {
            n.volume_percent = Some(v.iter().sum::<u32>() / v.len() as u32);
        }
    }
    n.muted = s["mute"].as_bool();
    if let Some(m) = p.as_object() {
        for (k, v) in m {
            if kept(k) {
                n.props
                    .insert(k.clone(), v.as_str().unwrap_or("").to_string());
            }
        }
    }
    Some(n)
}

/// `s16le 2ch 48000Hz` → format, channels, rate.
pub(crate) fn sample_spec(spec: &str) -> Option<Format> {
    let mut it = spec.split_whitespace();
    let format = it.next()?.to_string();
    let channels = it.next()?.trim_end_matches("ch").parse().ok()?;
    let rate = it.next()?.trim_end_matches("Hz").parse().ok()?;
    Some(Format {
        subtype: "raw".into(),
        format,
        rate,
        channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_and_list() {
        let info: Value = serde_json::from_str(
            r#"{"server_name":"pulseaudio","server_version":"17.0","default_sink_name":"s1"}"#,
        )
        .unwrap();
        let list: Value = serde_json::from_str(
            r#"{"sinks":[{"index":3,"name":"s1","state":"RUNNING","card":0,"driver":"module-alsa-card.c",
                 "sample_specification":"s16le 2ch 48000Hz","channel_map":"front-left,front-right","mute":false,
                 "volume":{"front-left":{"value_percent":"40%"},"front-right":{"value_percent":"60%"}},
                 "properties":{"alsa.card":"1","alsa.device":"3","device.api":"alsa"},
                 "ports":[{"name":"hdmi-output-0"}],"active_port":"hdmi-output-0","formats":["pcm","ac3-iec61937, format.rate = \"[ 48000 ]\""]}],
               "sources":[{"index":1,"name":"s1.monitor","properties":{"device.class":"monitor"}}],
               "cards":[{"index":0,"name":"alsa_card.x","active_profile":"output:hdmi-stereo","properties":{"alsa.card":"1"},
                 "profiles":{"output:hdmi-stereo":{"description":"HDMI","available":false}},
                 "ports":{"hdmi-output-0":{"description":"HDMI","availability":"not available"}}}]}"#,
        )
        .unwrap();
        let d = parse(&info, Some(&list)).unwrap();
        assert_eq!(d.version.as_deref(), Some("17.0"));
        assert_eq!(d.nodes.len(), 1, "monitor skipped");
        let n = &d.nodes[0];
        assert_eq!((n.alsa_card, n.alsa_device), (Some(1), Some(3)));
        assert_eq!(n.state.as_deref(), Some("running"));
        assert_eq!(
            n.format.as_ref().map(|f| (f.rate, f.channels)),
            Some((48000, 2))
        );
        assert_eq!(
            n.iec958_codecs.as_deref(),
            Some(&["pcm".to_string(), "ac3-iec61937".into()][..])
        );
        assert_eq!(n.volume_percent, Some(50));
        let c = &d.devices[0];
        assert_eq!(c.alsa_card, Some(1));
        assert_eq!(c.profiles[0].available, Some(false));
        assert_eq!(c.routes[0].direction, "output");
        assert_eq!(c.routes[0].available, Some(false));
        assert!(c.routes[0].active);
    }
}
