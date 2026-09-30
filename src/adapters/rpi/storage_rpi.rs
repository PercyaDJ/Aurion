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

    /// The key is mounted. Without it the capture directory is a plain
    /// folder of the SD card: writing there would fill the system.
    fn key_mounted(&self) -> bool {
        crate::sys::storage_health(std::path::Path::new(&self.mount_point)).is_mountpoint
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
        // Not mounted: the free space measured would be the SD card's
        if !self.key_mounted() {
            return Err(StorageError::IoError(format!("aucune clé USB montée sur {}", self.mount_point)));
        }
        let (total_bytes, free_bytes) = crate::sys::disk_usage(std::path::Path::new(&self.mount_point))
            .ok_or_else(|| StorageError::IoError(format!("{} inaccessible", self.mount_point)))?;
        Ok(StorageInfo { total_bytes, free_bytes, mount_point: self.mount_point.clone() })
    }

    async fn save_file(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        // Key missing or pulled out: never write the images to the SD card
        if !self.key_mounted() {
            return Err(StorageError::WriteFailed(format!("aucune clé USB montée sur {}", self.mount_point)));
        }
        let full_path = std::path::Path::new(&self.mount_point).join(path);
        // Up to ~20 MB written and fsynced to a USB key that can be slow:
        // on a blocking thread, never on the async workers
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || write_durably(&full_path, &data))
            .await
            .map_err(|e| StorageError::WriteFailed(e.to_string()))?
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

/// Write a file so that after a power cut it is either complete or absent.
fn write_durably(full_path: &std::path::Path, data: &[u8]) -> Result<(), StorageError> {
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
    std::fs::rename(&tmp, full_path).map_err(to_err)?;
    if let Some(parent) = full_path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nothing_is_written_to_the_sd_card_without_a_key() {
        // A plain folder (not a mount point) stands for /mnt/capture without key
        let dir = tempfile::tempdir().unwrap();
        let storage = StorageRpi::new(dir.path().to_string_lossy().to_string());
        assert!(storage.save_file("sessions/n/JPG/a.jpg", b"jpeg").await.is_err());
        assert!(!dir.path().join("sessions").exists(), "no folder created on the card");
        assert!(storage.info().is_err(), "the SD card free space is not the key's");
    }

    #[tokio::test]
    async fn a_file_is_written_whole_on_a_mounted_key() {
        // /dev/shm: a writable mount point on any Linux machine
        let shm = std::path::Path::new("/dev/shm");
        if !crate::sys::storage_health(shm).is_mountpoint {
            return;
        }
        let storage = StorageRpi::new("/dev/shm".into());
        let rel = format!("aurion-test-{}/JPG/a.jpg", std::process::id());
        storage.save_file(&rel, b"jpeg bytes").await.unwrap();
        assert_eq!(std::fs::read(shm.join(&rel)).unwrap(), b"jpeg bytes");
        assert!(!shm.join(format!("aurion-test-{}/JPG/.a.jpg.part", std::process::id())).exists());
        std::fs::remove_dir_all(shm.join(format!("aurion-test-{}", std::process::id()))).unwrap();
    }

    #[test]
    fn a_mounted_file_system_is_measured() {
        let storage = StorageRpi::new("/".into());
        assert!(storage.info().is_ok());
    }
}
