use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

// ── Protocol ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "set_nickname")]
    SetNickname { nickname: String },
    #[serde(rename = "create_room")]
    CreateRoom { password: String, game_info: GameInfo },
    #[serde(rename = "join_room")]
    JoinRoom { room_id: String, password: String },
    #[serde(rename = "leave_room")]
    LeaveRoom,
    #[serde(rename = "list_rooms")]
    ListRooms,
    #[serde(rename = "game_data")]
    GameData { connection_id: String, data: Vec<u8> },
    #[serde(rename = "new_connection")]
    NewConnection { connection_id: String },
    #[serde(rename = "close_connection")]
    CloseConnection { connection_id: String },
    #[serde(rename = "update_game_info")]
    UpdateGameInfo { game_info: GameInfo },
    #[serde(rename = "heartbeat")]
    Heartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "room_created")]
    RoomCreated { room_id: String },
    #[serde(rename = "room_joined")]
    RoomJoined {
        game_info: GameInfo,
        is_host: bool,
        members: Vec<MemberInfo>,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "peer_joined")]
    PeerJoined { peer_id: String, nickname: String },
    #[serde(rename = "peer_left")]
    PeerLeft { peer_id: String },
    #[serde(rename = "member_list")]
    MemberList { members: Vec<MemberInfo> },
    #[serde(rename = "room_list")]
    RoomList { rooms: Vec<RoomListing> },
    #[serde(rename = "game_data")]
    GameData { connection_id: String, from_peer: String, data: Vec<u8> },
    #[serde(rename = "new_connection")]
    NewConnection { connection_id: String, from_peer: String },
    #[serde(rename = "close_connection")]
    CloseConnection { connection_id: String, from_peer: String },
    #[serde(rename = "game_info_updated")]
    GameInfoUpdated { game_info: GameInfo },
    #[serde(rename = "heartbeat_ack")]
    HeartbeatAck,
    #[serde(rename = "room_closed")]
    RoomClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameInfo {
    motd: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemberInfo {
    peer_id: String,
    nickname: String,
    is_host: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoomListing {
    room_id: String,
    host_name: String,
    game_motd: String,
    player_count: usize,
    has_password: bool,
}

// ── State ─────────────────────────────────────────────

struct Peer {
    tx: mpsc::UnboundedSender<ServerMessage>,
    nickname: String,
    room_id: Option<String>,
}

struct Room {
    password: String,
    host_id: String,
    game_info: GameInfo,
    members: HashMap<String, mpsc::UnboundedSender<ServerMessage>>,
    nicknames: HashMap<String, String>,
}

type Rooms = Arc<DashMap<String, Room>>;
type Peers = Arc<DashMap<String, Peer>>;

fn generate_room_id() -> String {
    let id = Uuid::new_v4().as_u128();
    format!("{:06}", id % 1_000_000)
}

// ── Main ──────────────────────────────────────────────

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:9800".to_string());
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    log::info!("MCProxy relay server listening on {}", addr);

    let rooms: Rooms = Arc::new(DashMap::new());
    let peers: Peers = Arc::new(DashMap::new());

    while let Ok((stream, addr)) = listener.accept().await {
        let rooms = rooms.clone();
        let peers = peers.clone();
        tokio::spawn(handle_connection(stream, addr, rooms, peers));
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    rooms: Rooms,
    peers: Peers,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("WebSocket handshake failed for {}: {}", addr, e);
            return;
        }
    };

    let peer_id = Uuid::new_v4().to_string();
    log::info!("Peer connected: {} ({})", peer_id, addr);

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    peers.insert(
        peer_id.clone(),
        Peer {
            tx: tx.clone(),
            nickname: format!("Player_{}", &peer_id[..4]),
            room_id: None,
        },
    );

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    handle_client_message(&peer_id, client_msg, &rooms, &peers).await;
                }
            }
            Message::Binary(data) => {
                if data.len() < 4 { continue; }
                let conn_id_len = u16::from_be_bytes([data[0], data[1]]) as usize;
                if data.len() < 2 + conn_id_len { continue; }
                let connection_id = String::from_utf8_lossy(&data[2..2 + conn_id_len]).to_string();
                let payload = data[2 + conn_id_len..].to_vec();
                relay_binary_data(&peer_id, &connection_id, payload, &rooms, &peers).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    cleanup_peer(&peer_id, &rooms, &peers).await;
    peers.remove(&peer_id);
    send_task.abort();
    log::info!("Peer disconnected: {}", peer_id);
}

// ── Message handling ──────────────────────────────────

async fn handle_client_message(peer_id: &str, msg: ClientMessage, rooms: &Rooms, peers: &Peers) {
    match msg {
        ClientMessage::SetNickname { nickname } => {
            let nickname = nickname.trim().to_string();
            if nickname.is_empty() || nickname.len() > 32 { return; }

            if let Some(mut peer) = peers.get_mut(peer_id) {
                peer.nickname = nickname.clone();
            }

            // If in a room, update nicknames and broadcast
            let room_id = peers.get(peer_id).and_then(|p| p.room_id.clone());
            if let Some(room_id) = room_id {
                if let Some(mut room) = rooms.get_mut(&room_id) {
                    room.nicknames.insert(peer_id.to_string(), nickname);
                    broadcast_member_list(&room, peers);
                }
            }
        }

        ClientMessage::CreateRoom { password, game_info } => {
            let room_id = loop {
                let id = generate_room_id();
                if !rooms.contains_key(&id) { break id; }
            };

            let (tx, nickname) = match peers.get(peer_id) {
                Some(p) => (p.tx.clone(), p.nickname.clone()),
                None => return,
            };

            let mut members = HashMap::new();
            members.insert(peer_id.to_string(), tx.clone());
            let mut nicknames = HashMap::new();
            nicknames.insert(peer_id.to_string(), nickname.clone());

            rooms.insert(room_id.clone(), Room {
                password,
                host_id: peer_id.to_string(),
                game_info: game_info.clone(),
                members,
                nicknames,
            });

            if let Some(mut peer) = peers.get_mut(peer_id) {
                peer.room_id = Some(room_id.clone());
            }

            let member_list = vec![MemberInfo {
                peer_id: peer_id.to_string(),
                nickname,
                is_host: true,
            }];

            let _ = tx.send(ServerMessage::RoomCreated { room_id: room_id.clone() });
            let _ = tx.send(ServerMessage::RoomJoined {
                game_info,
                is_host: true,
                members: member_list,
            });

            log::info!("Room {} created by {}", room_id, peer_id);
        }

        ClientMessage::JoinRoom { room_id, password } => {
            let (tx, nickname) = match peers.get(peer_id) {
                Some(p) => (p.tx.clone(), p.nickname.clone()),
                None => return,
            };

            let (game_info, member_list) = {
                let mut room = match rooms.get_mut(&room_id) {
                    Some(r) => r,
                    None => {
                        let _ = tx.send(ServerMessage::Error { message: "房间不存在".into() });
                        return;
                    }
                };

                if room.password != password {
                    let _ = tx.send(ServerMessage::Error { message: "密码错误".into() });
                    return;
                }

                room.members.insert(peer_id.to_string(), tx.clone());
                room.nicknames.insert(peer_id.to_string(), nickname.clone());

                // Notify existing members
                for (mid, mtx) in &room.members {
                    if mid != peer_id {
                        let _ = mtx.send(ServerMessage::PeerJoined {
                            peer_id: peer_id.to_string(),
                            nickname: nickname.clone(),
                        });
                    }
                }

                let member_list = build_member_list(&room, peers);
                (room.game_info.clone(), member_list)
            };

            if let Some(mut peer) = peers.get_mut(peer_id) {
                peer.room_id = Some(room_id.clone());
            }

            let _ = tx.send(ServerMessage::RoomJoined {
                game_info,
                is_host: false,
                members: member_list,
            });

            log::info!("Peer {} joined room {}", peer_id, room_id);
        }

        ClientMessage::LeaveRoom => {
            cleanup_peer(peer_id, rooms, peers).await;
        }

        ClientMessage::ListRooms => {
            let tx = match peers.get(peer_id) {
                Some(p) => p.tx.clone(),
                None => return,
            };

            let mut listings = Vec::new();
            for entry in rooms.iter() {
                let room = entry.value();
                let host_name = room.nicknames.get(&room.host_id)
                    .cloned()
                    .unwrap_or_else(|| "Unknown".into());
                listings.push(RoomListing {
                    room_id: entry.key().clone(),
                    host_name,
                    game_motd: room.game_info.motd.clone(),
                    player_count: room.members.len(),
                    has_password: !room.password.is_empty(),
                });
            }

            let _ = tx.send(ServerMessage::RoomList { rooms: listings });
        }

        ClientMessage::GameData { connection_id, data } => {
            relay_game_data(peer_id, &connection_id, data, rooms, peers).await;
        }

        ClientMessage::NewConnection { connection_id } => {
            let room_id = peers.get(peer_id).and_then(|p| p.room_id.clone());
            if let Some(room_id) = room_id {
                if let Some(room) = rooms.get(&room_id) {
                    for (mid, mtx) in &room.members {
                        if mid != peer_id {
                            let _ = mtx.send(ServerMessage::NewConnection {
                                connection_id: connection_id.clone(),
                                from_peer: peer_id.to_string(),
                            });
                        }
                    }
                }
            }
        }

        ClientMessage::CloseConnection { connection_id } => {
            let room_id = peers.get(peer_id).and_then(|p| p.room_id.clone());
            if let Some(room_id) = room_id {
                if let Some(room) = rooms.get(&room_id) {
                    for (mid, mtx) in &room.members {
                        if mid != peer_id {
                            let _ = mtx.send(ServerMessage::CloseConnection {
                                connection_id: connection_id.clone(),
                                from_peer: peer_id.to_string(),
                            });
                        }
                    }
                }
            }
        }

        ClientMessage::UpdateGameInfo { game_info } => {
            let room_id = peers.get(peer_id).and_then(|p| p.room_id.clone());
            if let Some(room_id) = room_id {
                if let Some(mut room) = rooms.get_mut(&room_id) {
                    if room.host_id == peer_id {
                        room.game_info = game_info.clone();
                        for (mid, mtx) in &room.members {
                            if mid != peer_id {
                                let _ = mtx.send(ServerMessage::GameInfoUpdated { game_info: game_info.clone() });
                            }
                        }
                    }
                }
            }
        }

        ClientMessage::Heartbeat => {
            if let Some(peer) = peers.get(peer_id) {
                let _ = peer.tx.send(ServerMessage::HeartbeatAck);
            }
        }
    }
}

fn build_member_list(room: &Room, _peers: &Peers) -> Vec<MemberInfo> {
    room.nicknames.iter().map(|(pid, nick)| MemberInfo {
        peer_id: pid.clone(),
        nickname: nick.clone(),
        is_host: pid == &room.host_id,
    }).collect()
}

fn broadcast_member_list(room: &Room, peers: &Peers) {
    let members = build_member_list(room, peers);
    for (_, mtx) in &room.members {
        let _ = mtx.send(ServerMessage::MemberList { members: members.clone() });
    }
}

async fn relay_game_data(from_peer: &str, connection_id: &str, data: Vec<u8>, rooms: &Rooms, peers: &Peers) {
    let room_id = peers.get(from_peer).and_then(|p| p.room_id.clone());
    if let Some(room_id) = room_id {
        if let Some(room) = rooms.get(&room_id) {
            for (mid, mtx) in &room.members {
                if mid != from_peer {
                    let _ = mtx.send(ServerMessage::GameData {
                        connection_id: connection_id.to_string(),
                        from_peer: from_peer.to_string(),
                        data: data.clone(),
                    });
                }
            }
        }
    }
}

async fn relay_binary_data(from_peer: &str, connection_id: &str, payload: Vec<u8>, rooms: &Rooms, peers: &Peers) {
    let room_id = peers.get(from_peer).and_then(|p| p.room_id.clone());
    if let Some(room_id) = room_id {
        if let Some(room) = rooms.get(&room_id) {
            for (mid, _) in &room.members {
                if mid == from_peer { continue; }
                if let Some(peer) = peers.get(mid) {
                    let _ = peer.tx.send(ServerMessage::GameData {
                        connection_id: connection_id.to_string(),
                        from_peer: from_peer.to_string(),
                        data: payload.clone(),
                    });
                }
            }
        }
    }
}

async fn cleanup_peer(peer_id: &str, rooms: &Rooms, peers: &Peers) {
    let room_id = match peers.get(peer_id) {
        Some(p) => p.room_id.clone(),
        None => return,
    };

    if let Some(room_id) = room_id {
        let should_remove = {
            if let Some(mut room) = rooms.get_mut(&room_id) {
                room.members.remove(peer_id);
                room.nicknames.remove(peer_id);

                if room.host_id == peer_id {
                    for (_, mtx) in &room.members {
                        let _ = mtx.send(ServerMessage::RoomClosed);
                    }
                    true
                } else {
                    for (mid, mtx) in &room.members {
                        if mid != peer_id {
                            let _ = mtx.send(ServerMessage::PeerLeft { peer_id: peer_id.to_string() });
                        }
                    }
                    broadcast_member_list(&room, peers);
                    room.members.is_empty()
                }
            } else {
                false
            }
        };

        if should_remove {
            rooms.remove(&room_id);
            log::info!("Room {} removed", room_id);
        }

        if let Some(mut peer) = peers.get_mut(peer_id) {
            peer.room_id = None;
        }
    }
}
