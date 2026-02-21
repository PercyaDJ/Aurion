use async_trait::async_trait;
use std::process::Command;
use tracing::info;

use crate::ports::network::{NetworkApPort, NetworkError};

/// Raspberry Pi network adapter using hostapd for Wi-Fi AP.
///
/// Prerequisites:
/// - hostapd installed: `sudo apt install hostapd`
/// - dnsmasq installed: `sudo apt install dnsmasq`
/// - Proper configuration files in /etc/hostapd/ and /etc/dnsmasq.conf
pub struct NetworkRpi;

impl NetworkRpi {
    pub fn new() -> Self {
        Self
    }

    fn write_hostapd_config(&self, ssid: &str, password: &str, channel: u32) -> Result<(), NetworkError> {
        let config = format!(
            "interface=wlan0\n\
             driver=nl80211\n\
             ssid={}\n\
             hw_mode=g\n\
             channel={}\n\
             wmm_enabled=0\n\
             macaddr_acl=0\n\
             auth_algs=1\n\
             ignore_broadcast_ssid=0\n\
             wpa=2\n\
             wpa_passphrase={}\n\
             wpa_key_mgmt=WPA-PSK\n\
             wpa_pairwise=TKIP\n\
             rsn_pairwise=CCMP\n",
            ssid, channel, password
        );

        std::fs::write("/tmp/aurora_hostapd.conf", &config)
            .map_err(|e| NetworkError::StartFailed(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl NetworkApPort for NetworkRpi {
    async fn start_ap(&self, ssid: &str, password: &str, channel: u32) -> Result<(), NetworkError> {
        info!("NetworkRpi: configuring AP '{}' on channel {}", ssid, channel);

        // Write hostapd config
        self.write_hostapd_config(ssid, password, channel)?;

        // Configure static IP for wlan0
        let _ = Command::new("sudo")
            .args(["ip", "addr", "flush", "dev", "wlan0"])
            .output();
        let _ = Command::new("sudo")
            .args(["ip", "addr", "add", "192.168.4.1/24", "dev", "wlan0"])
            .output();
        let _ = Command::new("sudo")
            .args(["ip", "link", "set", "wlan0", "up"])
            .output();

        // Start hostapd
        let output = Command::new("sudo")
            .args(["hostapd", "-B", "/tmp/aurora_hostapd.conf"])
            .output()
            .map_err(|e| NetworkError::StartFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NetworkError::StartFailed(format!(
                "hostapd failed: {}",
                stderr
            )));
        }

        // Start dnsmasq for DHCP + DNS redirect (captive portal)
        // --address=/#/192.168.4.1 redirects ALL DNS queries to the Pi
        // This makes the OS think there's no internet → triggers captive portal
        let _ = Command::new("sudo")
            .args([
                "dnsmasq",
                "--interface=wlan0",
                "--dhcp-range=192.168.4.10,192.168.4.50,255.255.255.0,24h",
                "--address=/#/192.168.4.1",
                "--no-resolv",
                "--no-hosts",
                "--log-queries",
            ])
            .spawn();

        info!("NetworkRpi: AP started — SSID: {}, IP: 192.168.4.1 (captive portal active)", ssid);
        Ok(())
    }

    async fn stop_ap(&self) -> Result<(), NetworkError> {
        info!("NetworkRpi: stopping AP...");

        // Kill hostapd
        let _ = Command::new("sudo")
            .args(["killall", "hostapd"])
            .output();

        // Kill dnsmasq
        let _ = Command::new("sudo")
            .args(["killall", "dnsmasq"])
            .output();

        // Bring down wlan0
        let _ = Command::new("sudo")
            .args(["ip", "link", "set", "wlan0", "down"])
            .output();

        info!("NetworkRpi: AP stopped");
        Ok(())
    }

    fn is_ap_active(&self) -> bool {
        Command::new("pgrep")
            .arg("hostapd")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
