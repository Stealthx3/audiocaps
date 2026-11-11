// SPDX-License-Identifier: LGPL-2.1-or-later
//! BlueZ over D-Bus: devices, the codecs their stream endpoints advertise (`MediaEndpoint1` objects
//! under a connected device), and the active transport's codec (`MediaTransport1`).
//!
//! Passive: one `GetManagedObjects` call, and only when `org.bluez` already owns its bus name
//! (a method call to an unowned name could D-Bus-activate bluetoothd). Nothing is set.
//! The live reply and the captured `busctl --json` output go through the same parser.

use std::collections::BTreeMap;

use audiocaps_model::UnknownReason;
use audiocaps_parse::bt;
use serde_json::{Map, Value};

/// What BlueZ reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BluezDump {
    /// Adapters: path and powered state.
    pub adapters: Vec<(String, Option<bool>)>,
    /// Devices.
    pub devices: Vec<BtDevice>,
}

/// A Bluetooth device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BtDevice {
    /// Object path.
    pub path: String,
    /// Address (sensitive; hashed on export).
    pub address: String,
    /// User-visible alias (personal; masked on export).
    pub alias: Option<String>,
    /// Connected.
    pub connected: bool,
    /// Service UUIDs.
    pub uuids: Vec<String>,
    /// Codecs the device's stream endpoints advertise.
    pub endpoint_codecs: Vec<String>,
    /// Active transports: (profile, codec, state).
    pub transports: Vec<(String, String, String)>,
}

impl BtDevice {
    /// Audio profiles the device offers.
    pub fn audio_profiles(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .uuids
            .iter()
            .filter_map(|u| bt::profile_name(u).map(String::from))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Whether the device does audio at all.
    pub fn is_audio(&self) -> bool {
        self.uuids
            .iter()
            .any(|u| bt::family(u) != bt::Family::Other)
    }
}

/// Unwraps `busctl --json` variants (`{"type": "s", "data": …}`) into plain JSON.
fn plain(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.contains_key("type") && m.contains_key("data") && m.len() == 2 => {
            plain(&m["data"])
        }
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), plain(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(plain).collect()),
        other => other.clone(),
    }
}

/// Parses captured `busctl --json=pretty call org.bluez / … GetManagedObjects` output.
pub fn from_busctl(json: &Value) -> Option<BluezDump> {
    let objs = json["data"].as_array()?.first()?;
    Some(parse_objects(&plain(objs)))
}

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_default()
}

/// Parses `{path: {interface: {property: value}}}` with plain JSON values.
pub fn parse_objects(objs: &Value) -> BluezDump {
    let mut d = BluezDump::default();
    let Some(map) = objs.as_object() else {
        return d;
    };
    let mut devices: BTreeMap<String, BtDevice> = BTreeMap::new();
    for (path, ifs) in map {
        if let Some(a) = ifs.get("org.bluez.Adapter1") {
            d.adapters.push((path.clone(), a["Powered"].as_bool()));
        }
        if let Some(dev) = ifs.get("org.bluez.Device1") {
            let e = devices.entry(path.clone()).or_default();
            e.path = path.clone();
            e.address = dev["Address"].as_str().unwrap_or("").to_string();
            e.alias = dev["Alias"].as_str().map(String::from);
            e.connected = dev["Connected"].as_bool().unwrap_or(false);
            e.uuids = dev["UUIDs"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|u| u.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    let owner = |p: &str, explicit: &Value| -> Option<String> {
        explicit
            .as_str()
            .map(String::from)
            .or_else(|| devices.keys().find(|d| p.starts_with(d.as_str())).cloned())
    };
    let mut endpoint_codecs: Vec<(String, String)> = vec![];
    let mut transports: Vec<(String, (String, String, String))> = vec![];
    for (path, ifs) in map {
        if let Some(ep) = ifs.get("org.bluez.MediaEndpoint1")
            && let Some(dev) = owner(path, &ep["Device"])
        {
            let codec = ep["Codec"].as_u64().unwrap_or(0) as u8;
            let uuid = ep["UUID"].as_str().unwrap_or("");
            let name = match bt::family(uuid) {
                bt::Family::LeAudio => bt::transport_codec(bt::Family::LeAudio, codec, &[]),
                _ => bt::a2dp_codec(codec, &bytes(&ep["Capabilities"])),
            };
            endpoint_codecs.push((dev, name));
        }
        if let Some(t) = ifs.get("org.bluez.MediaTransport1")
            && let Some(dev) = owner(path, &t["Device"])
        {
            let uuid = t["UUID"].as_str().unwrap_or("");
            let fam = bt::family(uuid);
            let codec = bt::transport_codec(
                fam,
                t["Codec"].as_u64().unwrap_or(0) as u8,
                &bytes(&t["Configuration"]),
            );
            let profile = bt::profile_name(uuid).unwrap_or("unknown").to_string();
            let state = t["State"].as_str().unwrap_or("").to_string();
            transports.push((dev, (profile, codec, state)));
        }
    }
    for (dev, c) in endpoint_codecs {
        if let Some(d) = devices.get_mut(&dev)
            && !d.endpoint_codecs.contains(&c)
        {
            d.endpoint_codecs.push(c);
        }
    }
    for (dev, t) in transports {
        if let Some(d) = devices.get_mut(&dev) {
            d.transports.push(t);
        }
    }
    d.devices = devices.into_values().collect();
    d
}

/// Converts a D-Bus value to plain JSON.
fn zv(v: &zbus::zvariant::Variant<'_>) -> Value {
    use zbus::zvariant::Variant as Z;
    match v {
        Z::U8(x) => Value::from(*x),
        Z::Bool(x) => Value::from(*x),
        Z::I16(x) => Value::from(*x),
        Z::U16(x) => Value::from(*x),
        Z::I32(x) => Value::from(*x),
        Z::U32(x) => Value::from(*x),
        Z::I64(x) => Value::from(*x),
        Z::U64(x) => Value::from(*x),
        Z::F64(x) => Value::from(*x),
        Z::Str(s) => Value::from(s.as_str()),
        Z::ObjectPath(p) => Value::from(p.as_str()),
        Z::Signature(s) => Value::from(s.to_string()),
        Z::Variant(inner) => zv(inner),
        Z::Array(a) => Value::Array(a.iter().map(zv).collect()),
        Z::Dict(d) => {
            let mut m = Map::new();
            for (k, v) in d.iter() {
                let key = match zv(k) {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                m.insert(key, zv(v));
            }
            Value::Object(m)
        }
        Z::Structure(s) => Value::Array(s.fields().iter().map(zv).collect()),
        _ => Value::Null,
    }
}

/// Reads BlueZ live (zbus is async; the call runs on zbus's own runtime helper).
pub fn read_live() -> Result<BluezDump, (UnknownReason, String)> {
    zruntime::block_on(read_live_async())
}

async fn read_live_async() -> Result<BluezDump, (UnknownReason, String)> {
    let conn = zbus::Connection::system().await.map_err(|e| {
        (
            UnknownReason::NotGranted,
            format!("no access to the system bus: {e}"),
        )
    })?;
    let dbus = zbus::fdo::DBusProxy::new(&conn)
        .await
        .map_err(|e| (UnknownReason::ReadError, e.to_string()))?;
    let name = zbus::names::BusName::try_from("org.bluez")
        .map_err(|e| (UnknownReason::ReadError, e.to_string()))?;
    match dbus.name_has_owner(name).await {
        Ok(true) => {}
        Ok(false) => {
            return Err((
                UnknownReason::ServiceNotRunning,
                "org.bluez is not running; not activating it".into(),
            ));
        }
        Err(e) => return Err((UnknownReason::NotGranted, e.to_string())),
    }
    // Setters record errors; `build` reports the first one.
    let om = zbus::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")
        .path("/")
        .build()
        .await
        .map_err(|e| (UnknownReason::ReadError, e.to_string()))?;
    let objs = om.get_managed_objects().await.map_err(|e| {
        let s = e.to_string();
        let reason = if s.contains("AccessDenied") {
            UnknownReason::NotGranted
        } else {
            UnknownReason::ReadError
        };
        (reason, s)
    })?;
    let mut root = Map::new();
    for (path, ifs) in objs {
        let mut im = Map::new();
        for (iface, props) in ifs {
            let mut pm = Map::new();
            for (k, v) in props {
                pm.insert(k.to_string(), zv(&v));
            }
            im.insert(iface.to_string(), Value::Object(pm));
        }
        root.insert(path.to_string(), Value::Object(im));
    }
    Ok(parse_objects(&Value::Object(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_transport() {
        let objs: Value = serde_json::from_str(
            r#"{
          "/org/bluez/hci0": {"org.bluez.Adapter1": {"Powered": true}},
          "/org/bluez/hci0/dev_X": {"org.bluez.Device1": {"Address": "addr-x", "Alias": "Headphones",
              "Connected": true, "UUIDs": ["0000110b-0000-1000-8000-00805f9b34fb", "0000111e-0000-1000-8000-00805f9b34fb"]}},
          "/org/bluez/hci0/dev_X/sep1": {"org.bluez.MediaEndpoint1": {"UUID": "0000110b-0000-1000-8000-00805f9b34fb",
              "Codec": 255, "Capabilities": [45, 1, 0, 0, 170, 0, 60, 7], "Device": "/org/bluez/hci0/dev_X"}},
          "/org/bluez/hci0/dev_X/sep2": {"org.bluez.MediaEndpoint1": {"UUID": "0000110b-0000-1000-8000-00805f9b34fb",
              "Codec": 0, "Capabilities": [255, 255, 2, 53]}},
          "/org/bluez/hci0/dev_X/sep2/fd0": {"org.bluez.MediaTransport1": {"Device": "/org/bluez/hci0/dev_X",
              "UUID": "0000110b-0000-1000-8000-00805f9b34fb", "Codec": 0, "Configuration": [33, 21, 2, 53], "State": "idle"}}
        }"#,
        )
        .unwrap();
        let d = parse_objects(&objs);
        assert_eq!(d.adapters, vec![("/org/bluez/hci0".into(), Some(true))]);
        let dev = &d.devices[0];
        assert!(dev.connected && dev.is_audio());
        assert_eq!(dev.endpoint_codecs, vec!["ldac", "sbc"]);
        assert_eq!(
            dev.transports,
            vec![("a2dp_sink".into(), "sbc".into(), "idle".into())]
        );
        assert_eq!(dev.audio_profiles(), vec!["a2dp_sink", "hfp_hf"]);
    }

    #[test]
    fn busctl_variants_unwrap() {
        let j: Value = serde_json::from_str(
            r#"{"type":"a{oa{sa{sv}}}","data":[{"/org/bluez/hci0/dev_Y":{"org.bluez.Device1":{
               "Address":{"type":"s","data":"addr-y"},"Connected":{"type":"b","data":false},
               "UUIDs":{"type":"as","data":["0000110b-0000-1000-8000-00805f9b34fb"]}}}}]}"#,
        )
        .unwrap();
        let d = from_busctl(&j).unwrap();
        assert_eq!(d.devices[0].address, "addr-y");
        assert!(!d.devices[0].connected);
    }
}
