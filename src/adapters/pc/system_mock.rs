use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::warn;

use crate::ports::system::{SystemError, SystemPort};

/// PC mock system: logs shutdown instead of actually shutting down.
/// Counts the calls so tests can assert the end-of-night behaviour.
#[derive(Clone, Default)]
pub struct SystemMock {
    shutdowns: Arc<AtomicUsize>,
}

impl SystemMock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of shutdown requests received.
    pub fn shutdown_count(&self) -> usize {
        self.shutdowns.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SystemPort for SystemMock {
    async fn shutdown(&self) -> Result<(), SystemError> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        warn!("SystemMock: SHUTDOWN requested (simulated — not actually shutting down)");
        Ok(())
    }
}
