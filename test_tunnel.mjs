import WebSocket from 'ws';
import net from 'net';

const SERVER = 'ws://127.0.0.1:9800';
const PASSWORD = 'tunnel-test';
const MC_PORT = 25599;

function connect(name) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(SERVER);
    const msgs = [];
    ws.on('open', () => {
      console.log(`[${name}] ws connected`);
      resolve({ ws, msgs, name });
    });
    ws.on('message', (raw) => {
      if (Buffer.isBuffer(raw) || raw instanceof ArrayBuffer) {
        const buf = Buffer.from(raw);
        if (buf.length >= 2) {
          const idLen = buf.readUInt16BE(0);
          if (buf.length >= 2 + idLen) {
            const connId = buf.subarray(2, 2 + idLen).toString();
            const payload = buf.subarray(2 + idLen);
            msgs.push({ _binary: true, connection_id: connId, data: payload });
            console.log(`[${name}] recv binary: conn=${connId} len=${payload.length}`);
            return;
          }
        }
      }
      const msg = JSON.parse(raw.toString());
      msgs.push(msg);
      console.log(`[${name}] recv:`, JSON.stringify(msg).slice(0, 120));
    });
    ws.on('error', (e) => reject(e));
    ws.on('close', () => console.log(`[${name}] ws disconnected`));
  });
}

function send(peer, msg) {
  peer.ws.send(JSON.stringify(msg));
}

function sendBinary(peer, connId, payload) {
  const idBuf = Buffer.from(connId);
  const frame = Buffer.alloc(2 + idBuf.length + payload.length);
  frame.writeUInt16BE(idBuf.length, 0);
  idBuf.copy(frame, 2);
  payload.copy(frame, 2 + idBuf.length);
  peer.ws.send(frame);
}

function waitFor(msgs, pred, timeout = 3000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const check = () => {
      const found = msgs.find(pred);
      if (found) return resolve(found);
      if (Date.now() - start > timeout) return reject(new Error('Timeout'));
      setTimeout(check, 50);
    };
    check();
  });
}

const sleep = (ms) => new Promise(r => setTimeout(r, ms));

let passed = 0, failed = 0;
function assert(ok, label) {
  if (ok) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

async function main() {
  console.log('=== MCProxy End-to-End Tunnel Test ===\n');

  // 1) Start a fake MC server on MC_PORT
  console.log('--- Step 1: Start Fake MC Server ---');
  let mcServerData = [];
  const mcServer = net.createServer((sock) => {
    console.log('[MC-SERVER] client connected');
    sock.on('data', (chunk) => {
      console.log(`[MC-SERVER] recv ${chunk.length} bytes: "${chunk.toString()}"`);
      mcServerData.push(chunk);
      // Echo back with prefix
      sock.write(Buffer.from(`ECHO:${chunk.toString()}`));
    });
    sock.on('close', () => console.log('[MC-SERVER] client disconnected'));
  });
  await new Promise(r => mcServer.listen(MC_PORT, '127.0.0.1', r));
  assert(true, `Fake MC server listening on ${MC_PORT}`);

  // 2) Connect two peers as host and guest
  console.log('\n--- Step 2: Connect Peers ---');
  const host = await connect('HOST');
  const guest = await connect('GUEST');
  assert(true, 'Both peers connected');

  // 3) Host creates room
  console.log('\n--- Step 3: Host Creates Room ---');
  send(host, {
    type: 'create_room',
    password: PASSWORD,
    game_info: { motd: 'Tunnel Test', port: MC_PORT },
  });
  const created = await waitFor(host.msgs, m => m.type === 'room_created');
  assert(!!created.room_id, `Room ${created.room_id} created`);

  // 4) Guest joins room
  console.log('\n--- Step 4: Guest Joins Room ---');
  send(guest, {
    type: 'join_room',
    room_id: created.room_id,
    password: PASSWORD,
  });
  const guestJoined = await waitFor(guest.msgs, m => m.type === 'room_joined');
  assert(guestJoined.game_info.port === MC_PORT, 'Guest received correct MC port');

  await sleep(200);

  // 5) Simulate tunnel: guest opens a connection, host connects to MC server
  console.log('\n--- Step 5: Simulate Full Tunnel Flow ---');
  const connId = 'tunnel-conn-001';

  // Guest announces new connection
  send(guest, { type: 'new_connection', connection_id: connId });
  const newConn = await waitFor(host.msgs, m => m.type === 'new_connection' && m.connection_id === connId);
  assert(!!newConn, 'Host received new_connection');

  // Host side: connect to local MC server (simulating what Tauri host does)
  const mcClient = net.createConnection({ host: '127.0.0.1', port: MC_PORT });
  await new Promise(r => mcClient.on('connect', r));
  assert(true, 'Host-side connected to MC server');

  // Set up relay: MC server responses → host → relay → guest
  let guestReceivedData = [];
  mcClient.on('data', (chunk) => {
    console.log(`[HOST-MC-CLIENT] MC server replied: "${chunk.toString()}"`);
    // Forward to guest via relay
    send(host, { type: 'game_data', connection_id: connId, data: Array.from(chunk) });
  });

  // Guest sends game data → relay → host → MC server
  const testPayload = 'Hello from guest player!';
  send(guest, {
    type: 'game_data',
    connection_id: connId,
    data: Array.from(Buffer.from(testPayload)),
  });

  // Host receives game data from relay
  const relayedData = await waitFor(host.msgs, m => m.type === 'game_data' && m.connection_id === connId);
  assert(!!relayedData, 'Host received relayed game data');

  // Host forwards to MC server
  const payload = Buffer.from(relayedData.data);
  mcClient.write(payload);
  assert(true, `Host forwarded ${payload.length} bytes to MC server`);

  // Wait for MC server to echo back
  await sleep(300);
  assert(mcServerData.length > 0, 'MC server received data');
  assert(mcServerData[0].toString() === testPayload, `MC server got: "${mcServerData[0]?.toString()}"`);

  // Check that host forwarded MC server's reply back to guest
  const guestReply = await waitFor(guest.msgs, m => m.type === 'game_data' && m.connection_id === connId);
  assert(!!guestReply, 'Guest received MC server reply via relay');
  const replyStr = Buffer.from(guestReply.data).toString();
  assert(replyStr === `ECHO:${testPayload}`, `Full round-trip data: "${replyStr}"`);

  // 6) Test binary frame tunnel
  console.log('\n--- Step 6: Binary Frame Tunnel ---');
  host.msgs.length = 0;
  const binPayload = Buffer.from('binary MC packet');
  sendBinary(guest, connId, binPayload);
  await sleep(300);
  const binRelayed = host.msgs.find(m => m.type === 'game_data' && m.connection_id === connId);
  assert(!!binRelayed, 'Binary frame relayed');
  if (binRelayed) {
    const binStr = Buffer.from(binRelayed.data).toString();
    assert(binStr === 'binary MC packet', `Binary payload intact: "${binStr}"`);
  }

  // 7) Close connection
  console.log('\n--- Step 7: Connection Close ---');
  host.msgs.length = 0;
  send(guest, { type: 'close_connection', connection_id: connId });
  const closed = await waitFor(host.msgs, m => m.type === 'close_connection');
  assert(closed.connection_id === connId, 'Connection close relayed');

  // Cleanup
  mcClient.destroy();
  mcServer.close();
  host.ws.close();
  guest.ws.close();

  console.log(`\n=== Results: ${passed} passed, ${failed} failed ===`);
  process.exit(failed > 0 ? 1 : 0);
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });
