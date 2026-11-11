// SPDX-License-Identifier: LGPL-2.1-or-later
//! JACK: sample rate, buffer size and physical ports with their latency.
//!
//! libjack is loaded at run time (jack2's or pipewire-jack's); a missing library is
//! `library_missing`, not an error. The client opens with `JackNoStartServer`, so no server is ever
//! started, and it is never activated, so it processes no audio. The capture tool saves the same
//! record (`commands/jack.json`) for fixtures.

use serde::{Deserialize, Serialize};

use audiocaps_model::UnknownReason;

/// Name of the captured command entry and output file.
pub const CAPTURE_NAME: &str = "jack";

/// What the JACK backend read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JackDump {
    /// Sample rate.
    pub rate: u32,
    /// Buffer size in frames.
    pub buffer_frames: u32,
    /// Physical port groups.
    pub ports: Vec<JackPorts>,
}

/// Physical ports of one client and direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JackPorts {
    /// Client, e.g. `system`.
    pub client: String,
    /// `playback` or `capture`.
    pub direction: String,
    /// Port count.
    pub channels: u32,
    /// Latency range in frames.
    pub latency: Option<(u32, u32)>,
}

/// Reads the running JACK server, if there is one.
pub fn read_live() -> Result<JackDump, (UnknownReason, String)> {
    if let Err(e) = jack::jack_sys::library() {
        return Err((UnknownReason::LibraryMissing, format!("libjack: {e}")));
    }
    // libjack prints to stderr when no server runs; a bundled library must stay quiet.
    jack::set_logger(jack::LoggerType::None);
    let (client, _status) =
        jack::Client::new("audiocaps-probe", jack::ClientOptions::NO_START_SERVER).map_err(
            |e| {
                (
                    UnknownReason::ServiceNotRunning,
                    format!("no JACK server running; not starting one ({e})"),
                )
            },
        )?;
    let mut groups: Vec<JackPorts> = vec![];
    let names = client.ports(
        None,
        Some(jack::jack_sys::FLOAT_MONO_AUDIO),
        jack::PortFlags::IS_PHYSICAL,
    );
    for name in names {
        let Some(port) = client.port_by_name(&name) else {
            continue;
        };
        let flags = port.flags().unwrap_or(jack::PortFlags::empty());
        // A physical input port receives what applications play.
        let (direction, mode) = if flags.contains(jack::PortFlags::IS_INPUT) {
            ("playback", jack::LatencyType::Playback)
        } else {
            ("capture", jack::LatencyType::Capture)
        };
        let owner = name.split(':').next().unwrap_or("").to_string();
        let latency = port.get_latency_range(mode).ok();
        match groups
            .iter_mut()
            .find(|g| g.client == owner && g.direction == direction)
        {
            Some(g) => {
                g.channels += 1;
                if let (Some((a, b)), Some((c, d))) = (g.latency, latency) {
                    g.latency = Some((a.min(c), b.max(d)));
                }
            }
            None => groups.push(JackPorts {
                client: owner,
                direction: direction.into(),
                channels: 1,
                latency,
            }),
        }
    }
    groups.sort_by(|a, b| (&a.client, &a.direction).cmp(&(&b.client, &b.direction)));
    Ok(JackDump {
        rate: client.sample_rate(),
        buffer_frames: client.buffer_size(),
        ports: groups,
    })
}
