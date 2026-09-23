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
}
