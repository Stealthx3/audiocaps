// SPDX-License-Identifier: LGPL-2.1-or-later
//! `audiocaps` command-line tool.

mod archive;
mod capture;
mod redact;
mod timefmt;

use std::path::PathBuf;
use std::process::ExitCode;

use audiocaps_backends::{ProbeOptions, Runtime, SystemRoot, probe};
use audiocaps_model::{EndpointId, ProbeScope};

const USAGE: &str = "\
audiocaps — AudioCaps command-line tool (phase 7)

USAGE:
  audiocaps capture [--out DIR] [--label NAME] [--cec]
      Save a redacted, passive capture of this system as a .tar.gz (default DIR: ./captures).
      --cec also records the CEC bus (opt-in: sends query messages on HDMI-CEC).
  audiocaps import <capture.tar.gz> <fixture-dir>
      Extract a capture into a fixture directory and check its redaction.
  audiocaps check-redaction <fixture-dir>
      Scan a fixture for MAC or IP addresses that should have been redacted.
  audiocaps redact-fixture <fixture-dir>
      Re-apply the redaction rules to an extracted fixture (after a rule change).
  audiocaps replay <fixture-dir>
      Probe a fixture with the full scope and print the snapshot as JSON.
  audiocaps probe [--endpoint ID]... [--unbounded] [--repeat N] [--quiet] [--cec]
                  [--quirk-dir DIR]... [--rule-dir DIR]... [--app-channels N] [--app-rate HZ]
                  [--app-endpoint ID] [--app-bits N] [--latency-target-us US]
      Probe the live system through the runtime (per-backend deadlines, 150 ms default) and
      print the last snapshot as JSON. --endpoint limits the scope to those endpoints;
      --repeat probes N times and prints each probe's time to stderr; --quiet skips the JSON.
      --cec opts in to CEC queries (sends query messages on HDMI-CEC); --quirk-dir and
      --rule-dir add application pack directories; --app-channels, --app-rate, --app-bits and
      --latency-target-us describe what the app will play (inputs to findings).
  audiocaps watch [--endpoint ID]... [--count N]
      Watch the live system and print one line per update: what triggered it, what changed and
      the time from the event to the new snapshot. Stops after N updates (default: never).
  audiocaps cec-dump
      Query every configured CEC adapter (opt-in: sends query messages on HDMI-CEC) and print
      what answered.
  audiocaps alsa-dump
      Print what the ALSA backend reads on this system (same passive rules as a probe).
  audiocaps server-dump <pipewire|pulse>
  audiocaps server-dump pw-dump <FILE>
  audiocaps server-dump pactl <INFO-FILE> <LIST-FILE>
      Print what the PipeWire or PulseAudio backend reads live (same passive rules as a probe),
      or the same record parsed from `pw-dump` / `pactl -f json` output.
  audiocaps schema
      Print the JSON schema of the snapshot (v1).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("capture") => cmd_capture(&args[1..]),
        Some("import") if args.len() == 3 => cmd_import(&args[1], &args[2]),
        Some("check-redaction") if args.len() == 2 => cmd_check(&args[1]),
        Some("redact-fixture") if args.len() == 2 => archive::redact_fixture(args[1].as_ref())
            .map_err(|e| e.to_string())
            .map(|n| println!("{n} files re-redacted")),
        Some("replay") if args.len() == 2 => SystemRoot::fixture(&args[1])
            .map_err(|e| e.to_string())
            .map(|r| print_snapshot(&r)),
        Some("probe") => cmd_probe(&args[1..]),
        Some("alsa-dump") => {
            let d = audiocaps_backends::alsa::live::dump_all(&SystemRoot::Live);
            println!(
                "{}",
                serde_json::to_string_pretty(&d).expect("dump serializes")
            );
            Ok(())
        }
        Some("server-dump") => cmd_server_dump(&args[1..]),
        Some("cec-dump") => audiocaps_backends::cec::read_live()
            .map(|d| {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&d).expect("dump serializes")
                )
            })
            .map_err(|(_, why)| why),
        Some("watch") => cmd_watch(&args[1..]),
        Some("schema") => {
            print!("{}", audiocaps_model::schema::snapshot_schema());
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("audiocaps: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_snapshot(root: &SystemRoot) {
    let snap = probe(root, &ProbeScope::everything());
    println!(
        "{}",
        serde_json::to_string_pretty(&snap).expect("snapshot serializes")
    );
}

fn cmd_probe(args: &[String]) -> Result<(), String> {
    let mut scope = ProbeScope::everything();
    let mut opts = ProbeOptions::default();
    let mut repeat = 1u32;
    let mut quiet = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--endpoint" => {
                let id = it.next().ok_or("--endpoint needs an id")?;
                scope = scope.with_endpoint(EndpointId(id.clone()));
            }
            "--unbounded" => opts = ProbeOptions::unbounded(),
            "--repeat" => {
                repeat = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--repeat needs a positive number")?
            }
            "--quiet" => quiet = true,
            "--cec" => opts = opts.with_cec(),
            "--quirk-dir" => {
                opts = opts.with_quirk_dir(it.next().ok_or("--quirk-dir needs a directory")?)
            }
            "--rule-dir" => {
                opts = opts.with_rule_dir(it.next().ok_or("--rule-dir needs a directory")?)
            }
            "--app-endpoint" => {
                let id = it.next().ok_or("--app-endpoint needs an id")?;
                let mut app = opts.app.clone().unwrap_or_default();
                app.endpoint = Some(EndpointId(id.clone()));
                opts = opts.with_app(app);
            }
            "--app-channels" | "--app-rate" | "--app-bits" | "--latency-target-us" => {
                let v: u32 = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or(format!("{a} needs a number"))?;
                let mut app = opts.app.clone().unwrap_or_default();
                match a.as_str() {
                    "--app-channels" => app.channels = u8::try_from(v).ok(),
                    "--app-rate" => app.rate_hz = Some(v),
                    "--app-bits" => app.bits = u8::try_from(v).ok(),
                    _ => app.latency_target_us = Some(v),
                }
                opts = opts.with_app(app);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let rt = Runtime::new(SystemRoot::Live).map_err(|e| e.to_string())?;
    let mut last = None;
    for i in 0..repeat {
        let t = std::time::Instant::now();
        let snap = rt.probe(&scope, &opts);
        if repeat > 1 {
            eprintln!(
                "probe {}: {:.2} ms, {} endpoints",
                i + 1,
                t.elapsed().as_secs_f64() * 1000.0,
                snap.endpoints.len()
            );
        }
        last = Some(snap);
    }
    if !quiet && let Some(snap) = last {
        println!(
            "{}",
            serde_json::to_string_pretty(&*snap).expect("snapshot serializes")
        );
    }
    Ok(())
}

fn cmd_watch(args: &[String]) -> Result<(), String> {
    let mut scope = ProbeScope::everything();
    let mut count: Option<u32> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--endpoint" => {
                let id = it.next().ok_or("--endpoint needs an id")?;
                scope = scope.with_endpoint(EndpointId(id.clone()));
            }
            "--count" => {
                count = Some(
                    it.next()
                        .and_then(|n| n.parse().ok())
                        .ok_or("--count needs a number")?,
                )
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let rt = Runtime::new(SystemRoot::Live).map_err(|e| e.to_string())?;
    let watch = rt
        .watch(&scope, &ProbeOptions::default())
        .map_err(|e| e.to_string())?;
    let mut seen = 0u32;
    while let Some(ev) = watch.recv() {
        let initial = ev.diff.from_generation == 0;
        println!(
            "update gen {}: {:?}, {} endpoints, +{} -{} ~{}{}, latency {:.2} ms",
            ev.snapshot.generation,
            ev.causes,
            ev.snapshot.endpoints.len(),
            ev.diff.added.len(),
            ev.diff.removed.len(),
            ev.diff.changed.len(),
            if ev.diff.system_changed {
                " +system"
            } else {
                ""
            },
            ev.latency.as_secs_f64() * 1000.0
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();
        if !initial {
            seen += 1;
        }
        if count.is_some_and(|c| seen >= c) {
            break;
        }
    }
    Ok(())
}

fn cmd_server_dump(args: &[String]) -> Result<(), String> {
    use audiocaps_backends::server::{self, ServerKind, pactl, pwdump};
    let read = |p: &String| -> Result<serde_json::Value, String> {
        let b = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
        serde_json::from_slice(&b).map_err(|e| format!("{p}: {e}"))
    };
    let dump = match args {
        [k] if k == "pipewire" => server::read_live(
            &SystemRoot::Live,
            ServerKind::PipeWire,
            std::time::Duration::from_secs(3),
        )?,
        [k] if k == "pulse" => server::read_live(
            &SystemRoot::Live,
            ServerKind::PulseAudio,
            std::time::Duration::from_secs(3),
        )?,
        [k, f] if k == "pw-dump" => pwdump::parse(&read(f)?).ok_or("not pw-dump output")?,
        [k, i, l] if k == "pactl" => {
            pactl::parse(&read(i)?, Some(&read(l)?)).ok_or("not pactl output")?
        }
        _ => {
            return Err(
                "usage: server-dump <pipewire|pulse> | pw-dump FILE | pactl INFO LIST".into(),
            );
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&dump).expect("dump serializes")
    );
    Ok(())
}

fn cmd_capture(args: &[String]) -> Result<(), String> {
    let mut opts = capture::Options {
        out_dir: PathBuf::from("captures"),
        label: "system".into(),
        cec: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => opts.out_dir = it.next().ok_or("--out needs a directory")?.into(),
            "--label" => opts.label = it.next().ok_or("--label needs a name")?.clone(),
            "--cec" => opts.cec = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let path = capture::capture(&opts).map_err(|e| e.to_string())?;
    println!("{}", path.display());
    Ok(())
}

fn cmd_import(tarball: &str, dest: &str) -> Result<(), String> {
    let n = archive::extract(tarball.as_ref(), dest.as_ref()).map_err(|e| e.to_string())?;
    SystemRoot::fixture(dest).map_err(|e| format!("not a capture: {e}"))?;
    let leaks = archive::check_redaction(dest.as_ref()).map_err(|e| e.to_string())?;
    if !leaks.is_empty() {
        return Err(format!("redaction check failed:\n  {}", leaks.join("\n  ")));
    }
    println!("imported {n} files into {dest}; redaction check passed");
    Ok(())
}

fn cmd_check(dir: &str) -> Result<(), String> {
    let leaks = archive::check_redaction(dir.as_ref()).map_err(|e| e.to_string())?;
    if leaks.is_empty() {
        println!("redaction check passed");
        Ok(())
    } else {
        Err(format!("redaction check failed:\n  {}", leaks.join("\n  ")))
    }
}
