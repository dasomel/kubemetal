//! D53: read-only, bounded snapshots; absence of a registered attempt proves no liveness.
use std::collections::HashMap;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

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
    pub observed_at_unix_ms: u64,
    pub entries: Vec<StagingEntry>,
    pub errors: Vec<String>,
    pub truncated: bool,
}

// fdopendir owns its descriptor. Enumeration and openat stay anchored even if a
// concurrent actor renames a directory or replaces its former path with a link.
struct Directory(*mut libc::DIR);
impl Directory {
    fn new(file: File) -> io::Result<Self> {
        let fd = file.into_raw_fd();
        // SAFETY: ownership of this live descriptor transfers only on success.
        let dir = unsafe { libc::fdopendir(fd) };
        if dir.is_null() {
            let error = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            Err(error)
        } else {
            Ok(Self(dir))
        }
    }
    fn next(&self) -> io::Result<Option<OsString>> {
        loop {
            // SAFETY: self owns a live DIR; errno is thread-local. Copy d_name
            // before the next readdir invalidates its borrowed storage.
            let name = unsafe {
                #[cfg(target_os = "macos")]
                let errno = libc::__error();
                #[cfg(not(target_os = "macos"))]
                let errno = libc::__errno_location();
                *errno = 0;
                let entry = libc::readdir(self.0);
                if entry.is_null() {
                    return if *errno == 0 {
                        Ok(None)
                    } else {
                        Err(io::Error::last_os_error())
                    };
                }
                CStr::from_ptr((*entry).d_name.as_ptr()).to_bytes().to_vec()
            };
            if name != b"." && name != b".." {
                return Ok(Some(OsString::from_vec(name)));
            }
        }
    }
    fn open(&self, name: &OsStr) -> io::Result<File> {
        let name = CString::new(name.as_bytes())?;
        // SAFETY: dirfd is borrowed from a live DIR; name is NUL terminated.
        let fd = unsafe {
            libc::openat(
                libc::dirfd(self.0),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a newly owned descriptor.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // SAFETY: self uniquely owns this DIR and its descriptor.
        unsafe { libc::closedir(self.0) };
    }
}

fn open_root(root: &Path) -> io::Result<File> {
    // Canonicalize the server-owned parent (macOS HOME/tmp may have aliases),
    // then refuse symlinks on every component while acquiring directory handles.
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::other("Missing staging parent"))?;
    let canonical = fs::canonicalize(parent)?.join(
        root.file_name()
            .ok_or_else(|| io::Error::other("Missing staging name"))?,
    );
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")?;
    for component in canonical.components() {
        if let Component::Normal(name) = component {
            file = Directory::new(file)?.open(name)?;
            if !file.metadata()?.is_dir() {
                return Err(io::Error::other("Staging path must be a real directory"));
            }
        }
    }
    Ok(file)
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
    while let Some(name) = dir.next().map_err(|e| e.to_string())? {
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
    let mut report = StagingInventory {
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
    let file = match open_root(&root) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(report),
        Err(e) => {
            report.errors.push(format!("Cannot read staging root: {e}"));
            return Ok(report);
        }
    };
    let dir = match Directory::new(file) {
        Ok(dir) => dir,
        Err(e) => {
            report
                .errors
                .push(format!("Cannot enumerate staging root: {e}"));
            return Ok(report);
        }
    };
    let mut remaining = MAX_ENTRIES;
    loop {
        let name = match dir.next() {
            Ok(Some(name)) => name,
            Ok(None) => break,
            Err(e) => {
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
