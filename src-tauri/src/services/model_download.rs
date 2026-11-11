//! Per-attempt downloads stay outside the catalog until exclusive, same-volume promotion.
use std::fs;
use std::future::Future;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(0);

struct Attempt {
    staging: PathBuf,
    destination: PathBuf,
}

impl Attempt {
    fn begin(models: &Path, name: &str) -> Result<Self, String> {
        let destination = models.join(name);
        match fs::symlink_metadata(&destination) {
            Ok(_) => {
                return Err(format!(
                    "Model destination already exists: {}",
                    destination.display()
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot inspect model destination: {e}")),
        }
        fs::create_dir_all(models).map_err(|e| format!("Cannot create models root: {e}"))?;
        let staging_root = models.with_file_name("model-downloads");
        match fs::DirBuilder::new().mode(0o700).create(&staging_root) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&staging_root)
                    .map_err(|e| format!("Cannot inspect staging root: {e}"))?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("Download staging root must be a real directory".into());
                }
            }
            Err(e) => return Err(format!("Cannot create staging root: {e}")),
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Cannot timestamp download attempt: {e}"))?
            .as_nanos();
        let staging = staging_root.join(format!(
            "{}-{stamp}-{}",
            std::process::id(),
            NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&staging)
            .map_err(|e| format!("Cannot create download attempt: {e}"))?;
        Ok(Self {
            staging,
            destination,
        })
    }

    fn finish(
        self,
        transfer: Result<(), String>,
        rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
        cleanup: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), String> {
        let result = transfer.and_then(|()| {
            // D22: only an exclusive rename publishes success. Cost: EXDEV fails;
            // retry on the same volume instead of copying or replacing existing data.
            rename(&self.staging, &self.destination)
                .map_err(|e| format!("Model promotion failed: {e}"))
        });
        if let Err(error) = result {
            return match cleanup(&self.staging) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(format!(
                    "{error}; staging cleanup failed ({}): {cleanup_error}",
                    self.staging.display()
                )),
            };
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both C strings are alive and NUL-terminated for the syscall.
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn rename_exclusive(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Exclusive model promotion requires macOS",
    ))
}

pub(crate) async fn download<F, Fut>(
    models: PathBuf,
    name: String,
    transfer: F,
) -> Result<(), String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let attempt = tokio::task::spawn_blocking(move || Attempt::begin(&models, &name))
        .await
        .map_err(|e| format!("Download preparation task failed: {e}"))??;
    // D22: a dropped task may leave this private attempt, never a catalog entry.
    // No Drop deletion: a child may still be exiting; orphan cleanup is a follow-up.
    let result = transfer(attempt.staging.clone()).await;
    tokio::task::spawn_blocking(move || {
        attempt.finish(result, rename_exclusive, |p| fs::remove_dir_all(p))
    })
    .await
    .map_err(|e| format!("Download finalization task failed: {e}"))?
}

#[cfg(test)]
mod tests;
