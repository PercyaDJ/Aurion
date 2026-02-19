use async_trait::async_trait;
use std::process::Command;
use tracing::{info, error};

use crate::core::models::StorageInfo;
use crate::ports::storage::{StorageError, StoragePort};

/// Raspberry Pi storage adapter for USB drive.
/// Handles mount/unmount via Linux mount commands and
/// file I/O to the mounted filesystem.
pub struct StorageRpi {
    mount_point: String,
    device: Option<String>,
}

impl StorageRpi {
    pub fn new(mount_point: String) -> Self {
        Self {
            mount_point,
            device: None,
        }
    }

    /// Auto-detect USB storage device (first /dev/sd* block device).
    fn detect_usb_device(&self) -> Option<String> {
        let output = Command::new("lsblk")
            .args(["-rno", "NAME,TYPE,MOUNTPOINT"])
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == "part" {
                let dev = format!("/dev/{}", parts[0]);
                // Prefer sda1, sdb1, etc. (USB drives)
                if parts[0].starts_with("sd") {
                    info!("StorageRpi: detected USB device {}", dev);
                    return Some(dev);
                }
            }
        }
        None
    }
}

#[async_trait]
impl StoragePort for StorageRpi {
    async fn mount(&self) -> Result<(), StorageError> {
        // Create mount point
        std::fs::create_dir_all(&self.mount_point)
            .map_err(|e| StorageError::MountFailed(e.to_string()))?;

        // Detect USB device
        let device = self
            .detect_usb_device()
            .ok_or_else(|| StorageError::MountFailed("No USB device found".into()))?;

        // Mount
        let output = Command::new("sudo")
            .args(["mount", &device, &self.mount_point])
            .output()
            .map_err(|e| StorageError::MountFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(StorageError::MountFailed(format!(
                "mount failed: {}",
                stderr
            )));
        }

        info!("StorageRpi: mounted {} at {}", device, self.mount_point);
        Ok(())
    }

    async fn unmount(&self) -> Result<(), StorageError> {
        let output = Command::new("sudo")
            .args(["umount", &self.mount_point])
            .output()
            .map_err(|e| StorageError::MountFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("StorageRpi: unmount failed: {}", stderr);
        } else {
            info!("StorageRpi: unmounted {}", self.mount_point);
        }
        Ok(())
    }

    fn info(&self) -> Result<StorageInfo, StorageError> {
        // Use statvfs via df command
        let output = Command::new("df")
            .args(["--output=size,avail", "-B1", &self.mount_point])
            .output()
            .map_err(|e| StorageError::IoError(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        if lines.len() < 2 {
            return Err(StorageError::IoError("Cannot parse df output".into()));
        }

        let parts: Vec<&str> = lines[1].split_whitespace().collect();
        if parts.len() < 2 {
            return Err(StorageError::IoError("Cannot parse df output".into()));
        }

        let total_bytes: u64 = parts[0]
            .parse()
            .map_err(|_| StorageError::IoError("Cannot parse total bytes".into()))?;
        let free_bytes: u64 = parts[1]
            .parse()
            .map_err(|_| StorageError::IoError("Cannot parse free bytes".into()))?;

        Ok(StorageInfo {
            total_bytes,
            free_bytes,
            mount_point: self.mount_point.clone(),
        })
    }

    async fn save_file(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let full_path = std::path::Path::new(&self.mount_point).join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
        }

        std::fs::write(&full_path, data)
            .map_err(|e| StorageError::WriteFailed(e.to_string()))?;

        Ok(())
    }

    async fn sync(&self) -> Result<(), StorageError> {
        Command::new("sync")
            .output()
            .map_err(|e| StorageError::IoError(e.to_string()))?;
        info!("StorageRpi: filesystem synced");
        Ok(())
    }

    fn is_available(&self) -> bool {
        std::path::Path::new(&self.mount_point).exists()
            && Command::new("mountpoint")
                .arg("-q")
                .arg(&self.mount_point)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }
}
