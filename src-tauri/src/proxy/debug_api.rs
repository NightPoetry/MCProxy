use super::ProxyState;
use crate::GameInfo;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

pub async fn start_debug_server(state: Arc<ProxyState>, port: u16) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Debug API failed to bind port {}: {}", port, e);
            return;
        }
    };
    log::info!("Debug API listening on http://127.0.0.1:{}", port);

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut buf_reader = BufReader::new(reader);
            let mut request_line = String::new();
            if buf_reader.read_line(&mut request_line).await.is_err() {
                return;
            }

            let mut headers = String::new();
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                if buf_reader.read_line(&mut line).await.is_err() {
                    return;
                }
                if line.trim().is_empty() {
                    break;
                }
                if let Some(val) = line.strip_prefix("Content-Length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
                if let Some(val) = line.strip_prefix("content-length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
                headers.push_str(&line);
            }

            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                let _ = tokio::io::AsyncReadExt::read_exact(&mut buf_reader, &mut body).await;
            }
            let body_str = String::from_utf8_lossy(&body).to_string();

            let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
            let (method, path) = if parts.len() >= 2 {
                (parts[0], parts[1])
            } else {
                ("GET", "/")
            };

            let response_body = handle_request(&state, method, path, &body_str).await;

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = writer.write_all(response.as_bytes()).await;
        });
    }
}

async fn handle_request(
    state: &Arc<ProxyState>,
    method: &str,
    path: &str,
    body: &str,
) -> String {
    if method == "OPTIONS" {
        return "{}".to_string();
    }

    match path {
        "/debug/ping" => {
            r#"{"ok":true,"service":"mcproxy"}"#.to_string()
        }

        "/debug/status" => {
            let status = state.get_status().await;
            serde_json::to_string(&status).unwrap_or_else(|_| r#"{"error":"serialize"}"#.to_string())
        }

        "/debug/connect" => {
            #[derive(serde::Deserialize)]
            struct Req { server_url: String }
            match serde_json::from_str::<Req>(body) {
                Ok(req) => match state.connect(&req.server_url).await {
                    Ok(()) => r#"{"ok":true}"#.to_string(),
                    Err(e) => format!(r#"{{"error":"{}"}}"#, e.replace('"', "'")),
                },
                Err(e) => format!(r#"{{"error":"bad request: {}"}}"#, e),
            }
        }

        "/debug/disconnect" => {
            state.disconnect().await;
            r#"{"ok":true}"#.to_string()
        }

        "/debug/set_lan_game" => {
            #[derive(serde::Deserialize)]
            struct Req { motd: String, port: u16 }
            match serde_json::from_str::<Req>(body) {
                Ok(req) => {
                    *state.lan_game.write().await = Some(GameInfo {
                        motd: req.motd,
                        port: req.port,
                    });
                    r#"{"ok":true}"#.to_string()
                }
                Err(e) => format!(r#"{{"error":"bad request: {}"}}"#, e),
            }
        }

        "/debug/create_room" => {
            #[derive(serde::Deserialize)]
            struct Req { password: String }
            match serde_json::from_str::<Req>(body) {
                Ok(req) => match state.create_room(req.password).await {
                    Ok(()) => r#"{"ok":true}"#.to_string(),
                    Err(e) => format!(r#"{{"error":"{}"}}"#, e.replace('"', "'")),
                },
                Err(e) => format!(r#"{{"error":"bad request: {}"}}"#, e),
            }
        }

        "/debug/join_room" => {
            #[derive(serde::Deserialize)]
            struct Req { room_id: String, password: String }
            match serde_json::from_str::<Req>(body) {
                Ok(req) => match state.join_room(req.room_id, req.password).await {
                    Ok(()) => r#"{"ok":true}"#.to_string(),
                    Err(e) => format!(r#"{{"error":"{}"}}"#, e.replace('"', "'")),
                },
                Err(e) => format!(r#"{{"error":"bad request: {}"}}"#, e),
            }
        }

        "/debug/leave_room" => {
            match state.leave_room().await {
                Ok(()) => r#"{"ok":true}"#.to_string(),
                Err(e) => format!(r#"{{"error":"{}"}}"#, e.replace('"', "'")),
            }
        }

        "/debug/events" => {
            let events = state.get_event_log().await;
            serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string())
        }

        "/debug/clear_events" => {
            state.clear_event_log().await;
            r#"{"ok":true}"#.to_string()
        }

        _ => {
            r#"{"error":"not found","endpoints":["/debug/ping","/debug/status","/debug/connect","/debug/disconnect","/debug/set_lan_game","/debug/create_room","/debug/join_room","/debug/leave_room","/debug/events","/debug/clear_events"]}"#.to_string()
        }
    }
}
