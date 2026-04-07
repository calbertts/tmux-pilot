use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::config::data_dir;

/// Database for persisting session↔work-item mappings and copilot session tracking
pub struct Store {
    conn: Connection,
}

/// A persisted session-to-feature mapping
#[derive(Debug, Clone)]
pub struct SessionMapping {
    pub session_name: String,
    pub work_item_id: Option<u64>,
    pub work_item_title: Option<String>,
    pub work_item_type: Option<String>,
    pub template: Option<String>,
    pub created_at: String,
}

/// A persisted window-to-task mapping
#[derive(Debug, Clone)]
pub struct WindowMapping {
    pub session_name: String,
    pub window_name: String,
    pub work_item_id: Option<u64>,
    pub work_item_title: Option<String>,
    pub work_item_type: Option<String>,
    pub copilot_session_id: Option<String>,
    pub window_type: String, // "copilot" | "shell"
}

/// A notification entry
#[derive(Debug, Clone)]
pub struct Notification {
    pub id: i64,
    pub level: String,    // info | warn | error | success
    pub title: String,
    pub body: Option<String>,
    pub source: Option<String>,
    pub link: Option<String>,
    pub read: bool,
    pub created_at: String,
}

/// A background watcher entry
#[derive(Debug, Clone)]
pub struct Watcher {
    pub id: String,
    pub watcher_type: String,
    pub config: String,
    pub pid: Option<u32>,
    pub status: String,
    pub started_at: String,
    pub last_check_at: Option<String>,
}

impl Store {
    pub fn open() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("tcs.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS session_mappings (
                session_name    TEXT PRIMARY KEY,
                work_item_id    INTEGER,
                work_item_title TEXT,
                work_item_type  TEXT,
                template        TEXT,
                created_at      TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS window_mappings (
                session_name       TEXT NOT NULL,
                window_name        TEXT NOT NULL,
                work_item_id       INTEGER,
                work_item_title    TEXT,
                work_item_type     TEXT,
                copilot_session_id TEXT,
                window_type        TEXT DEFAULT 'shell',
                created_at         TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (session_name, window_name)
            );

            CREATE TABLE IF NOT EXISTS azdo_cache (
                cache_key   TEXT PRIMARY KEY,
                data        TEXT NOT NULL,
                fetched_at  TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS notifications (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                level       TEXT NOT NULL DEFAULT 'info',
                title       TEXT NOT NULL,
                body        TEXT,
                source      TEXT,
                link        TEXT,
                read        INTEGER DEFAULT 0,
                created_at  TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS watchers (
                id              TEXT PRIMARY KEY,
                watcher_type    TEXT NOT NULL,
                config          TEXT NOT NULL,
                pid             INTEGER,
                status          TEXT DEFAULT 'running',
                started_at      TEXT DEFAULT (datetime('now')),
                last_check_at   TEXT
            );
            ",
        )?;
        Ok(())
    }

    // ─── Session mappings ────────────────────────────────────

    pub fn save_session_mapping(&self, mapping: &SessionMapping) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO session_mappings 
             (session_name, work_item_id, work_item_title, work_item_type, template)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                mapping.session_name,
                mapping.work_item_id,
                mapping.work_item_title,
                mapping.work_item_type,
                mapping.template,
            ],
        )?;
        Ok(())
    }

    pub fn get_session_mapping(&self, session_name: &str) -> Result<Option<SessionMapping>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_name, work_item_id, work_item_title, work_item_type, template, created_at
             FROM session_mappings WHERE session_name = ?1",
        )?;
        let result = stmt
            .query_row(params![session_name], |row| {
                Ok(SessionMapping {
                    session_name: row.get(0)?,
                    work_item_id: row.get(1)?,
                    work_item_title: row.get(2)?,
                    work_item_type: row.get(3)?,
                    template: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .optional()?;
        Ok(result)
    }

    pub fn list_session_mappings(&self) -> Result<Vec<SessionMapping>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_name, work_item_id, work_item_title, work_item_type, template, created_at
             FROM session_mappings ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SessionMapping {
                    session_name: row.get(0)?,
                    work_item_id: row.get(1)?,
                    work_item_title: row.get(2)?,
                    work_item_type: row.get(3)?,
                    template: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_session_mapping(&self, session_name: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM session_mappings WHERE session_name = ?1",
            params![session_name],
        )?;
        self.conn.execute(
            "DELETE FROM window_mappings WHERE session_name = ?1",
            params![session_name],
        )?;
        Ok(())
    }

    // ─── Window mappings ─────────────────────────────────────

    pub fn save_window_mapping(&self, mapping: &WindowMapping) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO window_mappings
             (session_name, window_name, work_item_id, work_item_title, work_item_type, copilot_session_id, window_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                mapping.session_name,
                mapping.window_name,
                mapping.work_item_id,
                mapping.work_item_title,
                mapping.work_item_type,
                mapping.copilot_session_id,
                mapping.window_type,
            ],
        )?;
        Ok(())
    }

    pub fn get_window_mappings(&self, session_name: &str) -> Result<Vec<WindowMapping>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_name, window_name, work_item_id, work_item_title, work_item_type, copilot_session_id, window_type
             FROM window_mappings WHERE session_name = ?1 ORDER BY window_name",
        )?;
        let rows = stmt
            .query_map(params![session_name], |row| {
                Ok(WindowMapping {
                    session_name: row.get(0)?,
                    window_name: row.get(1)?,
                    work_item_id: row.get(2)?,
                    work_item_title: row.get(3)?,
                    work_item_type: row.get(4)?,
                    copilot_session_id: row.get(5)?,
                    window_type: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_copilot_session_id(
        &self,
        session_name: &str,
        window_name: &str,
        copilot_session_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE window_mappings SET copilot_session_id = ?1
             WHERE session_name = ?2 AND window_name = ?3",
            params![copilot_session_id, session_name, window_name],
        )?;
        Ok(())
    }

    // ─── AzDo cache ──────────────────────────────────────────

    pub fn get_cached(&self, key: &str, max_age_minutes: i64) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM azdo_cache 
             WHERE cache_key = ?1 
             AND datetime(fetched_at, '+' || ?2 || ' minutes') > datetime('now')",
        )?;
        let result = stmt
            .query_row(params![key, max_age_minutes], |row| row.get(0))
            .optional()?;
        Ok(result)
    }

    pub fn set_cached(&self, key: &str, data: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO azdo_cache (cache_key, data) VALUES (?1, ?2)",
            params![key, data],
        )?;
        Ok(())
    }

    // ─── Notifications ───────────────────────────────────────

    pub fn add_notification(
        &self,
        level: &str,
        title: &str,
        body: Option<&str>,
        source: Option<&str>,
        link: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notifications (level, title, body, source, link)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![level, title, body, source, link],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn unread_count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE read = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn list_notifications(&self, limit: usize) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, level, title, body, source, link, read, created_at
             FROM notifications ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Notification {
                    id: row.get(0)?,
                    level: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    source: row.get(4)?,
                    link: row.get(5)?,
                    read: row.get::<_, i64>(6)? != 0,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_notification_read(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET read = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_all_read(&self) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE notifications SET read = 1 WHERE read = 0",
            [],
        )?;
        Ok(count)
    }

    pub fn delete_notification(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM notifications WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn cleanup_old_notifications(&self, max_age_days: i64) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM notifications WHERE datetime(created_at, '+' || ?1 || ' days') < datetime('now')",
            params![max_age_days],
        )?;
        Ok(count)
    }

    // ─── Watchers ────────────────────────────────────────────

    pub fn save_watcher(&self, watcher: &Watcher) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO watchers (id, watcher_type, config, pid, status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                watcher.id,
                watcher.watcher_type,
                watcher.config,
                watcher.pid,
                watcher.status,
            ],
        )?;
        Ok(())
    }

    pub fn list_watchers(&self) -> Result<Vec<Watcher>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, watcher_type, config, pid, status, started_at, last_check_at
             FROM watchers ORDER BY started_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Watcher {
                    id: row.get(0)?,
                    watcher_type: row.get(1)?,
                    config: row.get(2)?,
                    pid: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    last_check_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_watcher_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE watchers SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn update_watcher_check(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE watchers SET last_check_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_watcher(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM watchers WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

/// Extension trait for optional query results
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
