#!/bin/bash

API="http://127.0.0.1:9801"
RELAY="ws://127.0.0.1:9800"
PASS=0
FAIL=0

assert() {
  local label="$1"
  local condition="$2"
  if eval "$condition" 2>/dev/null; then
    echo "  ✓ $label"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $label"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== MCProxy E2E Test via Debug API ==="
echo ""

# Test 1: Ping
echo "--- Test 1: Debug API Ping ---"
PING=$(curl -s $API/debug/ping)
assert "Ping returns ok" '[ "$(echo $PING | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

# Test 2: Initial status
echo ""
echo "--- Test 2: Initial Status ---"
STATUS=$(curl -s $API/debug/status)
echo "  Status: $STATUS"
CONNECTED=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('connected'))")
assert "Initially disconnected" '[ "$CONNECTED" = "False" ]'

# Test 3: Connect to relay server
echo ""
echo "--- Test 3: Connect to Relay Server ---"
CONNECT=$(curl -s -X POST $API/debug/connect -d "{\"server_url\":\"$RELAY\"}")
echo "  Connect result: $CONNECT"
assert "Connect returns ok" '[ "$(echo $CONNECT | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

sleep 0.5
STATUS=$(curl -s $API/debug/status)
CONNECTED=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('connected'))")
assert "Status shows connected" '[ "$CONNECTED" = "True" ]'

# Test 4: Set fake LAN game (bypass real scanning)
echo ""
echo "--- Test 4: Set Fake LAN Game ---"
SET_LAN=$(curl -s -X POST $API/debug/set_lan_game -d '{"motd":"Debug Test World","port":25577}')
echo "  Set LAN result: $SET_LAN"
assert "Set LAN game ok" '[ "$(echo $SET_LAN | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

STATUS=$(curl -s $API/debug/status)
LAN_MOTD=$(echo $STATUS | python3 -c "import sys,json;g=json.load(sys.stdin).get('lan_game');print(g.get('motd') if g else 'None')")
assert "Status shows LAN game" '[ "$LAN_MOTD" = "Debug Test World" ]'

# Test 5: Create room
echo ""
echo "--- Test 5: Create Room ---"
CREATE=$(curl -s -X POST $API/debug/create_room -d '{"password":"test123"}')
echo "  Create result: $CREATE"
assert "Create room ok" '[ "$(echo $CREATE | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

sleep 0.5
STATUS=$(curl -s $API/debug/status)
echo "  Status: $STATUS"
ROOM_ID=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('room_id','None'))")
IS_HOST=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('is_host'))")
assert "Room ID is set" '[ "$ROOM_ID" != "None" ] && [ "$ROOM_ID" != "null" ]'
assert "Is host" '[ "$IS_HOST" = "True" ]'
echo "  Room ID: $ROOM_ID"

# Test 6: Check event log
echo ""
echo "--- Test 6: Event Log ---"
EVENTS=$(curl -s $API/debug/events)
EVENT_COUNT=$(echo $EVENTS | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")
echo "  Event count: $EVENT_COUNT"
assert "Events were logged" '[ "$EVENT_COUNT" -gt 0 ]'

# Show all events
echo "  Events:"
echo $EVENTS | python3 -c "
import sys, json
events = json.load(sys.stdin)
for e in events:
    print(f'    - {e.get(\"type\")}: {json.dumps(e, ensure_ascii=False)[:100]}')
"

# Test 7: A second client joins via WebSocket (simulated)
echo ""
echo "--- Test 7: Second Client Joins (via Node.js) ---"
JOIN_RESULT=$(node -e "
const WebSocket = require('ws');
const ws = new WebSocket('$RELAY');
let done = false;
const timer = setTimeout(() => { if(!done){done=true;console.log('{\"ok\":false,\"error\":\"timeout\"}');process.exit(1);} }, 5000);
ws.on('open', () => {
  ws.send(JSON.stringify({type:'join_room', room_id:'$ROOM_ID', password:'test123'}));
});
ws.on('message', (data) => {
  if(done) return;
  const msg = JSON.parse(data.toString());
  if (msg.type === 'room_joined') {
    done=true; clearTimeout(timer);
    console.log(JSON.stringify({ok:true, is_host:msg.is_host, motd:msg.game_info.motd}));
    setTimeout(() => { ws.close(); process.exit(0); }, 200);
  } else if (msg.type === 'error') {
    done=true; clearTimeout(timer);
    console.log(JSON.stringify({ok:false, error:msg.message}));
    ws.close(); process.exit(1);
  }
});
" 2>/dev/null)
echo "  Join result: $JOIN_RESULT"
assert "Second client joined" '[ "$(echo $JOIN_RESULT | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'
assert "Second client is not host" '[ "$(echo $JOIN_RESULT | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"is_host\"))")" = "False" ]'
assert "Second client sees correct motd" '[ "$(echo $JOIN_RESULT | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"motd\"))")" = "Debug Test World" ]'

sleep 0.5
STATUS=$(curl -s $API/debug/status)
PEER_COUNT=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('peer_count',0))")
echo "  Peer count after join+leave: $PEER_COUNT"

# Test 8: Leave room
echo ""
echo "--- Test 8: Leave Room ---"
LEAVE=$(curl -s -X POST $API/debug/leave_room)
echo "  Leave result: $LEAVE"
assert "Leave room ok" '[ "$(echo $LEAVE | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

sleep 0.3
STATUS=$(curl -s $API/debug/status)
ROOM_ID=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('room_id','None'))")
assert "Room cleared" '[ "$ROOM_ID" = "None" ]'

# Test 9: Disconnect
echo ""
echo "--- Test 9: Disconnect ---"
DISC=$(curl -s -X POST $API/debug/disconnect)
assert "Disconnect ok" '[ "$(echo $DISC | python3 -c "import sys,json;print(json.load(sys.stdin).get(\"ok\"))")" = "True" ]'

sleep 0.3
STATUS=$(curl -s $API/debug/status)
CONNECTED=$(echo $STATUS | python3 -c "import sys,json;print(json.load(sys.stdin).get('connected'))")
assert "Status shows disconnected" '[ "$CONNECTED" = "False" ]'

# Test 10: Clear events
echo ""
echo "--- Test 10: Clear Events ---"
curl -s -X POST $API/debug/clear_events > /dev/null
EVENTS=$(curl -s $API/debug/events)
EVENT_COUNT=$(echo $EVENTS | python3 -c "import sys,json;print(len(json.load(sys.stdin)))")
assert "Events cleared" '[ "$EVENT_COUNT" = "0" ]'

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
exit $FAIL
