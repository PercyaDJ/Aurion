use async_trait::async_trait;
use tracing::{info, error};

use crate::core::models::StorageInfo;
use crate::ports::storage::{StorageError, StoragePort};

/// Raspberry Pi storage adapter for the USB capture drive.
///
/// Mounting is automatic (udev rule installed by `install.sh`: any USB key
/// in vfat/exFAT/NTFS is mounted on the capture directory). `mount()` only
/// asks the privileged helper to retry that automount.
pub struct StorageRpi {
    mount_point: String,
}

impl StorageRpi {
    pub fn new(mount_point: String) -> Self {
        Self { mount_point }
    }
}

#[async_trait]
impl StoragePort for StorageRpi {
    async fn mount(&self) -> Result<(), StorageError> {
        crate::sys::helper(&["mount-usb"], None, std::time::Duration::from_secs(30))
            .await
            .map_err(StorageError::MountFailed)?;
        if self.is_available() {
            info!("StorageRpi: USB drive mounted at {}", self.mount_point);
            Ok(())
        } else {
            Err(StorageError::MountFailed("aucune clé USB détectée".into()))
        }
    }

    async fn unmount(&self) -> Result<(), StorageError> {
        crate::sys::helper(&["umount-usb"], None, std::time::Duration::from_secs(30))
            .await
            .map_err(|e| {
                error!("StorageRpi: unmount failed: {}", e);
                StorageError::MountFailed(e)
            })?;
        info!("StorageRpi: unmounted {}", self.mount_point);
        Ok(())
    }

    fn info(&self) -> Result<StorageInfo, StorageError> {
        let (total_bytes, free_bytes) = crate::sys::disk_usage(std::path::Path::new(&self.mount_point))
            .ok_or_else(|| StorageError::IoError(format!("{} inaccessible", self.mount_point)))?;
        Ok(StorageInfo { total_bytes, free_bytes, mount_point: self.mount_point.clone() })
    }

    async fn save_file(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let full_path = std::path::Path::new(&self.mount_point).join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
        }

        // Write to a temporary name then rename: a power cut never leaves a
        // truncated image with a valid name on the drive.
        let file_name = full_path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tmp = full_path.with_file_name(format!(".{}.part", file_name));
        let to_err = |e: std::io::Error| {
            if e.raw_os_error() == Some(libc::ENOSPC) {
                StorageError::DiskFull
            } else {
                StorageError::WriteFailed(e.to_string())
            }
        };
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).map_err(to_err)?;
            f.write_all(data).map_err(to_err)?;
            // Data on the key BEFORE the file gets its final name: after a
            // power cut an image is either complete or absent, never half.
            f.sync_all().map_err(to_err)?;
        }
        std::fs::rename(&tmp, &full_path).map_err(to_err)?;
        if let Some(parent) = full_path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }

    async fn sync(&self) -> Result<(), StorageError> {
        crate::sys::run("sync", &[], std::time::Duration::from_secs(60))
            .await
            .map_err(StorageError::IoError)?;
        info!("StorageRpi: filesystem synced");
        Ok(())
    }

    fn is_available(&self) -> bool {
        let h = crate::sys::storage_health(std::path::Path::new(&self.mount_point));
        h.is_mountpoint && h.writable
    }
}
