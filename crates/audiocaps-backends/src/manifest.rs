// SPDX-License-Identifier: LGPL-2.1-or-later
//! Capture manifest (`manifest.json`), shared by the capture tool and fixture replay.

use serde::{Deserialize, Serialize};

/// Current capture format version.
pub const CAPTURE_FORMAT: u32 = 1;

/// Describes one capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Capture format version.
    pub format: u32,
    /// Version of the tool that made the capture.
    pub tool_version: String,
    /// Capture time, ms since the Unix epoch.
    pub captured_at_ms: u64,
    /// Sandbox indicators seen by the capture process (environment variables are not captured).
    #[serde(default)]
    pub sandbox: CapturedSandbox,
    /// Number of files saved under `root/`.
    pub file_count: usize,
    /// Commands run, with results.
    pub commands: Vec<CapturedCommand>,
    /// Redaction applied, one line per rule.
    pub redaction: Vec<String>,
    /// Free-form notes (skipped paths, warnings).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Sandbox indicators at capture time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedSandbox {
    /// `SNAP_NAME` was set.
    pub snap: bool,
    /// `/.flatpak-info` existed.
    pub flatpak: bool,
}

/// One command run during capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedCommand {
    /// Short name, e.g. `pw-dump`.
    pub name: String,
    /// Full argument vector.
    pub argv: Vec<String>,
    /// Exit status, if it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Output file under `commands/`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
    /// Why it was skipped, if it didn't run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}
