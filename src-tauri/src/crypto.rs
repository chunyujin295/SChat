use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

pub const MAX_FRAME_BODY: usize = 262_144;
pub const HANDSHAKE_MAX: usize = 4096;

#[derive(Clone)]
pub struct SessionKeys {
    pub send_key: [u8; 32],
    pub send_prefix: [u8; 4],
    pub recv_key: [u8; 32],
    pub recv_prefix: [u8; 4],
}

pub fn derive_session_keys(shared: [u8; 32], nc: &[u8], ns: &[u8], is_client: bool) -> SessionKeys {
    let mut salt = Vec::with_capacity(nc.len() + ns.len());
    salt.extend_from_slice(nc);
    salt.extend_from_slice(ns);
    let hk = Hkdf::<Sha256>::new(Some(&salt), &shared);
    let mut okm = [0u8; 72];
    hk.expand(b"SCHAT-v1-session", &mut okm)
        .expect("hkdf expand");
    let kc: [u8; 32] = okm[0..32].try_into().unwrap();
    let ks: [u8; 32] = okm[32..64].try_into().unwrap();
    let pc: [u8; 4] = okm[64..68].try_into().unwrap();
    let ps: [u8; 4] = okm[68..72].try_into().unwrap();
    if is_client {
        SessionKeys {
            send_key: kc,
            send_prefix: pc,
            recv_key: ks,
            recv_prefix: ps,
        }
    } else {
        SessionKeys {
            send_key: ks,
            send_prefix: ps,
            recv_key: kc,
            recv_prefix: pc,
        }
    }
}

fn build_nonce(prefix: &[u8; 4], seq: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(prefix);
    n[4..].copy_from_slice(&seq.to_be_bytes());
    n
}

fn aad(ftype: u8, seq: u64) -> [u8; 9] {
    let mut a = [0u8; 9];
    a[0] = ftype;
    a[1..].copy_from_slice(&seq.to_be_bytes());
    a
}

pub fn seal(key: &[u8; 32], prefix: &[u8; 4], ftype: u8, seq: u64, plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let n = build_nonce(prefix, seq);
    cipher
        .encrypt(
            Nonce::from_slice(&n),
            Payload {
                msg: plaintext,
                aad: &aad(ftype, seq),
            },
        )
        .expect("aead seal")
}

pub fn open(
    key: &[u8; 32],
    prefix: &[u8; 4],
    ftype: u8,
    seq: u64,
    ciphertext: &[u8],
) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let n = build_nonce(prefix, seq);
    cipher
        .decrypt(
            Nonce::from_slice(&n),
            Payload {
                msg: ciphertext,
                aad: &aad(ftype, seq),
            },
        )
        .map_err(|_| "aead open failed".to_string())
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn fingerprint_display(pk: &[u8; 32]) -> String {
    let d = sha256(pk);
    let mut out = String::with_capacity(32);
    let mut bits: u32 = 0;
    let mut acc: u32 = 0;
    for &b in d.iter().take(15) {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((acc >> bits) & 31) as usize] as char);
            if out.len() == 24 {
                break;
            }
        }
        if out.len() == 24 {
            break;
        }
    }
    let groups: Vec<String> = (0..6)
        .map(|i| out[i * 4..i * 4 + 4].to_string())
        .collect();
    format!("SCAT-{}", groups.join("-"))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn un_b64(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn ym_of(secs: u64) -> (i64, u32) {
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yy = if m <= 2 { y + 1 } else { y };
    (yy, m as u32)
}

