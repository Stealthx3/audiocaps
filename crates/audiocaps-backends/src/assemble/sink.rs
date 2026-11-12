// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sink chain assembly: the sink record for an HDMI/DP PCM, built from its ELD (the ALSA `ELD`
//! control's raw bytes, or the kernel's ELD text when its converter is bound to the PCM), the DRM
//! connector's EDID with the same identity, the CEC bus when the app opted in, and quirk data.
//!
//! Sources in order of preference: raw ELD (keeps every SAD flag), the full EDID (CTA-861 blocks),
//! the kernel's ELD text. Downstream devices (a receiver on ARC/eARC) come only from CEC.

use audiocaps_model::{
    AppliedQuirk, Availability, CapabilitySet, CecBus, Codec, CodecCapability, Confidence,
    CorrelationId, CorrelationKey, FieldGroup, HdmiLink, LayerCapabilities, LayerKind, QuirkEffect,
    Range, RateSet, Sensitive, SensitiveKind, SinkIdentity, SinkRecord, Source, Speaker,
    UnknownReason, Value,
};
use audiocaps_parse::edid::{Edid, Latency, parse_edid};
use audiocaps_parse::eld::{Eld, EldText, parse_eld, pnp_id};
use audiocaps_parse::{ids, sad::Sad};
use audiocaps_rules::quirks::SinkKey;

use super::{Ctx, prov, unknown_layer};
use crate::probe::Outcome;
use crate::sysfs::{CardData, DrmConnector, same_slot};

/// The ELD that belongs to one PCM.
enum EldSrc {
    /// Raw bytes from the ALSA `ELD` control.
    Raw(Eld),
    /// Kernel ELD text.
    Text(EldText),
}

impl EldSrc {
    /// (manufacturer id as the kernel prints it, product code, monitor name)
    fn ids(&self) -> (u16, u16, Option<String>) {
        match self {
            EldSrc::Raw(e) => (
                e.manufacturer_id,
                e.product_code,
                Some(e.monitor_name.clone()).filter(|n| !n.is_empty()),
            ),
            EldSrc::Text(t) => (
                t.manufacture_id.unwrap_or(0),
                t.product_id.unwrap_or(0),
                t.monitor_name.clone(),
            ),
        }
    }
}

/// What the sink chain contributes to an endpoint.
pub(crate) struct SinkView {
    pub record: Option<SinkRecord>,
    pub layer: LayerCapabilities,
    pub correlation: Option<CorrelationKey>,
}

const NO_SINK: &str = "nothing connected (the ELD is empty)";

/// Speaker positions per CTA-861 speaker allocation bit.
fn speakers(mask: u16) -> Vec<Speaker> {
    use Speaker::*;
    let table: [&[Speaker]; 11] = [
        &[Fl, Fr],
        &[Lfe],
        &[Fc],
        &[Rl, Rr],
        &[Rc],
        &[Flc, Frc],
        &[Rlc, Rrc],
        &[Other("flw".into()), Other("frw".into())],
        &[Other("flh".into()), Other("frh".into())],
        &[Tc],
        &[Other("fch".into())],
    ];
    table
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .flat_map(|(_, s)| s.iter().cloned())
        .collect()
}

/// A kernel ELD text SAD as a capability (the text has lost the extra flag bits).
fn text_capability(s: &audiocaps_parse::eld::EldTextSad) -> CodecCapability {
    let probe = Sad::from_bytes([(s.coding_type & 0x0f) << 3, 0, 0]);
    CodecCapability {
        codec: probe.codec(),
        max_channels: Some(s.channels),
        rates_hz: s.rates_hz.clone(),
        bit_depths: s.bits.clone(),
        max_bitrate_kbps: s.max_bitrate_bps.map(|b| b / 1000),
        flags: probe.flags(),
    }
}

/// The 32-bit numeric serial and the serial descriptor of an EDID, when not zeroed or redacted.
fn edid_serial(raw: &[u8]) -> Option<String> {
    let mut parts = vec![];
    if raw.len() >= 16 {
        let n = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]);
        if n != 0 {
            parts.push(n.to_string());
        }
    }
    for off in [54usize, 72, 90, 108] {
        let Some(d) = raw.get(off..off + 18) else {
            continue;
        };
        if d[..4] == [0, 0, 0, 0xff] {
            let s: String = d[5..]
                .iter()
                .take_while(|b| **b != b'\n')
                .map(|b| *b as char)
                .collect();
            let s = s.trim().to_string();
            if !s.is_empty() && s != "REDACTED" {
                parts.push(s);
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl Ctx<'_> {
    fn eld_for(
        &self,
        data: Option<&CardData>,
        card: u32,
        device: u32,
    ) -> Result<(EldSrc, Source), (UnknownReason, String)> {
        // 1. The ALSA `ELD` control names its PCM.
        if let Some(c) = self.res.alsa.as_ref().and_then(|d| d.card(card))
            && let Some(ctl) = c
                .controls
                .iter()
                .flatten()
                .find(|x| x.name == "ELD" && x.device == device && x.kind == "bytes")
        {
            let bytes: Vec<u8> = ctl.values.iter().map(|v| *v as u8).collect();
            if bytes.iter().all(|b| *b == 0) {
                return Err((UnknownReason::NotExposed, NO_SINK.into()));
            }
            return match parse_eld(&bytes) {
                Ok(e) => Ok((
                    EldSrc::Raw(e),
                    self.alsa_source(format!(
                        "snd_hctl_elem_read(hw:{card}, ELD device {device})"
                    )),
                )),
                Err(e) => Err((UnknownReason::ParseError, format!("ELD control: {e:?}"))),
            };
        }
        // 2. The kernel's ELD text, when its converter is bound to this PCM in the codec dump.
        let Some(d) = data else {
            return Err((UnknownReason::NotExposed, "card not readable".into()));
        };
        for (file, t) in &d.elds {
            if !(t.eld_valid && t.monitor_present) {
                continue;
            }
            let Some(cvt) = t.codec_cvt_nid else { continue };
            let bound = d.codecs.iter().any(|(_, c)| {
                c.node(cvt)
                    .is_some_and(|n| n.pcm_devices.iter().any(|p| p.device == device))
            });
            if bound {
                return Ok((
                    EldSrc::Text(t.clone()),
                    Source::file(format!("/proc/asound/card{card}/{file}")),
                ));
            }
        }
        if d.elds.iter().any(|(_, t)| t.eld_valid) {
            Err((
                UnknownReason::NotExposed,
                "a sink is connected to this card, but nothing binds its ELD to this PCM (no ELD control in the data and no stream open)".into(),
            ))
        } else {
            Err((UnknownReason::NotExposed, NO_SINK.into()))
        }
    }

    /// The DRM connector whose EDID has the ELD's identity (same GPU when known).
    fn edid_for(&self, data: Option<&CardData>, ids: (u16, u16)) -> Option<(&DrmConnector, Edid)> {
        let bus = data.and_then(|d| d.bus_path.as_deref());
        let mut best: Option<(&DrmConnector, Edid, bool)> = None;
        for c in &self.res.drm {
            if c.status.as_deref() != Some("connected") || c.edid.is_empty() {
                continue;
            }
            let Ok(e) = parse_edid(&c.edid) else { continue };
            if (e.manufacturer_id.swap_bytes(), e.product_code) != ids {
                continue;
            }
            let slot =
                matches!((bus, c.bus_path.as_deref()), (Some(a), Some(b)) if same_slot(a, b));
            if best.as_ref().is_none_or(|(_, _, s)| !*s && slot) {
                best = Some((c, e, slot));
            }
        }
        best.map(|(c, e, _)| (c, e))
    }

    /// The sink chain of an HDMI/DP PCM.
    pub(crate) fn sink_view(&self, data: Option<&CardData>, card: u32, device: u32) -> SinkView {
        let layer = LayerKind::SinkChain;
        let fail = |reason: UnknownReason, detail: String| SinkView {
            record: None,
            layer: LayerCapabilities {
                layer,
                availability: unknown_layer(reason, detail),
                caps: CapabilitySet::default(),
                detail: None,
            },
            correlation: None,
        };
        if !self.field(FieldGroup::Sink) {
            return fail(
                UnknownReason::NotRequested,
                "the sink field group is not in scope".into(),
            );
        }
        let (eld, eld_src) = match self.eld_for(data, card, device) {
            Ok(x) => x,
            Err((r, d)) => {
                let d = if self.res.sysfs_timed_out || self.res.alsa_timed_out {
                    return fail(UnknownReason::Timeout, "backend deadline".into());
                } else {
                    d
                };
                return fail(r, d);
            }
        };
        let (mfg, product, eld_name) = eld.ids();
        let edid = self.edid_for(data, (mfg, product));
        let p_eld = prov(layer, eld_src.clone(), self.now);
        let p_edid = edid.as_ref().map(|(c, _)| {
            prov(
                layer,
                Source::file(format!("/sys/class/drm/{}/edid", c.name)),
                self.now,
            )
        });

        // Identity.
        let pnp = pnp_id(mfg.swap_bytes());
        let model = edid
            .as_ref()
            .and_then(|(_, e)| e.name.clone())
            .or(eld_name.clone());
        let serial = edid.as_ref().and_then(|(c, _)| edid_serial(&c.edid));
        let id_src = p_edid.clone().unwrap_or_else(|| p_eld.clone());
        let sink_key = match (&pnp, &serial) {
            (Some(m), Some(s)) => format!(
                "{m}-{product:04x}-{}",
                audiocaps_model::redact::stable_hash(s).trim_start_matches("sha256:")
            ),
            (Some(m), None) => format!("{m}-{product:04x}"),
            (None, _) => format!("{mfg:04x}-{product:04x}"),
        };
        let identity = SinkIdentity {
            manufacturer_id: match &pnp {
                Some(m) => Value::measured(m.clone(), id_src.clone()),
                None => Value::unknown(UnknownReason::ParseError, Some("invalid PNP id".into())),
            },
            manufacturer_name: match pnp.as_deref().and_then(ids::pnp_manufacturer) {
                Some(n) => Value::inferred(
                    n.to_string(),
                    prov(layer, Source::Table { name: "pnp".into() }, self.now),
                ),
                None => Value::unknown(
                    UnknownReason::NotExposed,
                    Some("manufacturer id not in the name table".into()),
                ),
            },
            product_code: Value::measured(product, id_src.clone()),
            model_name: match &model {
                Some(m) => Value::measured(m.clone(), id_src.clone()),
                None => Value::unknown(UnknownReason::NotExposed, None),
            },
            serial: serial.map(|s| Sensitive::new(SensitiveKind::Serial, s)),
            sink_key: Value::inferred(
                sink_key,
                prov(
                    layer,
                    Source::derived("manufacturer, product and serial"),
                    self.now,
                ),
            ),
        };

        // Audio descriptors: raw ELD, else EDID, else ELD text.
        let (sads, sads_src): (Vec<CodecCapability>, _) = match (&eld, &edid) {
            (EldSrc::Raw(e), _) => (
                e.sads.iter().map(Sad::to_capability).collect(),
                p_eld.clone(),
            ),
            (EldSrc::Text(_), Some((_, e))) => (
                e.sads().map(Sad::to_capability).collect(),
                p_edid.clone().unwrap_or_else(|| p_eld.clone()),
            ),
            (EldSrc::Text(t), None) => {
                (t.sads.iter().map(text_capability).collect(), p_eld.clone())
            }
        };
        let spk_mask: Option<u16> = match &eld {
            EldSrc::Raw(e) => Some(u16::from(e.speaker_allocation)),
            EldSrc::Text(t) => edid
                .as_ref()
                .and_then(|(_, e)| e.speaker_allocation())
                .map(|b| u16::from(b[0]) | (u16::from(b[1]) << 8))
                .or(t.speakers),
        };
        let sync = match &eld {
            EldSrc::Raw(e) => e.audio_sync_delay_ms,
            EldSrc::Text(t) => t.audio_sync_delay_ms,
        };
        let link = edid.as_ref().map(|(_, e)| {
            let h = e.hdmi();
            let lat = |l: Option<&Latency>| match l {
                Some(Latency::Ms(v)) => Some(*v),
                _ => None,
            };
            Value::measured(
                HdmiLink {
                    physical_address: h.map(|h| h.physical_address),
                    max_tmds_mhz: e
                        .hdmi_forum()
                        .map(|f| u32::from(f.max_tmds_mhz))
                        .filter(|v| *v > 0)
                        .or_else(|| h.and_then(|h| h.max_tmds_mhz).map(u32::from)),
                    max_frl_gbps: e.hdmi_forum().and_then(|f| f.max_frl_gbps()),
                    audio_latency_ms: lat(h.and_then(|h| h.audio_latency_ms.as_ref())),
                    video_latency_ms: lat(h.and_then(|h| h.video_latency_ms.as_ref())),
                    supports_ai: h.map(|h| h.supports_ai),
                },
                p_edid.clone().unwrap_or_else(|| p_eld.clone()),
            )
        });

        // CEC.
        let phys = link
            .as_ref()
            .and_then(|l| l.known())
            .and_then(|l| l.physical_address);
        let (cec, downstream) = self.cec_for(phys);

        // Quirks: the sink itself and whatever CEC found behind it.
        let mut quirks: Vec<AppliedQuirk> = vec![];
        if let Some(q) = &self.res.quirks {
            let mut keys = vec![SinkKey {
                manufacturer: pnp.as_deref(),
                product_code: Some(product),
                model_names: model.iter().map(String::as_str).collect(),
                cec_vendor: None,
            }];
            if let Some(Value::Known { value: bus, .. }) = &cec {
                for d in &bus.devices {
                    keys.push(SinkKey {
                        manufacturer: None,
                        product_code: None,
                        model_names: d.osd_name.iter().map(String::as_str).collect(),
                        cec_vendor: d.vendor_id.as_deref(),
                    });
                }
            }
            for k in keys {
                for a in q.matching(&k) {
                    if !quirks.iter().any(|x| x.id == a.id) {
                        quirks.push(a);
                    }
                }
            }
        }

        // The sink layer: what the sink accepts, with quirk corrections.
        let mut accepted = sads.clone();
        for q in &quirks {
            for e in &q.effects {
                match e {
                    QuirkEffect::AddCodec { codec }
                        if !accepted.iter().any(|c| c.codec == codec.codec) =>
                    {
                        accepted.push(codec.clone())
                    }
                    QuirkEffect::RemoveCodec { codec } => accepted.retain(|c| &c.codec != codec),
                    _ => {}
                }
            }
        }
        let quirked = quirks.iter().any(|q| {
            q.effects.iter().any(|e| {
                matches!(
                    e,
                    QuirkEffect::AddCodec { .. } | QuirkEffect::RemoveCodec { .. }
                )
            })
        });
        let caps_value = |v| {
            if quirked {
                Value::inferred(
                    v,
                    prov(
                        layer,
                        Source::Quirk {
                            id: quirks
                                .iter()
                                .map(|q| q.id.as_str())
                                .collect::<Vec<_>>()
                                .join(","),
                        },
                        self.now,
                    ),
                )
            } else {
                Value::measured(v, sads_src.clone())
            }
        };
        let lpcm: Vec<&CodecCapability> =
            accepted.iter().filter(|c| c.codec == Codec::Lpcm).collect();
        let mut caps = CapabilitySet::default();
        if self.field(FieldGroup::Passthrough) {
            caps.passthrough = Some(caps_value(
                accepted
                    .iter()
                    .filter(|c| c.codec != Codec::Lpcm)
                    .cloned()
                    .collect(),
            ));
        }
        if self.field(FieldGroup::Channels)
            && let Some(max) = lpcm.iter().filter_map(|c| c.max_channels).max()
        {
            caps.channels = Some(Value::measured(
                Range {
                    min: 2,
                    max: u32::from(max),
                },
                sads_src.clone(),
            ));
        }
        if self.field(FieldGroup::Rates) && !lpcm.is_empty() {
            let mut r: Vec<u32> = lpcm.iter().flat_map(|c| c.rates_hz.clone()).collect();
            r.sort_unstable();
            r.dedup();
            caps.rates = Some(Value::measured(
                RateSet::Discrete { rates_hz: r },
                sads_src.clone(),
            ));
        }

        let record = SinkRecord {
            identity,
            audio_descriptors: Value::measured(sads, sads_src.clone()),
            // 0: the sink states none. 0xffff in the kernel's text is its stand-in for "none".
            speaker_allocation: match spk_mask.filter(|m| *m != 0 && *m != 0xffff) {
                Some(m) => Value::measured(speakers(m), sads_src.clone()),
                None => Value::unknown(
                    UnknownReason::NotExposed,
                    Some("the sink states no speaker allocation".into()),
                ),
            },
            sync_delay_ms: match sync {
                Some(v) => Value::measured(v, p_eld.clone()),
                None => Value::unknown(
                    UnknownReason::NotExposed,
                    Some("the sink states no audio sync delay".into()),
                ),
            },
            downstream,
            link,
            cec,
            quirks,
        };
        let correlation = edid.as_ref().map(|(c, _)| CorrelationKey {
            id: CorrelationId::DrmConnector {
                name: c.name.clone(),
            },
            confidence: Confidence::Inferred,
        });
        SinkView {
            record: Some(record),
            layer: LayerCapabilities {
                layer,
                availability: Availability::Read,
                caps,
                detail: None,
            },
            correlation,
        }
    }

    /// The CEC bus whose adapter sits at `phys`, and the devices behind the sink.
    fn cec_for(&self, phys: Option<u16>) -> (Option<Value<CecBus>>, Value<Vec<SinkIdentity>>) {
        let not_requested = || {
            Value::unknown(
                UnknownReason::NotRequested,
                Some("devices behind the sink are read over CEC, which the application did not request".into()),
            )
        };
        let Some(outcome) = &self.res.cec else {
            return (None, not_requested());
        };
        let (dump, live) = match outcome {
            Outcome::Read { value, live } => (value, *live),
            Outcome::Unknown { reason, detail } => {
                return (
                    Some(Value::unknown(*reason, Some(detail.clone()))),
                    Value::unknown(*reason, Some(detail.clone())),
                );
            }
        };
        let adapter = dump
            .adapters
            .iter()
            .find(|a| a.skipped.is_none() && phys.is_some() && a.physical_address == phys)
            .or_else(|| {
                let ok: Vec<_> = dump
                    .adapters
                    .iter()
                    .filter(|a| a.skipped.is_none())
                    .collect();
                (ok.len() == 1).then(|| ok[0])
            });
        let Some(a) = adapter else {
            let why = dump
                .adapters
                .iter()
                .find_map(|a| a.skipped.clone())
                .unwrap_or_else(|| "no CEC adapter on this connector".into());
            return (
                Some(Value::unknown(UnknownReason::NotExposed, Some(why.clone()))),
                Value::unknown(UnknownReason::NotExposed, Some(why)),
            );
        };
        let src = if live {
            Source::api("linux-cec", format!("/dev/{}", a.node))
        } else {
            Source::command(crate::cec::CAPTURE_NAME)
        };
        let p = prov(LayerKind::SinkChain, src, self.now);
        let bus = CecBus {
            adapter: a.driver.clone(),
            physical_address: a.physical_address,
            own_logical_addresses: a.logical_addresses.clone(),
            devices: a.devices.clone(),
        };
        // Behind a TV: audio systems (ARC/eARC receivers, soundbars).
        let downstream: Vec<SinkIdentity> = a
            .devices
            .iter()
            .filter(|d| d.device_type == "audio_system")
            .map(|d| SinkIdentity {
                manufacturer_id: Value::unknown(
                    UnknownReason::NotExposed,
                    Some("CEC carries a vendor OUI, not a PNP id".into()),
                ),
                manufacturer_name: match &d.vendor {
                    Some(v) => Value::inferred(
                        v.clone(),
                        prov(
                            LayerKind::SinkChain,
                            Source::Table {
                                name: "cec-vendor".into(),
                            },
                            self.now,
                        ),
                    ),
                    None => Value::unknown(UnknownReason::NotExposed, d.vendor_id.clone()),
                },
                product_code: Value::unknown(UnknownReason::NotExposed, None),
                model_name: match &d.osd_name {
                    Some(n) => Value::measured(n.clone(), p.clone()),
                    None => Value::unknown(UnknownReason::NotExposed, None),
                },
                serial: None,
                sink_key: Value::inferred(
                    format!(
                        "cec-{}-{}",
                        d.vendor_id.as_deref().unwrap_or("unknown"),
                        d.osd_name.as_deref().unwrap_or("unnamed").replace(' ', "_")
                    ),
                    prov(
                        LayerKind::SinkChain,
                        Source::derived("CEC vendor and OSD name"),
                        self.now,
                    ),
                ),
            })
            .collect();
        (
            Some(Value::measured(bus, p.clone())),
            Value::measured(downstream, p),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_mask() {
        assert_eq!(
            speakers(0x0b),
            vec![
                Speaker::Fl,
                Speaker::Fr,
                Speaker::Lfe,
                Speaker::Rl,
                Speaker::Rr
            ]
        );
    }

    #[test]
    fn serial_from_edid() {
        let mut e = vec![0u8; 128];
        e[12..16].copy_from_slice(&1234u32.to_le_bytes());
        assert_eq!(edid_serial(&e).as_deref(), Some("1234"));
        e[12..16].fill(0);
        assert_eq!(edid_serial(&e), None);
    }
}
