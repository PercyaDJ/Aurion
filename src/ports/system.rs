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

    /// Night power profile (CPU frequency, unused network port).
    async fn set_power_profile(&self, _profile: PowerProfile) -> Result<(), SystemError> {
        Ok(())
    }

    /// Program the power-on time used after the next shutdown.
    async fn schedule_wake(&self, _at: chrono::DateTime<chrono::Utc>) -> Result<(), SystemError> {
        Err(SystemError::WakeUnsupported)
    }
}

/// Energy profile during the night.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    /// Watching the sky: one small photo per minute, CPU at minimum.
    Watch,
    /// Capturing: normal CPU frequency (each frame is encoded).
    Capture,
}

impl PowerProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            PowerProfile::Watch => "watch",
            PowerProfile::Capture => "capture",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("Shutdown failed: {0}")]
    ShutdownFailed(String),
    #[error("Réveil programmé impossible sur cette carte")]
    WakeUnsupported,
}
