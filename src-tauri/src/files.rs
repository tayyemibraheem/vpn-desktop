//! Talks to `file_manager` (the Tayyem Files backend) the same way `devices.rs` talks to
//! vpn_manager — reusing the platform access token, all HTTP done natively in Rust rather than
//! from the webview, since the webview's CSP doesn't (and shouldn't need to) allow this host.
//! The base URL is an internal implementation detail — never surfaced in the UI.

use base64::{engine::general_purpose::STANDARD, Engine as _};

const FILE_MANAGER_BASE_URL: &str = "https://api-files.tayyem.dev";

async fn error_message(resp: reqwest::Response, fallback: &str) -> String {
    resp.json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| fallback.to_string())
}

pub async fn list_folder(access_token: &str, path: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{FILE_MANAGER_BASE_URL}/api/files"))
        .query(&[("path", path)])
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not load your files").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the file service: {e}"))
}

pub async fn get_usage(access_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{FILE_MANAGER_BASE_URL}/api/files/usage"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not load your storage usage").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the file service: {e}"))
}

pub async fn create_folder(access_token: &str, path: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{FILE_MANAGER_BASE_URL}/api/files/folders"))
        .query(&[("path", path)])
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not create that folder").await);
    }
    Ok(())
}

pub async fn delete_folder(access_token: &str, path: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{FILE_MANAGER_BASE_URL}/api/files/folders"))
        .query(&[("path", path)])
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not delete that folder").await);
    }
    Ok(())
}

pub async fn delete_file(access_token: &str, id: i64) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{FILE_MANAGER_BASE_URL}/api/files/{id}"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not delete that file").await);
    }
    Ok(())
}

/// `data_base64` is the whole file, base64-encoded on the JS side (read via FileReader) — the
/// invoke bridge round-trips JSON, so base64 text is far cheaper to ship across it than the
/// equivalent JSON array of byte numbers would be.
pub async fn upload_file(
    access_token: &str,
    path: &str,
    filename: &str,
    content_type: &str,
    data_base64: &str,
) -> Result<serde_json::Value, String> {
    let bytes = STANDARD.decode(data_base64).map_err(|e| format!("Corrupt file data: {e}"))?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(content_type)
        .map_err(|e| format!("Invalid file type: {e}"))?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{FILE_MANAGER_BASE_URL}/api/files"))
        .query(&[("path", path)])
        .bearer_auth(access_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Upload failed").await);
    }
    resp.json().await.map_err(|e| format!("Unexpected response from the file service: {e}"))
}

/// Returns the whole file, base64-encoded, for the same round-trip-efficiency reason as upload.
/// The caller already knows the file's name/type from the listing it came from.
pub async fn download_file(access_token: &str, id: i64) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{FILE_MANAGER_BASE_URL}/api/files/{id}/download"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach the file service: {e}"))?;
    if !resp.status().is_success() {
        return Err(error_message(resp, "Could not download that file").await);
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Could not read the downloaded file: {e}"))?;
    Ok(STANDARD.encode(bytes))
}
