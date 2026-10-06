//! Fine-tune output lifecycle (issue #33, slice S1): staging directory, durable completion
//! record, verified promotion. Pure service: every path is derived from a caller-supplied
//! `root` (production: `~/.kubemetal`), nothing here takes a global lock or spawns a process.
//!
//! Layout:
//! - `<root>/adapter-staging/<attempt_id>/attempt.json` — written only by Rust (temp file +
//!   fsync + atomic rename), so a reader never observes a partial record.
//! - `<root>/adapter-staging/<attempt_id>/out/` — training output and the promotion source.
//! - `<root>/adapters/<name>` — final adapters. Sibling of `adapter-staging` so the adapter
//!   delete IPC's allowed root never contains staging.
//!
//! Safety rules enforced here: a final name that exists in ANY form is refused (never
//! overwritten or suffixed); promotion is `renamex_np(RENAME_EXCL)` on one filesystem and
//! fails rather than copying; a corrupt or unrecognised record is `Unknown` and protected.
//! Nothing in this module deletes anything.

use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::commands::mlx::validate_adapter_name;
use crate::services::artifact_manifest::{
    sha256_file, verify_manifest, write_manifest, ManifestContext,
};

const STAGING_DIR: &str = "adapter-staging";
const ADAPTERS_DIR: &str = "adapters";
const RECORD_FILE: &str = "attempt.json";
const OUT_DIR: &str = "out";
const MANIFEST_FILE: &str = "manifest.json";
const REQUIRED_FILES: [&str; 2] = ["adapters.safetensors", "adapter_config.json"];
const MAX_NAME_LEN: usize = 128;

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
pub enum StagingError {
    /// The final adapter name is already occupied; the existing path was left untouched.
    NameTaken(String),
    Other(String),
}

impl fmt::Display for StagingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameTaken(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

type Result<T> = std::result::Result<T, StagingError>;

fn other<T>(msg: impl Into<String>) -> Result<T> {
    Err(StagingError::Other(msg.into()))
}

fn io_err(what: &str, path: &Path, e: io::Error) -> StagingError {
    StagingError::Other(format!("{what} {}: {e}", path.display()))
}

/// `interrupted` is deliberately absent: it is derived by `reconcile`, never persisted, so a
/// crash can never leave a stale "interrupted" claim behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Created,
    Running,
    ExitedOk,
    Verified,
    Promoted,
    Failed,
    Killed,
    VerifiedUnpromoted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptRecord {
    pub attempt_id: String,
    pub adapter_name: String,
    pub runtime: String,
    pub base_model: String,
    pub iters: u32,
    pub mlflow_run_id: Option<String>,
    /// Process identity pair, same scheme as the MLX markers (pid + sysinfo start time).
    pub pid: Option<u32>,
    pub start_time: Option<u64>,
    pub manifest_sha256: Option<String>,
    pub state: AttemptState,
}

#[derive(Debug, Clone)]
pub struct AttemptSpec {
    pub adapter_name: String,
    pub runtime: String,
    pub base_model: String,
    pub iters: u32,
    pub mlflow_run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Attempt {
    root: PathBuf,
    // Private: a caller that could set `state = Verified` would skip `verify_out`.
    record: AttemptRecord,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedOut {
    pub manifest_sha256: String,
    pub file_count: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Promoted {
    pub final_path: PathBuf,
    pub warning: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// Record unreadable/corrupt/unrecognised, or inconsistent with the filesystem. Protected.
    Unknown { attempt_id: String, reason: String },
    /// `running` and the recorded process identity is still alive.
    StillRunning { attempt_id: String },
    /// Derived, not persisted: `running` but the process is gone. Staging is kept as a
    /// potential resume source.
    Interrupted { attempt_id: String },
    /// Rename completed but the record still said `verified`; record now says `promoted`.
    ConvergedPromoted { attempt_id: String },
    Unchanged {
        attempt_id: String,
        state: AttemptState,
    },
}

impl Attempt {
    pub fn record(&self) -> &AttemptRecord {
        &self.record
    }

    pub fn dir(&self) -> PathBuf {
        staging_root(&self.root).join(&self.record.attempt_id)
    }

    pub fn out_dir(&self) -> PathBuf {
        self.dir().join(OUT_DIR)
    }

    pub fn final_path(&self) -> PathBuf {
        adapters_root(&self.root).join(&self.record.adapter_name)
    }
}

fn staging_root(root: &Path) -> PathBuf {
    root.join(STAGING_DIR)
}

fn adapters_root(root: &Path) -> PathBuf {
    root.join(ADAPTERS_DIR)
}

/// True for anything under `<root>/adapter-staging`. Fails closed: a `..` component or a path
/// whose canonical form lands under staging (symlink in the way) counts as protected.
pub fn staging_path_is_protected(root: &Path, path: &Path) -> bool {
    let staging = staging_root(root);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
        || path.starts_with(&staging)
    {
        return true;
    }
    match (staging.canonicalize(), canonicalize_existing_prefix(path)) {
        (Ok(staging), Some(resolved)) => resolved.starts_with(staging),
        _ => false,
    }
}

/// Canonicalise the longest existing ancestor and re-append the rest, so a not-yet-created
/// path below a symlinked directory is still resolved.
fn canonicalize_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut tail = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = cur.canonicalize() {
            return Some(tail.iter().rev().fold(real, |acc, c| acc.join(c)));
        }
        tail.push(cur.file_name()?.to_owned());
        cur = cur.parent()?;
    }
}

fn validate_name(name: &str, what: &str) -> Result<()> {
    validate_adapter_name(name).map_err(StagingError::Other)?;
    if name.starts_with('.') || name.len() > MAX_NAME_LEN {
        return other(format!("Invalid {what}: {name}"));
    }
    Ok(())
}

fn st_dev(path: &Path) -> io::Result<u64> {
    fs::symlink_metadata(path).map(|m| m.dev())
}

type DevFn<'a> = &'a dyn Fn(&Path) -> io::Result<u64>;

/// Promotion by rename across filesystems would be a copy (EXDEV) — refuse up front.
fn require_same_device(a: &Path, b: &Path, dev: DevFn) -> Result<()> {
    let da = dev(a).map_err(|e| io_err("Failed to stat", a, e))?;
    let db = dev(b).map_err(|e| io_err("Failed to stat", b, e))?;
    if da != db {
        return other(format!(
            "{} and {} are on different filesystems; promotion would copy",
            a.display(),
            b.display()
        ));
    }
    Ok(())
}

/// Existing real directory, never a symlink; created with 0700 when `create` and absent.
fn ensure_real_dir(path: &Path, create: bool) -> Result<()> {
    for _ in 0..2 {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => {
                return other(format!("{} must not be a symlink", path.display()))
            }
            Ok(m) if m.is_dir() => return Ok(()),
            Ok(_) => return other(format!("{} is not a directory", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                match DirBuilder::new().mode(0o700).create(path) {
                    Ok(()) => {}
                    // Lost a creation race; re-lstat what the winner made.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(io_err("Failed to create", path, e)),
                }
            }
            Err(e) => return Err(io_err("Failed to inspect", path, e)),
        }
    }
    other(format!("{} changed while being created", path.display()))
}

fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| io_err("Failed to fsync", dir, e))
}

fn write_record(dir: &Path, record: &AttemptRecord) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|e| StagingError::Other(format!("Failed to serialize attempt record: {e}")))?;
    let tmp = dir.join(format!(
        ".{RECORD_FILE}.tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, dir.join(RECORD_FILE))
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(io_err("Failed to write attempt record in", dir, e));
    }
    sync_dir(dir)
}

/// Fail closed: anything but a regular `attempt.json` that parses strictly is an error.
fn read_record(dir: &Path) -> Result<AttemptRecord> {
    let path = dir.join(RECORD_FILE);
    let meta = fs::symlink_metadata(&path).map_err(|e| io_err("Failed to inspect", &path, e))?;
    if !meta.is_file() {
        return other(format!("{} is not a regular file", path.display()));
    }
    let bytes = fs::read(&path).map_err(|e| io_err("Failed to read", &path, e))?;
    let record: AttemptRecord = serde_json::from_slice(&bytes).map_err(|e| {
        StagingError::Other(format!("Corrupt attempt record {}: {e}", path.display()))
    })?;
    if dir.file_name().and_then(|n| n.to_str()) != Some(record.attempt_id.as_str()) {
        return other(format!("Attempt id mismatch in {}", path.display()));
    }
    Ok(record)
}

fn new_attempt_id() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(
        "{ms:013}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn create_attempt(root: &Path, spec: &AttemptSpec) -> Result<Attempt> {
    create_attempt_with(root, spec, &new_attempt_id(), &st_dev)
}

fn create_attempt_with(
    root: &Path,
    spec: &AttemptSpec,
    attempt_id: &str,
    dev: DevFn,
) -> Result<Attempt> {
    validate_name(&spec.adapter_name, "adapter_name")?;
    validate_name(attempt_id, "attempt_id")?;
    let final_path = adapters_root(root).join(&spec.adapter_name);
    // lstat: a dangling symlink or an empty dir at the final name still counts as taken.
    match fs::symlink_metadata(&final_path) {
        Ok(_) => {
            return Err(StagingError::NameTaken(format!(
                "Adapter {} already exists",
                spec.adapter_name
            )))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("Failed to inspect", &final_path, e)),
    }
    if !fs::metadata(root)
        .map_err(|e| io_err("Failed to stat", root, e))?
        .is_dir()
    {
        return other(format!("{} is not a directory", root.display()));
    }
    let staging = staging_root(root);
    ensure_real_dir(&adapters_root(root), true)?;
    ensure_real_dir(&staging, true)?;
    require_same_device(&staging, &adapters_root(root), dev)?;

    let dir = staging.join(attempt_id);
    // Exclusive create (never create_dir_all on the leaf): a pre-existing dir or symlink at
    // the attempt path, or a concurrent creator of the same id, loses here.
    DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| io_err("Failed to create attempt directory", &dir, e))?;
    DirBuilder::new()
        .mode(0o700)
        .create(dir.join(OUT_DIR))
        .map_err(|e| io_err("Failed to create out directory in", &dir, e))?;
    let record = AttemptRecord {
        attempt_id: attempt_id.to_string(),
        adapter_name: spec.adapter_name.clone(),
        runtime: spec.runtime.clone(),
        base_model: spec.base_model.clone(),
        iters: spec.iters,
        mlflow_run_id: spec.mlflow_run_id.clone(),
        pid: None,
        start_time: None,
        manifest_sha256: None,
        state: AttemptState::Created,
    };
    write_record(&dir, &record)?;
    Ok(Attempt {
        root: root.to_path_buf(),
        record,
    })
}

fn persist(attempt: &mut Attempt, update: impl FnOnce(&mut AttemptRecord)) -> Result<()> {
    let mut next = attempt.record.clone();
    update(&mut next);
    write_record(&attempt.dir(), &next)?;
    attempt.record = next;
    Ok(())
}

pub fn mark_running(attempt: &mut Attempt, pid: u32, start_time: Option<u64>) -> Result<()> {
    if attempt.record.state != AttemptState::Created {
        return other("Only a created attempt can start running");
    }
    persist(attempt, |r| {
        r.pid = Some(pid);
        r.start_time = start_time;
        r.state = AttemptState::Running;
    })
}

/// Trainer/publication failure transitions only; verified/promoted states are reachable solely through
/// `verify_out` / `promote`, which prove their precondition.
pub fn transition(attempt: &mut Attempt, to: AttemptState) -> Result<()> {
    use AttemptState::*;
    // D45: a verified output can still fail publication; retain all bytes/hash evidence,
    // but record failed so the app never confuses that attempt with completed promotion.
    // No terminal attempt can be revived; retry/recovery is a future explicit operation.
    let ok = matches!(
        (attempt.record.state, to),
        (Running, ExitedOk | Killed)
            | (
                Created | Running | ExitedOk | Verified | VerifiedUnpromoted,
                Failed
            )
    );
    if !ok {
        return other(format!(
            "Illegal transition {:?} -> {to:?}",
            attempt.record.state
        ));
    }
    persist(attempt, |r| r.state = to)
}

/// lstat every entry of `out/`: only top-level regular files pass. The manifest collector
/// skips symlinks silently, so this check cannot be delegated to it.
fn scan_out(out: &Path) -> Result<Vec<String>> {
    let meta = fs::symlink_metadata(out).map_err(|e| io_err("Failed to inspect", out, e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return other(format!("{} must be a real directory", out.display()));
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(out).map_err(|e| io_err("Failed to read", out, e))? {
        let entry = entry.map_err(|e| io_err("Failed to read entry in", out, e))?;
        let path = entry.path();
        let m = fs::symlink_metadata(&path).map_err(|e| io_err("Failed to inspect", &path, e))?;
        if !m.is_file() {
            return other(format!(
                "{} is not a regular file (symlinks, directories and special files are refused)",
                path.display()
            ));
        }
        // A second link can alias content that lives (and can change) outside staging.
        if m.nlink() > 1 {
            return other(format!("{} is hard-linked; refusing", path.display()));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StagingError::Other(format!("Non-UTF-8 name in {}", out.display())))?;
        names.push(name);
    }
    Ok(names)
}

pub fn verify_out(attempt: &mut Attempt) -> Result<VerifiedOut> {
    if attempt.record.state != AttemptState::ExitedOk {
        return other(format!(
            "Cannot verify an attempt in state {:?}",
            attempt.record.state
        ));
    }
    ensure_real_dir(&attempt.dir(), false)?;
    let out = attempt.out_dir();
    let names = scan_out(&out)?;
    for required in REQUIRED_FILES {
        let len = names
            .iter()
            .any(|n| n == required)
            .then(|| fs::symlink_metadata(out.join(required)).map(|m| m.len()))
            .transpose()
            .map_err(|e| io_err("Failed to inspect", &out, e))?;
        if len.unwrap_or(0) == 0 {
            return other(format!("Required output {required} is missing or empty"));
        }
    }
    write_manifest(
        &out,
        ManifestContext {
            runtime: attempt.record.runtime.clone(),
            base_model: attempt.record.base_model.clone(),
        },
    )
    .map_err(StagingError::Other)?;
    // Re-scan: nothing may have appeared (or been swapped for a symlink) while hashing.
    let file_count = scan_out(&out)?.len();
    let report = verify_manifest(&out).map_err(StagingError::Other)?;
    if !report.is_valid() {
        return other(format!("Manifest verification failed: {report:?}"));
    }
    let manifest_sha256 = sha256_file(&out.join(MANIFEST_FILE)).map_err(StagingError::Other)?;
    persist(attempt, |r| {
        r.manifest_sha256 = Some(manifest_sha256.clone());
        r.state = AttemptState::Verified;
    })?;
    Ok(VerifiedOut {
        manifest_sha256,
        file_count,
    })
}

/// `dir` must hold a valid manifest whose own hash is the one recorded at verification.
/// The manifest hash alone proves nothing about the bytes it describes.
fn require_verified_content(dir: &Path, expected_sha256: Option<&str>) -> Result<()> {
    let report = verify_manifest(dir).map_err(StagingError::Other)?;
    if !report.is_valid() {
        return other(format!(
            "{} no longer matches its manifest: {report:?}",
            dir.display()
        ));
    }
    let actual = sha256_file(&dir.join(MANIFEST_FILE)).map_err(StagingError::Other)?;
    if expected_sha256 != Some(actual.as_str()) {
        return other(format!(
            "{} manifest differs from the one that was verified",
            dir.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn rename_excl(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let cstr = |p: &Path| {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    };
    let (from, to) = (cstr(from)?, cstr(to)?);
    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call.
    let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Plain `rename` would silently replace an existing EMPTY directory, so there is no safe
/// fallback: non-macOS builds compile but refuse to promote.
#[cfg(not(target_os = "macos"))]
fn rename_excl(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive rename (renamex_np) is only available on macOS",
    ))
}

/// `_admission` is a witness that the caller holds the adapter-admission lock (S2 passes
/// `&MutexGuard<..>`); this module never acquires the real lock itself.
pub fn promote<G>(attempt: &mut Attempt, _admission: &G) -> Result<Promoted> {
    promote_with(attempt, &st_dev, &|| {}, &|| {})
}

fn promote_with(
    attempt: &mut Attempt,
    dev: DevFn,
    before_rename: &dyn Fn(),
    after_rename: &dyn Fn(),
) -> Result<Promoted> {
    if attempt.record.state != AttemptState::Verified {
        return other(format!(
            "Cannot promote an attempt in state {:?}",
            attempt.record.state
        ));
    }
    validate_name(&attempt.record.adapter_name, "adapter_name")?;
    ensure_real_dir(&attempt.dir(), false)?;
    let out = attempt.out_dir();
    scan_out(&out)?;
    let adapters = adapters_root(&attempt.root);
    ensure_real_dir(&adapters, false)?;
    let final_path = attempt.final_path();
    require_same_device(&out, &adapters, dev)?;
    match fs::symlink_metadata(&final_path) {
        Ok(_) => return name_taken(attempt),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("Failed to inspect", &final_path, e)),
    }
    before_rename();
    // Last check before the whole directory is renamed: whatever is in out/ now is promoted.
    require_verified_content(&out, attempt.record.manifest_sha256.as_deref())?;
    match rename_excl(&out, &final_path) {
        Ok(()) => {}
        Err(e) if matches!(e.raw_os_error(), Some(libc::EEXIST | libc::ENOTEMPTY)) => {
            return name_taken(attempt)
        }
        Err(e) => return Err(io_err("Failed to promote into", &final_path, e)),
    }
    after_rename();
    // The bytes are in place, so a failure below must not surface as Err: the caller would
    // retry and record `VerifiedUnpromoted` next to this attempt's own final directory. The
    // record stays `verified` with out/ gone, which `reconcile` converges to `promoted`.
    let warning = sync_dir(&adapters)
        .and_then(|()| sync_dir(&attempt.dir()))
        .and_then(|()| persist(attempt, |r| r.state = AttemptState::Promoted))
        .err()
        .map(|e| format!("promoted, but the record was not updated: {e}"));
    Ok(Promoted {
        final_path,
        warning,
    })
}

fn name_taken(attempt: &mut Attempt) -> Result<Promoted> {
    let name = attempt.record.adapter_name.clone();
    persist(attempt, |r| r.state = AttemptState::VerifiedUnpromoted)?;
    Err(StagingError::NameTaken(format!(
        "Adapter {name} already exists; staged output kept"
    )))
}

pub fn reconcile(root: &Path) -> Vec<ReconcileOutcome> {
    reconcile_with(root, &|pid, start| {
        crate::services::process::pid_is_alive(pid)
            && start.is_none_or(|s| crate::services::process::process_start_time(pid) == Some(s))
    })
}

fn reconcile_with(
    root: &Path,
    is_alive: &dyn Fn(u32, Option<u64>) -> bool,
) -> Vec<ReconcileOutcome> {
    let staging = staging_root(root);
    let Ok(entries) = fs::read_dir(&staging) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| reconcile_one(root, &staging.join(&name), name, is_alive))
        .collect()
}

/// Only NotFound means "gone"; any other lstat error (EACCES, EIO, ...) is not evidence.
fn out_gone(lstat: io::Result<fs::Metadata>) -> io::Result<bool> {
    match lstat {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e),
    }
}

fn reconcile_one(
    root: &Path,
    dir: &Path,
    attempt_id: String,
    is_alive: &dyn Fn(u32, Option<u64>) -> bool,
) -> ReconcileOutcome {
    let unknown = |reason: String| ReconcileOutcome::Unknown {
        attempt_id: attempt_id.clone(),
        reason,
    };
    if let Err(e) = ensure_real_dir(dir, false) {
        return unknown(e.to_string());
    }
    let record = match read_record(dir) {
        Ok(r) => r,
        Err(e) => return unknown(e.to_string()),
    };
    match record.state {
        AttemptState::Running => match record.pid {
            None => unknown("running record without a pid".into()),
            Some(pid) if is_alive(pid, record.start_time) => {
                ReconcileOutcome::StillRunning { attempt_id }
            }
            Some(_) => ReconcileOutcome::Interrupted { attempt_id },
        },
        AttemptState::Verified => match out_gone(fs::symlink_metadata(dir.join(OUT_DIR))) {
            Ok(false) => ReconcileOutcome::Unchanged {
                attempt_id,
                state: AttemptState::Verified,
            },
            Err(e) => unknown(format!("cannot tell whether out/ is gone: {e}")),
            Ok(true) => {
                let Some(expected) = record.manifest_sha256.clone() else {
                    return unknown("verified record without manifest_sha256".into());
                };
                let final_path = adapters_root(root).join(&record.adapter_name);
                // lstat first: verify_manifest would follow a symlinked final path.
                let is_real_dir = fs::symlink_metadata(&final_path).is_ok_and(|m| m.is_dir());
                if !is_real_dir || require_verified_content(&final_path, Some(&expected)).is_err() {
                    return unknown("out/ is gone but the final adapter does not match".into());
                }
                let mut attempt = Attempt {
                    root: root.to_path_buf(),
                    record,
                };
                match persist(&mut attempt, |r| r.state = AttemptState::Promoted) {
                    Ok(()) => ReconcileOutcome::ConvergedPromoted { attempt_id },
                    Err(e) => unknown(e.to_string()),
                }
            }
        },
        state => ReconcileOutcome::Unchanged { attempt_id, state },
    }
}

#[cfg(test)]
mod tests;
