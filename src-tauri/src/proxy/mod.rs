pub mod debug_api;
mod lan_scanner;
mod protocol;
mod tunnel;

use crate::{GameInfo, ProxyEvent};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, MemberInfo, RoomListing, ServerMessage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    pub connected: bool,
    pub room_id: Option<String>,
    pub is_host: bool,
    pub peer_count: usize,
    pub lan_game: Option<GameInfo>,
    pub tunnel_port: Option<u16>,
    pub scanning: bool,
    pub members: Vec<MemberInfo>,
    pub room_list: Vec<RoomListing>,
}

pub struct ProxyState {
    app: AppHandle,
    ws_tx: RwLock<Option<mpsc::UnboundedSender<Message>>>,
    connected: RwLock<bool>,
    room_id: RwLock<Option<String>>,
    is_host: RwLock<bool>,
    peer_count: RwLock<usize>,
    pub(crate) lan_game: RwLock<Option<GameInfo>>,
    tunnel_port: RwLock<Option<u16>>,
    scanning: RwLock<bool>,
    scan_cancel: RwLock<Option<tokio::sync::oneshot::Sender<()>>>,
    tunnel_cancel: RwLock<Option<tokio::sync::oneshot::Sender<()>>>,
    connections: Arc<Mutex<std::collections::HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>>,
    event_log: Mutex<Vec<crate::ProxyEvent>>,
    members: RwLock<Vec<MemberInfo>>,
    room_list: RwLock<Vec<RoomListing>>,
}

impl ProxyState {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            ws_tx: RwLock::new(None),
            connected: RwLock::new(false),
            room_id: RwLock::new(None),
            is_host: RwLock::new(false),
            peer_count: RwLock::new(0),
            lan_game: RwLock::new(None),
            tunnel_port: RwLock::new(None),
            scanning: RwLock::new(false),
            scan_cancel: RwLock::new(None),
            tunnel_cancel: RwLock::new(None),
            connections: Arc::new(Mutex::new(std::collections::HashMap::new())),
            event_log: Mutex::new(Vec::new()),
            members: RwLock::new(Vec::new()),
            room_list: RwLock::new(Vec::new()),
        }
    }

    fn emit(&self, event: ProxyEvent) {
        let _ = self.app.emit("proxy-event", &event);
        if let Ok(mut log) = self.event_log.try_lock() {
            if log.len() > 200 { log.drain(..100); }
            log.push(event);
        }
    }

    pub async fn get_event_log(&self) -> Vec<crate::ProxyEvent> {
        self.event_log.lock().await.clone()
    }

    pub async fn clear_event_log(&self) {
        self.event_log.lock().await.clear();
    }

    async fn send_ws(&self, msg: ClientMessage) -> Result<(), String> {
        let tx = self.ws_tx.read().await;
        if let Some(tx) = tx.as_ref() {
            let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
            tx.send(Message::Text(json.into())).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("未连接到服务器".to_string())
        }
    }

    async fn send_binary(&self, connection_id: &str, data: &[u8]) -> Result<(), String> {
        let tx = self.ws_tx.read().await;
        if let Some(tx) = tx.as_ref() {
            let conn_id_bytes = connection_id.as_bytes();
            let mut frame = Vec::with_capacity(2 + conn_id_bytes.len() + data.len());
            frame.extend_from_slice(&(conn_id_bytes.len() as u16).to_be_bytes());
            frame.extend_from_slice(conn_id_bytes);
            frame.extend_from_slice(data);
            tx.send(Message::Binary(frame.into())).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("未连接到服务器".to_string())
        }
    }

    pub async fn connect(self: &Arc<Self>, server_url: &str) -> Result<(), String> {
        if *self.connected.read().await {
            return Err("已经连接到服务器".to_string());
        }

        let url = if server_url.starts_with("ws://") || server_url.starts_with("wss://") {
            server_url.to_string()
        } else {
            format!("ws://{}", server_url)
        };

        let (ws_stream, _) = connect_async(&url).await.map_err(|e| format!("连接失败: {}", e))?;
        let (mut ws_write, mut ws_read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        *self.ws_tx.write().await = Some(tx);
        *self.connected.write().await = true;
        self.emit(ProxyEvent::Connected);

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_write.send(msg).await.is_err() { break; }
            }
        });

        let state = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_read.next().await {
                match msg {
                    Message::Text(text) => {
                        if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                            state.handle_server_message(server_msg).await;
                        }
                    }
                    Message::Binary(data) => { state.handle_binary_data(data.to_vec()).await; }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            *state.connected.write().await = false;
            *state.ws_tx.write().await = None;
            state.emit(ProxyEvent::Disconnected { reason: "连接断开".to_string() });
        });

        let state_hb = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if !*state_hb.connected.read().await { break; }
                let _ = state_hb.send_ws(ClientMessage::Heartbeat).await;
            }
        });

        Ok(())
    }

    pub async fn disconnect(&self) {
        *self.connected.write().await = false;
        *self.ws_tx.write().await = None;
        *self.room_id.write().await = None;
        *self.is_host.write().await = false;
        *self.peer_count.write().await = 0;
        *self.lan_game.write().await = None;
        *self.members.write().await = Vec::new();
        if let Some(cancel) = self.tunnel_cancel.write().await.take() { let _ = cancel.send(()); }
        *self.tunnel_port.write().await = None;
        self.connections.lock().await.clear();
        self.emit(ProxyEvent::Disconnected { reason: "主动断开".to_string() });
    }

    pub async fn set_nickname(&self, nickname: String) -> Result<(), String> {
        self.send_ws(ClientMessage::SetNickname { nickname }).await
    }

    pub async fn create_room(self: &Arc<Self>, password: String) -> Result<(), String> {
        let game = self.lan_game.read().await;
        let game_info = game.as_ref().ok_or("请先开启局域网游戏并扫描")?;
        self.send_ws(ClientMessage::CreateRoom {
            password,
            game_info: protocol::GameInfo { motd: game_info.motd.clone(), port: game_info.port },
        }).await?;
        self.start_host_tunnel().await?;
        Ok(())
    }

    pub async fn join_room(self: &Arc<Self>, room_id: String, password: String) -> Result<(), String> {
        self.send_ws(ClientMessage::JoinRoom { room_id, password }).await
    }

    pub async fn leave_room(&self) -> Result<(), String> {
        self.send_ws(ClientMessage::LeaveRoom).await?;
        *self.room_id.write().await = None;
        *self.is_host.write().await = false;
        *self.peer_count.write().await = 0;
        *self.members.write().await = Vec::new();
        if let Some(cancel) = self.tunnel_cancel.write().await.take() { let _ = cancel.send(()); }
        *self.tunnel_port.write().await = None;
        self.connections.lock().await.clear();
        Ok(())
    }

    pub async fn list_rooms(&self) -> Result<(), String> {
        self.send_ws(ClientMessage::ListRooms).await
    }

    pub async fn start_lan_scan(self: &Arc<Self>) -> Result<(), String> {
        if *self.scanning.read().await { return Ok(()); }
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        *self.scan_cancel.write().await = Some(cancel_tx);
        *self.scanning.write().await = true;
        let state = Arc::clone(self);
        tokio::spawn(async move { lan_scanner::scan_lan_games(state, cancel_rx).await; });
        Ok(())
    }

    pub async fn stop_lan_scan(&self) {
        if let Some(cancel) = self.scan_cancel.write().await.take() { let _ = cancel.send(()); }
        *self.scanning.write().await = false;
    }

    pub async fn get_status(&self) -> StatusInfo {
        StatusInfo {
            connected: *self.connected.read().await,
            room_id: self.room_id.read().await.clone(),
            is_host: *self.is_host.read().await,
            peer_count: *self.peer_count.read().await,
            lan_game: self.lan_game.read().await.clone(),
            tunnel_port: *self.tunnel_port.read().await,
            scanning: *self.scanning.read().await,
            members: self.members.read().await.clone(),
            room_list: self.room_list.read().await.clone(),
        }
    }

    async fn handle_server_message(self: &Arc<Self>, msg: ServerMessage) {
        match msg {
            ServerMessage::RoomCreated { room_id } => {
                *self.room_id.write().await = Some(room_id.clone());
                self.emit(ProxyEvent::RoomCreated { room_id });
            }
            ServerMessage::RoomJoined { game_info, is_host, members } => {
                *self.is_host.write().await = is_host;
                *self.peer_count.write().await = members.len().saturating_sub(1);
                *self.members.write().await = members;
                let gi = GameInfo { motd: game_info.motd.clone(), port: game_info.port };
                self.emit(ProxyEvent::RoomJoined { game_info: gi.clone(), is_host });
                if !is_host { let _ = self.start_guest_tunnel(gi).await; }
            }
            ServerMessage::Error { message } => {
                self.emit(ProxyEvent::Error { message });
            }
            ServerMessage::PeerJoined { peer_id, nickname } => {
                let mut count = self.peer_count.write().await;
                *count += 1;
                self.emit(ProxyEvent::PeerJoined { peer_id: format!("{} ({})", nickname, &peer_id[..8]) });
            }
            ServerMessage::PeerLeft { peer_id } => {
                let mut count = self.peer_count.write().await;
                *count = count.saturating_sub(1);
                self.emit(ProxyEvent::PeerLeft { peer_id });
            }
            ServerMessage::MemberList { members } => {
                *self.peer_count.write().await = members.len().saturating_sub(1);
                *self.members.write().await = members;
                self.emit(ProxyEvent::StatusUpdate {
                    status: "members_updated".to_string(),
                    detail: "Member list updated".to_string(),
                });
            }
            ServerMessage::RoomList { rooms } => {
                *self.room_list.write().await = rooms;
                self.emit(ProxyEvent::StatusUpdate {
                    status: "rooms_updated".to_string(),
                    detail: "Room list updated".to_string(),
                });
            }
            ServerMessage::GameData { connection_id, data, .. } => {
                let conns = self.connections.lock().await;
                if let Some(tx) = conns.get(&connection_id) { let _ = tx.send(data); }
            }
            ServerMessage::NewConnection { connection_id, .. } => {
                if *self.is_host.read().await {
                    let game = self.lan_game.read().await;
                    if let Some(game) = game.as_ref() {
                        let state = Arc::clone(self);
                        let port = game.port;
                        let conn_id = connection_id.clone();
                        tokio::spawn(async move { tunnel::host_handle_new_connection(state, conn_id, port).await; });
                    }
                }
            }
            ServerMessage::CloseConnection { connection_id, .. } => {
                self.connections.lock().await.remove(&connection_id);
            }
            ServerMessage::GameInfoUpdated { game_info } => {
                *self.lan_game.write().await = Some(GameInfo { motd: game_info.motd, port: game_info.port });
            }
            ServerMessage::HeartbeatAck => {}
            ServerMessage::RoomClosed => {
                *self.room_id.write().await = None;
                *self.is_host.write().await = false;
                *self.peer_count.write().await = 0;
                *self.members.write().await = Vec::new();
                if let Some(cancel) = self.tunnel_cancel.write().await.take() { let _ = cancel.send(()); }
                *self.tunnel_port.write().await = None;
                self.connections.lock().await.clear();
                self.emit(ProxyEvent::RoomClosed);
            }
        }
    }

    async fn handle_binary_data(self: &Arc<Self>, data: Vec<u8>) {
        if data.len() < 2 { return; }
        let conn_id_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        if data.len() < 2 + conn_id_len { return; }
        let connection_id = String::from_utf8_lossy(&data[2..2 + conn_id_len]).to_string();
        let payload = data[2 + conn_id_len..].to_vec();
        let conns = self.connections.lock().await;
        if let Some(tx) = conns.get(&connection_id) { let _ = tx.send(payload); }
    }

    async fn start_host_tunnel(self: &Arc<Self>) -> Result<(), String> {
        self.emit(ProxyEvent::StatusUpdate { status: "hosting".to_string(), detail: "主机隧道已就绪，等待连接...".to_string() });
        Ok(())
    }

    async fn start_guest_tunnel(self: &Arc<Self>, game_info: GameInfo) -> Result<(), String> {
        if let Some(cancel) = self.tunnel_cancel.write().await.take() { let _ = cancel.send(()); }
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("绑定端口失败: {}", e))?;
        let local_port = listener.local_addr().map_err(|e| e.to_string())?.port();
        *self.tunnel_port.write().await = Some(local_port);

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        *self.tunnel_cancel.write().await = Some(cancel_tx);
        self.emit(ProxyEvent::TunnelActive { local_port });

        let motd = game_info.motd.clone();
        tokio::spawn(async move { lan_scanner::broadcast_fake_lan(local_port, &motd).await; });

        let state = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        if let Ok((stream, _)) = result {
                            let conn_id = uuid::Uuid::new_v4().to_string();
                            let s = Arc::clone(&state);
                            tokio::spawn(async move { tunnel::guest_handle_connection(s, conn_id, stream).await; });
                        }
                    }
                    _ = &mut cancel_rx => { break; }
                }
            }
        });

        self.emit(ProxyEvent::StatusUpdate { status: "tunneling".to_string(), detail: format!("本地代理端口: {}", local_port) });
        Ok(())
    }
}
