use async_trait::async_trait;

/// Port for system-level operations (shutdown, reboot).
/// Implementations: SystemMock (PC, log only), SystemRpi (real shutdown).
#[async_trait]
pub trait SystemPort: Send + Sync {
    /// Perform a clean system shutdown.
    async fn shutdown(&self) -> Result<(), SystemError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("Shutdown failed: {0}")]
    ShutdownFailed(String),
}
