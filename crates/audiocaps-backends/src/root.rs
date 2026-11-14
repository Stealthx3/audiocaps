// SPDX-License-Identifier: LGPL-2.1-or-later
//! [`SystemRoot`]: every reader takes paths relative to a root, either the live system (`/`) or a
//! captured fixture tree. Readers never open device nodes: paths under `/dev` are refused here,
//! which is part of the "passive by default" guarantee.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::{CapturedCommand, Manifest};

/// Kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Regular file.
    File,
    /// Directory.
    Dir,
    /// Symbolic link.
    Symlink,
    /// Anything else (sockets, device nodes); never read.
    Other,
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// File name.
    pub name: String,
    /// Kind.
    pub kind: EntryKind,
}

/// The system being probed.
#[derive(Debug, Clone)]
pub enum SystemRoot {
    /// The running system.
    Live,
    /// A captured fixture tree (an extracted capture).
    Fixture(FixtureRoot),
}

/// A fixture directory: `manifest.json`, `root/` (files) and `commands/` (command outputs).
#[derive(Debug, Clone)]
pub struct FixtureRoot {
    dir: PathBuf,
    root: PathBuf,
    name: String,
    manifest: Manifest,
}

impl FixtureRoot {
    /// Opens a fixture directory and reads its manifest.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref().canonicalize()?;
        let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let root = dir.join("root");
        Ok(Self {
            dir,
            root,
            name,
            manifest,
        })
    }

    /// Fixture name (directory name).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The capture manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn resolve(&self, path: &str) -> io::Result<PathBuf> {
        let rel = relative(path)?;
        Ok(self.root.join(rel))
    }

    /// Ensures a resolved path stays inside the fixture root after following symlinks.
    fn contained(&self, p: &Path) -> io::Result<PathBuf> {
        let c = p.canonicalize()?;
        let root = self.root.canonicalize()?;
        if c.starts_with(&root) {
            Ok(c)
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes the fixture root",
            ))
        }
    }
}

/// Validates an absolute path and returns it relative to `/`, refusing `..` and `/dev`.
fn relative(path: &str) -> io::Result<PathBuf> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be absolute",
        ));
    }
    let mut rel = PathBuf::new();
    for c in p.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(x) => rel.push(x),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path must be normalized",
                ));
            }
        }
    }
    if rel.starts_with("dev") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "device nodes are never opened by readers",
        ));
    }
    Ok(rel)
}

impl SystemRoot {
    /// Opens a fixture directory.
    pub fn fixture(dir: impl AsRef<Path>) -> io::Result<Self> {
        FixtureRoot::open(dir).map(Self::Fixture)
    }

    /// Reads a whole file.
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        match self {
            Self::Live => {
                let rel = relative(path)?;
                fs::read(Path::new("/").join(rel))
            }
            Self::Fixture(f) => fs::read(f.contained(&f.resolve(path)?)?),
        }
    }

    /// Reads a whole file as UTF-8 (lossy).
    pub fn read_string(&self, path: &str) -> io::Result<String> {
        self.read(path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Whether a path exists (following symlinks).
    pub fn exists(&self, path: &str) -> bool {
        match self {
            Self::Live => relative(path)
                .map(|r| Path::new("/").join(r).exists())
                .unwrap_or(false),
            Self::Fixture(f) => f
                .resolve(path)
                .map(|p| f.contained(&p).is_ok())
                .unwrap_or(false),
        }
    }

    /// Whether `path` is a Unix socket (following symlinks). Fixtures hold no sockets, so there it
    /// means the path exists (captures record socket paths as placeholders).
    pub fn is_socket(&self, path: &str) -> bool {
        use std::os::unix::fs::FileTypeExt;
        match self {
            Self::Live => relative(path)
                .ok()
                .and_then(|r| fs::metadata(Path::new("/").join(r)).ok())
                .is_some_and(|m| m.file_type().is_socket()),
            Self::Fixture(_) => self.exists(path),
        }
    }

    /// Lists a directory, sorted by name. Entry kinds are not followed through symlinks.
    pub fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let dir = match self {
            Self::Live => Path::new("/").join(relative(path)?),
            Self::Fixture(f) => f.contained(&f.resolve(path)?)?,
        };
        let mut out = Vec::new();
        for e in fs::read_dir(dir)? {
            let e = e?;
            let ft = e.file_type()?;
            let kind = if ft.is_symlink() {
                EntryKind::Symlink
            } else if ft.is_dir() {
                EntryKind::Dir
            } else if ft.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            out.push(DirEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                kind,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Reads a symlink target as stored.
    pub fn read_link(&self, path: &str) -> io::Result<String> {
        let p = match self {
            Self::Live => Path::new("/").join(relative(path)?),
            Self::Fixture(f) => f.resolve(path)?,
        };
        Ok(fs::read_link(p)?.to_string_lossy().into_owned())
    }

    /// Whether `/dev/snd` can be listed (the sandbox exposes sound devices). Lists the directory
    /// only; no device node is opened. Unknown for a fixture.
    pub fn dev_snd_visible(&self) -> Option<bool> {
        match self {
            Self::Live => Some(fs::read_dir("/dev/snd").is_ok()),
            Self::Fixture(_) => None,
        }
    }

    /// Output of a command captured with a fixture. The library never runs commands itself, so
    /// this is always `None` on the live system.
    pub fn captured_command(&self, name: &str) -> Option<(CapturedCommand, Option<Vec<u8>>)> {
        let Self::Fixture(f) = self else { return None };
        let cmd = f.manifest.commands.iter().find(|c| c.name == name)?.clone();
        let out = cmd.output_file.as_ref().and_then(|file| {
            let p = f.dir.join("commands").join(file);
            f.contained_commands(&p).and_then(fs::read).ok()
        });
        Some((cmd, out))
    }

    /// The probe clock: now for the live system, the capture time for a fixture.
    pub fn now_ms(&self) -> u64 {
        match self {
            Self::Live => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            Self::Fixture(f) => f.manifest.captured_at_ms,
        }
    }
}

impl FixtureRoot {
    fn contained_commands(&self, p: &Path) -> io::Result<PathBuf> {
        let c = p.canonicalize()?;
        if c.starts_with(self.dir.join("commands")) {
            Ok(c)
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes the fixture",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_dev_and_relative_paths() {
        let r = SystemRoot::Live;
        assert_eq!(
            r.read("/dev/snd/pcmC0D0p").unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            r.read("/dev/../dev/snd/x").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            r.read("proc/asound/cards").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(r.read_dir("/dev/snd").is_err());
    }
}
