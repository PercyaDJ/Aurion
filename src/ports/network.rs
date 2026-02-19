use async_trait::async_trait;

/// Port for Wi-Fi access point management.
/// Implementations: NetworkMock (PC, no-op), NetworkRpi (hostapd).
#[async_trait]
pub trait NetworkApPort: Send + Sync {
    /// Start the Wi-Fi access point.
    async fn start_ap(&self, ssid: &str, password: &str, channel: u32) -> Result<(), NetworkError>;

    /// Stop the Wi-Fi access point.
    async fn stop_ap(&self) -> Result<(), NetworkError>;

    /// Check if the AP is currently active.
    fn is_ap_active(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Failed to start AP: {0}")]
    StartFailed(String),
    #[error("Failed to stop AP: {0}")]
    StopFailed(String),
}
