use super::ProxyState;
use super::protocol::ClientMessage;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

pub async fn guest_handle_connection(
    state: Arc<ProxyState>,
    connection_id: String,
    stream: TcpStream,
) {
    let (data_tx, mut data_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    state
        .connections
        .lock()
        .await
        .insert(connection_id.clone(), data_tx);

    let _ = state
        .send_ws(ClientMessage::NewConnection {
            connection_id: connection_id.clone(),
        })
        .await;

    let (mut read_half, mut write_half) = stream.into_split();

    let state_read = Arc::clone(&state);
    let conn_id_read = connection_id.clone();
    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if state_read
                        .send_binary(&conn_id_read, &buf[..n])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let write_task = tokio::spawn(async move {
        while let Some(data) = data_rx.recv().await {
            if write_half.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    tokio::select! {
        _ = read_task => {},
        _ = write_task => {},
    }

    state.connections.lock().await.remove(&connection_id);
    let _ = state
        .send_ws(ClientMessage::CloseConnection {
            connection_id: connection_id.clone(),
        })
        .await;
}

pub async fn host_handle_new_connection(
    state: Arc<ProxyState>,
    connection_id: String,
    mc_port: u16,
) {
    let stream = match TcpStream::connect(format!("127.0.0.1:{}", mc_port)).await {
        Ok(s) => s,
        Err(_) => {
            let _ = state
                .send_ws(ClientMessage::CloseConnection {
                    connection_id: connection_id.clone(),
                })
                .await;
            return;
        }
    };

    let (data_tx, mut data_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    state
        .connections
        .lock()
        .await
        .insert(connection_id.clone(), data_tx);

    let (mut read_half, mut write_half) = stream.into_split();

    let state_read = Arc::clone(&state);
    let conn_id_read = connection_id.clone();
    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if state_read
                        .send_binary(&conn_id_read, &buf[..n])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let write_task = tokio::spawn(async move {
        while let Some(data) = data_rx.recv().await {
            if write_half.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    tokio::select! {
        _ = read_task => {},
        _ = write_task => {},
    }

    state.connections.lock().await.remove(&connection_id);
    let _ = state
        .send_ws(ClientMessage::CloseConnection {
            connection_id: connection_id.clone(),
        })
        .await;
}
