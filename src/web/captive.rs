//! Captive portal detection.
//!
//! When a phone joins the Aurion Wi-Fi, its OS probes a well-known URL
//! (iOS `/hotspot-detect.html`, Android `/generate_204`, Windows
//! `/connecttest.txt`, Firefox `/canonical.html`...). dnsmasq resolves every
//! domain to the Pi and nftables redirects port 80 to the web server, so
//! answering those probes with a redirect makes the phone open the Aurion
//! interface automatically.

use axum::{extract::State, response::Redirect};

use crate::web::AppState;

/// Address of the Pi on its own hotspot.
pub const HOTSPOT_IP: &str = "192.168.4.1";

pub async fn captive_portal(State(state): State<AppState>) -> Redirect {
    let port = state.config.read().await.web.port;
    Redirect::temporary(&portal_url(port))
}

pub fn portal_url(port: u16) -> String {
    if port == 80 {
        format!("http://{}/", HOTSPOT_IP)
    } else {
        format!("http://{}:{}/", HOTSPOT_IP, port)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn portal_url() {
        assert_eq!(super::portal_url(8080), "http://192.168.4.1:8080/");
        assert_eq!(super::portal_url(80), "http://192.168.4.1/");
    }
}
