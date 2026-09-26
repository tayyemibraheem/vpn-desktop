#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod devices;
mod split_tunnel;
mod store;
mod wireguard;
mod wireguard_nt;

use split_tunnel::app_tunnel::{list_candidate_apps, AppTunnel, CandidateApp};
use split_tunnel::destination_routes;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

struct AppState {
    data: Mutex<store::AppData>,
    wireguard: Arc<wireguard::WireguardClient>,
    app_tunnel: Arc<AppTunnel>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
async fn auth_login(state: State<'_, AppState>, username_or_email: String, password: String, remember: bool) -> Result<serde_json::Value, ()> {
    let outcome = auth::login(&username_or_email, &password).await;
    if outcome.ok {
        let mut data = state.data.lock().unwrap();
        data.session = Some(store::Session {
            access_token: outcome.access_token.clone().unwrap_or_default(),
            refresh_token: outcome.refresh_token.clone().unwrap_or_default(),
            username: outcome.username.clone().unwrap_or_default(),
            email: outcome.email.clone(),
            expires_at: now_ms() + outcome.expires_in_seconds.unwrap_or(0) * 1000,
            remember,
        });
        store::save(&data);
    }
    Ok(serde_json::json!({
        "ok": outcome.ok,
        "error": outcome.error,
        "username": outcome.username,
        "email": outcome.email,
    }))
}

#[tauri::command]
fn auth_logout(state: State<'_, AppState>) {
    let mut data = state.data.lock().unwrap();
    data.session = None;
    store::save(&data);
}

/// Called once on app startup. Only a session saved with "keep me logged in" checked is eligible;
/// a still-valid token is reused as-is, an expired one is refreshed transparently, and anything
/// that fails is treated as signed out rather than surfacing a confusing error at launch.
#[tauri::command]
async fn auth_restore(state: State<'_, AppState>) -> Result<serde_json::Value, ()> {
    let session = state.data.lock().unwrap().session.clone();
    let session = match session {
        Some(s) if s.remember => s,
        _ => return Ok(serde_json::json!({ "ok": false })),
    };

    if now_ms() < session.expires_at - REFRESH_SKEW_MS {
        return Ok(serde_json::json!({ "ok": true, "username": session.username, "email": session.email }));
    }

    match auth::refresh(&session.refresh_token).await {
        Ok(r) => {
            let mut data = state.data.lock().unwrap();
            let username = session.username.clone();
            let email = session.email.clone();
            data.session = Some(store::Session {
                access_token: r.access_token,
                refresh_token: r.refresh_token,
                username: username.clone(),
                email: email.clone(),
                expires_at: now_ms() + r.expires_in_seconds * 1000,
                remember: true,
            });
            store::save(&data);
            Ok(serde_json::json!({ "ok": true, "username": username, "email": email }))
        }
        Err(_) => {
            let mut data = state.data.lock().unwrap();
            data.session = None;
            store::save(&data);
            Ok(serde_json::json!({ "ok": false }))
        }
    }
}

const REFRESH_SKEW_MS: i64 = 30_000;

/// The token to use for a vpn_manager/devices call — transparently refreshes first if the
/// cached one is about to expire, mirroring the website's own getAccessToken() behavior.
async fn valid_access_token(state: &State<'_, AppState>) -> Result<String, String> {
    let session = state
        .data
        .lock()
        .unwrap()
        .session
        .clone()
        .ok_or_else(|| "Not signed in".to_string())?;

    if now_ms() < session.expires_at - REFRESH_SKEW_MS {
        return Ok(session.access_token);
    }

    let refreshed = auth::refresh(&session.refresh_token).await.map_err(|_| "Your session expired — please log in again".to_string())?;
    let mut data = state.data.lock().unwrap();
    let new_token = refreshed.access_token.clone();
    data.session = Some(store::Session {
        access_token: refreshed.access_token,
        refresh_token: refreshed.refresh_token,
        username: session.username,
        email: session.email,
        expires_at: now_ms() + refreshed.expires_in_seconds * 1000,
        remember: session.remember,
    });
    store::save(&data);
    Ok(new_token)
}

#[tauri::command]
async fn devices_list(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    devices::list_devices(&valid_access_token(&state).await?).await
}

#[tauri::command]
async fn devices_request_verification(state: State<'_, AppState>, body: serde_json::Value) -> Result<serde_json::Value, String> {
    devices::request_device_verification(&valid_access_token(&state).await?, body).await
}

#[tauri::command]
async fn devices_confirm_verification(state: State<'_, AppState>, code: String) -> Result<serde_json::Value, String> {
    devices::confirm_device_verification(&valid_access_token(&state).await?, &code).await
}

#[tauri::command]
async fn subscription_me(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    devices::my_subscription(&valid_access_token(&state).await?).await
}

#[tauri::command]
async fn vpn_connect(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<serde_json::Value, ()> {
    let access_token = match valid_access_token(&state).await {
        Ok(t) => t,
        Err(e) => return Ok(serde_json::json!({ "ok": false, "error": e })),
    };
    let split_tunnel_config = state.data.lock().unwrap().split_tunnel.clone();
    let resources_dir = app
        .path()
        .resource_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("resources"));

    let wireguard = state.wireguard.clone();
    let app_tunnel = state.app_tunnel.clone();
    let app_for_events = app.clone();

    let result = wireguard
        .connect(access_token, resources_dir, split_tunnel_config, app_tunnel, move |event_name, payload| {
            let _ = app_for_events.emit(event_name, payload);
        })
        .await;

    Ok(serde_json::json!({ "ok": result.is_ok(), "error": result.err() }))
}

#[tauri::command]
async fn vpn_disconnect(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), ()> {
    state.wireguard.disconnect().await;
    state.app_tunnel.stop();
    let _ = app.emit("vpn:status", serde_json::json!({ "state": "disconnected", "detail": null }));
    Ok(())
}

#[tauri::command]
fn vpn_get_status(state: State<'_, AppState>) -> serde_json::Value {
    let status = state.wireguard.status();
    serde_json::json!({ "state": status.0, "detail": status.1 })
}

#[tauri::command]
fn split_tunnel_get_config(state: State<'_, AppState>) -> store::SplitTunnelConfig {
    state.data.lock().unwrap().split_tunnel.clone()
}

#[tauri::command]
async fn split_tunnel_set_config(state: State<'_, AppState>, config: store::SplitTunnelConfig) -> Result<(), ()> {
    {
        let mut data = state.data.lock().unwrap();
        data.split_tunnel = config.clone();
        store::save(&data);
    }
    // Live-apply if the tunnel is already up.
    if state.wireguard.status().0 == "connected" {
        if let Some(physical) = state.wireguard.physical_gateway() {
            destination_routes::clear_all(&state.wireguard.applied_destination_routes());
            let applied = destination_routes::apply(&config.destinations, &physical).await;
            state.wireguard.set_applied_destination_routes(applied);
            state.app_tunnel.set_excluded_apps(config.apps);
        }
    }
    Ok(())
}

#[tauri::command]
fn split_tunnel_list_candidate_apps() -> Vec<CandidateApp> {
    list_candidate_apps()
}

fn main() {
    let initial_data = store::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            data: Mutex::new(initial_data),
            wireguard: Arc::new(wireguard::WireguardClient::new()),
            app_tunnel: Arc::new(AppTunnel::new()),
        })
        .invoke_handler(tauri::generate_handler![
            auth_login,
            auth_logout,
            auth_restore,
            vpn_connect,
            vpn_disconnect,
            vpn_get_status,
            split_tunnel_get_config,
            split_tunnel_set_config,
            split_tunnel_list_candidate_apps,
            devices_list,
            devices_request_verification,
            devices_confirm_verification,
            subscription_me,
        ])
        .on_window_event(|window, event| {
            // A disconnect on close is not optional — leaving split-tunnel routes or a live
            // tunnel behind after the app closes would silently change the user's networking
            // until reboot.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state: State<AppState> = window.state();
                let wireguard = state.wireguard.clone();
                let app_tunnel = state.app_tunnel.clone();
                tauri::async_runtime::block_on(async move {
                    wireguard.disconnect().await;
                });
                app_tunnel.stop();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running TayyemVPN");
}
