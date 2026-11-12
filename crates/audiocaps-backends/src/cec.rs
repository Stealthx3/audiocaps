// SPDX-License-Identifier: LGPL-2.1-or-later
//! CEC (opt-in): who is on the HDMI bus, and what an audio system behind the TV accepts.
//!
//! Runs only when the application asks (`ProbeOptions::cec`). Rules:
//! - only adapters the system already configured are used: an adapter without a claimed logical
//!   address is reported, never configured (claiming an address changes the bus);
//! - the file handle stays in the default mode (initiator, no follower), so nothing that other
//!   CEC users receive is taken away;
//! - only query messages are sent: polls, `<Give Physical Address>`, `<Give OSD Name>`,
//!   `<Give Device Vendor ID>` and, to an audio system, `<Request Short Audio Descriptor>`.
//!   Nothing that switches inputs, changes power or volume, or starts system audio is sent.
//!
//! `/dev/cecN` is opened directly here; it is the one device node the library may open, and only
//! on this opt-in path.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use audiocaps_model::{CecDevice, UnknownReason};
use audiocaps_parse::{cec as pcec, ids};

/// Name of the captured command entry and output file.
pub const CAPTURE_NAME: &str = "cec";

/// What the CEC reader found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CecDump {
    /// Adapters.
    pub adapters: Vec<CecAdapter>,
}

/// One adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CecAdapter {
    /// Device node name, e.g. `cec0`.
    pub node: String,
    /// Driver.
    pub driver: String,
    /// Own physical address.
    pub physical_address: Option<u16>,
    /// Own logical addresses.
    pub logical_addresses: Vec<u8>,
    /// Devices that answered.
    pub devices: Vec<CecDevice>,
    /// Why the bus wasn't queried, when it wasn't.
    pub skipped: Option<String>,
}

/// Lists CEC device nodes (`/dev/cec*`) without opening them.
fn nodes() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/dev")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("cec") && n[3..].bytes().all(|b| b.is_ascii_digit()))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Reads every configured CEC adapter.
pub fn read_live() -> Result<CecDump, (UnknownReason, String)> {
    let nodes = nodes();
    if nodes.is_empty() {
        return Err((
            UnknownReason::NotExposed,
            "no CEC adapter (/dev/cec*); most desktop GPUs have none".into(),
        ));
    }
    let mut dump = CecDump::default();
    for n in nodes {
        dump.adapters.push(read_adapter(&n));
    }
    Ok(dump)
}

fn read_adapter(node: &str) -> CecAdapter {
    use cec_linux::{CecDevice as Dev, CecLogicalAddress as La, CecOpcode as Op};
    let mut a = CecAdapter {
        node: node.to_string(),
        ..CecAdapter::default()
    };
    let dev = match Dev::open(format!("/dev/{node}")) {
        Ok(d) => d,
        Err(e) => {
            a.skipped = Some(format!("open failed: {e}"));
            return a;
        }
    };
    if let Ok(c) = dev.get_capas() {
        a.driver = c.driver().to_string();
    }
    a.physical_address = dev
        .get_phys()
        .ok()
        .map(|p| p.to_num())
        .filter(|p| *p != 0xffff);
    let log = match dev.get_log() {
        Ok(l) => l,
        Err(e) => {
            a.skipped = Some(format!("no logical addresses: {e}"));
            return a;
        }
    };
    a.logical_addresses = log.addresses().iter().map(|l| *l as u8).collect();
    let Some(&from) = log.addresses().first() else {
        a.skipped = Some(
            "the adapter has no logical address; not configuring it (that would change the bus)"
                .into(),
        );
        return a;
    };
    const ALL: [La; 15] = [
        La::Tv,
        La::Record1,
        La::Record2,
        La::Tuner1,
        La::Playback1,
        La::Audiosystem,
        La::Tuner2,
        La::Tuner3,
        La::Playback2,
        La::Record3,
        La::Tuner4,
        La::Playback3,
        La::Backup1,
        La::Backup2,
        La::Specific,
    ];
    for to in ALL {
        if log.addresses().contains(&to) || dev.transmit_poll(from, to).is_err() {
            continue;
        }
        let la = to as u8;
        let ask = |op: Op, reply: Op, data: &[u8]| dev.request_data(from, to, op, data, reply).ok();
        let phys = ask(Op::GivePhysicalAddr, Op::ReportPhysicalAddr, b"")
            .and_then(|p| pcec::physical_address(&p));
        let osd = ask(Op::GiveOsdName, Op::SetOsdName, b"").and_then(|p| pcec::osd_name(&p));
        let vendor =
            ask(Op::GiveDeviceVendorId, Op::DeviceVendorId, b"").and_then(|p| pcec::vendor_id(&p));
        let mut sads = vec![];
        if to == La::Audiosystem {
            for req in pcec::SAD_REQUESTS {
                if let Some(p) = ask(
                    Op::RequestShortAudioDescriptor,
                    Op::ReportShortAudioDescriptor,
                    req,
                ) {
                    for s in pcec::short_audio_descriptors(&p) {
                        let c = s.to_capability();
                        if !sads.contains(&c) {
                            sads.push(c);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        a.devices.push(CecDevice {
            logical_address: la,
            device_type: pcec::device_type(la).into(),
            physical_address: phys,
            osd_name: osd,
            vendor_id: vendor.map(|v| format!("{v:06x}")),
            vendor: vendor.and_then(ids::cec_vendor).map(String::from),
            audio_descriptors: sads,
        });
    }
    a
}
