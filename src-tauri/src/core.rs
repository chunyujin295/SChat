use crate::config::Config;
use crate::crypto;
use crate::identity::Identity;
use crate::store::Db;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct Session {
    pub fp: [u8; 32],
    tx: tokio::sync::mpsc::UnboundedSender<(u8, Vec<u8>)>,
    pub last_rx_ms: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    aborts: Arc<std::sync::Mutex<Vec<tokio::task::AbortHandle>>>,
}

impl Session {
    pub fn new(
        fp: [u8; 32],
        tx: tokio::sync::mpsc::UnboundedSender<(u8, Vec<u8>)>,
    ) -> Self {
        Session {
            fp,
            tx,
            last_rx_ms: Arc::new(AtomicU64::new(crypto::now_ms())),
            alive: Arc::new(AtomicBool::new(true)),
            aborts: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn send(&self, ftype: u8, payload: Vec<u8>) {
        let _ = self.tx.send((ftype, payload));
    }

    pub fn close(&self) {
        if self.alive.swap(false, Ordering::SeqCst) {
            self.tx.send((crate::transport::F_BYE, Vec::new())).ok();
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn add_abort(&self, h: tokio::task::AbortHandle) {
        self.aborts.lock().unwrap().push(h);
    }

    pub fn alive_flag_off(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    pub fn kill(&self) {
        self.close();
        for h in self.aborts.lock().unwrap().drain(..) {
            h.abort();
        }
    }
}

pub type Sessions = Arc<Mutex<HashMap<[u8; 32], Arc<Session>>>>;

#[derive(Serialize, Clone)]
pub struct PeerEntry {
    pub fp: String,
    pub inst: String,
    pub nick: String,
    pub ip: String,
    pub port: u16,
    pub ava_ver: u64,
    pub online: bool,
    pub last_seen_ms: u64,
}

pub type Peers = Arc<Mutex<HashMap<[u8; 32], PeerEntry>>>;

pub struct AppCore {
    pub app: AppHandle,
    pub dir: PathBuf,
    pub identity: Identity,
    pub cfg: RwLock<Config>,
    pub db: Db,
    pub peers: Peers,
    pub sessions: Sessions,
    pub transfers: crate::transfer::Transfers,
    pub tcp_port: AtomicU16,
    pub ava_pending: Mutex<HashSet<String>>,
    pub instance_id: String,
}

pub type SharedCore = Arc<AppCore>;

impl AppCore {
    pub fn files_dir(&self) -> PathBuf {
        self.dir.join("files")
    }

    pub fn avatars_dir(&self) -> PathBuf {
        self.dir.join("avatars")
    }

    pub fn nickname(&self) -> String {
        self.cfg.read().map(|c| c.nickname.clone()).unwrap_or_default()
    }

    pub fn avatar_version(&self) -> u64 {
        self.cfg.read().map(|c| c.avatar_ver).unwrap_or(0)
    }

    pub fn tcp_port_value(&self) -> u16 {
        self.tcp_port.load(Ordering::Relaxed)
    }

    pub fn emit<T: Serialize>(&self, event: &str, payload: &T) {
        let _ = self.app.emit(event, payload);
    }
}

#[derive(Serialize, Clone)]
pub struct PeerView {
    pub fp: String,
    pub nick: String,
    pub online: bool,
    pub ip: String,
    pub port: u16,
    #[serde(rename = "avaVer")]
    pub ava_ver: u64,
    pub confirmed: bool,
    pub blocked: bool,
    #[serde(rename = "lastSeen")]
    pub last_seen: u64,
}

pub fn peers_snapshot(core: &AppCore) -> Vec<PeerView> {
    let rt = core.peers.lock().unwrap();
    let rows = core.db.list_peers().unwrap_or_default();
    let mut out = Vec::with_capacity(rows.len());
    for (fp, nick_db, ava_db, confirmed, blocked, _seen) in rows {
        let fpb = match fp_bytes(&fp) {
            Some(b) => b,
            None => continue,
        };
        let live = rt.get(&fpb);
        let online = live.map(|e| e.online).unwrap_or(false);
        let nick = live.map(|e| e.nick.clone()).unwrap_or(nick_db);
        let (ip, port, ava) = match live {
            Some(e) => (e.ip.clone(), e.port, e.ava_ver.max(ava_db)),
            None => (String::new(), 0, ava_db),
        };
        if blocked {
            continue;
        }
        out.push(PeerView {
            fp,
            nick,
            online,
            ip,
            port,
            ava_ver: ava,
            confirmed,
            blocked,
            last_seen: live.map(|e| e.last_seen_ms).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| b.online.cmp(&a.online).then(b.last_seen.cmp(&a.last_seen)));
    out
}

pub fn emit_peers(core: &AppCore) {
    let snap = peers_snapshot(core);
    core.emit("peers", &snap);
}

pub fn fp_bytes(hexs: &str) -> Option<[u8; 32]> {
    if hexs.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hexs[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn avatar_file(dir: &Path, fp_hex: &str) -> Option<PathBuf> {
    for ext in ["png", "jpg"] {
        let p = dir.join("avatars").join(format!("{}.{}", fp_hex, ext));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn self_avatar_file(core: &AppCore) -> Option<PathBuf> {
    avatar_file(&core.dir, "self")
}

pub fn get_session(core: &AppCore, fp: &[u8; 32]) -> Option<Arc<Session>> {
    core.sessions.lock().unwrap().get(fp).cloned()
}

pub fn register_session(core: &AppCore, sess: Arc<Session>) -> Option<Arc<Session>> {
    let mut map = core.sessions.lock().unwrap();
    let old = map.insert(sess.fp, sess);
    if let Some(o) = &old {
        o.close();
    }
    old
}

pub fn unregister_session(core: &AppCore, fp: &[u8; 32], sess: &Arc<Session>) {
    let mut map = core.sessions.lock().unwrap();
    let remove = match map.get(fp) {
        Some(cur) => Arc::ptr_eq(cur, sess),
        None => false,
    };
    if remove {
        map.remove(fp);
    }
}

pub fn build_msg_view(core: &AppCore, row: crate::store::MsgRow) -> serde_json::Value {
    let mut v = serde_json::json!({
        "mid": row.mid,
        "fp": row.fp,
        "dir": row.dir,
        "kind": row.kind,
        "body": row.body,
        "ts": row.ts,
        "state": row.state,
    });
    if let Some(fid) = &row.fid {
        if let Some((name, _sha, size, mime, _kind, _dir, path)) =
            core.db.file_info(fid).ok().flatten()
        {
            v["fid"] = serde_json::Value::String(fid.clone());
            v["fname"] = serde_json::Value::String(name);
            v["fsize"] = serde_json::Value::Number(size.into());
            v["mime"] = serde_json::Value::String(mime);
            v["fpath"] = serde_json::Value::String(path);
        } else {
            v["fid"] = serde_json::Value::String(fid.clone());
        }
    }
    v
}

pub async fn ensure_session(core: &SharedCore, fp: &[u8; 32]) -> Result<Arc<Session>, String> {
    if let Some(s) = get_session(core, fp) {
        if s.is_alive() {
            return Ok(s);
        }
    }
    let entry = {
        let map = core.peers.lock().unwrap();
        map.get(fp).cloned()
    };
    let entry = entry.ok_or("未找到该联系人")?;
    if !entry.online {
        return Err("对方当前不在线".into());
    }
    let ip: Ipv4Addr = entry.ip.parse().map_err(|_| "bad ip".to_string())?;
    crate::transport::dial(core, ip, entry.port, *fp).await
}

pub fn flush_outbox(core: &SharedCore, fp_hex: &str) {
    let fp = match fp_bytes(fp_hex) {
        Some(f) => f,
        None => return,
    };
    let sess = match get_session(core, &fp) {
        Some(s) if s.is_alive() => s,
        _ => return,
    };
    let mids = core.db.pop_outbox(fp_hex).unwrap_or_default();
    for mid in mids {
        let row = match core.db.msg_by_mid(&mid) {
            Some(r) => r,
            None => continue,
        };
        if row.dir != 0 || row.kind != "text" {
            continue;
        }
        let payload = serde_json::json!({
            "mid": row.mid, "kind": "text",
            "body": row.body.clone().unwrap_or_default(),
            "ts": row.ts
        });
        sess.send(
            crate::transport::F_MSG,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = core.db.update_msg_state(&mid, "sent");
        core.emit(
            "message-state",
            &serde_json::json!({"mid": mid, "state": "sent"}),
        );
    }
}

