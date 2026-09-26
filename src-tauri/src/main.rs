#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod split_tunnel;
mod store;
mod wireguard;

use split_tunnel::app_tunnel::{list_candidate_apps, AppTunnel, CandidateApp};
use split_tunnel::destination_routes;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

struct AppState {
    data: Mutex<store::AppData>,
    wireguard: Arc<wireguard::WireguardClient>,
    app_tunnel: Arc<AppTunnel>,
}

#[tauri::command]
async fn auth_login(state: State<'_, AppState>, username_or_email: String, password: String) -> Result<serde_json::Value, ()> {
    let outcome = auth::login(&username_or_email, &password).await;
    if outcome.ok {
        let mut data = state.data.lock().unwrap();
        data.session = Some(store::Session {
            access_token: outcome.access_token.clone().unwrap_or_default(),
            refresh_token: outcome.refresh_token.clone().unwrap_or_default(),
            username: outcome.username.clone().unwrap_or_default(),
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

#[tauri::command]
async fn vpn_connect(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<serde_json::Value, ()> {
    let access_token = match state.data.lock().unwrap().session.clone() {
        Some(s) => s.access_token,
        None => return Ok(serde_json::json!({ "ok": false, "error": "Not signed in" })),
    };
    let split_tunnel_config = state.data.lock().unwrap().split_tunnel.clone();

    let wireguard = state.wireguard.clone();
    let app_tunnel = state.app_tunnel.clone();
    let app_for_events = app.clone();

    let result = wireguard
        .connect(access_token, split_tunnel_config, app_tunnel, move |event_name, payload| {
            let _ = app_for_events.emit(event_name, payload);
        })
        .await;

    Ok(serde_json::json!({ "ok": result.is_ok(), "error": result.err() }))
}

#[tauri::command]
async fn vpn_disconnect(state: State<'_, AppState>) -> Result<(), ()> {
    state.wireguard.disconnect().await;
    state.app_tunnel.stop();
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
            vpn_connect,
            vpn_disconnect,
            vpn_get_status,
            split_tunnel_get_config,
            split_tunnel_set_config,
            split_tunnel_list_candidate_apps,
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
