import WebSocket from 'ws';

const SERVER = 'ws://127.0.0.1:9800';
const PASSWORD = 'test123';

function connect(name) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(SERVER);
    const msgs = [];
    ws.on('open', () => {
      console.log(`[${name}] connected`);
      resolve({ ws, msgs });
    });
    ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());
      msgs.push(msg);
      console.log(`[${name}] recv:`, JSON.stringify(msg));
    });
    ws.on('error', (e) => {
      console.error(`[${name}] error:`, e.message);
      reject(e);
    });
    ws.on('close', () => console.log(`[${name}] disconnected`));
  });
}

function send(ws, msg) {
  ws.send(JSON.stringify(msg));
}

function waitForMsg(msgs, type, timeout = 3000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const check = () => {
      const found = msgs.find(m => m.type === type);
      if (found) return resolve(found);
      if (Date.now() - start > timeout) return reject(new Error(`Timeout waiting for ${type}`));
      setTimeout(check, 50);
    };
    check();
  });
}

async function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

let passed = 0;
let failed = 0;
function assert(condition, label) {
  if (condition) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

async function main() {
  console.log('=== MCProxy Server Integration Test ===\n');

  // Test 1: Basic connection
  console.log('--- Test 1: Basic WebSocket Connection ---');
  let host, guest;
  try {
    host = await connect('HOST');
    assert(true, 'Host connected');
  } catch (e) {
    assert(false, `Host connection failed: ${e.message}`);
    process.exit(1);
  }

  // Test 2: Create room
  console.log('\n--- Test 2: Create Room ---');
  send(host.ws, {
    type: 'create_room',
    password: PASSWORD,
    game_info: { motd: 'Test World', port: 25565 },
  });

  try {
    const created = await waitForMsg(host.msgs, 'room_created');
    assert(!!created.room_id, `Room created with ID: ${created.room_id}`);
    assert(created.room_id.length === 6, `Room ID is 6 digits: ${created.room_id}`);

    const joined = await waitForMsg(host.msgs, 'room_joined');
    assert(joined.is_host === true, 'Host is_host = true');
    assert(joined.game_info.motd === 'Test World', 'Game info motd correct');
    assert(joined.game_info.port === 25565, 'Game info port correct');

    // Test 3: Join room
    console.log('\n--- Test 3: Join Room ---');
    guest = await connect('GUEST');
    assert(true, 'Guest connected');

    // Test 3a: Wrong password
    console.log('\n--- Test 3a: Wrong Password ---');
    send(guest.ws, {
      type: 'join_room',
      room_id: created.room_id,
      password: 'wrongpass',
    });
    const err = await waitForMsg(guest.msgs, 'error');
    assert(err.message === '密码错误', `Correct error for wrong password: ${err.message}`);

    // Test 3b: Non-existent room
    console.log('\n--- Test 3b: Non-existent Room ---');
    guest.msgs.length = 0;
    send(guest.ws, {
      type: 'join_room',
      room_id: '999999',
      password: PASSWORD,
    });
    const err2 = await waitForMsg(guest.msgs, 'error');
    assert(err2.message === '房间不存在', `Correct error for missing room: ${err2.message}`);

    // Test 3c: Correct join
    console.log('\n--- Test 3c: Correct Join ---');
    guest.msgs.length = 0;
    host.msgs.length = 0;
    send(guest.ws, {
      type: 'join_room',
      room_id: created.room_id,
      password: PASSWORD,
    });

    const guestJoined = await waitForMsg(guest.msgs, 'room_joined');
    assert(guestJoined.is_host === false, 'Guest is_host = false');
    assert(guestJoined.game_info.motd === 'Test World', 'Guest sees correct game info');

    const peerJoined = await waitForMsg(host.msgs, 'peer_joined');
    assert(!!peerJoined.peer_id, `Host notified of peer join: ${peerJoined.peer_id.slice(0, 8)}...`);

    // Test 4: Game data relay
    console.log('\n--- Test 4: Game Data Relay ---');
    host.msgs.length = 0;
    guest.msgs.length = 0;

    send(guest.ws, {
      type: 'new_connection',
      connection_id: 'conn-001',
    });
    const newConn = await waitForMsg(host.msgs, 'new_connection');
    assert(newConn.connection_id === 'conn-001', 'Host received new_connection');

    // Send game data from guest to host
    host.msgs.length = 0;
    send(guest.ws, {
      type: 'game_data',
      connection_id: 'conn-001',
      data: Array.from(Buffer.from('hello mc')),
    });
    const gameData = await waitForMsg(host.msgs, 'game_data');
    assert(gameData.connection_id === 'conn-001', 'Game data connection_id correct');
    const receivedStr = Buffer.from(gameData.data).toString();
    assert(receivedStr === 'hello mc', `Game data payload correct: "${receivedStr}"`);

    // Send data in reverse direction
    guest.msgs.length = 0;
    send(host.ws, {
      type: 'game_data',
      connection_id: 'conn-001',
      data: Array.from(Buffer.from('world reply')),
    });
    const gameData2 = await waitForMsg(guest.msgs, 'game_data');
    const receivedStr2 = Buffer.from(gameData2.data).toString();
    assert(receivedStr2 === 'world reply', `Reverse relay correct: "${receivedStr2}"`);

    // Test 5: Binary frame relay
    console.log('\n--- Test 5: Binary Frame Relay ---');
    host.msgs.length = 0;
    const connIdBytes = Buffer.from('conn-bin-01');
    const payload = Buffer.from('binary game packet data');
    const frame = Buffer.alloc(2 + connIdBytes.length + payload.length);
    frame.writeUInt16BE(connIdBytes.length, 0);
    connIdBytes.copy(frame, 2);
    payload.copy(frame, 2 + connIdBytes.length);
    guest.ws.send(frame);

    await sleep(500);
    // Binary frames are forwarded as game_data server messages
    const binRelay = host.msgs.find(m => m.type === 'game_data' && m.connection_id === 'conn-bin-01');
    assert(!!binRelay, 'Binary frame relayed to host');
    if (binRelay) {
      const binPayload = Buffer.from(binRelay.data).toString();
      assert(binPayload === 'binary game packet data', `Binary payload correct: "${binPayload}"`);
    }

    // Test 6: Heartbeat
    console.log('\n--- Test 6: Heartbeat ---');
    host.msgs.length = 0;
    send(host.ws, { type: 'heartbeat' });
    const hbAck = await waitForMsg(host.msgs, 'heartbeat_ack');
    assert(!!hbAck, 'Heartbeat acknowledged');

    // Test 7: Close connection
    console.log('\n--- Test 7: Close Connection ---');
    host.msgs.length = 0;
    send(guest.ws, {
      type: 'close_connection',
      connection_id: 'conn-001',
    });
    const closedConn = await waitForMsg(host.msgs, 'close_connection');
    assert(closedConn.connection_id === 'conn-001', 'Close connection relayed');

    // Test 8: Leave room
    console.log('\n--- Test 8: Leave Room ---');
    host.msgs.length = 0;
    send(guest.ws, { type: 'leave_room' });
    const peerLeft = await waitForMsg(host.msgs, 'peer_left');
    assert(!!peerLeft.peer_id, 'Host notified of peer leave');

    // Test 9: Room closes when host leaves
    console.log('\n--- Test 9: Room Close on Host Leave ---');
    // Re-join first
    guest.msgs.length = 0;
    send(guest.ws, {
      type: 'join_room',
      room_id: created.room_id,
      password: PASSWORD,
    });
    await waitForMsg(guest.msgs, 'room_joined');

    guest.msgs.length = 0;
    send(host.ws, { type: 'leave_room' });
    const roomClosed = await waitForMsg(guest.msgs, 'room_closed');
    assert(!!roomClosed, 'Guest notified when host closes room');

  } catch (e) {
    console.error(`  ✗ Test failed:`, e.message);
    failed++;
  }

  // Cleanup
  if (host) host.ws.close();
  if (guest) guest.ws.close();

  console.log(`\n=== Results: ${passed} passed, ${failed} failed ===`);
  process.exit(failed > 0 ? 1 : 0);
}

main();
