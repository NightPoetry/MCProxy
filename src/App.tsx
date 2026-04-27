import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { t, Lang } from "./i18n";

interface GameInfo { motd: string; port: number }
interface MemberInfo { peer_id: string; nickname: string; is_host: boolean }
interface RoomListing { room_id: string; host_name: string; game_motd: string; player_count: number; has_password: boolean }
interface StatusInfo {
  connected: boolean; room_id: string | null; is_host: boolean; peer_count: number;
  lan_game: GameInfo | null; tunnel_port: number | null; scanning: boolean;
  members: MemberInfo[]; room_list: RoomListing[];
}
interface ProxyEvent {
  type: string; room_id?: string; game_info?: GameInfo; is_host?: boolean;
  peer_id?: string; message?: string; motd?: string; port?: number;
  local_port?: number; status?: string; detail?: string; reason?: string;
}
interface LogEntry { time: string; text: string; level: "info" | "error" | "success" }

type View = "console" | "room" | "lobby";
type Theme = "dark" | "cream";

function ts() {
  const d = new Date();
  return [d.getHours(), d.getMinutes(), d.getSeconds()].map(n => String(n).padStart(2, "0")).join(":");
}

const AVATAR_COLORS = ["#569cd6", "#4ec9b0", "#c586c0", "#ce9178", "#dcdcaa", "#4fc1ff", "#d16969", "#b5cea8"];
function avatarColor(id: string) { let h = 0; for (const c of id) h = (h * 31 + c.charCodeAt(0)) | 0; return AVATAR_COLORS[Math.abs(h) % AVATAR_COLORS.length]; }

export default function App() {
  const [lang, setLang] = useState<Lang>(() => (localStorage.getItem("lang") as Lang) || "zh");
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem("theme") as Theme) || "dark");
  const [view, setView] = useState<View>("console");
  const [tab, setTab] = useState<"host" | "join">("host");
  const [serverUrl, setServerUrl] = useState("ws://127.0.0.1:9800");
  const [nickname, setNickname] = useState("");
  const [nickSaved, setNickSaved] = useState(false);
  const [status, setStatus] = useState<StatusInfo>({ connected: false, room_id: null, is_host: false, peer_count: 0, lan_game: null, tunnel_port: null, scanning: false, members: [], room_list: [] });
  const [roomInput, setRoomInput] = useState("");
  const [pwdInput, setPwdInput] = useState("");
  const [hostPwd, setHostPwd] = useState("");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [toasts, setToasts] = useState<{ id: number; text: string; type: "info" | "error" | "success" }[]>([]);
  const [joinTarget, setJoinTarget] = useState<string | null>(null);
  const [joinPwd, setJoinPwd] = useState("");
  const tid = useRef(0);
  const logEnd = useRef<HTMLDivElement>(null);
  const $ = useCallback((key: Parameters<typeof t>[0]) => t(key, lang), [lang]);

  useEffect(() => { document.documentElement.setAttribute("data-theme", theme); localStorage.setItem("theme", theme); }, [theme]);
  useEffect(() => { localStorage.setItem("lang", lang); }, [lang]);

  const log = useCallback((text: string, level: LogEntry["level"] = "info") => { setLogs(p => [...p.slice(-499), { time: ts(), text, level }]); }, []);
  const toast = useCallback((text: string, type: "info" | "error" | "success" = "info") => { const id = ++tid.current; setToasts(p => [...p, { id, text, type }]); setTimeout(() => setToasts(p => p.filter(t => t.id !== id)), 3500); }, []);
  const refresh = useCallback(async () => { try { setStatus(await invoke<StatusInfo>("get_status")); } catch {} }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<ProxyEvent>("proxy-event", ev => {
      const e = ev.payload;
      switch (e.type) {
        case "connected": log("Connected", "success"); toast($("connected"), "success"); break;
        case "disconnected": log(`Disconnected: ${e.reason || ""}`, "error"); toast($("disconnected"), "error"); break;
        case "room_created": log(`Room: ${e.room_id}`, "success"); toast(`Room ${e.room_id}`, "success"); break;
        case "room_joined": log(`Joined ${e.is_host ? "HOST" : "CLIENT"} — ${e.game_info?.motd}`, "success"); break;
        case "peer_joined": log(`+ ${e.peer_id}`, "info"); break;
        case "peer_left": log(`- ${e.peer_id?.slice(0, 8)}`, "info"); break;
        case "lan_game_found": log(`LAN: ${e.motd} :${e.port}`, "success"); toast(`${e.motd}`, "success"); break;
        case "error": log(`ERR: ${e.message}`, "error"); toast(e.message || "Error", "error"); break;
        case "room_closed": log("Room closed", "error"); toast("Room closed", "error"); break;
        case "tunnel_active": log(`Tunnel :${e.local_port}`, "success"); break;
        case "status_update": if (e.detail && !e.detail.includes("updated")) log(e.detail, "info"); break;
      }
      refresh();
    }).then(fn => { unlisten = fn; });
    return () => unlisten?.();
  }, [log, toast, refresh, $]);

  useEffect(() => { logEnd.current?.scrollIntoView({ behavior: "smooth" }); }, [logs]);

  const doConnect = async () => { try { await invoke("connect_server", { serverUrl }); await refresh(); } catch (e: any) { toast(String(e), "error"); } };
  const doDisconnect = async () => { try { await invoke("disconnect_server"); await refresh(); } catch (e: any) { toast(String(e), "error"); } };
  const doSetNick = async () => { if (!nickname.trim()) return; try { await invoke("set_nickname", { nickname: nickname.trim() }); setNickSaved(true); setTimeout(() => setNickSaved(false), 2000); } catch (e: any) { toast(String(e), "error"); } };
  const doScan = async () => { try { await invoke("start_lan_scan"); } catch (e: any) { toast(String(e), "error"); } };
  const doStopScan = async () => { try { await invoke("stop_lan_scan"); await refresh(); } catch (e: any) { toast(String(e), "error"); } };
  const doCreate = async () => { try { await invoke("create_room", { password: hostPwd }); } catch (e: any) { toast(String(e), "error"); } };
  const doJoin = async (rid?: string, pwd?: string) => { try { await invoke("join_room", { roomId: rid || roomInput, password: pwd ?? pwdInput }); setJoinTarget(null); } catch (e: any) { toast(String(e), "error"); } };
  const doLeave = async () => { try { await invoke("leave_room"); await refresh(); } catch (e: any) { toast(String(e), "error"); } };
  const doListRooms = async () => { try { await invoke("list_rooms"); setTimeout(refresh, 300); } catch (e: any) { toast(String(e), "error"); } };

  const inRoom = !!status.room_id;
  const barClass = inRoom ? "in-room" : status.connected ? "" : "disconnected";

  const renderConsole = () => (
    <div className="view-console">
      <div className="panel-header">
        <span className="panel-tab active">{$("console")}</span>
        <div className="panel-header-actions">
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{logs.length} {$("lines")}</span>
          <button className="btn-icon" onClick={() => setLogs([])} title={$("clear")}>{"×"}</button>
        </div>
      </div>
      <div className="log-body">
        {logs.length === 0 ? <div className="log-empty">{$("waiting")}</div> : logs.map((l, i) => (
          <div key={i} className="log-line">
            <span className="log-time">{l.time}</span>
            <span className={`log-level ${l.level}`}>{l.level === "error" ? "ERR " : l.level === "success" ? " OK " : "INFO"}</span>
            <span className="log-msg">{l.text}</span>
          </div>
        ))}
        <div ref={logEnd} />
      </div>
    </div>
  );

  const renderRoom = () => {
    if (!status.connected) return <div className="view-fill"><div className="empty-hero"><div className="hero-icon">▦</div><div className="hero-title">{$("notConnected")}</div><div className="hero-sub">{$("connectFirst")}</div></div></div>;

    if (!inRoom) return (
      <div className="view-room-setup">
        <div className="panel-header">
          <button className={`panel-tab ${tab === "host" ? "active" : ""}`} onClick={() => setTab("host")}>{$("host")}</button>
          <button className={`panel-tab ${tab === "join" ? "active" : ""}`} onClick={() => setTab("join")}>{$("join")}</button>
        </div>
        <div className="controls-body">
          {tab === "host" ? (
            <div className="form-stack">
              <div className="section">
                <div className="section-label"><span className="color-mark" style={{ background: "var(--green)" }} />{$("lanGame")}</div>
                {status.lan_game ? (
                  <div className="detect-bar"><div className="detect-dot found" /><div className="detect-info"><div className="detect-motd">{status.lan_game.motd}</div><div className="detect-port">:{status.lan_game.port}</div></div><span className="badge badge-green">{$("detected")}</span></div>
                ) : (
                  <div className="detect-bar"><div className={`detect-dot ${status.scanning ? "scanning" : "idle"}`} /><span className="detect-hint">{status.scanning ? $("scanning") : $("noGame")}</span><button className="btn btn-sm btn-ghost" onClick={status.scanning ? doStopScan : doScan}>{status.scanning ? $("stop") : $("scan")}</button></div>
                )}
              </div>
              <div className="section"><div className="section-label"><span className="color-mark" style={{ background: "var(--purple)" }} />{$("password")}</div><input className="input" type="password" placeholder={$("setPwd")} value={hostPwd} onChange={e => setHostPwd(e.target.value)} /></div>
              <button className="btn btn-green btn-block" disabled={!status.lan_game} onClick={doCreate}>{$("createRoom")}</button>
            </div>
          ) : (
            <div className="form-stack">
              <div className="section"><div className="section-label"><span className="color-mark" style={{ background: "var(--cyan)" }} />{$("roomId")}</div><input className="input" placeholder={$("enterCode")} value={roomInput} onChange={e => setRoomInput(e.target.value)} maxLength={6} style={{ fontWeight: 700, letterSpacing: "4px" }} /></div>
              <div className="section"><div className="section-label"><span className="color-mark" style={{ background: "var(--purple)" }} />{$("password")}</div><input className="input" type="password" placeholder={$("enterPwd")} value={pwdInput} onChange={e => setPwdInput(e.target.value)} /></div>
              <button className="btn btn-accent btn-block" disabled={!roomInput} onClick={() => doJoin()}>{$("joinRoom")}</button>
            </div>
          )}
        </div>
      </div>
    );

    return (
      <div className="view-room-active">
        <div className="room-banner">
          <div className="room-banner-id">{status.room_id}</div>
          <div className="room-banner-info">
            {status.lan_game && <span className="room-banner-game">{status.lan_game.motd} :{status.lan_game.port}</span>}
            {status.tunnel_port && <span className="room-banner-proxy">proxy → 127.0.0.1:{status.tunnel_port}</span>}
          </div>
          <span className={`badge ${status.is_host ? "badge-blue" : "badge-green"}`}>{status.is_host ? "HOST" : "CLIENT"}</span>
          <button className="btn btn-sm btn-danger" onClick={doLeave}>{$("leave")}</button>
        </div>
        <div className="panel-header">
          <span className="panel-tab active">{$("members")} ({status.members.length})</span>
        </div>
        <div className="member-grid">
          {status.members.length === 0 ? <div className="member-empty">{$("noMembers")}</div> : status.members.map((m, i) => (
            <div key={i} className={`member-card ${m.is_host ? "is-host" : ""}`}>
              <div className="member-avatar" style={{ background: avatarColor(m.peer_id) }}>{m.nickname[0]?.toUpperCase() || "?"}</div>
              <div className="member-nick">{m.nickname}</div>
              <div className="member-pid">{m.peer_id.slice(0, 8)}</div>
              {m.is_host && <span className="badge badge-blue">HOST</span>}
            </div>
          ))}
        </div>
      </div>
    );
  };

  const renderLobby = () => {
    if (!status.connected) return <div className="view-fill"><div className="empty-hero"><div className="hero-icon">◎</div><div className="hero-title">{$("notConnected")}</div><div className="hero-sub">{$("browseHint")}</div></div></div>;
    return (
      <div className="view-lobby">
        <div className="panel-header">
          <span className="panel-tab active">{$("lobby")}</span>
          <div className="panel-header-actions">
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{status.room_list.length} {$("rooms")}</span>
            <button className="btn btn-sm btn-ghost" onClick={doListRooms}>{$("scanRooms")}</button>
          </div>
        </div>
        <div className="lobby-body">
          {status.room_list.length === 0 ? (
            <div className="lobby-empty">
              <div style={{ color: "var(--text-muted)", fontSize: 13 }}>{$("noRooms")}</div>
              <button className="btn btn-accent" onClick={doListRooms}>{$("scanRooms")}</button>
            </div>
          ) : (
            <div className="room-grid">
              {status.room_list.map((r, i) => (
                <div key={i} className="room-card" onClick={() => {
                  if (r.has_password) { setJoinTarget(r.room_id); setJoinPwd(""); }
                  else doJoin(r.room_id, "");
                }}>
                  <div className="room-card-header">
                    <span className="room-card-id">{r.room_id}</span>
                    {r.has_password && <span className="badge badge-orange">{$("locked")}</span>}
                  </div>
                  <div className="room-card-body">{r.game_motd || "—"}</div>
                  <div className="room-card-footer">
                    <span className="room-card-host">{r.host_name}</span>
                    <span className="room-card-count">{r.player_count} {r.player_count === 1 ? $("player") : $("players_plural")}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
        {joinTarget && (
          <div className="modal-overlay" onClick={() => setJoinTarget(null)}>
            <div className="modal-box" onClick={e => e.stopPropagation()}>
              <div className="modal-title">{$("enterRoomPwd")}</div>
              <div className="modal-sub">Room {joinTarget}</div>
              <input className="input" type="password" placeholder={$("password")} autoFocus value={joinPwd} onChange={e => setJoinPwd(e.target.value)} onKeyDown={e => e.key === "Enter" && doJoin(joinTarget, joinPwd)} />
              <div className="modal-actions">
                <button className="btn btn-ghost" onClick={() => setJoinTarget(null)}>{$("cancel")}</button>
                <button className="btn btn-accent" onClick={() => doJoin(joinTarget, joinPwd)}>{$("join")}</button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="app-root">
      <div className="titlebar">
        <div className="titlebar-drag">
          <div className="titlebar-icon" />
          <span className="titlebar-title">MCProxy</span>
        </div>
        <div className="titlebar-actions">
          <button className="tb-btn" onClick={() => setTheme(theme === "dark" ? "cream" : "dark")}>{theme === "dark" ? "☀" : "☾"}</button>
          <button className="tb-btn" onClick={() => setLang(lang === "zh" ? "en" : "zh")}>{lang === "zh" ? "EN" : "中"}</button>
        </div>
      </div>

      <div className="connect-bar">
        <input className="input" value={serverUrl} onChange={e => setServerUrl(e.target.value)} placeholder="ws://relay:9800" disabled={status.connected} onKeyDown={e => e.key === "Enter" && !status.connected && doConnect()} />
        <div className="nick-wrap">
          <input className="nick-input input" value={nickname} onChange={e => { setNickname(e.target.value); setNickSaved(false); }} placeholder={$("nickname")} maxLength={20} onKeyDown={e => e.key === "Enter" && doSetNick()} />
          {nickSaved && <span className="nick-saved">{$("saved")}</span>}
        </div>
        {status.connected ? <button className="btn btn-ghost" onClick={doDisconnect}>{$("disconnect")}</button> : <button className="btn btn-accent" onClick={doConnect}>{$("connect")}</button>}
      </div>

      <div className="hsplit">
        <div className="sidebar">
          <button className={`sidebar-btn ${view === "console" ? "active" : ""}`} onClick={() => setView("console")} title={$("console")}>{">_"}<span className={`sidebar-indicator ${status.connected ? "on" : "off"}`} /></button>
          <button className={`sidebar-btn ${view === "room" ? "active" : ""}`} onClick={() => setView("room")} title={$("room")}>{"▦"}{inRoom && <span className="sidebar-indicator on" />}</button>
          <button className={`sidebar-btn ${view === "lobby" ? "active" : ""}`} onClick={() => { setView("lobby"); if (status.connected) doListRooms(); }} title={$("lobby")}>{"◎"}{status.room_list.length > 0 && <span className="sidebar-indicator on" />}</button>
          <div className="sidebar-spacer" />
        </div>
        <div className="content">
          {view === "console" && renderConsole()}
          {view === "room" && renderRoom()}
          {view === "lobby" && renderLobby()}
        </div>
      </div>

      <div className={`statusbar ${barClass}`}>
        <span className="status-item"><span className="status-dot-sm" style={{ background: status.connected ? "#73c991" : "#ccc" }} />{status.connected ? $("connected") : $("disconnected")}</span>
        {status.room_id && <span className="status-item">Room: {status.room_id}</span>}
        {status.lan_game && <span className="status-item">{status.lan_game.motd}</span>}
        <span className="status-spacer" />
        {inRoom && <span className="status-item">{status.peer_count + 1} {$("online")}</span>}
        {status.tunnel_port && <span className="status-item">:{status.tunnel_port}</span>}
        <span className="status-item">v0.1.0</span>
      </div>

      <div className="toast-container">
        {toasts.map(tt => (
          <div key={tt.id} className={`toast toast-${tt.type}`}>
            <span className="toast-label">{tt.type === "error" ? "ERR" : tt.type === "success" ? "OK" : "INFO"}</span>
            {tt.text}
          </div>
        ))}
      </div>
    </div>
  );
}
