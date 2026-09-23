use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::info;

use crate::core::models::StorageInfo;
use crate::ports::storage::{StorageError, StoragePort};

/// PC mock storage: writes files to a local output directory.
/// Can simulate disk full scenarios.
pub struct StorageMock {
    output_dir: PathBuf,
    mounted: Mutex<bool>,
    total_bytes: u64,
    used_bytes: Mutex<u64>,
    simulate_full: bool,
}

impl StorageMock {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            mounted: Mutex::new(false),
            total_bytes: 128_000_000_000, // 128 GB simulated
            used_bytes: Mutex::new(10_000_000_000), // 10 GB used
            simulate_full: false,
        }
    }

    /// Create a mock that simulates a nearly-full disk.
    pub fn nearly_full(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            mounted: Mutex::new(false),
            total_bytes: 128_000_000_000,
            used_bytes: Mutex::new(124_000_000_000), // ~97% used
            simulate_full: false,
        }
    }

    /// Create a mock that will become full during operation.
    pub fn will_fill(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            mounted: Mutex::new(false),
            total_bytes: 128_000_000_000,
            used_bytes: Mutex::new(121_000_000_000), // ~94.5% used, will fill soon
            simulate_full: true,
        }
    }
}

#[async_trait]
impl StoragePort for StorageMock {
    async fn mount(&self) -> Result<(), StorageError> {
        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| StorageError::MountFailed(e.to_string()))?;
        *self.mounted.lock().unwrap() = true;
        info!("StorageMock: mounted at {}", self.output_dir.display());
        Ok(())
    }

    async fn unmount(&self) -> Result<(), StorageError> {
        *self.mounted.lock().unwrap() = false;
        info!("StorageMock: unmounted");
        Ok(())
    }

    fn info(&self) -> Result<StorageInfo, StorageError> {
        let used = *self.used_bytes.lock().unwrap();
        Ok(StorageInfo {
            total_bytes: self.total_bytes,
            free_bytes: self.total_bytes.saturating_sub(used),
            mount_point: self.output_dir.to_string_lossy().into(),
        })
    }

    async fn save_file(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !*self.mounted.lock().unwrap() {
            return Err(StorageError::NotMounted);
        }

        let full_path = self.output_dir.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
        }

        // Simulate disk fill
        let data_size = data.len() as u64;
        {
            let mut used = self.used_bytes.lock().unwrap();
            if self.simulate_full {
                *used += data_size * 1000; // Accelerate fill for testing
            } else {
                *used += data_size;
            }
        }

        let info = self.info()?;
        if info.free_bytes == 0 {
            return Err(StorageError::DiskFull);
        }

        std::fs::write(&full_path, data)
            .map_err(|e| StorageError::WriteFailed(e.to_string()))?;

        info!("StorageMock: saved {} ({} bytes)", full_path.display(), data.len());
        Ok(())
    }

    async fn sync(&self) -> Result<(), StorageError> {
        info!("StorageMock: sync (no-op)");
        Ok(())
    }

    fn is_available(&self) -> bool {
        *self.mounted.lock().unwrap()
    }
}
