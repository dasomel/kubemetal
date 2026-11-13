//! D53: read-only, bounded snapshots; absence of a registered attempt proves no liveness.
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

mod directory;
use directory::{open_inventory_root, Directory};

const MAX_ENTRIES: usize = 4096;
const MAX_ATTEMPTS: usize = 128;
const MAX_DEPTH: usize = 32;

#[derive(Debug, Serialize)]
pub struct StagingEntry {
    pub id: String,
    pub size_bytes: Option<u64>,
    pub state: &'static str,
    pub reason: &'static str,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StagingInventory {
    pub state: &'static str,
    pub observed_at_unix_ms: u64,
    pub entries: Vec<StagingEntry>,
    pub errors: Vec<String>,
    pub truncated: bool,
}

fn measure(
    file: File,
    remaining: &mut usize,
    depth: usize,
    truncated: &mut bool,
) -> Result<u64, String> {
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err("Unsupported staging entry type".into());
    }
    if depth >= MAX_DEPTH {
        *truncated = true;
        return Err("Staging depth limit reached".into());
    }
    let dir = Directory::new(file).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    while let Some(name) = dir.next().map_err(|e| {
        *truncated = true;
        e.to_string()
    })? {
        if *remaining == 0 {
            *truncated = true;
            return Err("Staging entry limit reached".into());
        }
        *remaining -= 1;
        let child = dir
            .open(&name)
            .map_err(|e| format!("{}: {e}", name.to_string_lossy()))?;
        total = total
            .checked_add(measure(child, remaining, depth + 1, truncated)?)
            .ok_or("Staging size overflow")?;
    }
    Ok(total)
}

fn entry_id(name: &OsStr) -> String {
    name.as_bytes()
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() || b"-_.".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(super) fn scan(
    models: &Path,
    active: &HashMap<std::path::PathBuf, (u64, u64)>,
) -> Result<StagingInventory, String> {
    scan_with(models, active, || {}, |dir| dir.next())
}

fn scan_with(
    models: &Path,
    active: &HashMap<std::path::PathBuf, (u64, u64)>,
    before_open: impl FnOnce(),
    mut next: impl FnMut(&Directory) -> io::Result<Option<OsString>>,
) -> Result<StagingInventory, String> {
    let mut report = StagingInventory {
        state: "unknown",
        observed_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis()
            .try_into()
            .map_err(|_| "Timestamp overflow")?,
        entries: vec![],
        errors: vec![],
        truncated: false,
    };
    let root = super::staging_root(models);
    let file = match open_inventory_root(&root, before_open) {
        Ok(Some(file)) => file,
        Ok(None) => {
            report.state = "not_created";
            return Ok(report);
        }
        Err(e) => {
            report.errors.push(format!("Cannot read staging root: {e}"));
            return Ok(report);
        }
    };
    let dir = match Directory::new(file) {
        Ok(dir) => dir,
        Err(e) => {
            report.truncated = true;
            report
                .errors
                .push(format!("Cannot enumerate staging root: {e}"));
            return Ok(report);
        }
    };
    report.state = "observed";
    let mut remaining = MAX_ENTRIES;
    loop {
        let name = match next(&dir) {
            Ok(Some(name)) => name,
            Ok(None) => break,
            Err(e) => {
                report.truncated = true;
                report
                    .errors
                    .push(format!("Cannot enumerate staging entry: {e}"));
                break;
            }
        };
        if remaining == 0 || report.entries.len() == MAX_ATTEMPTS {
            report.truncated = true;
            report.errors.push("Staging scan limit reached".into());
            break;
        }
        remaining -= 1;
        let opened = dir
            .open(&name)
            .and_then(|file| Ok((file.metadata()?, file)));
        let registered = opened.as_ref().is_ok_and(|(metadata, _)| {
            metadata.is_dir()
                && active.get(&root.join(&name)) == Some(&(metadata.dev(), metadata.ino()))
        });
        let mut entry = StagingEntry {
            id: entry_id(&name),
            size_bytes: None,
            state: if registered { "active" } else { "unknown" },
            reason: if registered {
                "registered_attempt"
            } else {
                "no_durable_liveness_evidence"
            },
            errors: vec![],
        };
        match opened
            .map(|(_, file)| file)
            .map_err(|e| e.to_string())
            .and_then(|file| measure(file, &mut remaining, 0, &mut report.truncated))
        {
            Ok(size) => entry.size_bytes = Some(size),
            Err(e) => {
                entry.errors.push(e);
            }
        }
        report.entries.push(entry);
    }
    report.entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(report)
}

pub(crate) async fn list(models: std::path::PathBuf) -> Result<StagingInventory, String> {
    tokio::task::spawn_blocking(move || {
        let active = super::ACTIVE_ATTEMPTS
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        scan(&models, &active)
    })
    .await
    .map_err(|e| format!("Staging inventory worker failed: {e}"))?
}

#[cfg(test)]
mod tests;
