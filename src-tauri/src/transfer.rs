use crate::core::{self, AppCore, SharedCore};
use crate::crypto;
use crate::store::MsgRow;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const CHUNK: usize = 128 * 1024;
const WINDOW: u64 = (CHUNK * 16) as u64;
const ACK_EVERY: u64 = 512 * 1024;
const PROGRESS_MIN_MS: u128 = 150;

#[derive(Serialize, Deserialize, Clone)]
pub struct Meta {
    pub mid: String,
    pub fid: String,
    pub name: String,
    pub size: u64,
    pub sha: String,
    pub mime: String,
    pub kind: String,
}

#[derive(Deserialize)]
struct ChunkMsg {
    fid: String,
    off: u64,
    data: String,
}

#[derive(Deserialize)]
struct AckMsg {
    fid: String,
    have: u64,
}

pub(crate) enum Xfer {
    Out {
        meta: Meta,
        src: Option<PathBuf>,
        temp: bool,
        offset: u64,
        acked: u64,
    },
    In {
        meta: Meta,
        fp_hex: String,
        file: std::fs::File,
        part: PathBuf,
        have: u64,
        hasher: Sha256,
        since_ack: u64,
        last_emit: Instant,
    },
}

pub struct Transfers(pub Mutex<HashMap<String, Xfer>>);

impl Transfers {
    pub fn new() -> Self {
        Transfers(Mutex::new(HashMap::new()))
    }

    pub fn remove_by_mid(&self, mid: &str) {
        let mut map = self.0.lock().unwrap();
        map.retain(|_, x| match x {
            Xfer::Out { meta, .. } => meta.mid != mid,
            Xfer::In { meta, .. } => meta.mid != mid,
        });
    }

    pub fn cancel(&self, fid: &str) -> Option<String> {
        let mut map = self.0.lock().unwrap();
        map.remove(fid).map(|x| match x {
            Xfer::Out { meta, .. } => meta.mid,
            Xfer::In { meta, .. } => meta.mid,
        })
    }
}

fn guess_kind(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image",
        "mp3" | "wav" | "ogg" | "oga" | "m4a" | "flac" | "aac" => "audio",
        "mp4" | "mkv" | "mov" | "avi" | "m4v" | "webm" => "video",
        _ => "file",
    }
}

pub fn guess_mime(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let m = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "weba" => "audio/webm",
        "webm" => "video/webm",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    };
    m.to_string()
}

fn sanitize_name(name: &str) -> String {
    let base = name.rsplit(['\\', '/']).next().unwrap_or("file");
    let cleaned: String = base
        .chars()
        .filter(|c| !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "file".to_string()
    } else {
        trimmed.to_string()
    }
}

fn stream_sha(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(crypto::hex(&h.finalize()))
}

fn month_dir(core: &AppCore) -> PathBuf {
    let d = core.files_dir();
    let secs = crypto::now_ms() / 1000;
    let (y, m) = crypto::ym_of(secs);
    let p = d.join(format!("{:04}{:02}", y, m));
    let _ = std::fs::create_dir_all(&p);
    p
}

fn progress_event(core: &AppCore, mid: &str, fid: &str, dir: i32, sent: u64, total: u64) {
    core.emit(
        "transfer",
        &serde_json::json!({"mid": mid, "fid": fid, "dir": dir, "sent": sent, "total": total}),
    );
}

pub async fn send_path(
    core: &SharedCore,
    fp_hex: &str,
    path: &str,
    kind_hint: Option<&str>,
) -> Result<serde_json::Value, String> {
    let pb = PathBuf::from(path);
    let raw_name = pb
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or("无效路径")?;
    let name = sanitize_name(&raw_name);
    let size = std::fs::metadata(&pb)
        .map_err(|e| format!("读取文件失败: {e}"))?
        .len();
    let kind = kind_hint.unwrap_or_else(|| guess_kind(&name)).to_string();
    let mime = guess_mime(&name);
    let sha = stream_sha(&pb)?;
    start_outgoing(core, fp_hex, pb, false, name, size, sha, mime, kind).await
}

pub async fn send_bytes(
    core: &SharedCore,
    fp_hex: &str,
    bytes: Vec<u8>,
    name: &str,
    kind: &str,
) -> Result<serde_json::Value, String> {
    let name = sanitize_name(name);
    let tmp_dir = core.dir.join("outgoing");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp = tmp_dir.join(format!("{}_{}", crypto::new_id(), name));
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    let sha = stream_sha(&tmp)?;
    let size = bytes.len() as u64;
    let mime = guess_mime(&name);
    start_outgoing(
        core,
        fp_hex,
        tmp,
        true,
        name,
        size,
        sha,
        mime,
        kind.to_string(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn start_outgoing(
    core: &SharedCore,
    fp_hex: &str,
    src: PathBuf,
    temp: bool,
    name: String,
    size: u64,
    sha: String,
    mime: String,
    kind: String,
) -> Result<serde_json::Value, String> {
    let fpb = core::fp_bytes(fp_hex).ok_or("bad fingerprint")?;
    let sess = core::ensure_session(core, &fpb).await?;

    let fid = crypto::new_id();
    let mid = crypto::new_id();
    let final_path = month_dir(core).join(format!("{}_{}", fid, name));

    // The sender must also keep a real file at `fpath` so its own message can
    // preview images, open files, and be forwarded later (fpath is what the UI
    // resolves). Move temp blobs into place; copy user-selected paths in.
    if src != final_path {
        if temp {
            std::fs::rename(&src, &final_path)
                .or_else(|_| {
                    std::fs::copy(&src, &final_path)?;
                    std::fs::remove_file(&src)
                })
                .map_err(|e| format!("保存文件失败: {e}"))?;
        } else {
            std::fs::copy(&src, &final_path).map_err(|e| format!("复制文件失败: {e}"))?;
        }
    }

    let meta = Meta {
        mid: mid.clone(),
        fid: fid.clone(),
        name: name.clone(),
        size,
        sha,
        mime,
        kind: kind.clone(),
    };

    core.db.upsert_file_meta(
        &fid,
        fp_hex,
        &name,
        size as i64,
        &meta.sha,
        &meta.mime,
        &kind,
        &final_path.to_string_lossy(),
        0,
    )?;
    let row = MsgRow {
        mid: mid.clone(),
        fp: fp_hex.to_string(),
        dir: 0,
        kind,
        body: None,
        fid: Some(fid.clone()),
        fname: Some(name),
        fsize: Some(size as i64),
        ts: crypto::now_ms(),
        state: "pending".into(),
    };
    core.db.insert_message(&row)?;
    let view = core::build_msg_view(core, row);
    core.emit("message-new", &view);

    core.transfers.0.lock().unwrap().insert(
        fid.clone(),
        Xfer::Out {
            meta: meta.clone(),
            src: Some(final_path.clone()),
            temp: false,
            offset: 0,
            acked: 0,
        },
    );

    sess.send(
        crate::transport::F_META,
        serde_json::to_vec(&meta).unwrap_or_default(),
    );
    tokio::spawn(pump_out(core.clone(), fpb, fid));
    Ok(view)
}

async fn pump_out(core: SharedCore, fp: [u8; 32], fid: String) {
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_millis(400);
    let mut fh: Option<std::fs::File> = None;
    let mut src_path: Option<PathBuf> = None;

    loop {
        tokio::time::sleep(Duration::from_millis(80)).await;
        let sess = match core::get_session(&core, &fp) {
            Some(s) if s.is_alive() => s,
            _ => break,
        };
        enum Act {
            Send(u64, usize),
            Idle,
        }
        let action = {
            let mut map = core.transfers.0.lock().unwrap();
            let Some(Xfer::Out {
                meta,
                src,
                offset,
                acked,
                ..
            }) = map.get_mut(&fid)
            else {
                break;
            };
            if src_path.is_none() {
                src_path = src.clone();
            }
            if started.elapsed() > Duration::from_secs(3600) {
                break;
            }
            if *offset < meta.size && (*offset - *acked) < WINDOW {
                let off = *offset;
                let n = (meta.size - off).min(CHUNK as u64) as usize;
                *offset += n as u64;
                Act::Send(off, n)
            } else {
                Act::Idle
            }
        };
        match action {
            Act::Send(off, n) => {
                if fh.is_none() {
                    fh = src_path.as_ref().and_then(|p| std::fs::File::open(p).ok());
                }
                let mut buf = vec![0u8; n];
                let ok = match fh.as_mut() {
                    Some(f) => {
                        let seek_ok = f.seek(SeekFrom::Start(off)).is_ok();
                        let mut filled = 0usize;
                        while seek_ok && filled < n {
                            match f.read(&mut buf[filled..]) {
                                Ok(0) => break,
                                Ok(k) => filled += k,
                                Err(_) => break,
                            }
                        }
                        seek_ok && filled == n
                    }
                    None => false,
                };
                if !ok {
                    fail_outgoing(&core, &fid);
                    break;
                }
                let payload =
                    serde_json::json!({"fid": fid, "off": off, "data": crypto::b64(&buf)});
                sess.send(
                    crate::transport::F_CHUNK,
                    serde_json::to_vec(&payload).unwrap_or_default(),
                );
                let (sent_now, total, mid) = {
                    let map = core.transfers.0.lock().unwrap();
                    match map.get(&fid) {
                        Some(Xfer::Out { meta, offset, .. }) => (*offset, meta.size, meta.mid.clone()),
                        _ => break,
                    }
                };
                if last_emit.elapsed().as_millis() >= PROGRESS_MIN_MS {
                    last_emit = Instant::now();
                    progress_event(&core, &mid, &fid, 0, sent_now.min(total), total);
                }
            }
            Act::Idle => {}
        }
    }
}

fn fail_outgoing(core: &AppCore, fid: &str) {
    let removed = {
        let mut map = core.transfers.0.lock().unwrap();
        map.remove(fid)
    };
    if let Some(Xfer::Out { meta, .. }) = removed {
        let _ = core.db.update_msg_state(&meta.mid, "failed");
        core.emit(
            "message-state",
            &serde_json::json!({"mid": meta.mid, "state": "failed"}),
        );
    }
}

pub async fn on_frame(
    core: &AppCore,
    tx: &mpsc::UnboundedSender<(u8, Vec<u8>)>,
    ftype: u8,
    fp_hex: &str,
    pl: &[u8],
) {
    match ftype {
        crate::transport::F_META => on_meta(core, tx, fp_hex, pl),
        crate::transport::F_CHUNK => on_chunk(core, tx, pl).await,
        crate::transport::F_FACK => on_fack(core, pl),
        _ => {}
    }
}

fn on_meta(core: &AppCore, tx: &mpsc::UnboundedSender<(u8, Vec<u8>)>, fp_hex: &str, pl: &[u8]) {
    let meta: Meta = match serde_json::from_slice(pl) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.size > 20 * 1024 * 1024 * 1024 {
        return;
    }
    let name = sanitize_name(&meta.name);
    let part = month_dir(core).join(format!("{}_{}.part", meta.fid, name));
    let mut have: u64 = 0;
    let mut hasher = Sha256::new();
    if part.exists() {
        have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > 0 {
            if let Ok(bytes) = std::fs::read(&part) {
                hasher.update(&bytes);
            }
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(have > 0)
        .truncate(have == 0)
        .open(&part);
    let file = match file {
        Ok(f) => f,
        Err(_) => return,
    };
    core.transfers.0.lock().unwrap().insert(
        meta.fid.clone(),
        Xfer::In {
            meta: meta.clone(),
            fp_hex: fp_hex.to_string(),
            file,
            part,
            have,
            hasher,
            since_ack: 0,
            last_emit: Instant::now() - Duration::from_millis(400),
        },
    );
    let ack = serde_json::json!({"fid": meta.fid, "have": have});
    tx.send((
        crate::transport::F_FACK,
        serde_json::to_vec(&ack).unwrap_or_default(),
    ))
    .ok();
    progress_event(core, &meta.mid, &meta.fid, 1, have, meta.size);
}

async fn on_chunk(core: &AppCore, tx: &mpsc::UnboundedSender<(u8, Vec<u8>)>, pl: &[u8]) {
    let ck: ChunkMsg = match serde_json::from_slice(pl) {
        Ok(c) => c,
        Err(_) => return,
    };
    let data = match crypto::un_b64(&ck.data) {
        Ok(d) if !d.is_empty() && d.len() <= CHUNK * 2 => d,
        _ => return,
    };
    enum Out {
        None,
        Ack(u64),
        Done,
    }
    let outcome = {
        let mut map = core.transfers.0.lock().unwrap();
        let Some(Xfer::In {
            meta,
            file,
            have,
            hasher,
            since_ack,
            last_emit,
            ..
        }) = map.get_mut(&ck.fid)
        else {
            return;
        };
        if ck.off != *have || *have >= meta.size {
            return;
        }
        if file.write_all(&data).is_err() {
            return;
        }
        hasher.update(&data);
        *have += data.len() as u64;
        *since_ack += data.len() as u64;
            if last_emit.elapsed().as_millis() >= PROGRESS_MIN_MS {
                *last_emit = Instant::now();
                progress_event(core, &meta.mid, &meta.fid, 1, *have, meta.size);
            }
        let finished = *have >= meta.size;
        let h = *have;
        if finished {
            Out::Done
        } else if *since_ack >= ACK_EVERY {
            *since_ack = 0;
            Out::Ack(h)
        } else {
            Out::None
        }
    };
    match outcome {
        Out::Ack(h) => {
            let a = serde_json::json!({"fid": ck.fid, "have": h});
            tx.send((
                crate::transport::F_FACK,
                serde_json::to_vec(&a).unwrap_or_default(),
            ))
            .ok();
        }
        Out::Done => finalize_incoming(core, tx, &ck.fid),
        Out::None => {}
    }
}

fn on_fack(core: &AppCore, pl: &[u8]) {
    let fa: AckMsg = match serde_json::from_slice(pl) {
        Ok(a) => a,
        Err(_) => return,
    };
    let complete = {
        let mut map = core.transfers.0.lock().unwrap();
        let Some(Xfer::Out { meta, acked, .. }) = map.get_mut(&fa.fid) else {
            return;
        };
        *acked = (*acked).max(fa.have.min(meta.size));
        *acked >= meta.size
    };
    if complete {
        let removed = {
            let mut map = core.transfers.0.lock().unwrap();
            map.remove(&fa.fid)
        };
        if let Some(Xfer::Out { src, temp, meta, .. }) = removed {
            if temp {
                if let Some(p) = src {
                    let _ = std::fs::remove_file(p);
                }
            }
            core.emit(
                "transfer-done",
                &serde_json::json!({"fid": fa.fid, "mid": meta.mid, "ok": true}),
            );
        }
    }
}

fn finalize_incoming(core: &AppCore, tx: &mpsc::UnboundedSender<(u8, Vec<u8>)>, fid: &str) {
    let taken = {
        let mut map = core.transfers.0.lock().unwrap();
        map.remove(fid)
    };
    let Some(Xfer::In {
        meta,
        fp_hex,
        part,
        hasher,
        ..
    }) = taken
    else {
        return;
    };
    let actual = crypto::hex(&hasher.finalize());
    if actual != meta.sha {
        let _ = std::fs::remove_file(&part);
        core.emit(
            "alert",
            &serde_json::json!({
                "code": "file-integrity",
                "message": format!("文件「{}」校验失败，已丢弃", meta.name),
                "fp": fp_hex
            }),
        );
        return;
    }
    let final_path = month_dir(core).join(format!("{}_{}", meta.fid, meta.name));
    let renamed = std::fs::rename(&part, &final_path).or_else(|_| -> Result<(), std::io::Error> {
        std::fs::copy(&part, &final_path)?;
        let _ = std::fs::remove_file(&part);
        Ok(())
    });
    if renamed.is_err() {
        return;
    }
    let _ = core.db.upsert_file_meta(
        &meta.fid,
        &fp_hex,
        &meta.name,
        meta.size as i64,
        &meta.sha,
        &meta.mime,
        &meta.kind,
        &final_path.to_string_lossy(),
        1,
    );
    let row = MsgRow {
        mid: meta.mid.clone(),
        fp: fp_hex.clone(),
        dir: 1,
        kind: meta.kind.clone(),
        body: None,
        fid: Some(meta.fid.clone()),
        fname: Some(meta.name.clone()),
        fsize: Some(meta.size as i64),
        ts: crypto::now_ms(),
        state: "unread".into(),
    };
    let view = core::build_msg_view(core, row.clone());
    let _ = core.db.insert_message(&row);
    let fin = serde_json::json!({"fid": fid, "have": meta.size});
    tx.send((
        crate::transport::F_FACK,
        serde_json::to_vec(&fin).unwrap_or_default(),
    ))
    .ok();
    let ack = serde_json::json!({"mid": meta.mid, "state": "delivered"});
    tx.send((
        crate::transport::F_ACK,
        serde_json::to_vec(&ack).unwrap_or_default(),
    ))
    .ok();
    core.emit("transfer-done", &serde_json::json!({"fid": fid, "mid": meta.mid, "ok": true}));
    core.emit("message-new", &view);
    core::notify_incoming(core, &fp_hex, &format!("[{}] {}", meta.kind, meta.name));
}
