// SPDX-License-Identifier: LGPL-2.1-or-later
//! Software codec check (opt-in): which codec libraries the application could load.
//!
//! Library directories are listed (never loaded): the app's own runtime when sandboxed (`/app/lib`,
//! `$SNAP/usr/lib`), then the system's. Each known library maps to what its standard builds decode
//! and encode, so the codec lists are `inferred`; a distribution that strips codecs from its build
//! (e.g. a patent-free FFmpeg) can differ, which is why the provider and version are reported too.

use audiocaps_model::{BluetoothCodec, Codec, CodecProvider, SoftwareCodecs};

use crate::root::SystemRoot;

/// Directories searched, in order. `{triplet}` is the multiarch triplet.
const DIRS: &[&str] = &[
    "/app/lib",
    "/app/lib/{triplet}",
    "/usr/lib/{triplet}",
    "/usr/lib64",
    "/usr/lib",
    "/lib/{triplet}",
];

fn triplet() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64-linux-gnu",
        "arm" => "arm-linux-gnueabihf",
        _ => "x86_64-linux-gnu",
    }
}

struct Known {
    name: &'static str,
    /// File name prefix (soname without version).
    prefix: &'static str,
    /// Subdirectory under the library directory.
    sub: &'static str,
    kind: &'static str,
    decodes: &'static [Codec],
    encodes: &'static [Codec],
    bluetooth: &'static [BluetoothCodec],
}

use BluetoothCodec as B;
use Codec as C;

const KNOWN: &[Known] = &[
    Known {
        name: "ffmpeg",
        prefix: "libavcodec.so.",
        sub: "",
        kind: "library",
        decodes: &[
            C::Ac3,
            C::Eac3,
            C::TrueHd,
            C::Dts,
            C::DtsHd,
            C::Aac,
            C::Mp3,
            C::Mpeg1,
            C::Mpeg2,
        ],
        encodes: &[C::Ac3, C::Eac3, C::Aac],
        bluetooth: &[],
    },
    Known {
        name: "alsa-a52",
        prefix: "libasound_module_pcm_a52.so",
        sub: "alsa-lib",
        kind: "alsa-plugin",
        decodes: &[],
        encodes: &[C::Ac3],
        bluetooth: &[],
    },
    Known {
        name: "liba52",
        prefix: "liba52",
        sub: "",
        kind: "library",
        decodes: &[C::Ac3],
        encodes: &[],
        bluetooth: &[],
    },
    Known {
        name: "libdca",
        prefix: "libdca.so.",
        sub: "",
        kind: "library",
        decodes: &[C::Dts],
        encodes: &[],
        bluetooth: &[],
    },
    Known {
        name: "gstreamer-libav",
        prefix: "libgstlibav.so",
        sub: "gstreamer-1.0",
        kind: "gstreamer",
        decodes: &[C::Ac3, C::Eac3, C::TrueHd, C::Dts, C::DtsHd, C::Aac],
        encodes: &[C::Ac3, C::Aac],
        bluetooth: &[],
    },
    Known {
        name: "pipewire-bluez5-aac",
        prefix: "libspa-codec-bluez5-aac.so",
        sub: "spa-0.2/bluez5",
        kind: "pipewire",
        decodes: &[],
        encodes: &[],
        bluetooth: &[B::Aac],
    },
    Known {
        name: "pipewire-bluez5-aptx",
        prefix: "libspa-codec-bluez5-aptx.so",
        sub: "spa-0.2/bluez5",
        kind: "pipewire",
        decodes: &[],
        encodes: &[],
        bluetooth: &[B::AptX, B::AptXHd, B::AptXLl],
    },
    Known {
        name: "pipewire-bluez5-ldac",
        prefix: "libspa-codec-bluez5-ldac.so",
        sub: "spa-0.2/bluez5",
        kind: "pipewire",
        decodes: &[],
        encodes: &[],
        bluetooth: &[B::Ldac],
    },
    Known {
        name: "pipewire-bluez5-lc3",
        prefix: "libspa-codec-bluez5-lc3.so",
        sub: "spa-0.2/bluez5",
        kind: "pipewire",
        decodes: &[],
        encodes: &[],
        bluetooth: &[B::Lc3],
    },
    Known {
        name: "pipewire-bluez5-opus",
        prefix: "libspa-codec-bluez5-opus.so",
        sub: "spa-0.2/bluez5",
        kind: "pipewire",
        decodes: &[],
        encodes: &[],
        bluetooth: &[B::Opus],
    },
];

/// Lists codec libraries. `None` when no library directory is readable (e.g. a fixture, which
/// doesn't capture them).
pub fn scan(root: &SystemRoot) -> Option<SoftwareCodecs> {
    let t = triplet();
    let snap = std::env::var("SNAP").ok().filter(|s| s.starts_with('/'));
    let mut dirs: Vec<String> = DIRS.iter().map(|d| d.replace("{triplet}", t)).collect();
    if let Some(s) = snap {
        dirs.insert(0, format!("{s}/usr/lib/{t}"));
    }
    let mut out = SoftwareCodecs::default();
    let mut any_dir = false;
    for d in &dirs {
        for k in KNOWN {
            let dir = if k.sub.is_empty() {
                d.clone()
            } else {
                format!("{d}/{}", k.sub)
            };
            let Ok(entries) = root.read_dir(&dir) else {
                continue;
            };
            // A directory counts only when it holds libraries (a capture has /usr/lib/os-release).
            any_dir |= entries.iter().any(|e| e.name.contains(".so"));
            let Some(e) = entries.iter().find(|e| e.name.starts_with(k.prefix)) else {
                continue;
            };
            if out.providers.iter().any(|p| p.name == k.name) {
                continue;
            }
            let version = e.name[k.prefix.len()..]
                .trim_start_matches('.')
                .trim_start_matches("so.")
                .to_string();
            out.providers.push(CodecProvider {
                name: k.name.into(),
                library: format!("{dir}/{}", e.name),
                version: (!version.is_empty()
                    && version.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
                .then_some(version),
                decodes: k.decodes.to_vec(),
                encodes: k.encodes.to_vec(),
                bluetooth: k.bluetooth.to_vec(),
                kind: k.kind.into(),
            });
        }
    }
    if !any_dir {
        return None;
    }
    for p in &out.providers {
        for c in &p.decodes {
            if !out.decoders.contains(c) {
                out.decoders.push(c.clone());
            }
        }
        for c in &p.encodes {
            if !out.encoders.contains(c) {
                out.encoders.push(c.clone());
            }
        }
    }
    Some(out)
}
