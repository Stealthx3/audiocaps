// SPDX-License-Identifier: LGPL-2.1-or-later
//! `pw-dump` JSON → [`ServerDump`]. Used for fixtures, and by the gate to compare with the live
//! PipeWire backend. Unknown or malformed objects are skipped, never fatal.

use serde_json::Value;

use super::{
    Clock, Device, Format, FormatRange, Node, NodeDir, Profile, Route, ServerDump, ServerKind,
    int_list, kept,
};

/// Parses `pw-dump` output.
pub fn parse(json: &Value) -> Option<ServerDump> {
    let objs = json.as_array()?;
    let mut d = ServerDump {
        kind: ServerKind::PipeWire,
        name: "pipewire".into(),
        version: None,
        clock: None,
        default_sink: None,
        default_source: None,
        policy: Default::default(),
        session_manager: None,
        devices: vec![],
        nodes: vec![],
    };
    let mut clock = Clock::default();
    let mut core_props: Option<&Value> = None;
    for o in objs {
        let ty = o["type"].as_str().unwrap_or("");
        let info = &o["info"];
        match ty {
            "PipeWire:Interface:Core" => {
                if let Some(n) = info["name"].as_str() {
                    d.name = n.to_string();
                }
                d.version = info["version"].as_str().map(String::from);
                core_props = Some(&info["props"]);
            }
            "PipeWire:Interface:Client" => {
                let p = &info["props"];
                if p["application.name"].as_str() == Some("WirePlumber") {
                    d.session_manager = Some("WirePlumber".into());
                }
            }
            "PipeWire:Interface:Metadata" => metadata(o, &mut d, &mut clock),
            "PipeWire:Interface:Device" => {
                if let Some(dev) = device(o) {
                    d.devices.push(dev);
                }
            }
            "PipeWire:Interface:Node" => {
                if let Some(n) = node(o) {
                    d.nodes.push(n);
                }
            }
            _ => {}
        }
    }
    // Core properties give the configured defaults when the settings metadata is missing.
    if let Some(p) = core_props {
        let num = |k: &str| as_u32(&p[k]);
        clock.rate = clock.rate.or(num("default.clock.rate"));
        clock.quantum = clock.quantum.or(num("default.clock.quantum"));
        clock.min_quantum = clock.min_quantum.or(num("default.clock.min-quantum"));
        clock.max_quantum = clock.max_quantum.or(num("default.clock.max-quantum"));
        if clock.allowed_rates.is_empty()
            && let Some(s) = p["default.clock.allowed-rates"].as_str()
        {
            clock.allowed_rates = int_list(s);
        }
    }
    if clock != Clock::default() {
        d.clock = Some(clock);
    }
    d.nodes.sort_by_key(|n| n.id);
    d.devices.sort_by_key(|n| n.id);
    Some(d)
}

fn metadata(o: &Value, d: &mut ServerDump, clock: &mut Clock) {
    let name = o["props"]["metadata.name"].as_str().unwrap_or("");
    let Some(entries) = o["metadata"].as_array() else {
        return;
    };
    for e in entries {
        if e["subject"].as_u64() != Some(0) {
            continue;
        }
        let key = e["key"].as_str().unwrap_or("");
        let v = &e["value"];
        match (name, key) {
            ("settings", "clock.rate") => clock.rate = as_u32(v),
            ("settings", "clock.allowed-rates") => {
                clock.allowed_rates = v.as_str().map(int_list).unwrap_or_default()
            }
            ("settings", "clock.quantum") => clock.quantum = as_u32(v),
            ("settings", "clock.min-quantum") => clock.min_quantum = as_u32(v),
            ("settings", "clock.max-quantum") => clock.max_quantum = as_u32(v),
            ("settings", "clock.force-rate") => clock.force_rate = as_u32(v),
            ("settings", "clock.force-quantum") => clock.force_quantum = as_u32(v),
            ("default", "default.audio.sink") => d.default_sink = meta_name(v),
            ("default", "default.audio.source") => d.default_source = meta_name(v),
            ("sm-settings", k) => {
                d.policy.insert(k.to_string(), value_text(v));
            }
            _ => {}
        }
    }
}

/// `{"name": "…"}` (possibly as a JSON string) → the name.
fn meta_name(v: &Value) -> Option<String> {
    match v {
        Value::Object(_) => v["name"].as_str().map(String::from),
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|j| j["name"].as_str().map(String::from)),
        _ => None,
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn device(o: &Value) -> Option<Device> {
    let info = &o["info"];
    let p = &info["props"];
    // Audio devices only, as the live client binds them (cameras are devices too).
    if p["media.class"].as_str() != Some("Audio/Device") {
        return None;
    }
    let params = &info["params"];
    let mut dev = Device {
        id: as_u32(&o["id"])?,
        name: p["device.name"].as_str().unwrap_or("").to_string(),
        api: p["device.api"].as_str().map(String::from),
        alsa_card: as_u32(&p["api.alsa.card"]).or_else(|| as_u32(&p["alsa.card"])),
        ..Device::default()
    };
    for e in params["EnumProfile"].as_array().into_iter().flatten() {
        dev.profiles.push(Profile {
            name: e["name"].as_str().unwrap_or("").into(),
            description: e["description"].as_str().unwrap_or("").into(),
            available: availability(&e["available"]),
        });
    }
    dev.active_profile = params["Profile"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|p| p["name"].as_str())
        .map(String::from);
    let active: Vec<(&str, &str)> = params["Route"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["name"].as_str()?, r["direction"].as_str()?)))
        .collect();
    for e in params["EnumRoute"].as_array().into_iter().flatten() {
        let name = e["name"].as_str().unwrap_or("");
        let direction = e["direction"].as_str().unwrap_or("").to_ascii_lowercase();
        let is_active = active
            .iter()
            .any(|(n, d)| *n == name && d.eq_ignore_ascii_case(&direction));
        dev.routes.push(Route {
            name: name.into(),
            description: e["description"].as_str().unwrap_or("").into(),
            direction,
            available: availability(&e["available"]),
            active: is_active,
            devices: e["devices"]
                .as_array()
                .map(|a| a.iter().filter_map(as_u32).collect())
                .unwrap_or_default(),
            // info: [count, key, value, key, value, …]
            detected_codecs: e["info"]
                .as_array()
                .and_then(|a| super::detected_codecs(a.iter().skip(1).filter_map(Value::as_str))),
        });
    }
    copy_props(p, &mut dev.props);
    Some(dev)
}

fn availability(v: &Value) -> Option<bool> {
    match v.as_str() {
        Some("yes") => Some(true),
        Some("no") => Some(false),
        _ => None,
    }
}

fn copy_props(p: &Value, out: &mut std::collections::BTreeMap<String, String>) {
    if let Some(m) = p.as_object() {
        for (k, v) in m {
            if kept(k) {
                out.insert(k.clone(), value_text(v));
            }
        }
    }
}

fn node(o: &Value) -> Option<Node> {
    let info = &o["info"];
    let p = &info["props"];
    let media_class = p["media.class"].as_str().unwrap_or("");
    if !media_class.starts_with("Audio/") {
        return None;
    }
    let dir = if media_class.contains("Sink") {
        Some(NodeDir::Sink)
    } else if media_class.contains("Source") {
        Some(NodeDir::Source)
    } else {
        None
    };
    let params = &info["params"];
    let mut n = Node {
        id: as_u32(&o["id"])?,
        name: p["node.name"].as_str().unwrap_or("").into(),
        description: p["node.description"].as_str().unwrap_or("").into(),
        media_class: media_class.into(),
        dir,
        device_id: as_u32(&p["device.id"]),
        api: p["device.api"].as_str().map(String::from),
        factory: p["factory.name"].as_str().map(String::from),
        alsa_card: as_u32(&p["api.alsa.pcm.card"]).or_else(|| as_u32(&p["alsa.card"])),
        alsa_device: as_u32(&p["api.alsa.pcm.device"]).or_else(|| as_u32(&p["alsa.device"])),
        state: info["state"].as_str().map(String::from),
        profile: p["device.profile.name"].as_str().map(String::from),
        ..Node::default()
    };
    n.format = params["Format"]
        .as_array()
        .and_then(|a| a.iter().find_map(format));
    n.enum_formats = params["EnumFormat"]
        .as_array()
        .map(|a| a.iter().filter_map(format_range).collect())
        .unwrap_or_default();
    for pr in params["Props"].as_array().into_iter().flatten() {
        if let Some(c) = pr["iec958Codecs"].as_array() {
            n.iec958_codecs = Some(
                c.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect(),
            );
        }
        if let Some(v) = pr["channelVolumes"].as_array() {
            let vals: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
            if !vals.is_empty() {
                let avg = vals.iter().sum::<f64>() / vals.len() as f64;
                n.volume_percent = Some(linear_to_percent(avg));
            }
        }
        if let Some(m) = pr["mute"].as_bool() {
            n.muted = Some(m);
        }
    }
    copy_props(p, &mut n.props);
    Some(n)
}

/// PipeWire volumes are linear amplitude; UIs (and `pactl`) show the cubic-root percentage.
pub(crate) fn linear_to_percent(v: f64) -> u32 {
    (v.max(0.0).cbrt() * 100.0).round() as u32
}

fn format(f: &Value) -> Option<Format> {
    if f["mediaType"].as_str() != Some("audio") {
        return None;
    }
    let subtype = f["mediaSubtype"].as_str().unwrap_or("raw").to_string();
    let format = if subtype == "iec958" {
        f["iec958Codec"].as_str()
    } else {
        f["format"].as_str()
    }
    .unwrap_or("")
    .to_string();
    Some(Format {
        subtype,
        format,
        rate: as_u32(&f["rate"]).unwrap_or(0),
        channels: as_u32(&f["channels"]).unwrap_or(0),
    })
}

/// Values of a pw-dump choice: a scalar, `{"default", "alt1", …}` or `{"default", "min", "max"}`.
fn choice(v: &Value) -> (Vec<Value>, Option<(Value, Value)>) {
    match v {
        Value::Object(m) if m.contains_key("min") && m.contains_key("max") => {
            (vec![], Some((m["min"].clone(), m["max"].clone())))
        }
        Value::Object(m) if m.contains_key("default") => {
            let mut out: Vec<Value> = Vec::new();
            let mut keys: Vec<&String> = m.keys().filter(|k| k.starts_with("alt")).collect();
            keys.sort_by_key(|k| k[3..].parse::<u32>().unwrap_or(0));
            let mut all = vec![&m["default"]];
            all.extend(keys.iter().map(|k| &m[k.as_str()]));
            for x in all {
                if !out.contains(x) {
                    out.push(x.clone());
                }
            }
            (out, None)
        }
        Value::Null => (vec![], None),
        other => (vec![other.clone()], None),
    }
}

fn format_range(f: &Value) -> Option<FormatRange> {
    if f["mediaType"].as_str() != Some("audio") {
        return None;
    }
    let subtype = f["mediaSubtype"].as_str().unwrap_or("raw").to_string();
    let key = if subtype == "iec958" {
        "iec958Codec"
    } else {
        "format"
    };
    let (formats, _) = choice(&f[key]);
    let (rates, rate_range) = choice(&f["rate"]);
    let (chans, chan_range) = choice(&f["channels"]);
    let channels = match chan_range {
        Some((a, b)) => as_u32(&a).zip(as_u32(&b)),
        None => {
            let c: Vec<u32> = chans.iter().filter_map(as_u32).collect();
            c.iter().min().copied().zip(c.iter().max().copied())
        }
    };
    Some(FormatRange {
        subtype,
        formats: formats
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        rates: rates.iter().filter_map(as_u32).collect(),
        rate_range: rate_range.and_then(|(a, b)| Some((as_u32(&a)?, as_u32(&b)?))),
        channels: if f["channels"].is_null() {
            None
        } else {
            channels
        },
        positions: f["position"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_devices_clock_and_defaults() {
        let j: Value = serde_json::from_str(
            r#"[
          {"id":0,"type":"PipeWire:Interface:Core","info":{"name":"pipewire-0","version":"1.4.2","props":{"default.clock.rate":48000}}},
          {"id":32,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"settings"},"metadata":[
             {"subject":0,"key":"clock.allowed-rates","value":"[ 44100 48000 ]"},{"subject":0,"key":"clock.quantum","value":1024}]},
          {"id":38,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[
             {"subject":0,"key":"default.audio.sink","type":"Spa:String:JSON","value":{"name":"alsa_output.x"}}]},
          {"id":42,"type":"PipeWire:Interface:Device","info":{"props":{"device.name":"alsa_card.x","device.api":"alsa","media.class":"Audio/Device","api.alsa.card":1},
             "params":{"EnumProfile":[{"index":1,"name":"output:hdmi-stereo","description":"HDMI","available":"no"}],
                       "Profile":[{"index":1,"name":"output:hdmi-stereo"}],
                       "EnumRoute":[{"index":0,"direction":"Output","name":"hdmi-output-0","description":"HDMI","available":"no","devices":[3]}],
                       "Route":[{"index":0,"direction":"Output","name":"hdmi-output-0","device":3}]}}},
          {"id":52,"type":"PipeWire:Interface:Node","info":{"state":"suspended","props":{"media.class":"Audio/Sink","node.name":"alsa_output.x",
             "alsa.card":1,"alsa.device":3,"device.id":42,"device.api":"alsa"},
             "params":{"EnumFormat":[{"mediaType":"audio","mediaSubtype":"raw","format":{"default":"S24LE","alt1":"S24LE","alt2":"S16LE"},
                                      "rate":{"default":48000,"min":44100,"max":96000},"channels":2,"position":["FL","FR"]},
                                     {"mediaType":"audio","mediaSubtype":"iec958","iec958Codec":{"default":"PCM","alt1":"PCM","alt2":"AC3"},"rate":48000}],
                       "Props":[{"iec958Codecs":["PCM","AC3"],"channelVolumes":[0.125,0.125],"mute":false}]}}},
          {"id":60,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Stream/Output/Audio","node.name":"app"}}}
        ]"#,
        )
        .unwrap();
        let d = parse(&j).unwrap();
        assert_eq!(d.version.as_deref(), Some("1.4.2"));
        let c = d.clock.unwrap();
        assert_eq!((c.rate, c.quantum), (Some(48000), Some(1024)));
        assert_eq!(c.allowed_rates, vec![44100, 48000]);
        assert_eq!(d.default_sink.as_deref(), Some("alsa_output.x"));
        assert_eq!(d.devices[0].alsa_card, Some(1));
        assert_eq!(d.devices[0].routes[0].available, Some(false));
        assert!(d.devices[0].routes[0].active);
        assert_eq!(d.nodes.len(), 1, "streams are not endpoints");
        let n = &d.nodes[0];
        assert_eq!((n.alsa_card, n.alsa_device), (Some(1), Some(3)));
        assert_eq!(n.enum_formats[0].formats, vec!["S24LE", "S16LE"]);
        assert_eq!(n.enum_formats[0].rate_range, Some((44100, 96000)));
        assert_eq!(n.enum_formats[0].channels, Some((2, 2)));
        assert_eq!(n.enum_formats[1].formats, vec!["PCM", "AC3"]);
        assert_eq!(
            n.iec958_codecs.as_deref(),
            Some(&["PCM".to_string(), "AC3".into()][..])
        );
        assert_eq!(n.volume_percent, Some(50));
    }
}
