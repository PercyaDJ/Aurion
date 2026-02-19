use async_trait::async_trait;
use crate::core::models::StorageInfo;

/// Port for external storage operations (USB drive).
/// Implementations: StorageMock (PC), StorageRpi (Raspberry Pi).
#[async_trait]
pub trait StoragePort: Send + Sync {
    /// Mount the external storage device.
    async fn mount(&self) -> Result<(), StorageError>;

    /// Unmount the external storage device.
    async fn unmount(&self) -> Result<(), StorageError>;

    /// Get current storage info (total, free, mount point).
    fn info(&self) -> Result<StorageInfo, StorageError>;

    /// Save a file to storage.
    async fn save_file(&self, path: &str, data: &[u8]) -> Result<(), StorageError>;

    /// Sync all pending writes to disk.
    async fn sync(&self) -> Result<(), StorageError>;

    /// Check if storage is mounted and available.
    fn is_available(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Storage not mounted")]
    NotMounted,
    #[error("Mount failed: {0}")]
    MountFailed(String),
    #[error("Write failed: {0}")]
    WriteFailed(String),
    #[error("Disk full")]
    DiskFull,
    #[error("IO error: {0}")]
    IoError(String),
}
