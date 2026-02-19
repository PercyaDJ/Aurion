use async_trait::async_trait;
use std::process::Command;
use tracing::{info, warn};

use crate::ports::system::{SystemError, SystemPort};

/// Raspberry Pi system adapter — real shutdown.
pub struct SystemRpi;

impl SystemRpi {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SystemPort for SystemRpi {
    async fn shutdown(&self) -> Result<(), SystemError> {
        info!("SystemRpi: initiating system shutdown...");

        let output = Command::new("sudo")
            .args(["shutdown", "-h", "now"])
            .output()
            .map_err(|e| SystemError::ShutdownFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("SystemRpi: shutdown command returned error: {}", stderr);
        }

        Ok(())
    }
}
