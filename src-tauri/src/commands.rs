use crate::core::{self, AppCore, SharedCore};
use crate::crypto;
use crate::store::MsgRow;
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tauri_plugin_notification::NotificationExt;

fn core<'a>(s: &'a State<'_, SharedCore>) -> &'a AppCore {
    s.inner().as_ref()
}

#[derive(serde::Serialize)]
pub struct Profile {
    nickname: String,
    fp: String,
    #[serde(rename = "fpDisplay")]
    fp_display: String,
    #[serde(rename = "avaVer")]
    ava_ver: u64,
}

fn profile_of(c: &AppCore) -> Profile {
    Profile {
        nickname: c.nickname(),
        fp: c.identity.fp_hex(),
        fp_display: c.identity.fp_display(),
        ava_ver: c.avatar_version(),
    }
}

fn config_value(c: &AppCore) -> Value {
    let g = c.cfg.read().unwrap();
    json!({
        "theme": g.theme,
        "hotkey": g.hotkey,
        "closeToTray": g.close_to_tray,
        "notifications": g.notifications,
        "onboarded": g.onboarded,
        "dataDir": g.data_dir,
    })
}

#[tauri::command]
pub fn get_bootstrap(state: State<'_, SharedCore>) -> Result<Value, String> {
    let c = core(&state);
    let peers = core::peers_snapshot(c);
    let mut conv_map: HashMap<String, (u64, u64)> = HashMap::new();
    for (fp, last_ts, unread) in c.db.conversations()? {
        conv_map.insert(fp, (last_ts, unread));
    }
    let mut conversations: Vec<Value> = Vec::new();
    for p in &peers {
        let (last_ts, unread) = conv_map.get(&p.fp).cloned().unwrap_or((0, 0));
        if last_ts == 0 && !p.online {
            continue;
        }
        let preview = last_preview(c, &p.fp);
        conversations.push(json!({
            "fp": p.fp, "nick": p.nick, "online": p.online,
            "confirmed": p.confirmed, "lastTs": last_ts,
            "preview": preview, "unread": unread,
        }));
    }
    conversations.sort_by_key(|v| -(v["lastTs"].as_u64().unwrap_or(0) as i64));
    Ok(json!({
        "profile": profile_of(c),
        "config": config_value(c),
        "peers": peers,
        "conversations": conversations,
    }))
}

fn last_preview(c: &AppCore, fp_hex: &str) -> String {
    let msgs = c.db.list_messages(fp_hex, 1).unwrap_or_default();
    match msgs.last() {
        Some(m) => match (&m.body, &m.fname) {
            (Some(b), _) => b.chars().take(60).collect(),
            (None, Some(n)) => format!("[文件] {}", n),
            _ => String::new(),
        },
        None => String::new(),
    }
}

#[tauri::command]
pub fn list_messages(state: State<'_, SharedCore>, fp: String, limit: u32) -> Result<Vec<Value>, String> {
    let c = core(&state);
    let rows = c.db.list_messages(&fp, limit.max(1).min(500))?;
    Ok(rows
        .into_iter()
        .map(|r| core::build_msg_view(c, r))
        .collect())
}

#[tauri::command]
pub async fn open_session(state: State<'_, SharedCore>, fp: String) -> Result<Value, String> {
    let c = state.inner().clone();
    let fpb = core::fp_bytes(&fp).ok_or("bad fingerprint")?;
    let sess = core::ensure_session(&c, &fpb).await?;
    let verified = c.db.peer_confirmed(&fp).unwrap_or(false);
    let _ = sess;
    Ok(json!({"ok": true, "verified": verified}))
}

#[tauri::command]
pub fn confirm_peer(state: State<'_, SharedCore>, fp: String) -> Result<(), String> {
    let c = core(&state);
    c.db.set_peer_flag(&fp, "confirmed", true)?;
    core::emit_peers(c);
    Ok(())
}

#[tauri::command]
pub fn set_blocked(state: State<'_, SharedCore>, fp: String, blocked: bool) -> Result<(), String> {
    let c = core(&state);
    c.db.set_peer_flag(&fp, "blocked", blocked)?;
    if blocked {
        if let Some(fpb) = core::fp_bytes(&fp) {
            if let Some(s) = core::get_session(c, &fpb) {
                s.kill();
            }
            c.peers.lock().unwrap().remove(&fpb);
        }
        core::emit_peers(c);
    }
    Ok(())
}

#[tauri::command]
pub fn forget_peer(state: State<'_, SharedCore>, fp: String) -> Result<(), String> {
    let c = core(&state);
    if let Some(fpb) = core::fp_bytes(&fp) {
        if let Some(s) = core::get_session(c, &fpb) {
            s.kill();
        }
        c.peers.lock().unwrap().remove(&fpb);
    }
    c.db.forget_peer(&fp)?;
    core::emit_peers(c);
    Ok(())
}

#[tauri::command]
pub async fn send_text(
    state: State<'_, SharedCore>,
    fp: String,
    body: String,
) -> Result<Value, String> {
    let c = state.inner().clone();
    deliver_text(&c, &fp, &body).await
}

async fn deliver_text(core: &SharedCore, fp: &str, body: &str) -> Result<Value, String> {
    let body_trimmed = body.trim().to_string();
    if body_trimmed.is_empty() {
        return Err("消息不能为空".into());
    }
    if body_trimmed.len() > 64_000 {
        return Err("消息过长".into());
    }
    let fpb = core::fp_bytes(fp).ok_or("bad fingerprint")?;
    let mid = crypto::new_id();
    let ts = crypto::now_ms();
    let row = MsgRow {
        mid: mid.clone(),
        fp: fp.to_string(),
        dir: 0,
        kind: "text".into(),
        body: Some(body_trimmed.clone()),
        fid: None,
        fname: None,
        fsize: None,
        ts,
        state: "pending".into(),
    };
    core.db.insert_message(&row)?;

    let delivered = match core::ensure_session(core, &fpb).await {
        Ok(sess) if sess.is_alive() => {
            let payload = json!({"mid": mid, "kind": "text", "body": body_trimmed, "ts": ts});
            sess.send(crate::transport::F_MSG, serde_json::to_vec(&payload).unwrap_or_default());
            tracing::info!("send_text: delivered via session fp={}", fp);
            true
        }
        Ok(_) => {
            tracing::warn!("send_text: session dead after ensure_session fp={}", fp);
            false
        }
        Err(e) => {
            tracing::warn!("send_text: ensure_session failed fp={}: {}", fp, e);
            false
        }
    };

    if delivered {
        let _ = core.db.update_msg_state(&mid, "sent");
        let _ = core.db.drop_outbox_mid(&mid);
    } else {
        let _ = core.db.enqueue_outbox(&mid, fp);
    }

    let mut view = core::build_msg_view(core, row);
    view["state"] = json!(if delivered { "sent" } else { "pending" });
    Ok(view)
}

#[tauri::command]
pub async fn forward_messages(
    state: State<'_, SharedCore>,
    mids: Vec<String>,
    targets: Vec<String>,
) -> Result<Value, String> {
    let c = state.inner().clone();
    let mut sent = 0usize;
    let mut failed = 0usize;
    for mid in &mids {
        let Some(row) = c.db.msg_by_mid(mid) else {
            continue;
        };
        for t in &targets {
            if t == &row.fp {
                continue;
            }
            let r = if row.kind == "text" {
                match deliver_text(&c, t, row.body.as_deref().unwrap_or("")).await {
                    Ok(view) => {
                        c.emit("message-new", &view);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            } else if let Some(fid) = &row.fid {
                match c.db.file_info(fid) {
                    Ok(Some((_name, _sha, _size, _mime, kind, _dir, path))) => {
                        crate::transfer::send_path(&c, t, &path, Some(&kind)).await.map(|_| ())
                    }
                    _ => Err("文件不存在".into()),
                }
            } else {
                Err("不支持的转发类型".into())
            };
            if r.is_ok() {
                sent += 1;
            } else {
                failed += 1;
            }
        }
    }
    Ok(json!({"sent": sent, "failed": failed}))
}

#[tauri::command]
pub fn typing(state: State<'_, SharedCore>, fp: String) {
    let c = core(&state);
    if let Some(fpb) = core::fp_bytes(&fp) {
        if let Some(s) = core::get_session(c, &fpb) {
            s.send(crate::transport::F_TYPING, b"t".to_vec());
        }
    }
}

#[tauri::command]
pub fn mark_read(state: State<'_, SharedCore>, fp: String) -> Result<(), String> {
    let c = core(&state);
    let read_mids = c.db.mark_incoming_read(&fp)?;
    if let Some(fpb) = core::fp_bytes(&fp) {
        if let Some(s) = core::get_session(c, &fpb) {
            for mid in read_mids {
                let a = json!({"mid": mid, "state": "read"});
                s.send(crate::transport::F_ACK, serde_json::to_vec(&a).unwrap_or_default());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn send_files(
    state: State<'_, SharedCore>,
    fp: String,
    paths: Vec<String>,
) -> Result<Vec<Value>, String> {
    let c = state.inner().clone();
    let mut out = Vec::new();
    for p in paths {
        let r = crate::transfer::send_path(&c, &fp, &p, None).await;
        match r {
            Ok(v) => out.push(v),
            Err(e) => out.push(json!({"error": e})),
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn send_media(
    state: State<'_, SharedCore>,
    fp: String,
    bytes: Vec<u8>,
    name: String,
    kind: String,
) -> Result<Value, String> {
    if bytes.is_empty() {
        return Err("空文件".into());
    }
    if bytes.len() > 200 * 1024 * 1024 {
        return Err("文件超过 200MB，请使用文件发送".into());
    }
    let c = state.inner().clone();
    crate::transfer::send_bytes(&c, &fp, bytes, &name, &kind).await
}

#[tauri::command]
pub fn test_notification(state: State<'_, SharedCore>) -> Result<(), String> {
    core::show_notification(core(&state), "SChat", "这是一条系统通知测试消息")
}

#[tauri::command]
pub fn get_media_url(
    state: State<'_, SharedCore>,
    fid: String,
    path: Option<String>,
) -> Result<String, String> {
    let c = core(&state);
    let path = path.or_else(|| {
        c.db
            .file_info(&fid)
            .ok()
            .flatten()
            .map(|info| info.6)
    }).ok_or_else(|| "图片文件不存在".to_string())?;
    let root = std::fs::canonicalize(c.files_dir()).map_err(|e| e.to_string())?;
    let file = std::fs::canonicalize(&path).map_err(|_| "图片文件不存在".to_string())?;
    if !file.starts_with(root) || !file.is_file() {
        return Err("图片文件路径无效".into());
    }
    let port = c.media_port.load(std::sync::atomic::Ordering::Relaxed);
    if port == 0 {
        return Err("图片服务尚未启动".into());
    }
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        path.as_bytes(),
    );
    Ok(format!("http://127.0.0.1:{port}/media-path/{encoded}"))
}

#[tauri::command]
pub fn cancel_transfer(state: State<'_, SharedCore>, fid: String) -> Result<(), String> {
    let c = core(&state);
    if let Some(mid) = c.transfers.cancel(&fid) {
        let _ = c.db.update_msg_state(&mid, "failed");
        c.emit("message-state", &json!({"mid": mid, "state": "failed"}));
    }
    Ok(())
}

#[tauri::command]
pub fn get_avatar(state: State<'_, SharedCore>, fp: String) -> Option<String> {
    let c = core(&state);
    let path = if fp == "self" {
        core::self_avatar_file(c)
    } else {
        core::avatar_file(&c.dir, &fp)
    };
    match path {
        Some(p) => {
            let mime = if p.extension().map(|e| e == "jpg").unwrap_or(false) {
                "image/jpeg"
            } else {
                "image/png"
            };
            std::fs::read(p)
                .ok()
                .map(|b| format!("data:{};base64,{}", mime, crypto::b64(&b)))
        }
        None => {
            if fp != "self" {
                if let Some(fpb) = core::fp_bytes(&fp) {
                    let online = c
                        .peers
                        .lock()
                        .unwrap()
                        .get(&fpb)
                        .map(|e| e.online)
                        .unwrap_or(false);
                    if online {
                        let shared = state.inner().clone();
                        crate::discovery::request_avatar(&shared, &fpb);
                    }
                }
            }
            None
        }
    }
}

#[tauri::command]
pub fn set_profile(
    state: State<'_, SharedCore>,
    nickname: String,
    avatar_data: Option<String>,
) -> Result<Profile, String> {
    let c = core(&state);
    let nick_clean = nickname.trim().chars().take(24).collect::<String>();
    if nick_clean.is_empty() {
        return Err("昵称不能为空".into());
    }
    let new_avatar: Option<(&str, Vec<u8>)> = match avatar_data.as_deref() {
        Some(data_url) => {
            let raw = data_url.split(',').nth(1).ok_or("bad avatar data")?;
            let bytes = crypto::un_b64(raw)?;
            if bytes.len() > 512 * 1024 {
                return Err("头像过大（≤512KB）".into());
            }
            if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                Some(("png", bytes))
            } else if bytes.starts_with(&[0xFF, 0xD8]) {
                Some(("jpg", bytes))
            } else {
                return Err("仅支持 PNG/JPG 头像".into());
            }
        }
        None => None,
    };
    if let Some((ext, bytes)) = new_avatar {
        let dir = c.avatars_dir();
        let _ = std::fs::create_dir_all(&dir);
        for other in ["png", "jpg"] {
            if other != ext {
                let _ = std::fs::remove_file(dir.join(format!("self.{}", other)));
            }
        }
        std::fs::write(dir.join(format!("self.{}", ext)), bytes).map_err(|e| e.to_string())?;
    }
    {
        let mut g = c.cfg.write().unwrap();
        g.nickname = nick_clean;
        if avatar_data.is_some() {
            g.avatar_ver += 1;
        }
        g.onboarded = true;
        g.save(&c.dir)?;
    }
    Ok(profile_of(c))
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<'_, SharedCore>, patch: Value) -> Result<Value, String> {
    let c = core(&state);
    if let Some(hk) = patch.get("hotkey").and_then(|v| v.as_str()) {
        if !hk.is_empty() {
            let old = {
                let g = c.cfg.read().unwrap();
                g.hotkey.clone()
            };
            let gs = app.global_shortcut();
            if let Ok(old_sc) = old.to_lowercase().parse::<Shortcut>() {
                let _ = gs.unregister(old_sc);
            }
            match hk.to_lowercase().parse::<Shortcut>() {
                Ok(sc) => match gs.register(sc) {
                    Ok(_) => {
                        let mut g = c.cfg.write().unwrap();
                        g.hotkey = hk.to_string();
                        g.save(&c.dir)?;
                    }
                    Err(e) => {
                        if let Ok(osc) = old.to_lowercase().parse::<Shortcut>() {
                            let _ = gs.register(osc);
                        }
                        return Err(format!("快捷键注册失败：{}", e));
                    }
                },
                Err(e) => return Err(format!("无效的快捷键：{}", e)),
            }
        }
    }
    {
        let mut g = c.cfg.write().unwrap();
        if let Some(t) = patch.get("theme").and_then(|v| v.as_str()) {
            g.theme = t.to_string();
        }
        if let Some(b) = patch.get("closeToTray").and_then(|v| v.as_bool()) {
            g.close_to_tray = b;
        }
        if let Some(b) = patch.get("notifications").and_then(|v| v.as_bool()) {
            g.notifications = b;
            if b {
                let _ = app.notification().request_permission();
            }
        }
        if let Some(d) = patch.get("dataDir").and_then(|v| v.as_str()) {
            let p = std::path::PathBuf::from(d);
            if !p.exists() {
                return Err("目录不存在".into());
            }
            g.data_dir = Some(d.to_string());
        }
        g.save(&c.dir)?;
    }
    Ok(config_value(c))
}

#[tauri::command]
pub fn clear_history(state: State<'_, SharedCore>, fp: Option<String>) -> Result<(), String> {
    core(&state).db.clear_history(fp.as_deref())
}

#[tauri::command]
pub fn delete_messages(state: State<'_, SharedCore>, fp: String, mids: Vec<String>) -> Result<(), String> {
    core(&state).db.delete_messages(&fp, &mids)
}

#[tauri::command]
pub fn reveal_path(_app: AppHandle, path: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("explorer")
        .args(["/select,", &path])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_path(_app: AppHandle, path: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn get_data_dir(state: State<'_, SharedCore>) -> String {
    core(&state).dir.to_string_lossy().to_string()
}
