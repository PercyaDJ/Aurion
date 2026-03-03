//! API integration tests using axum-test.
//! Tests the critical HTTP endpoints for correctness, security, and error handling.

#[cfg(test)]
mod api_tests {
    use axum_test::TestServer;
    use serde_json::{json, Value};

    use aurion::core::config::AppConfig;
    use aurion::web::AppState;

    /// Build a test server with default AppState.
    fn make_server() -> TestServer {
        let config = AppConfig::default();
        let state = AppState::new(config);

        let app = aurion::web::build_router(state);
        TestServer::new(app).unwrap()
    }

    // ─── GET /api/status ──────────────────────────────────────

    #[tokio::test]
    async fn test_get_status_returns_200() {
        let server = make_server();
        let response = server.get("/api/status").await;
        response.assert_status_ok();

        let body: Value = response.json();
        assert!(body.get("phase").is_some(), "missing 'phase' field");
        assert!(body.get("time").is_some(), "missing 'time' field");
    }

    // ─── GET /api/config ─────────────────────────────────────

    #[tokio::test]
    async fn test_get_config_returns_valid_json() {
        let server = make_server();
        let response = server.get("/api/config").await;
        response.assert_status_ok();

        let body: Value = response.json();
        // Must have exposure and detection sub-objects
        assert!(body.get("exposure").is_some(), "missing 'exposure'");
        assert!(body.get("detection").is_some(), "missing 'detection'");
        // WiFi password must be masked
        let password = body["network"]["password"].as_str().unwrap_or("");
        assert_eq!(password, "********", "WiFi password should be masked in GET /api/config");
    }

    // ─── POST /api/config ────────────────────────────────────

    #[tokio::test]
    async fn test_update_config_preserves_masked_password() {
        let server = make_server();

        // First get the config
        let get_resp = server.get("/api/config").await;
        let mut config: Value = get_resp.json();

        // Simulate frontend: sends back masked password
        config["network"]["password"] = json!("********");

        let post_resp = server.post("/api/config").json(&config).await;
        post_resp.assert_status_ok();

        // Verify via /api/config/wifi-password that real password was NOT overwritten
        let pw_resp = server.get("/api/config/wifi-password").await;
        pw_resp.assert_status_ok();
        let pw_body: Value = pw_resp.json();
        let stored_pw = pw_body["password"].as_str().unwrap_or("");
        assert_ne!(stored_pw, "********", "Real password was overwritten with masked value");
    }

    // ─── GET /api/logs ───────────────────────────────────────

    #[tokio::test]
    async fn test_get_logs_returns_array() {
        let server = make_server();
        let response = server.get("/api/logs").await;
        response.assert_status_ok();

        let body: Value = response.json();
        assert!(body.get("logs").is_some(), "missing 'logs' field");
        assert!(body["logs"].is_array(), "'logs' should be an array");
    }

    // ─── POST /api/gallery/delete — security ────────────────

    #[tokio::test]
    async fn test_delete_rejects_path_traversal() {
        let server = make_server();

        let payload = json!({ "filenames": ["../../../etc/passwd"] });
        let response = server.post("/api/gallery/delete").json(&payload).await;
        response.assert_status_ok(); // returns 200 with errors array

        let body: Value = response.json();
        let deleted = body["deleted"].as_u64().unwrap_or(99);
        assert_eq!(deleted, 0, "Path traversal should not delete any file");

        let errors = body["errors"].as_array().expect("errors array expected");
        assert!(!errors.is_empty(), "Should report error for path traversal attempt");
    }

    #[tokio::test]
    async fn test_delete_rejects_slash_in_filename() {
        let server = make_server();
        let payload = json!({ "filenames": ["subdir/evil.jpg"] });
        let response = server.post("/api/gallery/delete").json(&payload).await;
        response.assert_status_ok();

        let body: Value = response.json();
        assert_eq!(body["deleted"].as_u64().unwrap_or(99), 0);
    }

    // ─── POST /api/disconnect ────────────────────────────────

    #[tokio::test]
    async fn test_disconnect_transitions_phase() {
        let server = make_server();
        // POST /api/disconnect directly sets phase to Disconnect
        let response = server.post("/api/disconnect").await;
        response.assert_status_ok();

        // Verify phase changed
        let status = server.get("/api/status").await;
        let body: Value = status.json();
        let phase = body["phase"].as_str().unwrap_or("");
        assert_eq!(phase, "DISCONNECT", "Phase should be 'DISCONNECT' after calling /api/disconnect");
    }

    // ─── GET /api/gallery/sessions ───────────────────────────

    #[tokio::test]
    async fn test_get_sessions_returns_array() {
        let server = make_server();
        let response = server.get("/api/gallery/sessions").await;
        response.assert_status_ok();

        // Should return a JSON array (empty if no sessions on disk)
        let body: Value = response.json();
        assert!(body.is_array(), "Sessions response should be an array");
    }

    // ─── GET /api/diagnostics ────────────────────────────────

    #[tokio::test]
    async fn test_diagnostics_has_version() {
        let server = make_server();
        let response = server.get("/api/diagnostics").await;
        response.assert_status_ok();

        let body: Value = response.json();
        let version = body["version"].as_str().unwrap_or("");
        assert!(!version.is_empty(), "version field should be present and non-empty");
        assert!(version.starts_with("0."), "version should look like a semver string");
    }
}
