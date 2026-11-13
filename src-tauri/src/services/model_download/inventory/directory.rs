//! Descriptor-anchored, no-follow staging traversal.
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

// fdopendir owns its descriptor. Enumeration and openat stay anchored even if a
// concurrent actor renames a directory or replaces its former path with a link.
pub(super) struct Directory(*mut libc::DIR);
impl Directory {
    pub(super) fn new(file: File) -> io::Result<Self> {
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
    pub(super) fn next(&self) -> io::Result<Option<OsString>> {
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
    fn entry_exists(&self, name: &OsStr) -> io::Result<bool> {
        let name = CString::new(name.as_bytes())?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: live borrowed dirfd and writable stat storage; no symlink traversal.
        let result = unsafe {
            libc::fstatat(
                libc::dirfd(self.0),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
        // An unlinked parent cannot establish an ordinary, not-yet-created root.
        let mut parent = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: self owns the descriptor and parent points to writable storage.
        if unsafe { libc::fstat(libc::dirfd(self.0), parent.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful fstat initialized parent.
        if unsafe { parent.assume_init() }.st_nlink == 0 {
            return Err(error);
        }
        Ok(false)
    }
    pub(super) fn open(&self, name: &OsStr) -> io::Result<File> {
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

pub(super) fn open_root(root: &Path) -> io::Result<File> {
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

// D53: only absence under an opened, linked app parent means not_created.
// Cost: an extra no-follow stat; escape hatch: unknown on any uncertain open.
pub(super) fn open_inventory_root(
    root: &Path,
    before_open: impl FnOnce(),
) -> io::Result<Option<File>> {
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::other("Missing staging parent"))?;
    let parent = fs::canonicalize(parent)?;
    let dir = Directory::new(open_root(&parent)?)?;
    let name = root
        .file_name()
        .ok_or_else(|| io::Error::other("Missing staging name"))?;
    if !dir.entry_exists(name)? {
        return Ok(None);
    }
    before_open();
    let file = dir.open(name)?;
    if !file.metadata()?.is_dir() {
        return Err(io::Error::other("Staging path must be a real directory"));
    }
    Ok(Some(file))
}
