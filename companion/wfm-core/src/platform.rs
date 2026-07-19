//! OS-specific filesystem + user helpers shared across the core.
//!
//! Two cross-cutting invariants live here:
//!   * secrets and partial-state files are created at 0600 from the first
//!     syscall (`write_restricted`), never chmod'd after a default-umask
//!     create — there's no window where another local user can read them;
//!   * files written under `~` while running as root via `sudo` are chowned
//!     back to `$SUDO_USER` (`chown_to_real_user`) so the user's file manager
//!     can still read them.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Writes `bytes` to `path`, creating the file at 0o600 from the first
/// syscall on Unix. This avoids the race window where a default-umask
/// (0o644) file exists on disk before a later `restrict_file_perms` call
/// tightens it — a window in which another local user can read the
/// secret content. On Windows file ACLs are user-scoped by default; fall
/// back to plain write there.
pub fn write_restricted(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("creating {} with mode 0600", path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
            .with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(unix)]
pub fn restrict_dir_perms(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
pub fn restrict_dir_perms(_path: &std::path::Path) {}

#[cfg(unix)]
pub fn real_user_home() -> Option<PathBuf> {
    // Resolve $SUDO_USER's home when invoked via sudo so we don't drop the
    // file into /root.
    if unsafe { libc::geteuid() } != 0 {
        return None;
    }
    let user = std::env::var("SUDO_USER").ok()?;
    let c_user = std::ffi::CString::new(user).ok()?;
    unsafe {
        let pw = libc::getpwnam(c_user.as_ptr());
        if pw.is_null() {
            return None;
        }
        let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
        Some(PathBuf::from(dir.to_string_lossy().into_owned()))
    }
}

#[cfg(not(unix))]
pub fn real_user_home() -> Option<PathBuf> {
    None
}

pub fn dirs_home() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.home_dir().to_path_buf().into())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(unix)]
pub fn chown_to_real_user(path: &std::path::Path) {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let Ok(user) = std::env::var("SUDO_USER") else { return };
    let Ok(c_user) = std::ffi::CString::new(user) else { return };
    let Ok(c_path) = std::ffi::CString::new(path.to_string_lossy().into_owned()) else { return };
    unsafe {
        let pw = libc::getpwnam(c_user.as_ptr());
        if !pw.is_null() {
            libc::chown(c_path.as_ptr(), (*pw).pw_uid, (*pw).pw_gid);
        }
    }
}

#[cfg(not(unix))]
pub fn chown_to_real_user(_path: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("wfmcore-test-{}-{}.bin", std::process::id(), name));
        p
    }

    #[cfg(unix)]
    #[test]
    fn write_restricted_creates_file_at_0600_from_first_syscall() {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp_path("perms");
        let _ = std::fs::remove_file(&path);
        write_restricted(&path, b"hello").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file must be created at 0600, not chmod'd later");
        let _ = std::fs::remove_file(&path);
    }
}
