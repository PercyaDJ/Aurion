//! HTTP hardening middleware.
//!
//! The web interface has no login: whoever is connected to the Aurion Wi-Fi
//! (WPA2) is the operator. Two classic browser attacks still need to be
//! blocked, mainly when the Pi joins a home network in maintenance mode:
//!
//! - **CSRF**: a web page opened on the phone silently POSTs to
//!   `http://192.168.4.1:8080/api/...`. Blocked by checking `Origin` and
//!   `Sec-Fetch-Site` on every state-changing request.
//! - **DNS rebinding**: a malicious domain re-resolves to the Pi IP to read
//!   the API from a web page. Blocked by only accepting `Host` values that
//!   are IP addresses, `localhost`, `*.local` or the machine hostname.

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Strip the port from a `Host` / authority value (IPv6 aware).
fn host_without_port(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    }
}

fn system_hostname() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
    })
}

/// Is this `Host` header acceptable for the API?
pub fn is_allowed_host(host: &str) -> bool {
    let h = host_without_port(host).to_ascii_lowercase();
    if h.is_empty() {
        return false;
    }
    if h.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if h == "localhost" || h.ends_with(".local") || h == "aurion" {
        return true;
    }
    let own = system_hostname();
    !own.is_empty() && h == own
}

/// Does the `Origin` header designate the same host as `Host`?
pub fn is_same_origin(origin: &str, host: &str) -> bool {
    if origin == "null" {
        return false;
    }
    let authority = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin)
        .trim_end_matches('/');
    authority.eq_ignore_ascii_case(host.trim())
}

fn forbidden(msg: &'static str) -> Response {
    (StatusCode::FORBIDDEN, msg).into_response()
}

/// Guard applied to every `/api/*` route.
pub async fn api_guard(req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.to_string()));

    // Anti DNS-rebinding (requests without Host come from local tools).
    if let Some(ref h) = host {
        if !is_allowed_host(h) {
            tracing::warn!("API refusée: Host non autorisé '{}'", h);
            return forbidden("Host non autorisé");
        }
    }

    // Anti-CSRF on state-changing methods.
    let changes_state = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if changes_state {
        if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
            if site.eq_ignore_ascii_case("cross-site") {
                tracing::warn!("API refusée: requête cross-site");
                return forbidden("Requête cross-site refusée");
            }
        }
        if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let same = host.as_deref().map(|h| is_same_origin(origin, h)).unwrap_or(false);
            if !same {
                tracing::warn!("API refusée: Origin '{}' différent de Host {:?}", origin, host);
                return forbidden("Origine non autorisée");
            }
        }
    }

    next.run(req).await
}

/// Security headers added to every response.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
             script-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; \
             form-action 'self'; base-uri 'self'",
        ),
    );
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert!(is_allowed_host("192.168.4.1:8080"));
        assert!(is_allowed_host("192.168.4.1"));
        assert!(is_allowed_host("[::1]:8080"));
        assert!(is_allowed_host("localhost:8080"));
        assert!(is_allowed_host("aurion.local"));
        assert!(is_allowed_host("AURION.LOCAL:8080"));
        assert!(!is_allowed_host("evil.example.com"));
        assert!(!is_allowed_host("192.168.4.1.evil.com"));
        assert!(!is_allowed_host(""));
    }

    #[test]
    fn origins() {
        assert!(is_same_origin("http://192.168.4.1:8080", "192.168.4.1:8080"));
        assert!(is_same_origin("http://aurion.local:8080/", "aurion.local:8080"));
        assert!(!is_same_origin("http://evil.com", "192.168.4.1:8080"));
        assert!(!is_same_origin("null", "192.168.4.1:8080"));
        assert!(!is_same_origin("http://192.168.4.1:9999", "192.168.4.1:8080"));
    }

    #[test]
    fn strip_port() {
        assert_eq!(host_without_port("a.local:80"), "a.local");
        assert_eq!(host_without_port("[fe80::1]:80"), "fe80::1");
        assert_eq!(host_without_port("10.0.0.1"), "10.0.0.1");
    }
}
