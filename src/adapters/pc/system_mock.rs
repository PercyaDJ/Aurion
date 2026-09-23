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
    /// Simulate a Raspberry Pi 5 (RTC wake-up alarm).
    wake: bool,
    wakes: Arc<std::sync::Mutex<Vec<chrono::DateTime<chrono::Utc>>>>,
}

impl SystemMock {
    pub fn new() -> Self {
        Self::default()
    }

    /// A board that can wake itself up (Raspberry Pi 5).
    pub fn with_wake() -> Self {
        Self { wake: true, ..Self::default() }
    }

    /// Wake-up times programmed so far.
    pub fn wakes(&self) -> Vec<chrono::DateTime<chrono::Utc>> {
        self.wakes.lock().unwrap().clone()
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

    fn can_wake(&self) -> bool {
        self.wake
    }

    async fn schedule_wake(&self, at: chrono::DateTime<chrono::Utc>) -> Result<(), SystemError> {
        if !self.wake {
            return Err(SystemError::WakeUnsupported);
        }
        self.wakes.lock().unwrap().push(at);
        Ok(())
    }
}
