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

/// Enrolls a device that can't generate its own keypair — a phone that will scan the QR code
/// this returns. The response's configText contains a private key and is shown to the user
/// exactly once; it is never fetchable again after this call returns.
pub async fn enroll_device(access_token: &str, device_name: &str, platform: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{VPN_MANAGER_BASE_URL}/api/devices/enroll"))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "deviceName": device_name, "platform": platform }))
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not add this device").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))
}

pub async fn revoke_device(access_token: &str, device_id: i64) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{VPN_MANAGER_BASE_URL}/api/devices/{device_id}"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the VPN service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not remove this device").await);
    }
    Ok(())
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
