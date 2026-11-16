//! Local SQLite state.
//!
//! This database stores the managed directories, settings, a cache of snapshot
//! metadata for fast listing, and the activity log. None of it is required to
//! recover data: the cloud repository is authoritative. Deleting this file must
//! never lose a backup (only local convenience state).

use crate::error::Result;
use crate::model::{RootConfig, Settings, Snapshot, SnapshotStats};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// The kind of operation recorded in the activity log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Init,
    Clone,
    AddRoot,
    RemoveRoot,
    Backup,
    Restore,
    Verify,
    Config,
    Auth,
    Prune,
}

impl ActivityKind {
    fn as_str(&self) -> &'static str {
        match self {
            ActivityKind::Init => "init",
            ActivityKind::Clone => "clone",
            ActivityKind::AddRoot => "add_root",
            ActivityKind::RemoveRoot => "remove_root",
            ActivityKind::Backup => "backup",
            ActivityKind::Restore => "restore",
            ActivityKind::Verify => "verify",
            ActivityKind::Config => "config",
            ActivityKind::Auth => "auth",
            ActivityKind::Prune => "prune",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "init" => ActivityKind::Init,
            "clone" => ActivityKind::Clone,
            "add_root" => ActivityKind::AddRoot,
            "remove_root" => ActivityKind::RemoveRoot,
            "backup" => ActivityKind::Backup,
            "restore" => ActivityKind::Restore,
            "verify" => ActivityKind::Verify,
            "config" => ActivityKind::Config,
            "auth" => ActivityKind::Auth,
            "prune" => ActivityKind::Prune,
            _ => ActivityKind::Config,
        }
    }
}

/// Outcome of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    Running,
    Success,
    Failed,
    Interrupted,
}

impl ActivityStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ActivityStatus::Running => "running",
            ActivityStatus::Success => "success",
            ActivityStatus::Failed => "failed",
            ActivityStatus::Interrupted => "interrupted",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "running" => ActivityStatus::Running,
            "success" => ActivityStatus::Success,
            "failed" => ActivityStatus::Failed,
            "interrupted" => ActivityStatus::Interrupted,
            _ => ActivityStatus::Failed,
        }
    }
}

/// One row of the activity log.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub kind: ActivityKind,
    pub status: ActivityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

/// Cached snapshot metadata for fast history listing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedSnapshot {
    pub snapshot_id: String,
    pub parent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub stats: SnapshotStats,
}

/// Handle to the local database.
pub struct Db {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

impl Db {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        let db = Self {
            conn: Mutex::new(Connection::open_in_memory()?),
        };
        db.migrate()?;
        Ok(db)
    }

    fn configure(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS roots (
                root_id     TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                local_path  TEXT NOT NULL,
                remote_path TEXT NOT NULL,
                exclusions  TEXT NOT NULL DEFAULT '[]',
                enabled     INTEGER NOT NULL DEFAULT 1,
                created_at  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS activity (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                kind      TEXT NOT NULL,
                status    TEXT NOT NULL,
                root_id   TEXT,
                message   TEXT NOT NULL,
                detail    TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_activity_ts ON activity(timestamp DESC);
            CREATE TABLE IF NOT EXISTS snapshots (
                snapshot_id TEXT PRIMARY KEY,
                parent      TEXT,
                created_at  TEXT NOT NULL,
                file_count  INTEGER NOT NULL DEFAULT 0,
                logical_size INTEGER NOT NULL DEFAULT 0,
                stats       TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_created ON snapshots(created_at DESC);
            "#,
        )?;
        Ok(())
    }

    // -- meta ---------------------------------------------------------------

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let v = conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(v)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // -- settings -----------------------------------------------------------

    pub fn get_settings(&self) -> Result<Settings> {
        match self.get_meta("settings")? {
            Some(raw) => Ok(serde_json::from_str(&raw).unwrap_or_default()),
            None => Ok(Settings::default()),
        }
    }

    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        self.set_meta("settings", &serde_json::to_string(settings)?)
    }

    // -- roots --------------------------------------------------------------

    pub fn list_roots(&self) -> Result<Vec<RootConfig>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT root_id, name, local_path, remote_path, exclusions, enabled, created_at
             FROM roots ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            let exclusions: String = r.get(4)?;
            let created: String = r.get(6)?;
            Ok(RootConfig {
                root_id: r.get(0)?,
                name: r.get(1)?,
                local_path: r.get::<_, String>(2)?.into(),
                remote_path: r.get(3)?,
                exclusions: serde_json::from_str(&exclusions).unwrap_or_default(),
                enabled: r.get::<_, i64>(5)? != 0,
                created_at: created.parse().unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_root(&self, root_id: &str) -> Result<Option<RootConfig>> {
        Ok(self.list_roots()?.into_iter().find(|r| r.root_id == root_id))
    }

    pub fn upsert_root(&self, root: &RootConfig) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO roots(root_id, name, local_path, remote_path, exclusions, enabled, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(root_id) DO UPDATE SET
                name = excluded.name,
                local_path = excluded.local_path,
                remote_path = excluded.remote_path,
                exclusions = excluded.exclusions,
                enabled = excluded.enabled",
            params![
                root.root_id,
                root.name,
                root.local_path.to_string_lossy(),
                root.remote_path,
                serde_json::to_string(&root.exclusions)?,
                if root.enabled { 1 } else { 0 },
                root.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_root(&self, root_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM roots WHERE root_id = ?1", params![root_id])?;
        Ok(())
    }

    // -- activity -----------------------------------------------------------

    pub fn add_activity(
        &self,
        kind: ActivityKind,
        status: ActivityStatus,
        root_id: Option<&str>,
        message: &str,
        detail: serde_json::Value,
    ) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO activity(timestamp, kind, status, root_id, message, detail)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Utc::now().to_rfc3339(),
                kind.as_str(),
                status.as_str(),
                root_id,
                message,
                detail.to_string(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_activity(&self, limit: u32) -> Result<Vec<ActivityEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, kind, status, root_id, message, detail
             FROM activity ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            let ts: String = r.get(1)?;
            let kind: String = r.get(2)?;
            let status: String = r.get(3)?;
            let detail: String = r.get(6)?;
            Ok(ActivityEntry {
                id: r.get(0)?,
                timestamp: ts.parse().unwrap_or_else(|_| Utc::now()),
                kind: ActivityKind::from_str(&kind),
                status: ActivityStatus::from_str(&status),
                root_id: r.get(4)?,
                message: r.get(5)?,
                detail: serde_json::from_str(&detail).unwrap_or(serde_json::json!({})),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // -- snapshot cache -----------------------------------------------------

    pub fn cache_snapshot(&self, snap: &Snapshot) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO snapshots(snapshot_id, parent, created_at, file_count, logical_size, stats)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(snapshot_id) DO UPDATE SET
                parent = excluded.parent,
                file_count = excluded.file_count,
                logical_size = excluded.logical_size,
                stats = excluded.stats",
            params![
                snap.snapshot_id,
                snap.parent,
                snap.created_at.to_rfc3339(),
                snap.stats.file_count as i64,
                snap.logical_size() as i64,
                serde_json::to_string(&snap.stats)?,
            ],
        )?;
        Ok(())
    }

    pub fn list_cached_snapshots(&self, limit: u32) -> Result<Vec<CachedSnapshot>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT snapshot_id, parent, created_at, stats FROM snapshots
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            let created: String = r.get(2)?;
            let stats: String = r.get(3)?;
            Ok(CachedSnapshot {
                snapshot_id: r.get(0)?,
                parent: r.get(1)?,
                created_at: created.parse().unwrap_or_else(|_| Utc::now()),
                stats: serde_json::from_str(&stats).unwrap_or_default(),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn clear_snapshot_cache(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM snapshots", [])?;
        Ok(())
    }

    pub fn snapshot_cache_len(&self) -> Result<u32> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?;
        Ok(n as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let root = RootConfig {
            root_id: "r1".into(),
            name: "Docs".into(),
            local_path: "/home/x/docs".into(),
            remote_path: "docs".into(),
            exclusions: vec!["*.tmp".into()],
            enabled: true,
            created_at: Utc::now(),
        };
        db.upsert_root(&root).unwrap();
        let roots = db.list_roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].exclusions, vec!["*.tmp".to_string()]);
        db.delete_root("r1").unwrap();
        assert!(db.list_roots().unwrap().is_empty());
    }

    #[test]
    fn activity_log_orders_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.add_activity(
            ActivityKind::Backup,
            ActivityStatus::Success,
            None,
            "one",
            serde_json::json!({}),
        )
        .unwrap();
        db.add_activity(
            ActivityKind::Restore,
            ActivityStatus::Failed,
            None,
            "two",
            serde_json::json!({}),
        )
        .unwrap();
        let a = db.list_activity(10).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].message, "two");
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let mut s = Settings::default();
        s.upload_concurrency = 9;
        db.set_settings(&s).unwrap();
        assert_eq!(db.get_settings().unwrap().upload_concurrency, 9);
    }
}
