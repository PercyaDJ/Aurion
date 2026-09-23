use async_trait::async_trait;
use std::time::Duration;
use tracing::info;

use crate::ports::system::{SystemError, SystemPort};

/// Raspberry Pi system adapter — real shutdown through the privileged helper.
pub struct SystemRpi;

impl SystemRpi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemRpi {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SystemPort for SystemRpi {
    async fn shutdown(&self) -> Result<(), SystemError> {
        info!("SystemRpi: initiating system shutdown...");
        crate::sys::helper(&["shutdown"], None, Duration::from_secs(30))
            .await
            .map(|_| ())
            .map_err(SystemError::ShutdownFailed)
    }

    async fn set_power_profile(&self, profile: crate::ports::system::PowerProfile) -> Result<(), SystemError> {
        crate::sys::helper(&["power-profile", profile.as_str()], None, Duration::from_secs(10))
            .await
            .map(|_| ())
            .map_err(SystemError::ShutdownFailed)
    }

    fn can_wake(&self) -> bool {
        crate::sys::rtc_info().wake_capable
    }

    async fn schedule_wake(&self, at: chrono::DateTime<chrono::Utc>) -> Result<(), SystemError> {
        let epoch = at.timestamp().to_string();
        info!("SystemRpi: wake-up alarm at {}", at);
        crate::sys::helper(&["rtc-wake", &epoch], None, Duration::from_secs(10))
            .await
            .map(|_| ())
            .map_err(SystemError::ShutdownFailed)
    }
}
