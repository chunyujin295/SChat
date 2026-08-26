use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use rand::rngs::OsRng;
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

fn seal_field(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plain,
                aad,
            },
        )
        .expect("field seal");
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.append(&mut ct);
    out
}

fn open_field(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 13 {
        return Err("blob too short".into());
    }
    let (n, ct) = blob.split_at(12);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            Nonce::from_slice(n.try_into().unwrap()),
            Payload { msg: ct, aad },
        )
        .map_err(|_| "field open failed".into())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MsgRow {
    pub mid: String,
    pub fp: String,
    pub dir: i32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsize: Option<i64>,
    pub ts: u64,
    pub state: String,
}

pub struct Db {
    conn: Mutex<Connection>,
    key: [u8; 32],
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS peers(
  fp TEXT PRIMARY KEY,
  nick_enc BLOB,
  ava_ver INTEGER DEFAULT 0,
  last_ip TEXT DEFAULT '',
  tcp_port INTEGER DEFAULT 0,
  confirmed INTEGER DEFAULT 0,
  blocked INTEGER DEFAULT 0,
  first_seen INTEGER DEFAULT 0,
  last_seen INTEGER DEFAULT 0
);
CREATE TABLE IF NOT EXISTS alias(
  nick TEXT PRIMARY KEY,
  fp TEXT
);
CREATE TABLE IF NOT EXISTS messages(
  mid TEXT PRIMARY KEY,
  fp TEXT NOT NULL,
  dir INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body_enc BLOB,
  fid TEXT,
  ts INTEGER NOT NULL,
  state TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_msg_fp_ts ON messages(fp, ts);
CREATE TABLE IF NOT EXISTS files(
  fid TEXT PRIMARY KEY,
  fp TEXT,
  name_enc BLOB,
  size INTEGER,
  sha TEXT,
  mime TEXT,
  kind TEXT,
  path TEXT,
  dir INTEGER,
  created_at INTEGER
);
CREATE TABLE IF NOT EXISTS outbox(
  mid TEXT PRIMARY KEY,
  fp TEXT,
  queued_at INTEGER,
  attempts INTEGER DEFAULT 0
);
"#;

impl Db {
    pub fn open(path: &Path, key: [u8; 32]) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Db {
            conn: Mutex::new(conn),
            key,
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let c = self.conn.lock().map_err(|_| "db poisoned")?;
        f(&c).map_err(|e| e.to_string())
    }

    pub fn upsert_peer(&self, fp_hex: &str, nick: &str, ava_ver: u64, ip: &str, port: u16, ts: u64) -> Result<Option<String>, String> {
        let nick_enc = seal_field(&self.key, nick.as_bytes(), fp_hex.as_bytes());
        let prev_conflict = self.get_alias_fp(nick)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO peers(fp,nick_enc,ava_ver,last_ip,tcp_port,first_seen,last_seen)
                 VALUES(?1,?2,?3,?4,?5,?6,?6)
                 ON CONFLICT(fp) DO UPDATE SET nick_enc=excluded.nick_enc,
                   ava_ver=CASE WHEN excluded.ava_ver>=peers.ava_ver THEN excluded.ava_ver ELSE peers.ava_ver END,
                   last_ip=excluded.last_ip, tcp_port=excluded.tcp_port,
                   last_seen=MAX(peers.last_seen, excluded.last_seen)",
                params![fp_hex, nick_enc, ava_ver as i64, ip, port, ts as i64],
            )?;
            Ok(())
        })?;
        self.with(|c| {
            c.execute(
                "INSERT INTO alias(nick,fp) VALUES(?1,?2)
                 ON CONFLICT(nick) DO UPDATE SET fp=excluded.fp",
                params![nick, fp_hex],
            )?;
            Ok(())
        })?;
        Ok(prev_conflict.filter(|old| old != fp_hex))
    }

    pub fn touch_peer(&self, fp_hex: &str, ts: u64) -> Result<(), String> {
        self.with(|c| {
            c.execute(
                "UPDATE peers SET last_seen=?2 WHERE fp=?1",
                params![fp_hex, ts as i64],
            )?;
            Ok(())
        })
    }

    pub fn get_alias_fp(&self, nick: &str) -> Result<Option<String>, String> {
        self.with(|c| {
            let mut st = c.prepare("SELECT fp FROM alias WHERE nick=?1")?;
            let mut rows = st.query(params![nick])?;
            if let Some(r) = rows.next()? {
                Ok(r.get::<_, Option<String>>(0)?)
            } else {
                Ok(None)
            }
        })
    }

    pub fn ensure_peer(&self, fp_hex: &str) -> Result<(), String> {
        let nick_enc = seal_field(&self.key, "未命名设备".as_bytes(), fp_hex.as_bytes());
        let now = crate::crypto::now_ms() as i64;
        self.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO peers(fp,nick_enc,first_seen,last_seen) VALUES(?1,?2,?3,?3)",
                params![fp_hex, nick_enc, now],
            )?;
            Ok(())
        })
    }

    pub fn set_peer_flag(&self, fp_hex: &str, field: &str, on: bool) -> Result<(), String> {
        let sql = match field {
            "confirmed" => "UPDATE peers SET confirmed=?2 WHERE fp=?1",
            "blocked" => "UPDATE peers SET blocked=?2 WHERE fp=?1",
            _ => return Err("bad flag".into()),
        };
        self.with(|c| {
            c.execute(sql, params![fp_hex, on as i64])?;
            Ok(())
        })
    }

    pub fn forget_peer(&self, fp_hex: &str) -> Result<(), String> {
        self.with(|c| {
            c.execute("DELETE FROM messages WHERE fp=?1", params![fp_hex])?;
            c.execute("DELETE FROM files WHERE fp=?1", params![fp_hex])?;
            c.execute("DELETE FROM outbox WHERE fp=?1", params![fp_hex])?;
            c.execute("DELETE FROM alias WHERE fp=?1", params![fp_hex])?;
            c.execute("DELETE FROM peers WHERE fp=?1", params![fp_hex])?;
            Ok(())
        })
    }

    pub fn peer_confirmed(&self, fp_hex: &str) -> Result<bool, String> {
        self.with(|c| {
            let mut st = c.prepare("SELECT confirmed FROM peers WHERE fp=?1")?;
            let mut rows = st.query(params![fp_hex])?;
            if let Some(r) = rows.next()? {
                Ok(r.get::<_, i64>(0)? != 0)
            } else {
                Ok(false)
            }
        })
    }

    pub fn list_peers(&self) -> Result<Vec<(String, String, u64, bool, bool, u64)>, String> {
        let rows = self.with(|c| {
            let mut st = c.prepare(
                "SELECT fp, nick_enc, ava_ver, confirmed, blocked, last_seen FROM peers ORDER BY last_seen DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, i64>(3)? != 0,
                    r.get::<_, i64>(4)? != 0,
                    r.get::<_, i64>(5)? as u64,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        })?;
        let mut out = Vec::new();
        for (fp, enc, ava, conf, blocked, _seen) in rows {
            let nick = open_field(&self.key, &enc, fp.as_bytes())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            out.push((fp, nick, ava, conf, blocked, _seen));
        }
        Ok(out)
    }

    pub fn insert_message(&self, m: &MsgRow) -> Result<(), String> {
        let body_enc = match &m.body {
            Some(b) => Some(seal_field(&self.key, b.as_bytes(), m.mid.as_bytes())),
            None => None,
        };
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO messages(mid,fp,dir,kind,body_enc,fid,ts,state)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![m.mid, m.fp, m.dir, m.kind, body_enc, m.fid, m.ts as i64, m.state],
            )?;
            Ok(())
        })
    }

    pub fn update_msg_state(&self, mid: &str, state: &str) -> Result<bool, String> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE messages SET state=?2 WHERE mid=?1",
                params![mid, state],
            )?;
            Ok(n > 0)
        })
    }

    pub fn mark_incoming_read(&self, fp_hex: &str) -> Result<Vec<String>, String> {
        let unread = self.unread_mids(fp_hex)?;
        for mid in &unread {
            self.update_msg_state(mid, "read")?;
        }
        Ok(unread)
    }

    fn unread_mids(&self, fp_hex: &str) -> Result<Vec<String>, String> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT mid FROM messages WHERE fp=?1 AND dir=1 AND state!='read'",
            )?;
            let rows = st.query_map(params![fp_hex], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        })
    }

    pub fn list_messages(&self, fp_hex: &str, limit: u32) -> Result<Vec<MsgRow>, String> {
        let rows = self.with(|c| {
            let mut st = c.prepare(
                "SELECT mid,dir,kind,body_enc,fid,ts,state FROM messages
                 WHERE fp=?1 ORDER BY ts DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![fp_hex, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)? as u64,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let mut out: Vec<MsgRow> = rows
            .into_iter()
            .map(|(mid, dir, kind, enc, fid, ts, state)| {
                let body = enc
                    .and_then(|b| open_field(&self.key, &b, mid.as_bytes()).ok())
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                MsgRow {
                    mid,
                    fp: fp_hex.into(),
                    dir: dir as i32,
                    kind,
                    body,
                    fid,
                    fname: None,
                    fsize: None,
                    ts,
                    state,
                }
            })
            .collect();
        out.reverse();
        Ok(out)
    }

    pub fn msg_by_mid(&self, mid: &str) -> Option<MsgRow> {
        let raw = self
            .with(|c| {
                let mut st = c.prepare(
                    "SELECT fp,dir,kind,body_enc,fid,ts,state FROM messages WHERE mid=?1",
                )?;
                let mut rows = st.query(params![mid])?;
                if let Some(r) = rows.next()? {
                    Ok(Some((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<Vec<u8>>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, i64>(5)? as u64,
                        r.get::<_, String>(6)?,
                    )))
                } else {
                    Ok(None)
                }
            })
            .ok()??;
        let body = raw
            .3
            .and_then(|b| open_field(&self.key, &b, mid.as_bytes()).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        Some(MsgRow {
            mid: mid.to_string(),
            fp: raw.0,
            dir: raw.1 as i32,
            kind: raw.2,
            body,
            fid: raw.4,
            fname: None,
            fsize: None,
            ts: raw.5,
            state: raw.6,
        })
    }

    pub fn conversations(&self) -> Result<Vec<(String, u64, u64)>, String> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT p.fp, COALESCE(MAX(m.ts),0),
                        COALESCE(SUM(CASE WHEN m.dir=1 AND m.state!='read' THEN 1 ELSE 0 END),0)
                 FROM peers p LEFT JOIN messages m ON m.fp=p.fp
                 GROUP BY p.fp ORDER BY 2 DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        })
    }

    pub fn enqueue_outbox(&self, mid: &str, fp_hex: &str) -> Result<(), String> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO outbox(mid,fp,queued_at,attempts) VALUES(?1,?2,?3,0)",
                params![mid, fp_hex, crate::crypto::now_ms() as i64],
            )?;
            Ok(())
        })
    }

    pub fn pop_outbox(&self, fp_hex: &str) -> Result<Vec<String>, String> {
        let mids: Vec<String> = self.with(|c| {
            let mut st = c.prepare("SELECT mid FROM outbox WHERE fp=?1 ORDER BY queued_at")?;
            let rows = st.query_map(params![fp_hex], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        })?;
        for mid in &mids {
            self.with(|c| {
                c.execute("DELETE FROM outbox WHERE mid=?1", params![mid])?;
                Ok(())
            })?;
        }
        Ok(mids)
    }

    pub fn drop_outbox_mid(&self, mid: &str) -> Result<(), String> {
        self.with(|c| {
            c.execute("DELETE FROM outbox WHERE mid=?1", params![mid])?;
            Ok(())
        })
    }

    pub fn upsert_file_meta(
        &self,
        fid: &str,
        fp_hex: &str,
        name: &str,
        size: i64,
        sha: &str,
        mime: &str,
        kind: &str,
        path: &str,
        dir: i32,
    ) -> Result<(), String> {
        let name_enc = seal_field(&self.key, name.as_bytes(), fid.as_bytes());
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO files(fid,fp,name_enc,size,sha,mime,kind,path,dir,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    fid,
                    fp_hex,
                    name_enc,
                    size,
                    sha,
                    mime,
                    kind,
                    path,
                    dir,
                    crate::crypto::now_ms() as i64
                ],
            )?;
            Ok(())
        })
    }

    pub fn file_info(&self, fid: &str) -> Result<Option<(String, String, i64, String, String, i32, String)>, String> {
        let raw = self.with(|c| {
            let mut st = c.prepare(
                "SELECT name_enc,size,sha,mime,kind,path,dir FROM files WHERE fid=?1",
            )?;
            let mut rows = st.query(params![fid])?;
            if let Some(r) = rows.next()? {
                Ok(Some((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)? as i32,
                    r.get::<_, String>(6)?,
                )))
            } else {
                Ok(None)
            }
        })?;
        match raw {
            Some((enc, size, sha, mime, kind, dir, path)) => {
                let name = open_field(&self.key, &enc, fid.as_bytes())
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                Ok(Some((name, sha, size, mime, kind, dir, path)))
            }
            None => Ok(None),
        }
    }

    pub fn clear_history(&self, fp_hex: Option<&str>) -> Result<(), String> {
        self.with(|c| match fp_hex {
            Some(fp) => {
                c.execute("DELETE FROM messages WHERE fp=?1", params![fp])?;
                c.execute("DELETE FROM files WHERE fp=?1", params![fp])?;
                c.execute("DELETE FROM outbox WHERE fp=?1", params![fp])?;
                Ok(())
            }
            None => {
                c.execute_batch("DELETE FROM messages; DELETE FROM files; DELETE FROM outbox;")
            }
        })
    }

}

