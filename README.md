# MCProxy

Minecraft LAN proxy — play LAN multiplayer over the internet via relay server.

## Architecture

```
Player A (Host)                    Relay Server                   Player B (Client)
+-----------+     WebSocket      +------------+     WebSocket     +-----------+
| MC Server | <-> | MCProxy | <---> | Port 9800 | <---> | MCProxy | <-> | MC Client |
+-----------+     +----------+     +------------+     +----------+     +-----------+
```

## Quick Start

### 1. Deploy Relay Server

```bash
cd server
cargo build --release
# Binary at: target/release/mcproxy-server
```

Run it:
```bash
./mcproxy-server                    # listens on 0.0.0.0:9800
BIND_ADDR=0.0.0.0:8080 ./mcproxy-server  # custom port
```

### 2. Client App

```bash
npm install
npm run tauri dev      # development
npm run tauri build    # production (.app / .dmg)
```

## Server Deployment

See [server/DEPLOY.md](server/DEPLOY.md) for full deployment guide.

**TL;DR** — it's a single static binary, copy it to your server and run:
```bash
scp server/target/release/mcproxy-server user@your-server:~/
ssh user@your-server './mcproxy-server'
```

## Features

- Room-based matchmaking with 6-digit room codes
- Password protection
- Room lobby browser
- Nickname support with real-time member list
- TCP tunnel over WebSocket for game traffic
- Binary frame optimization for game data
- Dark / Cream theme toggle
- Chinese / English language toggle

## Tech Stack

- **Client**: Tauri 2 + React + TypeScript + Rust
- **Server**: Rust + Tokio + WebSocket
