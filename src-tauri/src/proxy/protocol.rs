use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub motd: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub peer_id: String,
    pub nickname: String,
    pub is_host: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomListing {
    pub room_id: String,
    pub host_name: String,
    pub game_motd: String,
    pub player_count: usize,
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
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
pub enum ServerMessage {
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
