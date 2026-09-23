//! Thin, testable wrappers around the operating system.
//!
//! - [`helper`] runs the privileged helper script through `sudo -n`. The
//!   service user is only allowed to run that single root-owned script
//!   (see `scripts/aurion-helper`), which re-validates every argument.
//!   Nothing else is executed as root.
//! - [`run`] runs an unprivileged command asynchronously with a timeout.
//! - [`storage_health`] / [`disk_usage`] inspect the capture drive without
//!   spawning processes or writing probe files.

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Installed location of the privileged helper.
pub const HELPER_PATH: &str = "/usr/local/sbin/aurion-helper";

/// Run the privileged helper: `sudo -n aurion-helper <args>`.
/// `stdin` is used for secrets (Wi-Fi passwords) so they never appear in
/// the process list. Returns stdout on success, a readable error otherwise.
pub async fn helper(args: &[&str], stdin: Option<&str>, timeout: Duration) -> Result<String, String> {
    let mut full: Vec<&str> = vec!["-n", HELPER_PATH];
    full.extend_from_slice(args);
    run_with_stdin("sudo", &full, stdin, timeout).await
}

/// Run an unprivileged command with a timeout and return its stdout.
pub async fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    run_with_stdin(program, args, None, timeout).await
}

async fn run_with_stdin(
    program: &str,
    args: &[&str],
    stdin: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| format!("{}: {}", program, e))?;
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(data.as_bytes()).await.map_err(|e| e.to_string())?;
        pipe.write_all(b"\n").await.map_err(|e| e.to_string())?;
        drop(pipe);
    }

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("{}: délai dépassé ({} s)", program, timeout.as_secs()))?
        .map_err(|e| format!("{}: {}", program, e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = if stderr.trim().is_empty() { stdout.trim().to_string() } else { stderr.trim().to_string() };
        Err(if msg.is_empty() { format!("{}: code {:?}", program, output.status.code()) } else { msg })
    }
}

/// State of the capture drive directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageHealth {
    /// The directory exists.
    pub exists: bool,
    /// The directory is the root of a mounted filesystem (a USB drive is
    /// plugged in), as opposed to a plain folder on the SD card.
    pub is_mountpoint: bool,
    /// The current user can write into it (and the filesystem is not
    /// read-only).
    pub writable: bool,
}

/// Inspect the capture directory without writing anything.
pub fn storage_health(path: &Path) -> StorageHealth {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = std::fs::metadata(path) else {
        return StorageHealth { exists: false, is_mountpoint: false, writable: false };
    };
    if !meta.is_dir() {
        return StorageHealth { exists: false, is_mountpoint: false, writable: false };
    }

    let is_mountpoint = match path.canonicalize().ok().and_then(|p| p.parent().map(|pp| pp.to_path_buf())) {
        Some(parent) => std::fs::metadata(&parent)
            .map(|pm| pm.dev() != meta.dev() || pm.ino() == meta.ino())
            .unwrap_or(false),
        None => true, // "/" is always a mount point
    };

    StorageHealth { exists: true, is_mountpoint, writable: is_writable(path) }
}

fn is_writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string for the call duration.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// `(total_bytes, available_bytes)` of the filesystem holding `path`.
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs only writes into the zeroed struct we own.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    // Field widths differ between 32 and 64-bit targets.
    #[allow(clippy::unnecessary_cast)]
    let (frsize, blocks, bavail) = (st.f_frsize as u64, st.f_blocks as u64, st.f_bavail as u64);
    Some((blocks * frsize, bavail * frsize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_of_missing_dir() {
        let h = storage_health(Path::new("/definitely/not/here"));
        assert!(!h.exists && !h.writable && !h.is_mountpoint);
    }

    #[test]
    fn health_of_plain_dir_is_not_a_mountpoint() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("capture");
        std::fs::create_dir(&sub).unwrap();
        let h = storage_health(&sub);
        assert!(h.exists);
        assert!(h.writable);
        assert!(!h.is_mountpoint, "a plain folder must not be reported as a mounted USB drive");
    }

    #[test]
    fn root_is_a_mountpoint() {
        assert!(storage_health(Path::new("/")).is_mountpoint);
    }

    #[test]
    fn disk_usage_works() {
        let (total, avail) = disk_usage(Path::new("/")).unwrap();
        assert!(total > 0 && avail <= total);
        assert!(disk_usage(Path::new("/nope/nope")).is_none());
    }

    #[tokio::test]
    async fn run_reports_failure_and_timeout() {
        assert!(run("true", &[], Duration::from_secs(5)).await.is_ok());
        assert!(run("false", &[], Duration::from_secs(5)).await.is_err());
        assert!(run("does-not-exist-aurion", &[], Duration::from_secs(5)).await.is_err());
        let err = run("sleep", &["5"], Duration::from_millis(200)).await.unwrap_err();
        assert!(err.contains("délai"));
    }

    #[tokio::test]
    async fn run_passes_stdin() {
        let out = run_with_stdin("cat", &[], Some("secret"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.trim(), "secret");
    }
}
