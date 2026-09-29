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
    profiles: Arc<std::sync::Mutex<Vec<crate::ports::system::PowerProfile>>>,
    /// Simulated processor temperature (°C).
    temperature: Option<f64>,
}

impl SystemMock {
    pub fn new() -> Self {
        Self::default()
    }

    /// A board that can wake itself up (Raspberry Pi 5).
    pub fn with_wake() -> Self {
        Self { wake: true, ..Self::default() }
    }

    /// A board reporting this processor temperature.
    pub fn with_temperature(mut self, celsius: f64) -> Self {
        self.temperature = Some(celsius);
        self
    }

    /// Wake-up times programmed so far.
    pub fn wakes(&self) -> Vec<chrono::DateTime<chrono::Utc>> {
        self.wakes.lock().unwrap().clone()
    }

    /// Power profiles applied so far.
    pub fn profiles(&self) -> Vec<crate::ports::system::PowerProfile> {
        self.profiles.lock().unwrap().clone()
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

    async fn set_power_profile(&self, profile: crate::ports::system::PowerProfile) -> Result<(), SystemError> {
        self.profiles.lock().unwrap().push(profile);
        Ok(())
    }

    fn can_wake(&self) -> bool {
        self.wake
    }

    fn temperature_c(&self) -> Option<f64> {
        self.temperature
    }

    async fn schedule_wake(&self, at: chrono::DateTime<chrono::Utc>) -> Result<(), SystemError> {
        if !self.wake {
            return Err(SystemError::WakeUnsupported);
        }
        self.wakes.lock().unwrap().push(at);
        Ok(())
    }
}
