use crate::core::{self, AppCore, Session, SharedCore};
use crate::crypto::{self, SessionKeys};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use x25519_dalek::{PublicKey as XPub, StaticSecret as XSec};

pub const F_MSG: u8 = 1;
pub const F_ACK: u8 = 2;
pub const F_TYPING: u8 = 3;
pub const F_META: u8 = 4;
pub const F_CHUNK: u8 = 5;
pub const F_FACK: u8 = 6;
pub const F_AVA_GET: u8 = 7;
pub const F_AVA_DATA: u8 = 8;
pub const F_AV: u8 = 9;
pub const F_PING: u8 = 10;
pub const F_PONG: u8 = 11;
pub const F_BYE: u8 = 12;
const F_SIGC: u8 = 13;

#[derive(Serialize, Deserialize)]
struct HClient {
    v: u8,
    epk: String,
    idpk: String,
    nc: String,
    ts: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct HServer {
    v: u8,
    epk: String,
    idpk: String,
    ns: String,
    ts: u64,
    sig: String,
}

fn hs_no_sig(h: &HServer) -> Vec<u8> {
    serde_json::to_vec(&HServer {
        sig: String::new(),
        ..h.clone()
    })
    .unwrap_or_default()
}

async fn hs_write(
    w: &mut TcpStream,
    data: &[u8],
) -> Result<(), String> {
    let len = (data.len() as u16).to_be_bytes();
    w.write_all(&len).await.map_err(|e| e.to_string())?;
    w.write_all(data).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())
}

async fn hs_read(r: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut lb = [0u8; 2];
    r.read_exact(&mut lb).await.map_err(|e| e.to_string())?;
    let len = u16::from_be_bytes(lb) as usize;
    if len == 0 || len > crypto::HANDSHAKE_MAX {
        return Err("bad handshake length".into());
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    Ok(buf)
}

pub async fn listen(core: SharedCore) {
    let listener = match TcpListener::bind(("0.0.0.0", 0)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("tcp bind: {e}");
            return;
        }
    };
    if let Ok(addr) = listener.local_addr() {
        core.tcp_port.store(addr.port(), Ordering::Relaxed);
        tracing::info!("listening on :{}", addr.port());
    }
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let c = core.clone();
                tokio::spawn(async move {
                    let _ = run_connection(c, s, false, None).await;
                });
            }
            Err(e) => {
                tracing::warn!("accept: {e}");
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    }
}

pub async fn dial(
    core: &SharedCore,
    ip: Ipv4Addr,
    port: u16,
    expect: [u8; 32],
) -> Result<Arc<Session>, String> {
    tracing::info!("dial: connecting to {}:{}", ip, port);
    let stream = match tokio::time::timeout(Duration::from_secs(4), TcpStream::connect((ip, port)))
        .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::warn!("dial: connect error {}:{}: {}", ip, port, e);
            return Err(format!("连接失败: {e}"));
        }
        Err(_) => {
            tracing::warn!("dial: connect timeout {}:{}", ip, port);
            return Err("连接超时".to_string());
        }
    };
    match run_connection(core.clone(), stream, true, Some(expect)).await {
        Ok(s) => {
            tracing::info!("dial: session established to {}:{}", ip, port);
            Ok(s)
        }
        Err(e) => {
            tracing::warn!("dial: handshake failed {}:{}: {}", ip, port, e);
            Err(e)
        }
    }
}

async fn run_connection(
    core: SharedCore,
    mut stream: TcpStream,
    client: bool,
    expect: Option<[u8; 32]>,
) -> Result<Arc<Session>, String> {
    let _ = stream.set_nodelay(true);
    let (keys, peer_fp, peer_vk, transcript, expect_sigc) = if client {
        handshake_client(&core, &mut stream, expect).await?
    } else {
        handshake_server(&core, &mut stream).await?
    };
    start_session(core, stream, keys, peer_fp, peer_vk, transcript, expect_sigc).await
}

type HandshakeOut = (
    SessionKeys,
    [u8; 32],
    VerifyingKey,
    [u8; 32],
    Option<[u8; 32]>,
);

async fn handshake_client(
    core: &AppCore,
    stream: &mut TcpStream,
    expect: Option<[u8; 32]>,
) -> Result<HandshakeOut, String> {
    tracing::info!("handshake_client: sending client hello");
    let mut eph_seed = [0u8; 32];
    OsRng.fill_bytes(&mut eph_seed);
    let eph = XSec::from(eph_seed);
    let mut nc = [0u8; 16];
    OsRng.fill_bytes(&mut nc);
    let hello = HClient {
        v: 1,
        epk: crypto::b64(XPub::from(&eph).as_bytes()),
        idpk: crypto::b64(&core.identity.sig_pk_bytes()),
        nc: crypto::b64(&nc),
        ts: crypto::now_ms(),
    };
    let cbytes = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    hs_write(stream, &cbytes).await?;

    let sraw = hs_read(stream).await?;
    let hs: HServer = serde_json::from_slice(&sraw).map_err(|e| e.to_string())?;
    let spk: [u8; 32] = crypto::un_b64(&hs.idpk)?
        .try_into()
        .map_err(|_| "bad server key".to_string())?;
    if let Some(exp) = expect {
        if crypto::sha256(&spk) != exp {
            return Err("对方设备指纹与预期不符".into());
        }
    }
    let vk = VerifyingKey::from_bytes(&spk).map_err(|_| "bad key".to_string())?;
    let nosig = hs_no_sig(&hs);
    let mut transcript_input = Vec::with_capacity(cbytes.len() + nosig.len());
    transcript_input.extend_from_slice(&cbytes);
    transcript_input.extend_from_slice(&nosig);
    let transcript = crypto::sha256(&transcript_input);
    let sig64: [u8; 64] = crypto::un_b64(&hs.sig)?
        .try_into()
        .map_err(|_| "bad sig".to_string())?;
    vk.verify(&transcript, &Signature::from_bytes(&sig64))
        .map_err(|_| "握手签名校验失败".to_string())?;
    let ns: [u8; 16] = crypto::un_b64(&hs.ns)?
        .try_into()
        .map_err(|_| "bad nonce".to_string())?;
    let sepk: [u8; 32] = crypto::un_b64(&hs.epk)?
        .try_into()
        .map_err(|_| "bad key".to_string())?;
    let shared = eph.diffie_hellman(&XPub::from(sepk));
    let keys = crypto::derive_session_keys(shared.to_bytes(), &nc, &ns, true);
    Ok((keys, crypto::sha256(&spk), vk, transcript, None))
}

async fn handshake_server(
    core: &AppCore,
    stream: &mut TcpStream,
) -> Result<HandshakeOut, String> {
    tracing::info!("handshake_server: reading client hello");
    let craw = hs_read(stream).await?;
    let hc: HClient = serde_json::from_slice(&craw).map_err(|e| e.to_string())?;
    if hc.v != 1 {
        return Err("protocol version".into());
    }
    let cpk: [u8; 32] = crypto::un_b64(&hc.idpk)?
        .try_into()
        .map_err(|_| "bad client key".to_string())?;
    let vk = VerifyingKey::from_bytes(&cpk).map_err(|_| "bad key".to_string())?;

    let mut eph_seed = [0u8; 32];
    OsRng.fill_bytes(&mut eph_seed);
    let eph = XSec::from(eph_seed);
    let mut ns = [0u8; 16];
    OsRng.fill_bytes(&mut ns);
    let skeleton = HServer {
        v: 1,
        epk: crypto::b64(XPub::from(&eph).as_bytes()),
        idpk: crypto::b64(&core.identity.sig_pk_bytes()),
        ns: crypto::b64(&ns),
        ts: crypto::now_ms(),
        sig: String::new(),
    };
    let nosig = serde_json::to_vec(&skeleton).map_err(|e| e.to_string())?;
    let mut ti = Vec::with_capacity(craw.len() + nosig.len());
    ti.extend_from_slice(&craw);
    ti.extend_from_slice(&nosig);
    let transcript = crypto::sha256(&ti);
    let mut full = skeleton;
    full.sig = crypto::b64(&core.identity.sig.sign(&transcript).to_bytes());
    let sbytes = serde_json::to_vec(&full).map_err(|e| e.to_string())?;
    hs_write(stream, &sbytes).await?;

    let nc: [u8; 16] = crypto::un_b64(&hc.nc)?
        .try_into()
        .map_err(|_| "bad nonce".to_string())?;
    let cepk: [u8; 32] = crypto::un_b64(&hc.epk)?
        .try_into()
        .map_err(|_| "bad key".to_string())?;
    let shared = eph.diffie_hellman(&XPub::from(cepk));
    let keys = crypto::derive_session_keys(shared.to_bytes(), &nc, &ns, false);
    Ok((
        keys,
        crypto::sha256(&cpk),
        vk,
        transcript,
        Some(transcript),
    ))
}

async fn start_session(
    core: SharedCore,
    stream: TcpStream,
    keys: SessionKeys,
    peer_fp: [u8; 32],
    peer_vk: VerifyingKey,
    transcript: [u8; 32],
    expect_sigc: Option<[u8; 32]>,
) -> Result<Arc<Session>, String> {
    if let Some(old) = core::get_session(&core, &peer_fp) {
        if old.is_alive() {
            tracing::info!("start_session: existing alive session for fp={}, dropping new connection", crypto::hex(&peer_fp));
            drop(stream);
            return Ok(old);
        }
    }

    let (tx, rx) = mpsc::unbounded_channel::<(u8, Vec<u8>)>();
    let sess = Arc::new(Session::new(peer_fp, tx.clone()));
    let (rd, wr) = stream.into_split();

    let w = tokio::spawn(writer_task(wr, rx, keys.clone()));
    let r = tokio::spawn(reader_task(
        core.clone(),
        rd,
        tx,
        keys,
        sess.clone(),
        peer_vk,
        transcript,
        expect_sigc,
    ));
    sess.add_abort(w.abort_handle());
    sess.add_abort(r.abort_handle());

    let fp_hex = crypto::hex(&peer_fp);
    let _ = core.db.ensure_peer(&fp_hex);
    let existing = core::register_session(&core, sess.clone());

    if let Some(_old) = existing {
        tracing::info!("start_session: race lost for fp={}, aborting new tasks", fp_hex);
        w.abort();
        r.abort();
        return Ok(_old);
    }

    let sigc_payload = crypto::b64(&core.identity.sig.sign(&transcript).to_bytes());
    sess.send(F_SIGC, sigc_payload.into_bytes());

    let verified = core.db.peer_confirmed(&fp_hex).unwrap_or(false);
    core.emit(
        "session",
        &serde_json::json!({"fp": fp_hex, "state": "open", "verified": verified}),
    );
    {
        let c = core.clone();
        let f = fp_hex.clone();
        tokio::spawn(async move { core::flush_outbox(&c, &f) });
    }
    if crate::core::avatar_file(&core.dir, &fp_hex).is_none() {
        core.ava_pending.lock().unwrap().remove(&fp_hex);
        crate::discovery::request_avatar(&core, &peer_fp);
    }
    Ok(sess)
}

async fn writer_task(
    mut wr: OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<(u8, Vec<u8>)>,
    k: SessionKeys,
) {
    let mut seq: u64 = 0;
    while let Some((t, pl)) = rx.recv().await {
        let ct = crypto::seal(&k.send_key, &k.send_prefix, t, seq, &pl);
        let total = (9 + ct.len()) as u32;
        let mut buf = Vec::with_capacity(4 + total as usize);
        buf.extend_from_slice(&total.to_be_bytes());
        buf.push(t);
        buf.extend_from_slice(&seq.to_be_bytes());
        buf.extend_from_slice(&ct);
        if wr.write_all(&buf).await.is_err() {
            break;
        }
        if wr.flush().await.is_err() {
            break;
        }
        if t == F_BYE {
            break;
        }
        seq += 1;
    }
    let _ = wr.shutdown().await;
}

#[allow(clippy::too_many_arguments)]
async fn reader_task(
    core: SharedCore,
    mut rd: OwnedReadHalf,
    tx: mpsc::UnboundedSender<(u8, Vec<u8>)>,
    k: SessionKeys,
    sess: Arc<Session>,
    peer_vk: VerifyingKey,
    _transcript: [u8; 32],
    expect_sigc: Option<[u8; 32]>,
) {
    let fp_hex = crypto::hex(&sess.fp);
    let mut next: u64 = 0;
    let mut sigc_pending = expect_sigc;
    tracing::info!("reader_task: started for fp={}", fp_hex);
    loop {
        let mut lb = [0u8; 4];
        if rd.read_exact(&mut lb).await.is_err() {
            tracing::info!("reader_task: read length failed for fp={}", fp_hex);
            break;
        }
        let total = u32::from_be_bytes(lb) as usize;
        if !(9..=crypto::MAX_FRAME_BODY).contains(&total) {
            tracing::warn!("reader_task: bad frame size={} fp={}", total, fp_hex);
            break;
        }
        let mut body = vec![0u8; total];
        if rd.read_exact(&mut body).await.is_err() {
            tracing::info!("reader_task: read body failed for fp={}", fp_hex);
            break;
        }
        let t = body[0];
        let seq = u64::from_be_bytes(body[1..9].try_into().unwrap());
        if seq != next {
            tracing::warn!("reader_task: seq mismatch got={} expected={} fp={}", seq, next, fp_hex);
            break;
        }
        next += 1;
        let pl = match crypto::open(&k.recv_key, &k.recv_prefix, t, seq, &body[9..]) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("reader_task: decrypt failed type={} seq={} fp={}: {}", t, seq, fp_hex, e);
                break;
            }
        };
        sess.last_rx_ms
            .store(crypto::now_ms(), Ordering::Relaxed);

        match t {
            F_BYE => {
                tracing::info!("reader_task: received F_BYE fp={}", fp_hex);
                break;
            }
            F_SIGC => {
                if let Some(tr) = sigc_pending.take() {
                    let ok = std::str::from_utf8(&pl)
                        .ok()
                        .and_then(|s| crypto::un_b64(s).ok())
                        .and_then(|b| <[u8; 64]>::try_from(b).ok())
                        .map(|s| peer_vk.verify(&tr, &Signature::from_bytes(&s)).is_ok())
                        .unwrap_or(false);
                    if !ok {
                        tracing::warn!("reader_task: F_SIGC verification failed fp={}", fp_hex);
                        break;
                    }
                    tracing::info!("reader_task: F_SIGC verified fp={}", fp_hex);
                }
            }
            F_PING => {
                tx.send((F_PONG, b"ok".to_vec())).ok();
            }
            F_PONG => {}
            F_MSG => msg_in(&core, &fp_hex, &pl).await,
            F_ACK => ack_in(&core, &pl),
            F_TYPING => {
                core.emit("typing", &serde_json::json!({"fp": fp_hex}));
            }
            F_META | F_CHUNK | F_FACK => {
                crate::transfer::on_frame(&core, &tx, t, &fp_hex, &pl).await;
            }
            F_AVA_GET => send_self_avatar(&core, &tx),
            F_AVA_DATA => save_avatar(&core, &fp_hex, &pl).await,
            F_AV => {
                if let Ok(s) = std::str::from_utf8(&pl) {
                    core.emit(
                        "call-signal",
                        &serde_json::json!({"fp": fp_hex, "payload": s}),
                    );
                }
            }
            _ => {}
        }
        if !sess.is_alive() {
            tracing::info!("reader_task: session not alive, breaking fp={}", fp_hex);
            break;
        }
    }
    tracing::info!("reader_task: exited loop for fp={}, calling unregister", fp_hex);
    sess.alive_flag_off();
    core::unregister_session(&core, &sess.fp, &sess);
    core.emit(
        "session",
        &serde_json::json!({"fp": fp_hex, "state": "closed"}),
    );
}

async fn msg_in(core: &AppCore, fp_hex: &str, pl: &[u8]) {
    #[derive(Deserialize)]
    struct Mp {
        mid: String,
        body: String,
        #[serde(default)]
        ts: u64,
    }
    let mp: Mp = match serde_json::from_slice(pl) {
        Ok(m) => m,
        Err(_) => return,
    };
    let now = crypto::now_ms();
    let ts = if mp.ts == 0 || now.abs_diff(mp.ts) > 3 * 86_400_000 {
        now
    } else {
        mp.ts
    };
    let notification_body = mp.body.clone();
    let row = crate::store::MsgRow {
        mid: mp.mid.clone(),
        fp: fp_hex.to_string(),
        dir: 1,
        kind: "text".into(),
        body: Some(mp.body),
        fid: None,
        fname: None,
        fsize: None,
        ts,
        state: "unread".into(),
    };
    if core.db.insert_message(&row).is_err() {
        return;
    }
    let ack = serde_json::json!({"mid": mp.mid, "state": "delivered"});
    tx_of(core, fp_hex).map(|s| {
        s.send(
            F_ACK,
            serde_json::to_vec(&ack).unwrap_or_default(),
        )
    });
    core.emit("message-new", &core::build_msg_view(core, row));
    core::notify_incoming(core, fp_hex, &notification_body);
}

fn tx_of(core: &AppCore, fp_hex: &str) -> Option<Arc<Session>> {
    let fp = core::fp_bytes(fp_hex)?;
    core::get_session(core, &fp)
}

fn ack_in(core: &AppCore, pl: &[u8]) {
    #[derive(Deserialize)]
    struct Aa {
        mid: String,
        state: String,
    }
    let aa: Aa = match serde_json::from_slice(pl) {
        Ok(a) => a,
        Err(_) => return,
    };
    if !matches!(aa.state.as_str(), "delivered" | "read") {
        return;
    }
    if aa.state == "delivered" {
        core.transfers.remove_by_mid(&aa.mid);
    }
    if core.db.update_msg_state(&aa.mid, &aa.state).unwrap_or(false) {
        core.emit(
            "message-state",
            &serde_json::json!({"mid": aa.mid, "state": aa.state}),
        );
    }
}

fn send_self_avatar(core: &AppCore, tx: &mpsc::UnboundedSender<(u8, Vec<u8>)>) {
    if let Some(p) = core::self_avatar_file(core) {
        if let Ok(bytes) = std::fs::read(p) {
            if bytes.len() <= 200_000 {
                tx.send((F_AVA_DATA, bytes)).ok();
            }
        }
    }
}

async fn save_avatar(core: &AppCore, fp_hex: &str, pl: &[u8]) {
    if pl.is_empty() || pl.len() > 200_000 {
        crate::discovery::avatar_fetch_done(core, fp_hex);
        return;
    }
    let ext = if pl.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if pl.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else {
        crate::discovery::avatar_fetch_done(core, fp_hex);
        return;
    };
    let dir = core.avatars_dir();
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(format!("{}.{}", fp_hex, ext));
    let old = core::avatar_file(&core.dir, fp_hex);
    if let Ok(()) = std::fs::write(&p, pl) {
        if let Some(o) = old {
            if o != p {
                let _ = std::fs::remove_file(o);
            }
        }
    }
    crate::discovery::avatar_fetch_done(core, fp_hex);
    core.emit(
        "avatar-changed",
        &serde_json::json!({"fp": fp_hex}),
    );
    core::emit_peers(core);
}
