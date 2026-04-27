use crate::{GameInfo, ProxyEvent};
use std::sync::Arc;
use tokio::net::UdpSocket;

use super::ProxyState;

pub async fn scan_lan_games(
    state: Arc<ProxyState>,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) {
    let socket = match UdpSocket::bind("0.0.0.0:4445").await {
        Ok(s) => s,
        Err(e) => {
            state.emit(ProxyEvent::Error {
                message: format!("无法绑定 UDP 4445 端口: {}。请确保没有其他 Minecraft 实例正在运行。", e),
            });
            *state.scanning.write().await = false;
            return;
        }
    };

    let _ = socket.set_broadcast(true);

    let mut buf = [0u8; 1024];

    state.emit(ProxyEvent::StatusUpdate {
        status: "scanning".to_string(),
        detail: "正在扫描局域网游戏...".to_string(),
    });

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, _addr)) => {
                        let data = String::from_utf8_lossy(&buf[..len]);
                        if let Some(game) = parse_lan_broadcast(&data) {
                            *state.lan_game.write().await = Some(game.clone());
                            state.emit(ProxyEvent::LanGameFound {
                                motd: game.motd,
                                port: game.port,
                            });
                        }
                    }
                    Err(_) => break,
                }
            }
            _ = &mut cancel => {
                break;
            }
        }
    }

    *state.scanning.write().await = false;
}

fn parse_lan_broadcast(data: &str) -> Option<GameInfo> {
    let motd_start = data.find("[MOTD]")? + 6;
    let motd_end = data.find("[/MOTD]")?;
    let port_start = data.find("[AD]")? + 4;
    let port_end = data.find("[/AD]")?;

    let motd = data[motd_start..motd_end].to_string();
    let port: u16 = data[port_start..port_end].parse().ok()?;

    Some(GameInfo { motd, port })
}

pub async fn broadcast_fake_lan(local_port: u16, motd: &str) {
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = socket.set_broadcast(true);

    let message = format!("[MOTD]{}[/MOTD][AD]{}[/AD]", motd, local_port);

    loop {
        let _ = socket
            .send_to(message.as_bytes(), "224.0.2.60:4445")
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    }
}
