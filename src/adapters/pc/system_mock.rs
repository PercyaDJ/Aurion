use async_trait::async_trait;
use tracing::warn;

use crate::ports::system::{SystemError, SystemPort};

/// PC mock system: logs shutdown instead of actually shutting down.
pub struct SystemMock;

impl SystemMock {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SystemPort for SystemMock {
    async fn shutdown(&self) -> Result<(), SystemError> {
        warn!("SystemMock: SHUTDOWN requested (simulated — not actually shutting down)");
        Ok(())
    }
}
