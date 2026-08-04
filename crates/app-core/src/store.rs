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
    /// 可选的自定义公共域名(CDN / CNAME);设了就用它拼永久公共直链。空表示未配置。
    pub custom_domain: String,
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
                endpoint          TEXT NOT NULL,
                custom_domain     TEXT NOT NULL DEFAULT ''
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
            );
            CREATE TABLE IF NOT EXISTS transfers (
                id      TEXT PRIMARY KEY,
                kind    TEXT NOT NULL,
                name    TEXT NOT NULL,
                account TEXT NOT NULL,
                remote  TEXT NOT NULL,
                local   TEXT NOT NULL,
                done    INTEGER NOT NULL,
                total   INTEGER NOT NULL,
                status  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS bookmarks (
                account    TEXT NOT NULL,
                path       TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (account, path)
            );
            CREATE TABLE IF NOT EXISTS ui_prefs (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS recent_locations (
                account TEXT NOT NULL,
                path    TEXT NOT NULL,
                seq     INTEGER NOT NULL,
                PRIMARY KEY (account, path)
            );
            CREATE TABLE IF NOT EXISTS sync_manifest (
                job  TEXT NOT NULL,
                rel  TEXT NOT NULL,
                size INTEGER NOT NULL,
                hash TEXT,
                PRIMARY KEY (job, rel)
            );
            CREATE TABLE IF NOT EXISTS sync_jobs (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                account       TEXT NOT NULL,
                local_dir     TEXT NOT NULL,
                remote_prefix TEXT NOT NULL,
                mode          TEXT NOT NULL,
                delete_extra  INTEGER NOT NULL,
                excludes      TEXT NOT NULL,
                interval_mins INTEGER NOT NULL,
                last_run      INTEGER NOT NULL,
                last_result   TEXT NOT NULL DEFAULT ''
            );",
        )?;
        // 对已有库补列(1.6.0 及更早没有 custom_domain);已存在则忽略错误。
        let _ = conn.execute(
            "ALTER TABLE accounts ADD COLUMN custom_domain TEXT NOT NULL DEFAULT ''",
            [],
        );
        // sync_jobs 的上次结果列(早期没有);已存在则忽略。
        let _ = conn.execute(
            "ALTER TABLE sync_jobs ADD COLUMN last_result TEXT NOT NULL DEFAULT ''",
            [],
        );
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 列出所有账号(按 id 字典序)。
    pub fn list(&self) -> rusqlite::Result<Vec<AccountRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, vendor, access_key_id, access_key_secret, endpoint, custom_domain
             FROM accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountRecord {
                id: r.get(0)?,
                vendor: r.get(1)?,
                access_key_id: r.get(2)?,
                access_key_secret: r.get(3)?,
                endpoint: r.get(4)?,
                custom_domain: r.get(5)?,
            })
        })?;
        rows.collect()
    }

    /// 新增或更新一条账号(按 id 覆盖)。
    pub fn upsert(&self, rec: &AccountRecord) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        // custom_domain 不在这里覆盖(编辑账号凭证时保留已配置的域名),改用 set_custom_domain。
        conn.execute(
            "INSERT INTO accounts (id, vendor, access_key_id, access_key_secret, endpoint, custom_domain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                vendor = ?2, access_key_id = ?3, access_key_secret = ?4, endpoint = ?5",
            params![
                rec.id,
                rec.vendor,
                rec.access_key_id,
                rec.access_key_secret,
                rec.endpoint,
                rec.custom_domain
            ],
        )?;
        Ok(())
    }

    /// 单独更新某账号的自定义公共域名(不动凭证)。
    pub fn set_custom_domain(&self, id: &str, domain: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET custom_domain = ?2 WHERE id = ?1",
            params![id, domain],
        )?;
        Ok(())
    }

    /// 按 id 读取一条账号(用于取自定义域名等)。
    pub fn get(&self, id: &str) -> rusqlite::Result<Option<AccountRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, vendor, access_key_id, access_key_secret, endpoint, custom_domain
             FROM accounts WHERE id = ?1",
            params![id],
            |r| {
                Ok(AccountRecord {
                    id: r.get(0)?,
                    vendor: r.get(1)?,
                    access_key_id: r.get(2)?,
                    access_key_secret: r.get(3)?,
                    endpoint: r.get(4)?,
                    custom_domain: r.get(5)?,
                })
            },
        )
        .optional()
    }

    /// 删除一条账号。
    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 把一个账号的 id 改名,并把引用它的其它表(书签 / 最近访问 / 同步任务 / 传输记录)
    /// 一起改过去——这几张表用裸字符串列引用账号、没有外键约束,不手动改就会变成孤儿数据。
    /// `accounts.id` 是主键,`new_id` 已存在时这里会报唯一约束错误。
    pub fn rename_account(&self, old_id: &str, new_id: &str) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE accounts SET id = ?2 WHERE id = ?1",
            params![old_id, new_id],
        )?;
        tx.execute(
            "UPDATE bookmarks SET account = ?2 WHERE account = ?1",
            params![old_id, new_id],
        )?;
        tx.execute(
            "UPDATE recent_locations SET account = ?2 WHERE account = ?1",
            params![old_id, new_id],
        )?;
        tx.execute(
            "UPDATE sync_jobs SET account = ?2 WHERE account = ?1",
            params![old_id, new_id],
        )?;
        tx.execute(
            "UPDATE transfers SET account = ?2 WHERE account = ?1",
            params![old_id, new_id],
        )?;
        tx.commit()
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

    /// 列出持久化的传输任务(用于重启后恢复面板)。
    pub fn list_transfers(&self) -> rusqlite::Result<Vec<crate::TransferRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, account, remote, local, done, total, status FROM transfers",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::TransferRecord {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                account: r.get(3)?,
                remote: r.get(4)?,
                local: r.get(5)?,
                done: r.get::<_, i64>(6)? as u64,
                total: r.get::<_, i64>(7)? as u64,
                status: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// 写入(或覆盖)一条传输任务。
    pub fn put_transfer(&self, t: &crate::TransferRecord) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transfers (id, kind, name, account, remote, local, done, total, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                kind = ?2, name = ?3, account = ?4, remote = ?5, local = ?6,
                done = ?7, total = ?8, status = ?9",
            params![
                t.id,
                t.kind,
                t.name,
                t.account,
                t.remote,
                t.local,
                t.done as i64,
                t.total as i64,
                t.status
            ],
        )?;
        Ok(())
    }

    /// 删除一条传输任务(完成或被清除时)。
    pub fn delete_transfer(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM transfers WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 列出所有收藏(最近收藏的在前)。
    pub fn list_bookmarks(&self) -> rusqlite::Result<Vec<BookmarkRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT account, path FROM bookmarks ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(BookmarkRow {
                account: r.get(0)?,
                path: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// 收藏一个位置(账号 + 路径);已存在则忽略。
    pub fn add_bookmark(&self, account: &str, path: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        conn.execute(
            "INSERT INTO bookmarks (account, path, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(account, path) DO NOTHING",
            params![account, path, now],
        )?;
        Ok(())
    }

    /// 取消收藏一个位置。
    pub fn remove_bookmark(&self, account: &str, path: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM bookmarks WHERE account = ?1 AND path = ?2",
            params![account, path],
        )?;
        Ok(())
    }

    /// 记录一次访问(账号 + 路径),已存在则顶到最前;只保留最近 `keep` 条。
    /// 用一个单调递增的逻辑序号 `seq` 排序(不依赖墙钟,避免同毫秒并列)。
    pub fn record_visit(&self, account: &str, path: &str, keep: usize) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO recent_locations (account, path, seq)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM recent_locations))
             ON CONFLICT(account, path) DO UPDATE
                SET seq = (SELECT COALESCE(MAX(seq), 0) + 1 FROM recent_locations)",
            params![account, path],
        )?;
        // 修剪:只留 seq 最大的 keep 条。
        conn.execute(
            "DELETE FROM recent_locations WHERE (account, path) NOT IN (
                 SELECT account, path FROM recent_locations
                 ORDER BY seq DESC LIMIT ?1
             )",
            params![keep as i64],
        )?;
        Ok(())
    }

    /// 列出最近访问(最新在前),最多 `limit` 条。
    pub fn list_recent(&self, limit: usize) -> rusqlite::Result<Vec<BookmarkRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT account, path FROM recent_locations ORDER BY seq DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(BookmarkRow {
                account: r.get(0)?,
                path: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// 读取一个界面偏好(主题 / 视图 / 语言 / 侧栏宽度等)。
    pub fn get_pref(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM ui_prefs WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
    }

    /// 写入(或覆盖)一个界面偏好。
    pub fn set_pref(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ui_prefs (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// 读取某同步任务的上次同步快照:`rel -> (size, 可选 hash)`。
    pub fn sync_manifest_load(
        &self,
        job: &str,
    ) -> rusqlite::Result<Vec<(String, u64, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT rel, size, hash FROM sync_manifest WHERE job = ?1")?;
        let rows = stmt.query_map(params![job], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        rows.collect()
    }

    /// 用新快照整体替换某同步任务的 manifest(先删后插,单事务)。
    pub fn sync_manifest_replace(
        &self,
        job: &str,
        entries: &[(String, u64, Option<String>)],
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sync_manifest WHERE job = ?1", params![job])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO sync_manifest (job, rel, size, hash) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (rel, size, hash) in entries {
                stmt.execute(params![job, rel, *size as i64, hash])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 列出所有已保存的同步任务(按名字排序)。`excludes` 为 JSON 字符串,由上层解析。
    pub fn list_sync_jobs(&self) -> rusqlite::Result<Vec<SyncJobRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, account, local_dir, remote_prefix, mode, delete_extra,
                    excludes, interval_mins, last_run, last_result
             FROM sync_jobs ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SyncJobRow {
                id: r.get(0)?,
                name: r.get(1)?,
                account: r.get(2)?,
                local_dir: r.get(3)?,
                remote_prefix: r.get(4)?,
                mode: r.get(5)?,
                delete_extra: r.get::<_, i64>(6)? != 0,
                excludes: r.get(7)?,
                interval_mins: r.get::<_, i64>(8)? as u32,
                last_run: r.get(9)?,
                last_result: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// 写入(或覆盖)一条同步任务。
    pub fn put_sync_job(&self, j: &SyncJobRow) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sync_jobs
                (id, name, account, local_dir, remote_prefix, mode, delete_extra,
                 excludes, interval_mins, last_run, last_result)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name = ?2, account = ?3, local_dir = ?4, remote_prefix = ?5, mode = ?6,
                delete_extra = ?7, excludes = ?8, interval_mins = ?9, last_run = ?10,
                last_result = ?11",
            params![
                j.id,
                j.name,
                j.account,
                j.local_dir,
                j.remote_prefix,
                j.mode,
                j.delete_extra as i64,
                j.excludes,
                j.interval_mins as i64,
                j.last_run,
                j.last_result
            ],
        )?;
        Ok(())
    }

    /// 删除一条同步任务(及其 manifest)。
    pub fn delete_sync_job(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sync_jobs WHERE id = ?1", params![id])?;
        Ok(())
    }
}

/// 一条持久化的同步任务(store 层原始行;`excludes` 为 JSON 字符串)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncJobRow {
    pub id: String,
    pub name: String,
    pub account: String,
    pub local_dir: String,
    pub remote_prefix: String,
    pub mode: String,
    pub delete_extra: bool,
    pub excludes: String,
    pub interval_mins: u32,
    pub last_run: i64,
    /// 上次运行结果的简短摘要(前端写入,如「↑5 ↓2」或错误)。
    pub last_result: String,
}

/// 一条收藏记录(账号 + 路径)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkRow {
    pub account: String,
    pub path: String,
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
            custom_domain: String::new(),
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
    fn rename_account_updates_the_row_and_moves_cascaded_references() {
        let store = AccountStore::open(":memory:").unwrap();
        store.upsert(&record("old")).unwrap();
        store.add_bookmark("old", "bucket/photos/").unwrap();
        store
            .put_transfer(&crate::TransferRecord {
                id: "dl:bucket/big.bin".into(),
                kind: "下载".into(),
                name: "big.bin".into(),
                account: "old".into(),
                remote: "bucket/big.bin".into(),
                local: "/tmp/big.bin".into(),
                done: 100,
                total: 500,
                status: "active".into(),
            })
            .unwrap();

        store.rename_account("old", "new").unwrap();

        assert!(store.get("old").unwrap().is_none());
        assert_eq!(store.get("new").unwrap().unwrap().id, "new");
        assert_eq!(store.list_bookmarks().unwrap()[0].account, "new");
        assert_eq!(store.list_transfers().unwrap()[0].account, "new");
    }

    #[test]
    fn rename_account_to_existing_id_fails() {
        let store = AccountStore::open(":memory:").unwrap();
        store.upsert(&record("a")).unwrap();
        store.upsert(&record("b")).unwrap();
        assert!(store.rename_account("a", "b").is_err());
        // 失败要保持原状,不能把 a 半改一半。
        assert!(store.get("a").unwrap().is_some());
        assert!(store.get("b").unwrap().is_some());
    }

    #[test]
    fn transfer_crud_roundtrip() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.list_transfers().unwrap().is_empty());

        let rec = crate::TransferRecord {
            id: "dl:bucket/big.bin".into(),
            kind: "下载".into(),
            name: "big.bin".into(),
            account: "oss".into(),
            remote: "bucket/big.bin".into(),
            local: "/tmp/big.bin".into(),
            done: 100,
            total: 500,
            status: "active".into(),
        };
        store.put_transfer(&rec).unwrap();
        assert_eq!(store.list_transfers().unwrap().len(), 1);

        // 覆盖:同 id 更新状态。
        let mut updated = rec.clone();
        updated.status = "interrupted".into();
        store.put_transfer(&updated).unwrap();
        let listed = store.list_transfers().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, "interrupted");

        store.delete_transfer(&rec.id).unwrap();
        assert!(store.list_transfers().unwrap().is_empty());
    }

    #[test]
    fn bookmark_crud_roundtrip() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.list_bookmarks().unwrap().is_empty());

        store.add_bookmark("oss", "bucket/photos/").unwrap();
        store.add_bookmark("s3", "logs/").unwrap();
        // 重复收藏不新增。
        store.add_bookmark("oss", "bucket/photos/").unwrap();
        assert_eq!(store.list_bookmarks().unwrap().len(), 2);

        store.remove_bookmark("oss", "bucket/photos/").unwrap();
        let listed = store.list_bookmarks().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].account, "s3");
    }

    #[test]
    fn recent_locations_tracks_and_prunes() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.list_recent(20).unwrap().is_empty());

        store.record_visit("oss", "a/", 3).unwrap();
        store.record_visit("oss", "b/", 3).unwrap();
        store.record_visit("oss", "c/", 3).unwrap();
        // 再访问 a/ 应把它顶到最前(不新增行)。
        store.record_visit("oss", "a/", 3).unwrap();
        let recent = store.list_recent(20).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].path, "a/");

        // 第 4 个不同位置触发修剪,最旧的 b/ 被淘汰。
        store.record_visit("oss", "d/", 3).unwrap();
        let recent = store.list_recent(20).unwrap();
        assert_eq!(recent.len(), 3);
        assert!(recent.iter().all(|r| r.path != "b/"));
    }

    #[test]
    fn ui_pref_roundtrip() {
        let store = AccountStore::open(":memory:").unwrap();
        assert!(store.get_pref("theme").unwrap().is_none());
        store.set_pref("theme", "light").unwrap();
        store.set_pref("theme", "dark").unwrap();
        assert_eq!(store.get_pref("theme").unwrap().as_deref(), Some("dark"));
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
