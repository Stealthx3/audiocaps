// SPDX-License-Identifier: LGPL-2.1-or-later
//! `audiocaps` command-line tool.

mod archive;
mod capture;
mod redact;
mod timefmt;

use std::path::PathBuf;
use std::process::ExitCode;

use audiocaps_backends::{SystemRoot, probe};
use audiocaps_model::ProbeScope;

const USAGE: &str = "\
audiocaps — AudioCaps command-line tool (phase 1)

USAGE:
  audiocaps capture [--out DIR] [--label NAME]
      Save a redacted, passive capture of this system as a .tar.gz (default DIR: ./captures).
  audiocaps import <capture.tar.gz> <fixture-dir>
      Extract a capture into a fixture directory and check its redaction.
  audiocaps check-redaction <fixture-dir>
      Scan a fixture for MAC or IP addresses that should have been redacted.
  audiocaps replay <fixture-dir>
      Probe a fixture with the full scope and print the snapshot as JSON.
  audiocaps probe
      Probe the live system with the full scope and print the snapshot as JSON.
  audiocaps schema
      Print the JSON schema of the snapshot (v1).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("capture") => cmd_capture(&args[1..]),
        Some("import") if args.len() == 3 => cmd_import(&args[1], &args[2]),
        Some("check-redaction") if args.len() == 2 => cmd_check(&args[1]),
        Some("replay") if args.len() == 2 => SystemRoot::fixture(&args[1])
            .map_err(|e| e.to_string())
            .map(|r| print_snapshot(&r)),
        Some("probe") => {
            print_snapshot(&SystemRoot::Live);
            Ok(())
        }
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

fn cmd_capture(args: &[String]) -> Result<(), String> {
    let mut opts = capture::Options {
        out_dir: PathBuf::from("captures"),
        label: "system".into(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => opts.out_dir = it.next().ok_or("--out needs a directory")?.into(),
            "--label" => opts.label = it.next().ok_or("--label needs a name")?.clone(),
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
