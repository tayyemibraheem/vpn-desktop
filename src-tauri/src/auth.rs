use serde::{Deserialize, Serialize};

const PLATFORM_BASE_URL: &str = "https://api.tayyem.dev";
const REQUIRED_SERVICE: &str = "vpn-access";

#[derive(Deserialize)]
struct ServiceInfo {
    key: String,
}

#[derive(Deserialize)]
struct UserResponse {
    username: String,
    email: Option<String>,
    #[serde(rename = "mustChangePassword")]
    must_change_password: bool,
    #[serde(rename = "emailVerified")]
    email_verified: bool,
    services: Vec<ServiceInfo>,
}

/// Both a successful login and an MFA challenge come back as 200 OK with different field sets —
/// every field here is optional so one struct can deserialize either shape, then `login()`
/// branches on whether `mfa_required` was actually present.
#[derive(Deserialize)]
struct LoginOrChallengeResponse {
    #[serde(rename = "mfaRequired")]
    mfa_required: Option<bool>,
    #[serde(rename = "challengeToken")]
    challenge_token: Option<String>,
    methods: Option<Vec<String>>,
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresInSeconds")]
    expires_in_seconds: Option<i64>,
    user: Option<UserResponse>,
}

/// tayyem_platform's GlobalExceptionHandler puts the real text in `message`; `error` there is
/// just the HTTP reason phrase ("Unauthorized", "Bad Request", ...), not the specific reason.
#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Serialize, Default)]
pub struct LoginOutcome {
    pub ok: bool,
    pub error: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in_seconds: Option<i64>,
    /// Password was correct but a second factor is required — no tokens exist yet. The caller
    /// collects a code and calls `verify_mfa_login`/`send_mfa_login_email_code` to finish.
    pub mfa_required: bool,
    pub mfa_challenge_token: Option<String>,
    pub mfa_methods: Option<Vec<String>>,
}

impl LoginOutcome {
    fn error(message: impl Into<String>) -> Self {
        LoginOutcome {
            error: Some(message.into()),
            ..Default::default()
        }
    }

    fn mfa_challenge(challenge_token: String, methods: Vec<String>) -> Self {
        LoginOutcome {
            mfa_required: true,
            mfa_challenge_token: Some(challenge_token),
            mfa_methods: Some(methods),
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
struct RefreshResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresInSeconds")]
    expires_in_seconds: i64,
}

pub struct RefreshOutcome {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_seconds: i64,
}

/// tayyem_platform's own refresh endpoint — vpn_manager and every other backend trust this same
/// token, but only the platform itself knows how to mint a new one.
pub async fn refresh(refresh_token: &str) -> Result<RefreshOutcome, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{PLATFORM_BASE_URL}/api/auth/refresh"))
        .json(&serde_json::json!({ "refreshToken": refresh_token }))
        .send()
        .await
        .map_err(|e| format!("Could not reach the platform: {e}"))?;

    if !resp.status().is_success() {
        return Err("Session expired".to_string());
    }

    let body: RefreshResponse = resp.json().await.map_err(|e| format!("Unexpected response from platform: {e}"))?;
    Ok(RefreshOutcome {
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_in_seconds: body.expires_in_seconds,
    })
}

async fn extract_error(resp: reqwest::Response, fallback: &str) -> String {
    resp.json::<ApiError>()
        .await
        .ok()
        .and_then(|e| e.message)
        .unwrap_or_else(|| fallback.to_string())
}

/// Shared by both `login()` (once Keycloak accepted the password directly) and
/// `verify_mfa_login()` (once an MFA code was also accepted) — same account-status/entitlement
/// checks either way, since both end with a real `LoginResponse` from tayyem_platform.
fn outcome_from_login(access_token: String, refresh_token: String, expires_in_seconds: i64, user: UserResponse) -> LoginOutcome {
    if user.must_change_password {
        return LoginOutcome::error("Finish setting up your account at myaccount.tayyem.dev first.");
    }
    if !user.email_verified {
        return LoginOutcome::error("Confirm your email at myaccount.tayyem.dev before using the VPN.");
    }
    if !user.services.iter().any(|s| s.key == REQUIRED_SERVICE) {
        return LoginOutcome::error("This account does not have VPN access. Contact an admin.");
    }

    LoginOutcome {
        ok: true,
        username: Some(user.username),
        email: user.email,
        access_token: Some(access_token),
        refresh_token: Some(refresh_token),
        expires_in_seconds: Some(expires_in_seconds),
        ..Default::default()
    }
}

/// Resolves to an outcome with `mfa_required: true` (no tokens yet) when the account has a
/// second factor enabled — the caller must collect a code and call `verify_mfa_login` to
/// actually finish signing in. Opt-in: an account with nothing enabled gets exactly the same
/// `ok: true` outcome as before.
pub async fn login(username_or_email: &str, password: &str) -> LoginOutcome {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{PLATFORM_BASE_URL}/api/auth/login"))
        .json(&serde_json::json!({
            "usernameOrEmail": username_or_email,
            "password": password,
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return LoginOutcome::error(format!("Could not reach the platform: {e}")),
    };

    if !resp.status().is_success() {
        let message = extract_error(resp, "Invalid username/email or password").await;
        return LoginOutcome::error(message);
    }

    let body = match resp.json::<LoginOrChallengeResponse>().await {
        Ok(b) => b,
        Err(e) => return LoginOutcome::error(format!("Unexpected response from platform: {e}")),
    };

    if body.mfa_required == Some(true) {
        return LoginOutcome::mfa_challenge(
            body.challenge_token.unwrap_or_default(),
            body.methods.unwrap_or_default(),
        );
    }

    match (body.access_token, body.refresh_token, body.expires_in_seconds, body.user) {
        (Some(access_token), Some(refresh_token), Some(expires_in_seconds), Some(user)) => {
            outcome_from_login(access_token, refresh_token, expires_in_seconds, user)
        }
        _ => LoginOutcome::error("Unexpected response from platform"),
    }
}

/// The second step of login once `login()` returned `mfa_required: true`.
pub async fn verify_mfa_login(challenge_token: &str, method: &str, code: &str) -> LoginOutcome {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{PLATFORM_BASE_URL}/api/auth/mfa/verify"))
        .json(&serde_json::json!({
            "challengeToken": challenge_token,
            "method": method,
            "code": code,
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return LoginOutcome::error(format!("Could not reach the platform: {e}")),
    };

    if !resp.status().is_success() {
        let message = extract_error(resp, "That code didn't match — try again.").await;
        return LoginOutcome::error(message);
    }

    #[derive(Deserialize)]
    struct LoginResponse {
        #[serde(rename = "accessToken")]
        access_token: String,
        #[serde(rename = "refreshToken")]
        refresh_token: String,
        #[serde(rename = "expiresInSeconds")]
        expires_in_seconds: i64,
        user: UserResponse,
    }

    match resp.json::<LoginResponse>().await {
        Ok(body) => outcome_from_login(body.access_token, body.refresh_token, body.expires_in_seconds, body.user),
        Err(e) => LoginOutcome::error(format!("Unexpected response from platform: {e}")),
    }
}

/// Only needed if the chosen method is EMAIL — TOTP/PIN need nothing sent first.
pub async fn send_mfa_login_email_code(challenge_token: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{PLATFORM_BASE_URL}/api/auth/mfa/email/send"))
        .json(&serde_json::json!({ "challengeToken": challenge_token }))
        .send()
        .await
        .map_err(|e| format!("Could not reach the platform: {e}"))?;

    if !resp.status().is_success() {
        return Err(extract_error(resp, "Failed to send the email code — try again.").await);
    }
    Ok(())
}
