use async_trait::async_trait;
use std::sync::Mutex;
use tracing::info;

use crate::ports::network::{NetworkApPort, NetworkError};

/// PC mock network: no-op, just logs AP start/stop.
pub struct NetworkMock {
    active: Mutex<bool>,
}

impl NetworkMock {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(false),
        }
    }
}

#[async_trait]
impl NetworkApPort for NetworkMock {
    async fn start_ap(&self, ssid: &str, _password: &str, channel: u32) -> Result<(), NetworkError> {
        info!("NetworkMock: starting AP '{}' on channel {}", ssid, channel);
        *self.active.lock().unwrap() = true;
        Ok(())
    }

    async fn stop_ap(&self) -> Result<(), NetworkError> {
        info!("NetworkMock: stopping AP");
        *self.active.lock().unwrap() = false;
        Ok(())
    }

    fn is_ap_active(&self) -> bool {
        *self.active.lock().unwrap()
    }
}
