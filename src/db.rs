use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const SELECT_COLS: &str = "id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename, \
    file_size, mode, uid, gid, quick_fingerprint, vault_path, deleted_at, status, \
    is_directory, symlink_target, link_type, restored_at, purged_at";

#[derive(Debug, Clone)]
pub struct NewEntry {
    pub dev_major: u32,
    pub dev_minor: u32,
    pub mnt_id: u64,
    pub inode_no: u64,
    pub original_path: String,
    pub filename: String,
    pub file_size: u64,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub quick_fingerprint: Option<String>,
    pub vault_path: String,
    pub deleted_at: DateTime<Utc>,
    pub status: String,
    pub is_directory: bool,
    pub symlink_target: Option<String>,
    pub link_type: String,
}

#[derive(Debug, Clone)]
pub struct EntryRecord {
    pub id: i64,
    pub dev_major: u32,
    pub dev_minor: u32,
    pub mnt_id: u64,
    pub inode_no: u64,
    pub original_path: String,
    pub filename: String,
    pub file_size: u64,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub quick_fingerprint: Option<String>,
    pub vault_path: String,
    pub deleted_at: DateTime<Utc>,
    pub status: String,
    pub is_directory: bool,
    pub symlink_target: Option<String>,
    pub link_type: String,
    pub restored_at: Option<DateTime<Utc>>,
    pub purged_at: Option<DateTime<Utc>>,
}

/// Resolves the storage path on disk, automatically handling legacy path translations if needed
pub fn resolve_storage_path(recorded_path: &str) -> Option<PathBuf> {
    let p = Path::new(recorded_path);
    if p.exists() {
        return Some(p.to_path_buf());
    }

    let candidates = [
        recorded_path.replace("/recent-inode/vault/", "/rinode/storage/"),
        recorded_path.replace("/recent-inode/", "/rinode/"),
        recorded_path.replace("/.rinode-vault/", "/.rinode-storage/"),
        recorded_path.replace("/rinode/storage/", "/recent-inode/vault/"),
        recorded_path.replace("/.rinode-storage/", "/.rinode-vault/"),
    ];

    for candidate in candidates {
        let cand_path = PathBuf::from(candidate);
        if cand_path.exists() {
            return Some(cand_path);
        }
    }

    None
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let base_data_dir = directories::BaseDirs::new()
            .map(|b| b.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        Self::migrate_legacy_storage(&base_data_dir);

        let db_dir = base_data_dir.join("rinode");
        fs::create_dir_all(&db_dir).ok();
        let db_path = db_dir.join("rinode.db");
        Self::open(&db_path)
    }

    pub fn migrate_legacy_storage(base_data_dir: &Path) {
        let legacy_dir = base_data_dir.join("recent-inode");
        let new_dir = base_data_dir.join("rinode");

        if legacy_dir.exists() {
            if !new_dir.exists() {
                let _ = fs::rename(&legacy_dir, &new_dir);
            } else {
                let legacy_db = legacy_dir.join("rinode.db");
                let new_db = new_dir.join("rinode.db");
                if legacy_db.exists() && !new_db.exists() {
                    let _ = fs::rename(&legacy_db, &new_db);
                }

                let legacy_vault = legacy_dir.join("vault");
                let new_storage = new_dir.join("storage");
                if legacy_vault.exists() {
                    let _ = fs::create_dir_all(&new_storage);
                    if let Ok(entries) = fs::read_dir(&legacy_vault) {
                        for entry in entries.flatten() {
                            let dest = new_storage.join(entry.file_name());
                            let _ = fs::rename(entry.path(), dest);
                        }
                    }
                    let _ = fs::remove_dir_all(&legacy_vault);
                }
                let _ = fs::remove_dir_all(&legacy_dir);
            }
        }

        if new_dir.exists() {
            let legacy_vault = new_dir.join("vault");
            let new_storage = new_dir.join("storage");
            if legacy_vault.exists() {
                if !new_storage.exists() {
                    let _ = fs::rename(&legacy_vault, &new_storage);
                } else {
                    let _ = fs::create_dir_all(&new_storage);
                    if let Ok(entries) = fs::read_dir(&legacy_vault) {
                        for entry in entries.flatten() {
                            let dest = new_storage.join(entry.file_name());
                            let _ = fs::rename(entry.path(), dest);
                        }
                    }
                    let _ = fs::remove_dir_all(&legacy_vault);
                }
            }
        }
    }

    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(db_path)?;

        // Enable Write-Ahead Logging (WAL) mode for concurrency and speed
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        let db = Db { conn };
        db.create_tables()?;
        db.migrate_legacy_db_paths().ok();
        Ok(db)
    }

    fn migrate_legacy_db_paths(&self) -> Result<()> {
        self.conn.execute_batch(
            "BEGIN TRANSACTION;
            UPDATE entries SET vault_path = REPLACE(vault_path, '/recent-inode/vault/', '/rinode/storage/') WHERE vault_path LIKE '%/recent-inode/vault/%';
            UPDATE entries SET vault_path = REPLACE(vault_path, '/recent-inode/', '/rinode/') WHERE vault_path LIKE '%/recent-inode/%';
            UPDATE entries SET vault_path = REPLACE(vault_path, '/.rinode-vault/', '/.rinode-storage/') WHERE vault_path LIKE '%/.rinode-vault/%';
            COMMIT;",
        )?;
        Ok(())
    }

    fn create_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                dev_major INTEGER NOT NULL,
                dev_minor INTEGER NOT NULL,
                mnt_id INTEGER NOT NULL,
                inode_no INTEGER NOT NULL,
                original_path TEXT NOT NULL,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                mode INTEGER NOT NULL,
                uid INTEGER NOT NULL,
                gid INTEGER NOT NULL,
                quick_fingerprint TEXT,
                vault_path TEXT NOT NULL,
                deleted_at TEXT NOT NULL,
                status TEXT NOT NULL,
                is_directory BOOLEAN NOT NULL DEFAULT 0,
                symlink_target TEXT,
                link_type TEXT NOT NULL,
                restored_at TEXT,
                purged_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_status ON entries(status);
            CREATE INDEX IF NOT EXISTS idx_dev_inode ON entries(dev_major, dev_minor, inode_no);
            CREATE INDEX IF NOT EXISTS idx_filename ON entries(filename);
            CREATE INDEX IF NOT EXISTS idx_deleted_at ON entries(deleted_at);
            ",
        )?;

        // Ensure restored_at and purged_at columns exist in databases created by earlier versions
        let _ = self.conn.execute("ALTER TABLE entries ADD COLUMN restored_at TEXT", []);
        let _ = self.conn.execute("ALTER TABLE entries ADD COLUMN purged_at TEXT", []);

        self.conn.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_restored_at ON entries(restored_at);
            CREATE INDEX IF NOT EXISTS idx_purged_at ON entries(purged_at);
            ",
        )?;

        // Migrate older databases having restrictive CHECK constraints on status
        let has_old_check: bool = self.conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='entries'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("CHECK(status IN ('PRESERVED', 'RESTORED', 'PURGED'))"))
            },
        ).unwrap_or(false);

        if has_old_check {
            self.conn.execute_batch(
                "
                PRAGMA foreign_keys=off;
                CREATE TABLE entries_v2 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    dev_major INTEGER NOT NULL,
                    dev_minor INTEGER NOT NULL,
                    mnt_id INTEGER NOT NULL,
                    inode_no INTEGER NOT NULL,
                    original_path TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    mode INTEGER NOT NULL,
                    uid INTEGER NOT NULL,
                    gid INTEGER NOT NULL,
                    quick_fingerprint TEXT,
                    vault_path TEXT NOT NULL,
                    deleted_at TEXT NOT NULL,
                    status TEXT NOT NULL,
                    is_directory BOOLEAN NOT NULL DEFAULT 0,
                    symlink_target TEXT,
                    link_type TEXT NOT NULL,
                    restored_at TEXT,
                    purged_at TEXT
                );
                INSERT INTO entries_v2 (
                    id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                    file_size, mode, uid, gid, quick_fingerprint, vault_path, deleted_at,
                    status, is_directory, symlink_target, link_type, restored_at, purged_at
                )
                SELECT
                    id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                    file_size, mode, uid, gid, quick_fingerprint, vault_path, deleted_at,
                    status, is_directory, symlink_target, link_type, restored_at, purged_at
                FROM entries;
                DROP TABLE entries;
                ALTER TABLE entries_v2 RENAME TO entries;
                CREATE INDEX IF NOT EXISTS idx_status ON entries(status);
                CREATE INDEX IF NOT EXISTS idx_dev_inode ON entries(dev_major, dev_minor, inode_no);
                CREATE INDEX IF NOT EXISTS idx_filename ON entries(filename);
                CREATE INDEX IF NOT EXISTS idx_deleted_at ON entries(deleted_at);
                CREATE INDEX IF NOT EXISTS idx_restored_at ON entries(restored_at);
                CREATE INDEX IF NOT EXISTS idx_purged_at ON entries(purged_at);
                PRAGMA foreign_keys=on;
                ",
            )?;
        }

        Ok(())
    }

    pub fn insert_entry(&self, entry: &NewEntry) -> Result<i64> {
        self.conn.execute(
            "
            INSERT INTO entries (
                dev_major, dev_minor, mnt_id, inode_no, original_path,
                filename, file_size, mode, uid, gid, quick_fingerprint,
                vault_path, deleted_at, status, is_directory,
                symlink_target, link_type
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
            )
            ",
            params![
                entry.dev_major,
                entry.dev_minor,
                entry.mnt_id,
                entry.inode_no,
                entry.original_path,
                entry.filename,
                entry.file_size,
                entry.mode,
                entry.uid,
                entry.gid,
                entry.quick_fingerprint,
                entry.vault_path,
                entry.deleted_at.to_rfc3339(),
                entry.status,
                entry.is_directory,
                entry.symlink_target,
                entry.link_type,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_active(&self, limit: Option<usize>) -> Result<Vec<EntryRecord>> {
        let limit_clause = match limit {
            Some(n) => format!("LIMIT {}", n),
            None => "".to_string(),
        };

        let sql = format!(
            "SELECT {} FROM entries WHERE status = 'PRESERVED' ORDER BY deleted_at DESC {}",
            SELECT_COLS, limit_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn list_history(&self, limit: Option<usize>) -> Result<Vec<EntryRecord>> {
        let limit_clause = match limit {
            Some(n) => format!("LIMIT {}", n),
            None => "".to_string(),
        };

        let sql = format!(
            "SELECT {} FROM entries WHERE status IN ('RESTORED', 'PURGED', 'EXCLUDED') \
             ORDER BY COALESCE(purged_at, restored_at, deleted_at) DESC, id DESC {}",
            SELECT_COLS, limit_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn list_all(&self, limit: Option<usize>) -> Result<Vec<EntryRecord>> {
        let limit_clause = match limit {
            Some(n) => format!("LIMIT {}", n),
            None => "".to_string(),
        };

        let sql = format!(
            "SELECT {} FROM entries ORDER BY id DESC {}",
            SELECT_COLS, limit_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<EntryRecord>> {
        let sql = format!("SELECT {} FROM entries WHERE id = ?1", SELECT_COLS);

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id], |row| Self::row_to_record(row))?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn find_by_filename(&self, name: &str) -> Result<Vec<EntryRecord>> {
        let sql = format!(
            "SELECT {} FROM entries WHERE status = 'PRESERVED' AND (filename = ?1 OR original_path LIKE ?2) \
             ORDER BY deleted_at DESC",
            SELECT_COLS
        );

        let search_pattern = format!("%{}", name);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![name, search_pattern], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn mark_restored(&self, id: i64) -> Result<()> {
        let now_str = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE entries SET status = 'RESTORED', restored_at = ?1 WHERE id = ?2",
            params![now_str, id],
        )?;
        Ok(())
    }

    pub fn mark_purged(&self, id: i64) -> Result<()> {
        let now_str = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE entries SET status = 'PURGED', purged_at = ?1 WHERE id = ?2",
            params![now_str, id],
        )?;
        Ok(())
    }

    pub fn purge_entry(&self, entry: &EntryRecord) -> Result<()> {
        if let Some(path) = resolve_storage_path(&entry.vault_path) {
            if entry.is_directory {
                std::fs::remove_dir_all(&path).ok();
            } else {
                std::fs::remove_file(&path).ok();
            }
        }
        self.mark_purged(entry.id)
    }

    pub fn get_expired(&self, days: u32) -> Result<Vec<EntryRecord>> {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let sql = format!(
            "SELECT {} FROM entries WHERE status = 'PRESERVED' AND deleted_at < ?1 ORDER BY deleted_at ASC",
            SELECT_COLS
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![cutoff.to_rfc3339()], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn get_data_version(&self) -> Result<i64> {
        self.conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))
    }

    fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<EntryRecord> {
        let deleted_at_str: String = row.get(13)?;
        let deleted_at = DateTime::parse_from_rfc3339(&deleted_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let restored_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(18)
            .ok()
            .flatten()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

        let purged_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(19)
            .ok()
            .flatten()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

        Ok(EntryRecord {
            id: row.get(0)?,
            dev_major: row.get(1)?,
            dev_minor: row.get(2)?,
            mnt_id: row.get(3)?,
            inode_no: row.get(4)?,
            original_path: row.get(5)?,
            filename: row.get(6)?,
            file_size: row.get(7)?,
            mode: row.get(8)?,
            uid: row.get(9)?,
            gid: row.get(10)?,
            quick_fingerprint: row.get(11)?,
            vault_path: row.get(12)?,
            deleted_at,
            status: row.get(14)?,
            is_directory: row.get(15)?,
            symlink_target: row.get(16)?,
            link_type: row.get(17)?,
            restored_at,
            purged_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_version_updates() {
        let test_dir = std::env::temp_dir().join(format!("rinode_test_ver_{}", std::process::id()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let db_path = test_dir.join("test.db");

        let db1 = Db::open(&db_path).unwrap();
        let db2 = Db::open(&db_path).unwrap();

        let v1 = db1.get_data_version().unwrap();

        let record = NewEntry {
            dev_major: 1,
            dev_minor: 2,
            mnt_id: 3,
            inode_no: 12345,
            original_path: "/test/file.txt".into(),
            filename: "file.txt".into(),
            file_size: 100,
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            quick_fingerprint: None,
            vault_path: "/vault/file.txt".into(),
            deleted_at: Utc::now(),
            status: "PRESERVED".into(),
            is_directory: false,
            symlink_target: None,
            link_type: "RENAME".into(),
        };

        db2.insert_entry(&record).unwrap();

        let v2 = db1.get_data_version().unwrap();
        assert_ne!(v1, v2, "data_version in db1 should change after db2 inserts");

        std::fs::remove_dir_all(&test_dir).ok();
    }
}
