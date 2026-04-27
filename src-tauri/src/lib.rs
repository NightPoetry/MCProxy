mod proxy;

use proxy::ProxyState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub motd: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProxyEvent {
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "disconnected")]
    Disconnected { reason: String },
    #[serde(rename = "room_created")]
    RoomCreated { room_id: String },
    #[serde(rename = "room_joined")]
    RoomJoined { game_info: GameInfo, is_host: bool },
    #[serde(rename = "peer_joined")]
    PeerJoined { peer_id: String },
    #[serde(rename = "peer_left")]
    PeerLeft { peer_id: String },
    #[serde(rename = "lan_game_found")]
    LanGameFound { motd: String, port: u16 },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "room_closed")]
    RoomClosed,
    #[serde(rename = "tunnel_active")]
    TunnelActive { local_port: u16 },
    #[serde(rename = "status_update")]
    StatusUpdate { status: String, detail: String },
}

#[tauri::command]
async fn connect_server(
    state: tauri::State<'_, Arc<ProxyState>>,
    server_url: String,
) -> Result<(), String> {
    state.connect(&server_url).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn disconnect_server(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<(), String> {
    state.disconnect().await;
    Ok(())
}

#[tauri::command]
async fn create_room(
    state: tauri::State<'_, Arc<ProxyState>>,
    password: String,
) -> Result<(), String> {
    state.create_room(password).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn join_room(
    state: tauri::State<'_, Arc<ProxyState>>,
    room_id: String,
    password: String,
) -> Result<(), String> {
    state
        .join_room(room_id, password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn leave_room(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<(), String> {
    state.leave_room().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_lan_scan(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<(), String> {
    state.start_lan_scan().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_lan_scan(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<(), String> {
    state.stop_lan_scan().await;
    Ok(())
}

#[tauri::command]
async fn set_nickname(
    state: tauri::State<'_, Arc<ProxyState>>,
    nickname: String,
) -> Result<(), String> {
    state.set_nickname(nickname).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_rooms(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<(), String> {
    state.list_rooms().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_status(
    state: tauri::State<'_, Arc<ProxyState>>,
) -> Result<proxy::StatusInfo, String> {
    Ok(state.get_status().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = Arc::new(ProxyState::new(handle));
            app.manage(state.clone());

            #[cfg(debug_assertions)]
            {
                let debug_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    proxy::debug_api::start_debug_server(debug_state, 9801).await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect_server,
            disconnect_server,
            set_nickname,
            create_room,
            join_room,
            leave_room,
            list_rooms,
            start_lan_scan,
            stop_lan_scan,
            get_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
