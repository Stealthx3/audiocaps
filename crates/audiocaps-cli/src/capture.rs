// SPDX-License-Identifier: LGPL-2.1-or-later
//! `audiocaps capture`: saves the files and command outputs that AudioCaps reads into a redacted
//! tarball. Read-only and passive:
//! - only regular files under `/proc`, `/sys`, `/etc` and `/usr/share/alsa` are read, never `/dev`;
//! - commands are client tools that only query (`pw-dump`, `pactl`, `busctl`);
//! - the ALSA dump (`commands/alsa.json`) comes from the library's own ALSA reader, with the same
//!   passive rules as a probe: PCMs are opened non-blocking only when no sound server runs and the
//!   device is free, parameters are never installed, and controls are only read;
//! - `pactl` runs only when a PulseAudio socket exists, with autospawn disabled;
//! - BlueZ is queried only when `org.bluez` already owns its bus name (no D-Bus activation).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audiocaps_backends::manifest::{CAPTURE_FORMAT, CapturedCommand, CapturedSandbox, Manifest};
use audiocaps_parse::edid;

use crate::redact::{RULES, Redactor};

const MAX_FILE: u64 = 1 << 20;
const CMD_TIMEOUT: Duration = Duration::from_secs(5);

/// Attributes copied from sound and DRM device directories and their parents. Serial numbers are
/// deliberately absent.
const DEVICE_ATTRS: &[&str] = &[
    "vendor",
    "device",
    "subsystem_vendor",
    "subsystem_device",
    "class",
    "revision",
    "modalias",
    "uevent",
    "idVendor",
    "idProduct",
    "bcdDevice",
    "bcdUSB",
    // Raw device and configuration descriptors (USB Audio Class layout, no serial strings).
    "descriptors",
    "speed",
    "version",
    "manufacturer",
    "product",
    "bInterfaceClass",
    "bInterfaceSubClass",
    "bInterfaceProtocol",
    "bAlternateSetting",
    "bNumEndpoints",
    "id",
    "number",
    "status",
    "enabled",
    "dpms",
    "edid",
    "power/control",
    "power/runtime_status",
];

const DMI_ATTRS: &[&str] = &[
    "sys_vendor",
    "product_name",
    "product_version",
    "product_family",
    "board_vendor",
    "board_name",
    "board_version",
    "bios_vendor",
    "bios_version",
    "bios_date",
    "chassis_type",
    "chassis_vendor",
];

/// Options for a capture.
#[derive(Debug, Clone)]
pub struct Options {
    /// Directory to write the tarball into.
    pub out_dir: PathBuf,
    /// Label for the tarball name, e.g. `htpc`.
    pub label: String,
    /// Opt-in: record the CEC bus.
    pub cec: bool,
}

struct Staging {
    dir: PathBuf,
    redactor: Redactor,
    files: usize,
    seen: BTreeSet<PathBuf>,
    notes: Vec<String>,
}

impl Staging {
    fn dest(&self, live: &Path) -> PathBuf {
        self.dir
            .join("root")
            .join(live.strip_prefix("/").unwrap_or(live))
    }

    /// Copies one regular file (redacting it), or recreates one symlink with the same target.
    fn copy(&mut self, live: &Path) {
        if !self.seen.insert(live.to_path_buf()) || live.starts_with("/dev") {
            return;
        }
        let Ok(meta) = fs::symlink_metadata(live) else {
            return;
        };
        let dest = self.dest(live);
        if let Some(p) = dest.parent() {
            let _ = fs::create_dir_all(p);
        }
        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(live) {
                let _ = symlink(target, &dest);
            }
            return;
        }
        if !meta.is_file() {
            return;
        }
        let mut buf = Vec::new();
        // procfs/sysfs report size 0 or 4096, so read with a cap instead of trusting the size.
        match fs::File::open(live).and_then(|f| f.take(MAX_FILE).read_to_end(&mut buf)) {
            Ok(_) => {}
            Err(e) => {
                if e.kind() != io::ErrorKind::PermissionDenied {
                    self.notes
                        .push(format!("unreadable: {} ({})", live.display(), e.kind()));
                }
                return;
            }
        }
        let data = if live.file_name().is_some_and(|n| n == "edid") {
            edid::redact_serials(&mut buf);
            buf
        } else if live.file_name().is_some_and(|n| n == "descriptors") {
            // USB descriptors carry string indexes only, never the serial string itself.
            buf
        } else if let Ok(text) = std::str::from_utf8(&buf) {
            self.redactor.text(text).into_bytes()
        } else {
            self.notes.push(format!(
                "binary file not redacted, skipped: {}",
                live.display()
            ));
            return;
        };
        if fs::write(&dest, data).is_ok() {
            self.files += 1;
        }
    }

    /// Copies a directory tree of regular files and symlinks (not following links).
    fn copy_tree(&mut self, live: &Path, depth: u32) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(live) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                self.copy_tree(&p, depth - 1);
            } else if ft.is_file() || ft.is_symlink() {
                self.copy(&p);
            }
        }
    }

    /// Records a /sys/class/<class>/<entry> link and copies device attributes from the target
    /// and up to `parents` parent devices.
    fn copy_device(&mut self, class_entry: &Path, parents: u32) {
        self.copy(class_entry);
        let Ok(real) = fs::canonicalize(class_entry) else {
            return;
        };
        let mut dir = real;
        for level in 0..=parents {
            self.link_chain(&dir);
            for a in DEVICE_ATTRS {
                let p = dir.join(a);
                if p.is_file() {
                    self.copy(&p);
                }
            }
            for l in ["driver", "subsystem", "device"] {
                let p = dir.join(l);
                if p.is_symlink() {
                    self.copy(&p);
                }
            }
            if level == parents {
                break;
            }
            // The card's `device` link points at the parent device (PCI function or USB interface).
            let next = dir.join("device");
            dir = match fs::canonicalize(&next) {
                Ok(d) if next.is_symlink() => d,
                _ => match dir.parent() {
                    Some(p) if p.starts_with("/sys/devices/") => p.to_path_buf(),
                    _ => break,
                },
            };
        }
    }

    /// Ensures intermediate symlinks on the way to `dir` don't exist (sysfs device paths are real
    /// directories), so the fixture tree mirrors the real layout.
    fn link_chain(&mut self, dir: &Path) {
        let _ = fs::create_dir_all(self.dest(dir));
    }
}

/// Runs one query command with a timeout and stores its redacted output.
fn run_command(
    st: &mut Staging,
    name: &str,
    argv: &[&str],
    env: &[(&str, &str)],
) -> CapturedCommand {
    let mut rec = CapturedCommand {
        name: name.to_string(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        exit_code: None,
        output_file: None,
        skipped: None,
    };
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            rec.skipped = Some(format!("not run: {}", e.kind()));
            return rec;
        }
    };
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > CMD_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let out = reader.join().unwrap_or_default();
    match status {
        None => rec.skipped = Some("timed out after 5 s".into()),
        Some(s) => {
            rec.exit_code = s.code();
            let text = String::from_utf8_lossy(&out);
            let (file, data) = match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(mut v) => {
                    st.redactor.json(&mut v);
                    let mut s = serde_json::to_string_pretty(&v).unwrap_or_default();
                    s.push('\n');
                    (format!("{name}.json"), s)
                }
                Err(_) => (format!("{name}.txt"), st.redactor.text(&text)),
            };
            if !data.trim().is_empty()
                && fs::write(st.dir.join("commands").join(&file), data).is_ok()
            {
                rec.output_file = Some(file);
            }
        }
    }
    rec
}

/// Runs the library's ALSA reader in-process and stores its redacted dump.
fn alsa_dump(st: &mut Staging) -> CapturedCommand {
    use audiocaps_backends::alsa::{dump, live};
    let mut rec = CapturedCommand {
        name: dump::CAPTURE_NAME.into(),
        argv: vec!["(in-process)".into(), "audiocaps alsa reader".into()],
        exit_code: Some(0),
        output_file: None,
        skipped: None,
    };
    let d = live::dump_all(&audiocaps_backends::SystemRoot::Live);
    match serde_json::to_value(&d) {
        Ok(mut v) => {
            st.redactor.json(&mut v);
            let mut s = serde_json::to_string_pretty(&v).unwrap_or_default();
            s.push('\n');
            let file = format!("{}.json", dump::CAPTURE_NAME);
            if fs::write(st.dir.join("commands").join(&file), s).is_ok() {
                rec.output_file = Some(file);
            }
        }
        Err(e) => {
            rec.exit_code = None;
            rec.skipped = Some(format!("dump failed: {e}"));
        }
    }
    rec
}

/// Runs the CEC reader when the user opted in.
fn cec_dump(st: &mut Staging, opted_in: bool) -> CapturedCommand {
    use audiocaps_backends::cec;
    let mut rec = CapturedCommand {
        name: cec::CAPTURE_NAME.into(),
        argv: vec!["(in-process)".into(), "audiocaps cec reader".into()],
        exit_code: None,
        output_file: None,
        skipped: None,
    };
    if !opted_in {
        rec.skipped = Some("CEC not requested (capture --cec)".into());
        return rec;
    }
    match cec::read_live() {
        Ok(d) => {
            let mut s = serde_json::to_string_pretty(&d).unwrap_or_default();
            s.push('\n');
            let file = format!("{}.json", cec::CAPTURE_NAME);
            if fs::write(st.dir.join("commands").join(&file), st.redactor.text(&s)).is_ok() {
                rec.exit_code = Some(0);
                rec.output_file = Some(file);
            }
        }
        Err((_, why)) => rec.skipped = Some(why),
    }
    rec
}

/// Runs the library's JACK reader in-process (never starts a server) and stores its record.
fn jack_dump(st: &mut Staging) -> CapturedCommand {
    use audiocaps_backends::jack;
    let mut rec = CapturedCommand {
        name: jack::CAPTURE_NAME.into(),
        argv: vec!["(in-process)".into(), "audiocaps jack reader".into()],
        exit_code: None,
        output_file: None,
        skipped: None,
    };
    match jack::read_live() {
        Ok(d) => {
            let mut s = serde_json::to_string_pretty(&d).unwrap_or_default();
            s.push('\n');
            let file = format!("{}.json", jack::CAPTURE_NAME);
            if fs::write(st.dir.join("commands").join(&file), st.redactor.text(&s)).is_ok() {
                rec.exit_code = Some(0);
                rec.output_file = Some(file);
            }
        }
        Err((_, why)) => rec.skipped = Some(why),
    }
    rec
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn commands(st: &mut Staging, cec: bool) -> Vec<CapturedCommand> {
    let mut out = vec![];
    let skip = |name: &str, why: &str| CapturedCommand {
        name: name.into(),
        argv: vec![],
        exit_code: None,
        output_file: None,
        skipped: Some(why.into()),
    };

    out.push(alsa_dump(st));
    out.push(jack_dump(st));
    out.push(cec_dump(st, cec));
    if which("pw-dump") {
        out.push(run_command(st, "pw-dump", &["pw-dump"], &[]));
    } else {
        out.push(skip("pw-dump", "pw-dump not installed"));
    }

    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let pulse_socket = std::env::var_os("PULSE_SERVER").is_some()
        || (!runtime.is_empty() && Path::new(&runtime).join("pulse/native").exists());
    if !which("pactl") {
        out.push(skip("pactl-info", "pactl not installed"));
        out.push(skip("pactl-list", "pactl not installed"));
    } else if !pulse_socket {
        out.push(skip(
            "pactl-info",
            "no PulseAudio socket; not starting a server",
        ));
        out.push(skip(
            "pactl-list",
            "no PulseAudio socket; not starting a server",
        ));
    } else {
        let conf = st.dir.join("pulse-client.conf");
        let _ = fs::write(&conf, "autospawn = no\n");
        let conf = conf.to_string_lossy().into_owned();
        let env = [("PULSE_CLIENTCONFIG", conf.as_str())];
        out.push(run_command(
            st,
            "pactl-info",
            &["pactl", "-f", "json", "info"],
            &env,
        ));
        out.push(run_command(
            st,
            "pactl-list",
            &["pactl", "-f", "json", "list"],
            &env,
        ));
        let _ = fs::remove_file(st.dir.join("pulse-client.conf"));
    }

    if which("busctl") {
        let has = Command::new("busctl")
            .args([
                "--system",
                "call",
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "NameHasOwner",
                "s",
                "org.bluez",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "b true")
            .unwrap_or(false);
        if has {
            out.push(run_command(
                st,
                "bluez",
                &[
                    "busctl",
                    "--system",
                    "--json=short",
                    "call",
                    "org.bluez",
                    "/",
                    "org.freedesktop.DBus.ObjectManager",
                    "GetManagedObjects",
                ],
                &[],
            ));
        } else {
            out.push(skip("bluez", "org.bluez not running; not activating it"));
        }
    } else {
        out.push(skip("bluez", "busctl not installed"));
    }
    out
}

/// Runs a capture and returns the tarball path.
pub fn capture(opts: &Options) -> io::Result<PathBuf> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let stamp = crate::timefmt::stamp(now_ms / 1000);
    let base = format!("audiocaps-capture-{}-{stamp}", sanitize(&opts.label));
    fs::create_dir_all(&opts.out_dir)?;
    let dir = opts.out_dir.join(format!(".{base}.staging"));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(dir.join("root"))?;
    fs::create_dir_all(dir.join("commands"))?;

    let host = fs::read_to_string("/proc/sys/kernel/hostname").ok();
    let user = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok());
    let mut st = Staging {
        dir: dir.clone(),
        redactor: Redactor::new(host.as_deref(), user.as_deref()),
        files: 0,
        seen: BTreeSet::new(),
        notes: vec![],
    };

    st.copy_tree(Path::new("/proc/asound"), 5);
    // alsa-lib's per-card and PCM definitions, used to resolve the default device's plugin chain.
    st.copy(Path::new("/usr/share/alsa/alsa.conf"));
    // Files hooks load (pulse-alsa.conf and the like) sit next to it.
    if let Ok(rd) = fs::read_dir("/usr/share/alsa") {
        let mut v: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "conf") && p.is_file())
            .collect();
        v.sort();
        for p in v {
            st.copy(&p);
        }
    }
    st.copy_tree(Path::new("/usr/share/alsa/cards"), 1);
    st.copy_tree(Path::new("/usr/share/alsa/pcm"), 1);
    for f in [
        "/proc/sys/kernel/osrelease",
        "/etc/os-release",
        "/usr/lib/os-release",
        "/.flatpak-info",
    ] {
        st.copy(Path::new(f));
    }
    for (class, parents) in [("/sys/class/sound", 2), ("/sys/class/drm", 1)] {
        if let Ok(rd) = fs::read_dir(class) {
            let mut names: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            names.sort();
            for p in names {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // DRM: connectors only (cardN-HDMI-A-1), not render nodes.
                if class.ends_with("drm") && !n.contains('-') {
                    continue;
                }
                st.copy_device(&p, parents);
            }
        }
    }
    for a in DMI_ATTRS {
        st.copy(&Path::new("/sys/class/dmi/id").join(a));
    }
    if let Ok(rd) = fs::read_dir("/sys/module") {
        let mut mods: Vec<_> = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("snd"))
            .map(|e| e.path())
            .collect();
        mods.sort();
        for m in mods {
            st.copy_tree(&m.join("parameters"), 1);
        }
    }

    let cmds = commands(&mut st, opts.cec);
    let manifest = Manifest {
        format: CAPTURE_FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        captured_at_ms: now_ms,
        sandbox: CapturedSandbox {
            snap: std::env::var_os("SNAP_NAME").is_some(),
            flatpak: Path::new("/.flatpak-info").exists(),
        },
        file_count: st.files,
        commands: cmds,
        redaction: RULES.iter().map(|s| s.to_string()).collect(),
        notes: st.notes.clone(),
    };
    let mut m = serde_json::to_string_pretty(&manifest).map_err(io::Error::other)?;
    m.push('\n');
    fs::write(dir.join("manifest.json"), m)?;

    let tarball = opts.out_dir.join(format!("{base}.tar.gz"));
    crate::archive::write(&dir, &base, &tarball)?;
    fs::remove_dir_all(&dir)?;
    Ok(tarball)
}

fn sanitize(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() { "system".into() } else { s }
}
