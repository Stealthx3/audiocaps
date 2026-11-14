// SPDX-License-Identifier: LGPL-2.1-or-later
//! Effective sets and path reports, computed from a snapshot's per-layer capabilities.
//!
//! - **Effective set** per access path: what survives every layer on that path. Layers, far end
//!   first: the sink (HDMI/DP), host hardware, ALSA, and for server paths the sound server (and
//!   PipeWire's allowed graph rates). A layer that couldn't be read is skipped and the set is
//!   `partial`; `limited_by` names the layer that narrowed each field.
//! - **Path report**: the hops from the application to the device with the best LPCM format the
//!   path carries, bit rates, ceilings, latency and losses, and the bottleneck (the hop of the
//!   layer that limited channels or rate first).

use audiocaps_model::{
    AccessPath, Availability, CapabilitySet, Codec, CodecCapability, Confidence, Direction,
    EffectiveSet, Endpoint, FieldGroup, Hop, HopKind, LayerCapabilities, LayerDetail, LayerKind,
    Limit, Loss, LossKind, PathReport, Range, RateSet, SampleFormat, Snapshot, SoundServer,
    StreamFormat, Transport, UnknownReason, Value,
};

use crate::facts::{derived, format_bits};

/// Standard rates, for intersecting continuous ranges with discrete lists.
const RATES: [u32; 15] = [
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176400, 192000, 352800,
    384000, 768000,
];

/// HDMI audio ceiling: 8 channels at 192 kHz, 24-bit (also the HBR carrier's payload).
const HDMI_AUDIO_BPS: u64 = 8 * 192_000 * 24;

fn rate_list(r: &RateSet) -> Vec<u32> {
    match r {
        RateSet::Discrete { rates_hz } => rates_hz.clone(),
        RateSet::Continuous { min_hz, max_hz } => RATES
            .iter()
            .copied()
            .filter(|x| (*min_hz..=*max_hz).contains(x))
            .collect(),
    }
}

fn layer(e: &Endpoint, k: LayerKind) -> Option<&LayerCapabilities> {
    e.layers.iter().find(|l| l.layer == k)
}

fn views(e: &Endpoint) -> Vec<&audiocaps_model::ServerView> {
    match layer(e, LayerKind::SoundServer).and_then(|l| l.detail.as_ref()) {
        Some(LayerDetail::SoundServer(d)) => d.views.iter().collect(),
        _ => vec![],
    }
}

/// The access paths an endpoint can be reached by.
pub fn access_paths(e: &Endpoint) -> Vec<AccessPath> {
    let mut v = vec![];
    if e.id.0.starts_with("alsa:card") && e.id.0.contains("/pcm") {
        v.push(AccessPath::AlsaDirect);
    }
    for view in views(e) {
        let p = match view.server {
            SoundServer::PipeWire => AccessPath::PipeWire,
            SoundServer::PulseAudio => AccessPath::PulseAudio,
            _ => continue,
        };
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

#[derive(Default)]
struct Acc {
    rates: Option<Vec<u32>>,
    channels: Option<(u32, u32)>,
    formats: Option<Vec<SampleFormat>>,
    passthrough: Option<Vec<CodecCapability>>,
    limits: Vec<Limit>,
    partial: bool,
}

impl Acc {
    fn apply(&mut self, k: LayerKind, c: &CapabilitySet, rates_too: bool) {
        let mut any_known = false;
        if rates_too && let Some(Value::Known { value, .. }) = &c.rates {
            any_known = true;
            let r = rate_list(value);
            let next = match &self.rates {
                Some(cur) => cur.iter().copied().filter(|x| r.contains(x)).collect(),
                None => r,
            };
            if self
                .rates
                .as_ref()
                .is_some_and(|cur| next.len() < cur.len())
            {
                self.limits.push(Limit {
                    layer: k,
                    field: FieldGroup::Rates,
                });
            }
            self.rates = Some(next);
        }
        if let Some(Value::Known { value: r, .. }) = &c.channels {
            any_known = true;
            let next = match self.channels {
                Some((lo, hi)) => (lo.max(r.min), hi.min(r.max)),
                None => (r.min, r.max),
            };
            if self.channels.is_some_and(|(_, hi)| next.1 < hi) {
                self.limits.push(Limit {
                    layer: k,
                    field: FieldGroup::Channels,
                });
            }
            self.channels = Some(next);
        }
        if let Some(Value::Known { value: f, .. }) = &c.sample_formats {
            any_known = true;
            let next: Vec<SampleFormat> = match &self.formats {
                Some(cur) => cur.iter().filter(|x| f.contains(x)).cloned().collect(),
                None => f.clone(),
            };
            if self
                .formats
                .as_ref()
                .is_some_and(|cur| next.len() < cur.len())
            {
                self.limits.push(Limit {
                    layer: k,
                    field: FieldGroup::Formats,
                });
            }
            self.formats = Some(next);
        }
        if let Some(Value::Known { value: p, .. }) = &c.passthrough {
            any_known = true;
            let next: Vec<CodecCapability> = match &self.passthrough {
                Some(cur) => cur
                    .iter()
                    .filter(|x| p.iter().any(|y| y.codec == x.codec))
                    .cloned()
                    .collect(),
                None => p.clone(),
            };
            if self
                .passthrough
                .as_ref()
                .is_some_and(|cur| next.len() < cur.len())
            {
                self.limits.push(Limit {
                    layer: k,
                    field: FieldGroup::Passthrough,
                });
            }
            self.passthrough = Some(next);
        }
        if !any_known {
            self.partial = true;
        }
    }
}

fn effective(e: &Endpoint, path: AccessPath, s: &Snapshot) -> (EffectiveSet, Acc) {
    let mut acc = Acc::default();
    let server_path = path != AccessPath::AlsaDirect;
    let hdmi = matches!(e.transport.known(), Some(Transport::HdmiDp));
    let order: &[LayerKind] = if server_path {
        &[
            LayerKind::SinkChain,
            LayerKind::HostHardware,
            LayerKind::AlsaUserspace,
            LayerKind::SoundServer,
        ]
    } else {
        &[
            LayerKind::SinkChain,
            LayerKind::HostHardware,
            LayerKind::AlsaUserspace,
        ]
    };
    for &k in order {
        if k == LayerKind::SinkChain && !hdmi {
            continue;
        }
        let Some(l) = layer(e, k) else { continue };
        if matches!(l.availability, Availability::Unknown { .. }) && k != LayerKind::AlsaUserspace {
            acc.partial = true;
            continue;
        }
        // With a server running, ALSA ranges are unknown (managed); that isn't a gap in the path.
        acc.apply(k, &l.caps, true);
    }
    // ALSA direct to an HDMI pin without HBR: HBR codecs can't pass.
    if !server_path
        && let Some(LayerDetail::HostHardware(d)) = layer(e, LayerKind::HostHardware).and_then(|l| l.detail.as_ref())
        // AMD/ATI HDMI codecs don't state HBR in the pin caps (vendor verb instead).
        && !d
            .codec
            .as_ref()
            .and_then(|c| c.known())
            .is_some_and(|n| n.starts_with("ATI") || n.starts_with("AMD"))
        && let Some(Value::Known { value: pins, .. }) = &d.pins
        && pins.iter().any(|p| p.hdmi || p.display_port)
        && !pins.iter().any(|p| p.hbr)
        && let Some(pt) = &mut acc.passthrough
    {
        let before = pt.len();
        pt.retain(|c| !c.flags.contains(&audiocaps_model::CodecFlag::NeedsHbr));
        if pt.len() < before {
            acc.limits.push(Limit {
                layer: LayerKind::HostHardware,
                field: FieldGroup::Passthrough,
            });
        }
    }
    // PipeWire resamples to its allowed graph rates, also for PulseAudio clients of
    // pipewire-pulse (same graph).
    let pulse_on_pipewire = s.servers.iter().any(|x| {
        x.server == SoundServer::PulseAudio
            && x.info
                .as_ref()
                .and_then(|i| i.known())
                .is_some_and(|i| i.name.contains("PipeWire"))
    });
    if (path == AccessPath::PipeWire || (path == AccessPath::PulseAudio && pulse_on_pipewire))
        && let Some(st) = s.servers.iter().find(|x| x.server == SoundServer::PipeWire)
        && let Some(Value::Known { value: c, .. }) = &st.clock
        && !c.allowed_rates_hz.is_empty()
        && let Some(r) = &mut acc.rates
    {
        let before = r.len();
        r.retain(|x| c.allowed_rates_hz.contains(x));
        if r.len() < before {
            acc.limits.push(Limit {
                layer: LayerKind::SoundServer,
                field: FieldGroup::Rates,
            });
        }
    }
    let p = derived(
        LayerKind::Environment,
        s.taken_at_ms,
        "intersection of the path's layers",
    );
    let caps = CapabilitySet {
        sample_formats: acc.formats.clone().map(|v| Value::inferred(v, p.clone())),
        rates: acc
            .rates
            .clone()
            .map(|r| Value::inferred(RateSet::Discrete { rates_hz: r }, p.clone())),
        channels: acc
            .channels
            .map(|(min, max)| Value::inferred(Range { min, max }, p.clone())),
        passthrough: acc
            .passthrough
            .clone()
            .map(|v| Value::inferred(v, p.clone())),
        ..CapabilitySet::default()
    };
    let availability = if acc.partial {
        Availability::Partial
    } else {
        Availability::Read
    };
    (
        EffectiveSet {
            access_path: path,
            availability,
            caps,
            limited_by: acc.limits.clone(),
        },
        acc,
    )
}

fn unknown<T>() -> Value<T> {
    Value::unknown(UnknownReason::NotExposed, None)
}

fn hop_of(k: LayerKind) -> HopKind {
    match k {
        LayerKind::SinkChain => HopKind::Sink,
        LayerKind::HostHardware | LayerKind::KernelDriver => HopKind::DriverHost,
        LayerKind::AlsaUserspace => HopKind::Alsa,
        LayerKind::SoundServer => HopKind::SoundServer,
        LayerKind::Wireless => HopKind::BluetoothLink,
        LayerKind::Environment => HopKind::WholePath,
    }
}

fn report(e: &Endpoint, path: AccessPath, acc: &Acc, s: &Snapshot) -> PathReport {
    let t = s.taken_at_ms;
    let p = || derived(LayerKind::Environment, t, "path report");
    let best = match (
        acc.channels,
        acc.rates.as_ref().and_then(|r| r.iter().max()),
    ) {
        (Some((_, ch)), Some(rate)) => {
            let bits = acc
                .formats
                .as_ref()
                .and_then(|f| f.iter().filter_map(format_bits).max())
                .unwrap_or(24);
            Some((ch, *rate, bits))
        }
        _ => None,
    };
    let fmt = |codec: Codec| match best {
        Some((ch, rate, _)) => Value::inferred(
            StreamFormat {
                codec,
                sample_format: None,
                rate_hz: rate,
                channels: u8::try_from(ch).unwrap_or(u8::MAX),
                iec61937: None,
            },
            p(),
        ),
        None => unknown(),
    };
    let rate_of = || match best {
        Some((ch, rate, bits)) => {
            Value::inferred(u64::from(ch) * u64::from(rate) * u64::from(bits), p())
        }
        None => unknown(),
    };
    let mut hops = vec![];
    let app = s.app_request.as_ref();
    hops.push(Hop {
        kind: HopKind::Application,
        label: "application".into(),
        format: match app.and_then(|a| Some((a.channels?, a.rate_hz?))) {
            Some((ch, rate)) => Value::measured(
                StreamFormat {
                    codec: app.and_then(|a| a.codec.clone()).unwrap_or(Codec::Lpcm),
                    sample_format: None,
                    rate_hz: rate,
                    channels: ch,
                    iec61937: None,
                },
                p(),
            ),
            None => fmt(Codec::Lpcm),
        },
        bitrate_bps: rate_of(),
        ceiling_bps: unknown(),
        latency_us: unknown(),
        losses: vec![],
    });
    if path != AccessPath::AlsaDirect {
        let api = if path == AccessPath::PipeWire {
            SoundServer::PipeWire
        } else {
            SoundServer::PulseAudio
        };
        let view = views(e).into_iter().find(|v| v.server == api);
        let mut losses = vec![];
        for l in acc
            .limits
            .iter()
            .filter(|l| l.layer == LayerKind::SoundServer)
        {
            let (kind, detail) = match l.field {
                FieldGroup::Rates => (
                    LossKind::Resample,
                    "the server's rates don't cover the hardware's".into(),
                ),
                FieldGroup::Channels => (
                    LossKind::Downmix,
                    "the server's profile carries fewer channels".into(),
                ),
                FieldGroup::Passthrough => (
                    LossKind::CodecDropped,
                    "passthrough not enabled for every codec the sink takes".into(),
                ),
                _ => continue,
            };
            losses.push(Loss {
                kind,
                detail,
                finding: None,
            });
        }
        if let Some(Value::Known { value: v, .. }) = view.and_then(|v| v.volume.as_ref())
            && (v.percent != 100 || v.muted)
        {
            losses.push(Loss {
                kind: LossKind::Volume,
                detail: format!(
                    "server volume {}%{}",
                    v.percent,
                    if v.muted { ", muted" } else { "" }
                ),
                finding: None,
            });
        }
        let latency = s
            .servers
            .iter()
            .find(|x| x.server == api)
            .and_then(|x| x.clock.as_ref())
            .and_then(|c| c.known())
            .and_then(|c| Some(u64::from(c.quantum?) * 1_000_000 / u64::from(c.rate_hz?.max(1))))
            .map(|us| Value::inferred(u32::try_from(us).unwrap_or(u32::MAX), p()))
            .unwrap_or_else(unknown);
        hops.push(Hop {
            kind: HopKind::SoundServer,
            label: view
                .map(|v| v.object.clone())
                .unwrap_or_else(|| format!("{api:?}")),
            bitrate_bps: match view
                .and_then(|v| v.current.as_ref())
                .and_then(|c| c.known())
            {
                Some(c) => Value::measured(
                    u64::from(c.channels)
                        * u64::from(c.rate_hz)
                        * u64::from(format_bits(&c.format).unwrap_or(24)),
                    p(),
                ),
                None => rate_of(),
            },
            format: match view
                .and_then(|v| v.current.as_ref())
                .and_then(|c| c.known())
            {
                Some(c) => Value::measured(
                    StreamFormat {
                        codec: Codec::Lpcm,
                        sample_format: Some(c.format.clone()),
                        rate_hz: c.rate_hz,
                        channels: u8::try_from(c.channels).unwrap_or(u8::MAX),
                        iec61937: None,
                    },
                    p(),
                ),
                None => fmt(Codec::Lpcm),
            },
            ceiling_bps: unknown(),
            latency_us: latency,
            losses,
        });
    }
    let device = match layer(e, LayerKind::AlsaUserspace).and_then(|l| l.detail.as_ref()) {
        Some(LayerDetail::Device(d)) => d.device.clone(),
        _ => e.id.0.clone(),
    };
    hops.push(Hop {
        kind: HopKind::Alsa,
        label: device,
        format: fmt(Codec::Lpcm),
        bitrate_bps: rate_of(),
        ceiling_bps: unknown(),
        latency_us: unknown(),
        losses: vec![],
    });
    let usb_ceiling = match layer(e, LayerKind::HostHardware).and_then(|l| l.detail.as_ref()) {
        Some(LayerDetail::HostHardware(d)) => d
            .usb_speed
            .as_ref()
            .and_then(|v| v.known())
            .and_then(|sp| match sp.as_str() {
                "full" => Some(12_000_000u64),
                "high" => Some(480_000_000),
                "super" | "super_plus" => Some(5_000_000_000),
                _ => None,
            }),
        _ => None,
    };
    hops.push(Hop {
        kind: HopKind::DriverHost,
        label: e
            .transport
            .known()
            .map(|t| format!("{t:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "host".into()),
        format: fmt(Codec::Lpcm),
        bitrate_bps: rate_of(),
        ceiling_bps: usb_ceiling
            .map(|c| Value::inferred(c, p()))
            .unwrap_or_else(unknown),
        latency_us: unknown(),
        losses: vec![],
    });
    let mut sink_latency = None;
    if let Some(sink) = &e.sink {
        let link = sink.link.as_ref().and_then(|l| l.known());
        hops.push(Hop {
            kind: HopKind::Link,
            label: link
                .and_then(|l| l.connector.clone())
                .unwrap_or_else(|| "hdmi".into()),
            format: fmt(Codec::Lpcm),
            bitrate_bps: rate_of(),
            ceiling_bps: Value::inferred(HDMI_AUDIO_BPS, p()),
            latency_us: unknown(),
            losses: vec![],
        });
        let sink_ceiling = sink.audio_descriptors.known().and_then(|v| {
            let l = v.iter().find(|c| c.codec == Codec::Lpcm)?;
            Some(
                u64::from(l.max_channels?)
                    * u64::from(*l.rates_hz.iter().max()?)
                    * u64::from(*l.bit_depths.iter().max().unwrap_or(&16)),
            )
        });
        sink_latency = sink.sync_delay_ms.known().map(|ms| u32::from(*ms) * 1000);
        hops.push(Hop {
            kind: HopKind::Sink,
            label: sink
                .identity
                .model_name
                .known()
                .cloned()
                .unwrap_or_else(|| "sink".into()),
            format: fmt(Codec::Lpcm),
            bitrate_bps: rate_of(),
            ceiling_bps: sink_ceiling
                .map(|c| Value::inferred(c, p()))
                .unwrap_or_else(unknown),
            latency_us: sink_latency
                .map(|v| Value::inferred(v, p()))
                .unwrap_or_else(unknown),
            losses: vec![],
        });
        if let Some(down) = sink.downstream.known().filter(|d| !d.is_empty()) {
            hops.push(Hop {
                kind: HopKind::SinkChain,
                label: down[0]
                    .model_name
                    .known()
                    .cloned()
                    .unwrap_or_else(|| "downstream".into()),
                format: unknown(),
                bitrate_bps: unknown(),
                ceiling_bps: unknown(),
                latency_us: unknown(),
                losses: vec![],
            });
        }
    }
    let bottleneck = acc
        .limits
        .iter()
        .find(|l| matches!(l.field, FieldGroup::Channels | FieldGroup::Rates))
        .and_then(|l| {
            let k = hop_of(l.layer);
            hops.iter().position(|h| h.kind == k)
        })
        .and_then(|i| u16::try_from(i).ok());
    let known: Vec<u32> = hops
        .iter()
        .filter_map(|h| h.latency_us.known().copied())
        .collect();
    let latency_us = if known.is_empty() {
        Value::unknown(
            UnknownReason::NotExposed,
            Some("no hop states its latency".into()),
        )
    } else {
        Value::inferred(known.iter().sum(), p())
    };
    let _ = sink_latency;
    PathReport {
        access_path: path,
        hops,
        bottleneck,
        latency_us,
    }
}

/// Fills effective sets and path reports on every endpoint in scope.
pub fn compute(s: &mut Snapshot) {
    let want_eff = s.scope.fields.includes(&FieldGroup::Effective);
    let want_paths = s.scope.fields.includes(&FieldGroup::Paths);
    if !want_eff && !want_paths {
        return;
    }
    let mut out = vec![];
    for e in &s.endpoints {
        if e.direction == Direction::Midi {
            out.push((vec![], vec![]));
            continue;
        }
        let mut eff = vec![];
        let mut paths = vec![];
        for ap in access_paths(e)
            .into_iter()
            .filter(|a| s.scope.access_paths.includes(a))
        {
            let (set, acc) = effective(e, ap, s);
            if want_paths && e.direction == Direction::Playback {
                paths.push(report(e, ap, &acc, s));
            }
            if want_eff {
                eff.push(set);
            }
        }
        out.push((eff, paths));
    }
    for (e, (eff, paths)) in s.endpoints.iter_mut().zip(out) {
        e.effective = eff;
        e.paths = paths;
    }
    let _ = Confidence::Inferred;
}
