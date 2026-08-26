use crate::core::{fp_bytes, AppCore, PeerEntry, SharedCore};
use crate::crypto::{hex, now_ms, sha256};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

pub const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 53, 67);
pub const DPORT: u16 = 53080;
const HEARTBEAT_MS: u64 = 3000;
const OFFLINE_AFTER_MS: u64 = 12000;

#[derive(Serialize, Deserialize)]
struct Body {
    v: u8,
    t: String,
    inst: String,
    nick: String,
    fp: String,
    pk: String,
    port: u16,
    ava: u64,
    ts: u64,
}

#[derive(Serialize, Deserialize)]
struct Pkt {
    p: Body,
    sig: String,
}

pub async fn run(core: SharedCore) {
    let udp = match open_socket() {
        Ok(u) => Arc::new(u),
        Err(e) => {
            tracing::error!("discovery socket: {e}");
            return;
        }
    };
    let send_lock = Arc::new(AsyncMutex::new(()));

    let c2 = core.clone();
    let s1 = udp.clone();
    let l1 = send_lock.clone();
    tokio::spawn(async move { announcer(c2, s1, l1).await });

    let c3 = core.clone();
    let s2 = udp.clone();
    tokio::spawn(async move { receiver(c3, s2).await });

    let c4 = core.clone();
    tokio::spawn(async move { sweep(c4).await });
}

fn open_socket() -> Result<tokio::net::UdpSocket, String> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| e.to_string())?;
    sock.set_reuse_address(true).map_err(|e| e.to_string())?;
    sock.bind(&SocketAddr::from(([0, 0, 0, 0], DPORT)).into())
        .map_err(|e| e.to_string())?;
    sock.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| e.to_string())?;
    let _ = sock.set_multicast_loop_v4(true);
    let std_sock: std::net::UdpSocket = sock.into();
    std_sock
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    tokio::net::UdpSocket::from_std(std_sock).map_err(|e| e.to_string())
}

fn build_packet(core: &AppCore, kind: &str) -> Vec<u8> {
    let body = Body {
        v: 1,
        t: kind.to_string(),
        inst: core.instance_id.clone(),
        nick: core.nickname(),
        fp: core.identity.fp_hex(),
        pk: crate::b64(&core.identity.sig_pk_bytes()),
        port: core.tcp_port_value(),
        ava: core.avatar_version(),
        ts: now_ms(),
    };
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    let sig = core.identity.sig.sign(&body_bytes);
    let pkt = Pkt {
        p: body,
        sig: crate::b64(&sig.to_bytes()),
    };
    serde_json::to_vec(&pkt).unwrap_or_default()
}

async fn announcer(
    core: SharedCore,
    udp: Arc<tokio::net::UdpSocket>,
    lock: Arc<AsyncMutex<()>>,
) {
    for _ in 0..3 {
        send_one(&core, &udp, &lock, "query").await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut tick = tokio::time::interval(Duration::from_millis(HEARTBEAT_MS));
    loop {
        tick.tick().await;
        send_one(&core, &udp, &lock, "announce").await;
    }
}

async fn send_one(
    core: &SharedCore,
    udp: &tokio::net::UdpSocket,
    _lock: &AsyncMutex<()>,
    kind: &str,
) {
    let bytes = build_packet(core, kind);
    let _ = udp.send_to(&bytes, (GROUP, DPORT)).await;
    let _ = udp.send_to(&bytes, (Ipv4Addr::BROADCAST, DPORT)).await;
}

pub fn goodbye_blocking(core: &SharedCore) {
    if let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", 0)) {
        let bytes = build_packet(core, "goodbye");
        let _ = sock.send_to(&bytes, (GROUP, DPORT));
        let _ = sock.send_to(&bytes, (Ipv4Addr::BROADCAST, DPORT));
    }
}

async fn receiver(core: SharedCore, udp: Arc<tokio::net::UdpSocket>) {
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, from) = match udp.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        handle_packet(&core, &buf[..n], from).await;
    }
}

async fn handle_packet(core: &SharedCore, data: &[u8], from: SocketAddr) {
    let pkt: Pkt = match serde_json::from_slice(data) {
        Ok(p) => p,
        Err(_) => return,
    };
    if pkt.p.v != 1 || !matches!(pkt.p.t.as_str(), "announce" | "query" | "goodbye") {
        return;
    }
    let pk_bytes: [u8; 32] = match crate::un_b64(&pkt.p.pk) {
        Ok(b) if b.len() == 32 => b.try_into().unwrap(),
        _ => return,
    };
    if pk_bytes == core.identity.sig_pk_bytes() {
        return;
    }
    if hex(&sha256(&pk_bytes)) != pkt.p.fp {
        return;
    }
    let body_bytes = match serde_json::to_vec(&pkt.p) {
        Ok(b) => b,
        Err(_) => return,
    };
    let sig_bytes: [u8; 64] = match crate::un_b64(&pkt.sig) {
        Ok(b) if b.len() == 64 => b.try_into().unwrap(),
        _ => return,
    };
    let vk = VerifyingKey::from_bytes(&pk_bytes).unwrap();
    if vk.verify(&body_bytes, &Signature::from_bytes(&sig_bytes)).is_err() {
        return;
    }
    let fpb = match fp_bytes(&pkt.p.fp) {
        Some(f) => f,
        None => return,
    };

    if pkt.p.t == "goodbye" {
        let mut map = core.peers.lock().unwrap();
        if let Some(e) = map.get_mut(&fpb) {
            if e.online {
                e.online = false;
                drop(map);
                crate::core::emit_peers(core);
            }
        }
        return;
    }

    let now = now_ms();
    let conflict = core
        .db
        .upsert_peer(&pkt.p.fp, &pkt.p.nick, pkt.p.ava, &from.ip().to_string(), pkt.p.port, now)
        .ok()
        .flatten();
    if let Some(old_fp) = conflict {
        core.emit(
            "alert",
            &serde_json::json!({
                "code": "alias-changed",
                "nick": pkt.p.nick,
                "knownFp": old_fp,
                "newFp": pkt.p.fp
            }),
        );
    }

    let changed = {
        let mut map = core.peers.lock().unwrap();
        let e = map.entry(fpb).or_insert_with(|| PeerEntry {
            fp: pkt.p.fp.clone(),
            inst: pkt.p.inst.clone(),
            nick: pkt.p.nick.clone(),
            ip: from.ip().to_string(),
            port: pkt.p.port,
            ava_ver: pkt.p.ava,
            online: false,
            last_seen_ms: now,
        });
        let changed =
            !e.online || e.nick != pkt.p.nick || e.ava_ver != pkt.p.ava || e.inst != pkt.p.inst;
        e.inst = pkt.p.inst.clone();
        e.nick = pkt.p.nick.clone();
        e.ip = from.ip().to_string();
        e.port = pkt.p.port;
        e.ava_ver = pkt.p.ava;
        e.last_seen_ms = now;
        e.online = true;
        changed
    };

    if changed {
        crate::core::emit_peers(core);
        let c = core.clone();
        let fp_hex = pkt.p.fp.clone();
        tokio::spawn(async move {
            crate::core::flush_outbox(&c, &fp_hex);
        });
    }

    if crate::core::avatar_file(&core.dir, &pkt.p.fp).is_none() {
        maybe_fetch_avatar(core, &fpb);
    }
}

fn maybe_fetch_avatar(core: &SharedCore, fpb: &[u8; 32]) {
    {
        let mut pending = core.ava_pending.lock().unwrap();
        let key = hex(fpb);
        if pending.contains(&key) {
            return;
        }
        pending.insert(key);
    }
    spawn_fetch(core.clone(), *fpb);
}

pub fn request_avatar(core: &SharedCore, fpb: &[u8; 32]) {
    maybe_fetch_avatar(core, fpb);
}

fn spawn_fetch(core: SharedCore, fpb: [u8; 32]) {
    tokio::spawn(async move {
        let got = (|| async {
            let sess = crate::core::ensure_session(&core, &fpb).await?;
            sess.send(crate::transport::F_AVA_GET, Vec::new());
            Ok::<(), String>(())
        })()
        .await;
        if got.is_err() {
            core.ava_pending.lock().unwrap().remove(&hex(&fpb));
        }
    });
}

pub fn avatar_fetch_done(core: &crate::core::AppCore, fp_hex: &str) {
    core.ava_pending.lock().unwrap().remove(fp_hex);
}

async fn sweep(core: SharedCore) {
    let mut tick = tokio::time::interval(Duration::from_millis(HEARTBEAT_MS));
    loop {
        tick.tick().await;
        let now = now_ms();
        let went_offline: Vec<String> = {
            let mut map = core.peers.lock().unwrap();
            let mut out = Vec::new();
            for e in map.values_mut() {
                if e.online && now.saturating_sub(e.last_seen_ms) > OFFLINE_AFTER_MS {
                    e.online = false;
                    out.push(e.fp.clone());
                }
            }
            out
        };
        if !went_offline.is_empty() {
            for fp in &went_offline {
                let _ = core.db.touch_peer(fp, now);
            }
            crate::core::emit_peers(&core);
        }
    }
}
