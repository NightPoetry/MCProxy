# MCProxy Server Deployment Guide

## Overview

MCProxy relay server is a single Rust binary (~2.5MB), no dependencies, no config file needed. It listens for WebSocket connections on port 9800 and relays game data between clients in the same room.

## Option 1: Direct Binary

### Build for your target platform

```bash
cd server
cargo build --release
# Output: target/release/mcproxy-server
```

Cross-compile for Linux (from macOS):
```bash
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

### Upload and run

```bash
scp target/release/mcproxy-server user@your-server:~/mcproxy-server
ssh user@your-server
chmod +x mcproxy-server
./mcproxy-server
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BIND_ADDR` | `0.0.0.0:9800` | Listen address and port |
| `RUST_LOG` | `info` | Log level (error/warn/info/debug/trace) |

Example:
```bash
BIND_ADDR=0.0.0.0:443 RUST_LOG=debug ./mcproxy-server
```

## Option 2: systemd Service (Linux)

Create `/etc/systemd/system/mcproxy.service`:

```ini
[Unit]
Description=MCProxy Relay Server
After=network.target

[Service]
Type=simple
User=mcproxy
ExecStart=/opt/mcproxy/mcproxy-server
Environment=BIND_ADDR=0.0.0.0:9800
Environment=RUST_LOG=info
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Then:
```bash
sudo useradd -r -s /bin/false mcproxy
sudo mkdir -p /opt/mcproxy
sudo cp mcproxy-server /opt/mcproxy/
sudo systemctl daemon-reload
sudo systemctl enable --now mcproxy
sudo systemctl status mcproxy
```

## Option 3: Docker

Create `Dockerfile` in the `server/` directory:

```dockerfile
FROM rust:1.83-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /app/target/release/mcproxy-server /usr/local/bin/
EXPOSE 9800
CMD ["mcproxy-server"]
```

Build and run:
```bash
docker build -t mcproxy-server .
docker run -d -p 9800:9800 --name mcproxy mcproxy-server
```

## Firewall

Open port 9800 (or your custom port):

```bash
# UFW (Ubuntu)
sudo ufw allow 9800/tcp

# firewalld (CentOS/RHEL)
sudo firewall-cmd --add-port=9800/tcp --permanent
sudo firewall-cmd --reload

# Cloud provider security group
# Add inbound rule: TCP 9800 from 0.0.0.0/0
```

## Reverse Proxy (Optional, for WSS)

Nginx config for WebSocket + TLS:

```nginx
server {
    listen 443 ssl;
    server_name mc.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://127.0.0.1:9800;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 86400;
    }
}
```

Then clients connect with `wss://mc.example.com`.

## Verify

```bash
# Check if server is running
curl -i --no-buffer \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" \
  -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  http://your-server:9800/
# Should return: HTTP/1.1 101 Switching Protocols
```

## Resource Usage

- Memory: ~5MB idle, ~10-20MB under load
- CPU: negligible for typical usage (< 50 rooms)
- Bandwidth: proportional to game traffic (Minecraft LAN uses ~50-200 KB/s per player)
