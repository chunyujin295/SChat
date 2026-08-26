use crate::crypto::{fingerprint_display, hex, sha256};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rand::RngCore;
use std::path::{Path, PathBuf};
use x25519_dalek::StaticSecret as XSec;

pub struct Identity {
    pub sig: SigningKey,
    pub x: XSec,
}

impl Identity {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let sig = SigningKey::from_bytes(&seed);
        let mut xb = [0u8; 32];
        OsRng.fill_bytes(&mut xb);
        let x = XSec::from(xb);
        Identity { sig, x }
    }

    pub fn sig_pk_bytes(&self) -> [u8; 32] {
        self.sig.verifying_key().to_bytes()
    }

    pub fn fp(&self) -> [u8; 32] {
        sha256(&self.sig_pk_bytes())
    }

    pub fn fp_hex(&self) -> String {
        hex(&self.fp())
    }

    pub fn fp_display(&self) -> String {
        fingerprint_display(&self.sig_pk_bytes())
    }
}

const MAGIC: &[u8; 5] = b"SCAT1";

pub fn load_or_create(dir: &Path) -> Result<Identity, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path: PathBuf = dir.join("identity.bin");
    if path.exists() {
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        if raw.len() < 9 || &raw[..5] != MAGIC {
            return Err("identity.bin corrupted".into());
        }
        let blen =
            u32::from_le_bytes([raw[5], raw[6], raw[7], raw[8]]) as usize;
        let blob = raw
            .get(9..9 + blen)
            .ok_or_else(|| "identity.bin truncated".to_string())?;
        let plain = dpapi_unprotect(blob)?;
        if plain.len() != 64 {
            return Err("identity secret size mismatch".into());
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&plain[..32]);
        let mut xb = [0u8; 32];
        xb.copy_from_slice(&plain[32..]);
        Ok(Identity {
            sig: SigningKey::from_bytes(&seed),
            x: XSec::from(xb),
        })
    } else {
        let id = Identity::generate();
        let mut plain = Vec::with_capacity(64);
        plain.extend_from_slice(id.sig.to_bytes().as_slice());
        plain.extend_from_slice(id.x.to_bytes().as_slice());
        let blob = dpapi_protect(&plain)?;
        let mut out = Vec::with_capacity(9 + blob.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&blob);
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(id)
    }
}

pub fn load_or_create_db_key(dir: &Path) -> Result<[u8; 32], String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path: PathBuf = dir.join("keyring.bin");
    if path.exists() {
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        if raw.len() < 9 || &raw[..5] != MAGIC {
            return Err("keyring.bin corrupted".into());
        }
        let blen =
            u32::from_le_bytes([raw[5], raw[6], raw[7], raw[8]]) as usize;
        let blob = raw
            .get(9..9 + blen)
            .ok_or_else(|| "keyring.bin truncated".to_string())?;
        let plain = dpapi_unprotect(blob)?;
        plain
            .try_into()
            .map_err(|_| "db key size mismatch".to_string())
    } else {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let blob = dpapi_protect(&key)?;
        let mut out = Vec::with_capacity(9 + blob.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&blob);
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(key)
    }
}

mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
        CRYPTPROTECT_UI_FORBIDDEN,
    };

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let mut inp = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptProtectData(
                &mut inp,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            );
            if ok == 0 {
                return Err("CryptProtectData failed".into());
            }
            let slice = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
            let v = slice.to_vec();
            LocalFree(out.pbData as _);
            Ok(v)
        }
    }
    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let inp = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptUnprotectData(
                &inp,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut out,
            );
            if ok == 0 {
                return Err("CryptUnprotectData failed".into());
            }
            let slice = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
            let v = slice.to_vec();
            LocalFree(out.pbData as _);
            Ok(v)
        }
    }
}

use dpapi::{protect as dpapi_protect, unprotect as dpapi_unprotect};
