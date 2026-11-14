// SPDX-License-Identifier: LGPL-2.1-or-later
//! Wireless assembly: the Bluetooth layer (advertised, host-supported and negotiated codecs),
//! Bluetooth endpoints BlueZ knows but no server exposes, JACK server state, and sandbox gaps.

use audiocaps_model::redact::stable_hash;
use audiocaps_model::{
    Availability, BluetoothCodec, BluetoothCodecState, BluetoothDetail, BluetoothTransport,
    CapabilitySet, ClockSettings, Confidence, CorrelationId, CorrelationKey, Direction, Endpoint,
    EndpointClass, EndpointId, EndpointName, EnvironmentInfo, FieldGroup, LayerCapabilities,
    LayerDetail, LayerKind, PortGroup, Range, Sandbox, Sensitive, SensitiveKind, ServerInfo,
    ServerState, SoundServer, Source, Transport, UnknownReason, Value,
};

use super::{Ctx, prov, unknown_layer};
use crate::bluez::BtDevice;
use crate::probe::Outcome;

/// BlueZ codec names (as PipeWire spells them) → model.
pub(crate) fn bt_codec(name: &str) -> BluetoothCodec {
    match name.to_ascii_lowercase().replace('-', "_").as_str() {
        "sbc" => BluetoothCodec::Sbc,
        "sbc_xq" => BluetoothCodec::SbcXq,
        "aac" => BluetoothCodec::Aac,
        "aptx" => BluetoothCodec::AptX,
        "aptx_hd" => BluetoothCodec::AptXHd,
        "aptx_ll" | "aptx_ll_duplex" => BluetoothCodec::AptXLl,
        "ldac" => BluetoothCodec::Ldac,
        "lc3" | "lc3_swb" | "lc3plus_hr" => BluetoothCodec::Lc3,
        "opus" | "opus_05" | "opus_g" => BluetoothCodec::Opus,
        "msbc" => BluetoothCodec::Msbc,
        "cvsd" => BluetoothCodec::Cvsd,
        other => BluetoothCodec::Other(other.to_string()),
    }
}

impl Ctx<'_> {
    fn bluez_source(&self, live: bool) -> Source {
        if live {
            Source::DbusProperty {
                service: "org.bluez".into(),
                path: "/".into(),
                interface: "org.freedesktop.DBus.ObjectManager".into(),
                property: "GetManagedObjects".into(),
            }
        } else {
            Source::command("bluez")
        }
    }

    pub(crate) fn bt_device(&self, address: &str) -> Option<(&BtDevice, bool)> {
        match &self.res.bluez {
            Some(Outcome::Read { value, live }) => value
                .devices
                .iter()
                .find(|d| d.address.eq_ignore_ascii_case(address))
                .map(|d| (d, *live)),
            _ => None,
        }
    }

    /// The wireless layer of a Bluetooth endpoint. `host` is what the sound server offers for the
    /// device (per-codec profiles), when a server exposes it.
    pub(crate) fn wireless_layer(
        &self,
        address: Option<&str>,
        host: Option<&[BluetoothCodecState]>,
    ) -> LayerCapabilities {
        let layer = LayerKind::Wireless;
        let dev = address.and_then(|a| self.bt_device(a));
        let availability = match (&self.res.bluez, dev) {
            (_, Some(_)) => Availability::Read,
            (Some(Outcome::Unknown { reason, detail }), _) => {
                unknown_layer(*reason, self.with_fix(layer, detail))
            }
            (Some(Outcome::Read { .. }), None) => {
                unknown_layer(UnknownReason::NotExposed, "BlueZ doesn't list this device")
            }
            (None, None) => {
                unknown_layer(UnknownReason::NotRequested, "wireless layer not in scope")
            }
        };
        let mut caps = CapabilitySet::default();
        if self.field(FieldGroup::BluetoothCodecs) {
            caps.bluetooth_codecs = Some(match dev {
                Some((d, live)) => Value::measured(
                    merge_codecs(d, host),
                    prov(layer, self.bluez_source(live), self.now),
                ),
                None => match &availability {
                    Availability::Unknown { reason, detail } => {
                        Value::unknown(*reason, detail.clone())
                    }
                    _ => Value::unknown(UnknownReason::NotExposed, None),
                },
            });
        }
        let detail = (self.field(FieldGroup::Identity) || self.field(FieldGroup::BluetoothCodecs))
            .then_some(dev)
            .flatten()
            .map(|(d, live)| {
                let p = prov(layer, self.bluez_source(live), self.now);
                LayerDetail::Wireless(BluetoothDetail {
                    connected: Value::measured(d.connected, p.clone()),
                    profiles: d.audio_profiles(),
                    transport: d.transports.first().map(|(profile, codec, state)| {
                        Value::measured(
                            BluetoothTransport {
                                profile: profile.clone(),
                                codec: bt_codec(codec),
                                state: state.clone(),
                            },
                            p.clone(),
                        )
                    }),
                })
            });
        LayerCapabilities {
            layer,
            availability,
            caps,
            detail,
        }
    }

    /// Bluetooth audio devices BlueZ lists as connected that no sound server exposes.
    pub(crate) fn bt_only_endpoints(&self, known: &[String]) -> Vec<Endpoint> {
        let Some(Outcome::Read { value, live }) = &self.res.bluez else {
            return vec![];
        };
        let mut out = vec![];
        for d in value.devices.iter().filter(|d| d.connected && d.is_audio()) {
            if known.iter().any(|a| a.eq_ignore_ascii_case(&d.address)) {
                continue;
            }
            let id = EndpointId(format!(
                "bt:{}",
                stable_hash(&d.address.to_ascii_uppercase()).trim_start_matches("sha256:")
            ));
            if !self
                .scope
                .includes_endpoint(EndpointClass::Bluetooth, Direction::Playback, &id)
            {
                continue;
            }
            let p = prov(LayerKind::Wireless, self.bluez_source(*live), self.now);
            let identity = self.field(FieldGroup::Identity);
            let mut layers = vec![];
            if self.layer(LayerKind::SoundServer) {
                layers.push(LayerCapabilities {
                    layer: LayerKind::SoundServer,
                    availability: unknown_layer(
                        UnknownReason::NotExposed,
                        "no sound server exposes this Bluetooth device",
                    ),
                    caps: CapabilitySet::default(),
                    detail: None,
                });
            }
            if self.layer(LayerKind::Wireless) {
                layers.push(self.wireless_layer(Some(&d.address), None));
            }
            out.push(Endpoint {
                id,
                class: EndpointClass::Bluetooth,
                direction: Direction::Playback,
                transport: Value::measured(Transport::Bluetooth, p.clone()),
                name: identity.then(|| {
                    Value::measured(
                        EndpointName::Personal(Sensitive::new(
                            SensitiveKind::DeviceName,
                            d.alias.clone().unwrap_or_default(),
                        )),
                        p.clone(),
                    )
                }),
                correlation: if identity {
                    vec![bluez_key(&d.address)]
                } else {
                    vec![]
                },
                sink: None,
                active_probe: None,
                midi: None,
                layers,
                effective: vec![],
                paths: vec![],
            });
        }
        out
    }

    /// JACK server state, when the sound-server layer is in scope.
    pub(crate) fn jack_state(&self) -> Option<ServerState> {
        let o = self.res.jack.as_ref()?;
        Some(match o {
            Outcome::Read { value, live } => {
                let p = prov(
                    LayerKind::SoundServer,
                    if *live {
                        Source::api("libjack", "jack_get_sample_rate")
                    } else {
                        Source::command("jack")
                    },
                    self.now,
                );
                ServerState {
                    server: SoundServer::Jack,
                    availability: Availability::Read,
                    info: Some(Value::measured(
                        ServerInfo {
                            name: "jack".into(),
                            version: None,
                        },
                        p.clone(),
                    )),
                    clock: Some(Value::measured(
                        ClockSettings {
                            rate_hz: Some(value.rate),
                            quantum: Some(value.buffer_frames),
                            ..ClockSettings::default()
                        },
                        p.clone(),
                    )),
                    default_sink: None,
                    default_source: None,
                    policy: None,
                    ports: Some(Value::measured(
                        value
                            .ports
                            .iter()
                            .map(|g| PortGroup {
                                client: g.client.clone(),
                                direction: g.direction.clone(),
                                channels: g.channels,
                                latency_frames: g.latency.map(|(min, max)| Range { min, max }),
                            })
                            .collect(),
                        p,
                    )),
                }
            }
            Outcome::Unknown { reason, detail } => ServerState {
                server: SoundServer::Jack,
                availability: Availability::Unknown {
                    reason: *reason,
                    detail: Some(detail.clone()),
                },
                info: None,
                clock: None,
                default_sink: None,
                default_source: None,
                policy: None,
                ports: None,
            },
        })
    }

    /// Adds the sandbox fix to a detail when a missing permission hides `layer`.
    pub(crate) fn with_fix(&self, layer: LayerKind, detail: &str) -> String {
        match self.gap_for(layer) {
            Some((perm, fix)) => format!("{detail}; missing {perm}: {fix}"),
            None => detail.to_string(),
        }
    }

    fn gap_for(&self, layer: LayerKind) -> Option<(String, String)> {
        let env = self.res.environment.as_ref()?;
        let missing = match env.sandbox.known()? {
            Sandbox::Flatpak { missing, .. } | Sandbox::Snap { missing, .. } => missing,
            Sandbox::None => return None,
        };
        missing
            .iter()
            .find(|g| g.layers.contains(&layer))
            .map(|g| (g.permission.clone(), g.fix.clone()))
    }

    /// Server presence from BlueZ and JACK, and the snap `bluez` gap.
    pub(crate) fn wireless_environment(&self, env: &mut EnvironmentInfo) {
        if let Some(Outcome::Read { live: true, .. }) = &self.res.bluez {
            env.servers.push(audiocaps_model::ServerPresence {
                server: SoundServer::Bluez,
                info: Value::measured(
                    ServerInfo {
                        name: "bluetoothd".into(),
                        version: None,
                    },
                    prov(LayerKind::Wireless, self.bluez_source(true), self.now),
                ),
            });
        }
        if let Some(Outcome::Unknown {
            reason: UnknownReason::NotGranted,
            ..
        }) = &self.res.bluez
            && let Value::Known {
                value: Sandbox::Snap { missing, .. },
                ..
            } = &mut env.sandbox
            && !missing.iter().any(|g| g.permission == "bluez")
        {
            missing.push(crate::sandbox::snap_bluez_gap());
        }
    }
}

/// Correlation key for a BlueZ device (address hashed on export).
pub(crate) fn bluez_key(address: &str) -> CorrelationKey {
    CorrelationKey {
        id: CorrelationId::BluezDevice {
            address: Sensitive::new(
                SensitiveKind::BluetoothAddress,
                address.to_ascii_uppercase(),
            ),
        },
        confidence: Confidence::Measured,
    }
}

/// Merges what the device advertises, what the host supports and what is negotiated.
fn merge_codecs(d: &BtDevice, host: Option<&[BluetoothCodecState]>) -> Vec<BluetoothCodecState> {
    let negotiated: Vec<BluetoothCodec> = d.transports.iter().map(|t| bt_codec(&t.1)).collect();
    let advertised: Vec<BluetoothCodec> = d.endpoint_codecs.iter().map(|c| bt_codec(c)).collect();
    let mut all: Vec<BluetoothCodec> = advertised.clone();
    for c in host
        .unwrap_or_default()
        .iter()
        .map(|h| h.codec.clone())
        .chain(negotiated.iter().cloned())
    {
        if !all.contains(&c) {
            all.push(c);
        }
    }
    all.into_iter()
        .map(|codec| {
            let h = host.and_then(|h| h.iter().find(|x| x.codec == codec));
            BluetoothCodecState {
                advertised_by_device: advertised.contains(&codec)
                    || (advertised.is_empty() && h.is_some_and(|h| h.advertised_by_device)),
                available_on_host: h.is_some() || negotiated.contains(&codec),
                negotiated: negotiated.contains(&codec),
                quality_mode: h.and_then(|h| h.quality_mode.clone()),
                codec,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldac_advertised_but_missing_on_host() {
        let d = BtDevice {
            endpoint_codecs: vec!["ldac".into(), "aac".into(), "sbc".into()],
            transports: vec![("a2dp_sink".into(), "sbc".into(), "active".into())],
            ..BtDevice::default()
        };
        let host = [
            BluetoothCodecState {
                codec: BluetoothCodec::Sbc,
                advertised_by_device: true,
                available_on_host: true,
                negotiated: true,
                quality_mode: None,
            },
            BluetoothCodecState {
                codec: BluetoothCodec::Aac,
                advertised_by_device: true,
                available_on_host: true,
                negotiated: false,
                quality_mode: None,
            },
        ];
        let m = merge_codecs(&d, Some(&host));
        let ldac = m.iter().find(|c| c.codec == BluetoothCodec::Ldac).unwrap();
        assert!(ldac.advertised_by_device && !ldac.available_on_host && !ldac.negotiated);
        let sbc = m.iter().find(|c| c.codec == BluetoothCodec::Sbc).unwrap();
        assert!(sbc.negotiated && sbc.available_on_host);
        let aac = m.iter().find(|c| c.codec == BluetoothCodec::Aac).unwrap();
        assert!(aac.available_on_host && !aac.negotiated);
    }
}
