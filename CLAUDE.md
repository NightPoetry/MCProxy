# MCProxy

Minecraft LAN proxy - enables virtual LAN multiplayer over the internet.

## Architecture

- `server/` - Relay server (standalone Rust binary, WebSocket on port 9800)
- `src-tauri/` - Tauri 2.x client backend (Rust: LAN scan, TCP tunnel, WebSocket)
- `src/` - React + TypeScript frontend (Fluent/Microsoft design style)

## How It Works

1. **Host**: Opens MC to LAN → MCProxy detects via UDP 4445 → Creates room on relay server → Tunnels incoming TCP connections to local MC
2. **Guest**: Joins room by ID+password → MCProxy creates local TCP proxy → Broadcasts fake LAN discovery → MC connects to local proxy → Data tunneled through relay to host

## Dev Commands

- `npm run tauri dev` — Start client in dev mode
- `cd server && cargo run` — Start relay server
- `npx tsc --noEmit` — Type check frontend

## Key Ports

- 9800: Relay server WebSocket
- 4445: MC LAN discovery (UDP)
- Dynamic: Local TCP proxy for guest connections
