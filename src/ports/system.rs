use async_trait::async_trait;

/// Port for system-level operations (shutdown, reboot).
/// Implementations: SystemMock (PC, log only), SystemRpi (real shutdown).
#[async_trait]
pub trait SystemPort: Send + Sync {
    /// Perform a clean system shutdown.
    async fn shutdown(&self) -> Result<(), SystemError>;

    /// The board can power itself on at a given time (RTC alarm, Pi 5).
    fn can_wake(&self) -> bool {
        false
    }

    /// Program the power-on time used after the next shutdown.
    async fn schedule_wake(&self, _at: chrono::DateTime<chrono::Utc>) -> Result<(), SystemError> {
        Err(SystemError::WakeUnsupported)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("Shutdown failed: {0}")]
    ShutdownFailed(String),
    #[error("Réveil programmé impossible sur cette carte")]
    WakeUnsupported,
}
