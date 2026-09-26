use crate::wireguard::VPN_MANAGER_BASE_URL;

/// Every call here reuses the same platform access token the WireGuard tunnel itself
/// authenticates with — vpn_manager treats the desktop app and a phone's browser identically.
async fn error_message(resp: reqwest::Response, fallback: &str) -> String {
    resp.json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| fallback.to_string())
}

pub async fn list_devices(access_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{VPN_MANAGER_BASE_URL}/api/devices"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not load your devices").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))
}

/// Step 1 of adding or removing a device: emails a 6-digit code and stashes the action
/// server-side. `body` is `{"action":"ENROLL","deviceName":...,"platform":...}` or
/// `{"action":"REVOKE","deviceId":...}`.
pub async fn request_device_verification(access_token: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{VPN_MANAGER_BASE_URL}/api/devices/verify/request"))
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not send a verification code").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))
}

/// Step 2: checks the emailed code and, if it matches, performs whichever action was stashed —
/// the response is the new device's config for an ENROLL, or empty for a REVOKE.
pub async fn confirm_device_verification(access_token: &str, code: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{VPN_MANAGER_BASE_URL}/api/devices/verify/confirm"))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not confirm that code").await);
    }
    let text = resp.text().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))?;
    if text.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("Unexpected response from the VPN service: {e}"))
}

pub async fn my_subscription(access_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{VPN_MANAGER_BASE_URL}/api/subscribers/me"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not load your subscription").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))
}
