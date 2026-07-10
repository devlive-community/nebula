//! 账号持久化(SQLite)。凭证以明文存本地库;后续可换成系统钥匙串。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

/// 一条账号记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: String,
    /// 厂商标识,如 `aliyun`。
    pub vendor: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub endpoint: String,
}

/// 基于 SQLite 的账号存储。跨命令线程共享,内部用 Mutex 串行化访问。
pub struct AccountStore {
    conn: Mutex<Connection>,
}

impl AccountStore {
    /// 打开(或创建)库文件并建表。
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (
                id                TEXT PRIMARY KEY,
                vendor            TEXT NOT NULL,
                access_key_id     TEXT NOT NULL,
                access_key_secret TEXT NOT NULL,
                endpoint          TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS upload_sessions (
                session_key TEXT PRIMARY KEY,
                upload_id   TEXT NOT NULL,
                size        INTEGER NOT NULL,
                mtime       INTEGER NOT NULL,
                part_size   INTEGER NOT NULL,
                parts       TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 列出所有账号(按 id 字典序)。
    pub fn list(&self) -> rusqlite::Result<Vec<AccountRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, vendor, access_key_id, access_key_secret, endpoint
             FROM accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountRecord {
                id: r.get(0)?,
                vendor: r.get(1)?,
                access_key_id: r.get(2)?,
                access_key_secret: r.get(3)?,
                endpoint: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// 新增或更新一条账号(按 id 覆盖)。
    pub fn upsert(&self, rec: &AccountRecord) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, vendor, access_key_id, access_key_secret, endpoint)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                vendor = ?2, access_key_id = ?3, access_key_secret = ?4, endpoint = ?5",
            params![
                rec.id,
                rec.vendor,
                rec.access_key_id,
                rec.access_key_secret,
                rec.endpoint
            ],
        )?;
        Ok(())
    }

    /// 删除一条账号。
    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 读取一个设置项。
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
    }

    /// 写入(或覆盖)一个设置项。
    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// 读取一个断点续传上传会话(不存在返回 `None`)。
    pub fn get_upload_session(&self, key: &str) -> rusqlite::Result<Option<UploadSessionRow>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT upload_id, size, mtime, part_size, parts
             FROM upload_sessions WHERE session_key = ?1",
            params![key],
            |r| {
                Ok(UploadSessionRow {
                    upload_id: r.get(0)?,
                    size: r.get::<_, i64>(1)? as u64,
                    mtime: r.get::<_, i64>(2)? as u64,
                    part_size: r.get::<_, i64>(3)? as u64,
                    parts: r.get(4)?,
                })
            },
        )
        .optional()
    }

    /// 写入(或覆盖)一个上传会话。
    pub fn put_upload_session(&self, key: &str, row: &UploadSessionRow) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO upload_sessions (session_key, upload_id, size, mtime, part_size, parts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_key) DO UPDATE SET
                upload_id = ?2, size = ?3, mtime = ?4, part_size = ?5, parts = ?6",
            params![
                key,
                row.upload_id,
                row.size as i64,
                row.mtime as i64,
                row.part_size as i64,
                row.parts
            ],
        )?;
        Ok(())
    }

    /// 删除一个上传会话(上传完成或放弃时)。
    pub fn delete_upload_session(&self, key: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM upload_sessions WHERE session_key = ?1",
            params![key],
        )?;
        Ok(())
    }
}

/// 一条断点续传上传会话记录。`parts` 是 `[(分片号, ETag)]` 的 JSON。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadSessionRow {
    pub upload_id: String,
    pub size: u64,
    pub mtime: u64,
    pub part_size: u64,
    /// 已完成分片,序列化为 JSON 的 `[[番号, etag], ...]`。
    pub parts: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> AccountRecord {
        AccountRecord {
            id: id.to_string(),
            vendor: "aliyun".into(),
            access_key_id: "ak".into(),
            access_key_secret: "sk".into(),
            endpoint: "oss-cn-hangzhou.aliyuncs.com".into(),
        }
    }

    #[test]
    fn crud_roundtrip_in_memory() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.list().unwrap().is_empty());

        store.upsert(&record("a")).unwrap();
        store.upsert(&record("b")).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);

        // 覆盖 upsert 不新增。
        let mut updated = record("a");
        updated.endpoint = "oss-cn-beijing.aliyuncs.com".into();
        store.upsert(&updated).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].endpoint, "oss-cn-beijing.aliyuncs.com");

        store.delete("a").unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "b");
    }

    #[test]
    fn persists_across_reopen() {
        let path = std::env::temp_dir().join(format!(
            "nebula-store-test-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let store = AccountStore::open(&path).unwrap();
            store.upsert(&record("persist")).unwrap();
        }
        {
            let store = AccountStore::open(&path).unwrap();
            let listed = store.list().unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, "persist");
        }
        let _ = std::fs::remove_file(&path);
    }
}
