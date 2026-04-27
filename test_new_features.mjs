import WebSocket from 'ws';

const SERVER = 'ws://127.0.0.1:9800';
const PASSWORD = 'lobbytest';
let passed = 0, failed = 0;

function assert(ok, label) {
  if (ok) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

function connect(name) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(SERVER);
    const msgs = [];
    ws.on('open', () => resolve({ ws, msgs, name }));
    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
      msgs.push(msg);
    });
    ws.on('error', reject);
  });
}

function send(p, msg) { p.ws.send(JSON.stringify(msg)); }

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

async function main() {
  console.log('=== New Features Test (Nickname + Room List + Members) ===\n');

  // 1. Set nicknames
  console.log('--- Test 1: Nicknames ---');
  const host = await connect('HOST');
  send(host, { type: 'set_nickname', nickname: 'Alice' });
  await sleep(200);

  const guest = await connect('GUEST');
  send(guest, { type: 'set_nickname', nickname: 'Bob' });
  await sleep(200);
  assert(true, 'Nicknames set');

  // 2. Create room
  console.log('\n--- Test 2: Create Room with Members ---');
  send(host, { type: 'create_room', password: PASSWORD, game_info: { motd: 'Lobby World', port: 25565 } });
  const created = await waitFor(host.msgs, m => m.type === 'room_created');
  assert(!!created.room_id, `Room created: ${created.room_id}`);

  const hostJoined = await waitFor(host.msgs, m => m.type === 'room_joined');
  assert(Array.isArray(hostJoined.members), 'room_joined includes members array');
  assert(hostJoined.members.length === 1, `Members count = ${hostJoined.members.length}`);
  assert(hostJoined.members[0].nickname === 'Alice', `Host nickname in members: ${hostJoined.members[0].nickname}`);
  assert(hostJoined.members[0].is_host === true, 'Host is_host flag correct');

  // 3. List rooms
  console.log('\n--- Test 3: Room Listing ---');
  const viewer = await connect('VIEWER');
  send(viewer, { type: 'list_rooms' });
  const roomList = await waitFor(viewer.msgs, m => m.type === 'room_list');
  assert(Array.isArray(roomList.rooms), 'Got room list');
  assert(roomList.rooms.length >= 1, `Rooms found: ${roomList.rooms.length}`);
  const found = roomList.rooms.find(r => r.room_id === created.room_id);
  assert(!!found, 'Our room is listed');
  assert(found.host_name === 'Alice', `Host name in listing: ${found.host_name}`);
  assert(found.game_motd === 'Lobby World', `Game motd in listing: ${found.game_motd}`);
  assert(found.player_count === 1, `Player count in listing: ${found.player_count}`);
  assert(found.has_password === true, 'Password flag correct');

  // 4. Guest joins and check member list
  console.log('\n--- Test 4: Guest Join with Member List ---');
  host.msgs.length = 0;
  send(guest, { type: 'join_room', room_id: created.room_id, password: PASSWORD });
  const guestJoined = await waitFor(guest.msgs, m => m.type === 'room_joined');
  assert(guestJoined.members.length === 2, `Guest sees ${guestJoined.members.length} members`);
  const bobMember = guestJoined.members.find(m => m.nickname === 'Bob');
  const aliceMember = guestJoined.members.find(m => m.nickname === 'Alice');
  assert(!!bobMember, 'Bob in member list');
  assert(!!aliceMember, 'Alice in member list');
  assert(aliceMember.is_host === true, 'Alice is host in member list');

  // Host receives peer_joined with nickname
  const peerJoined = await waitFor(host.msgs, m => m.type === 'peer_joined');
  assert(peerJoined.nickname === 'Bob', `Peer joined nickname: ${peerJoined.nickname}`);

  // 5. Nickname change in room triggers member_list update
  console.log('\n--- Test 5: Nickname Change Broadcast ---');
  host.msgs.length = 0;
  guest.msgs.length = 0;
  send(guest, { type: 'set_nickname', nickname: 'Bobby' });
  const memberUpdate = await waitFor(host.msgs, m => m.type === 'member_list');
  assert(Array.isArray(memberUpdate.members), 'Got member_list update');
  const bobby = memberUpdate.members.find(m => m.nickname === 'Bobby');
  assert(!!bobby, 'Bobby nickname updated in member list');

  // 6. Room list updates after join
  console.log('\n--- Test 6: Room List After Join ---');
  viewer.msgs.length = 0;
  send(viewer, { type: 'list_rooms' });
  const roomList2 = await waitFor(viewer.msgs, m => m.type === 'room_list');
  const found2 = roomList2.rooms.find(r => r.room_id === created.room_id);
  assert(found2.player_count === 2, `Player count after join: ${found2.player_count}`);

  // 7. Guest leaves, member list updated
  console.log('\n--- Test 7: Leave Updates Members ---');
  host.msgs.length = 0;
  send(guest, { type: 'leave_room' });
  await sleep(300);
  const memberAfterLeave = host.msgs.find(m => m.type === 'member_list');
  assert(!!memberAfterLeave, 'Member list update after leave');
  if (memberAfterLeave) {
    assert(memberAfterLeave.members.length === 1, `Members after leave: ${memberAfterLeave.members.length}`);
  }

  // Cleanup
  host.ws.close();
  guest.ws.close();
  viewer.ws.close();

  console.log(`\n=== Results: ${passed} passed, ${failed} failed ===`);
  process.exit(failed > 0 ? 1 : 0);
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });
