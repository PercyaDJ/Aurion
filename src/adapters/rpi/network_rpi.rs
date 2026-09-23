use async_trait::async_trait;
use std::time::Duration;
use tracing::info;

use crate::core::validate;
use crate::ports::network::{NetworkApPort, NetworkError};

/// Raspberry Pi Wi-Fi access point.
///
/// All the work is done by the privileged helper (`aurion-helper ap-start`),
/// which uses NetworkManager when it manages `wlan0` (Raspberry Pi OS
/// Bookworm / Trixie) and falls back to hostapd + dnsmasq otherwise. It also
/// sets up the captive portal (DNS → 192.168.4.1, port 80 → web port).
/// The Wi-Fi password is passed on stdin, never on the command line.
pub struct NetworkRpi;

impl NetworkRpi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NetworkRpi {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkApPort for NetworkRpi {
    async fn start_ap(&self, ssid: &str, password: &str, channel: u32) -> Result<(), NetworkError> {
        validate::validate_ssid(ssid).map_err(NetworkError::StartFailed)?;
        validate::validate_wpa_passphrase(password, 8).map_err(NetworkError::StartFailed)?;
        validate::validate_channel(channel).map_err(NetworkError::StartFailed)?;

        info!("NetworkRpi: starting AP '{}' on channel {}", ssid, channel);
        let channel = channel.to_string();
        crate::sys::helper(&["ap-start", ssid, &channel], Some(password), Duration::from_secs(60))
            .await
            .map_err(NetworkError::StartFailed)?;
        info!("NetworkRpi: AP started — SSID: {}, IP: 192.168.4.1 (captive portal active)", ssid);
        Ok(())
    }

    async fn stop_ap(&self) -> Result<(), NetworkError> {
        info!("NetworkRpi: stopping AP...");
        crate::sys::helper(&["ap-stop"], None, Duration::from_secs(30))
            .await
            .map_err(NetworkError::StopFailed)?;
        info!("NetworkRpi: AP stopped");
        Ok(())
    }

    fn is_ap_active(&self) -> bool {
        std::process::Command::new("iw")
            .args(["dev", "wlan0", "info"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "type AP"))
            .unwrap_or(false)
    }
}
