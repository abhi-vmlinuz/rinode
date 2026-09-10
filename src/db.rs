use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result};
use std::fs;
use std::path::{Path, PathBuf};

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
    pub full_hash: Option<String>,
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
    pub full_hash: Option<String>,
    pub vault_path: String,
    pub deleted_at: DateTime<Utc>,
    pub status: String,
    pub is_directory: bool,
    pub symlink_target: Option<String>,
    pub link_type: String,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let db_dir = directories::BaseDirs::new()
            .map(|b| b.data_local_dir().join("recent-inode"))
            .unwrap_or_else(|| PathBuf::from(".rinode"));

        fs::create_dir_all(&db_dir).ok();
        let db_path = db_dir.join("rinode.db");
        Self::open(&db_path)
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
        Ok(db)
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
                full_hash TEXT,
                vault_path TEXT NOT NULL,
                deleted_at TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('PRESERVED', 'RESTORED', 'PURGED')),
                is_directory BOOLEAN NOT NULL DEFAULT 0,
                symlink_target TEXT,
                link_type TEXT NOT NULL CHECK(link_type IN ('RENAME_MOVE', 'SYMLINK', 'REFLINK', 'COPY'))
            );

            CREATE INDEX IF NOT EXISTS idx_status ON entries(status);
            CREATE INDEX IF NOT EXISTS idx_dev_inode ON entries(dev_major, dev_minor, inode_no);
            CREATE INDEX IF NOT EXISTS idx_filename ON entries(filename);
            CREATE INDEX IF NOT EXISTS idx_deleted_at ON entries(deleted_at);
            ",
        )?;
        Ok(())
    }

    pub fn insert_entry(&self, entry: &NewEntry) -> Result<i64> {
        self.conn.execute(
            "
            INSERT INTO entries (
                dev_major, dev_minor, mnt_id, inode_no, original_path,
                filename, file_size, mode, uid, gid, quick_fingerprint,
                full_hash, vault_path, deleted_at, status, is_directory,
                symlink_target, link_type
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
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
                entry.full_hash,
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
            "SELECT id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                    file_size, mode, uid, gid, quick_fingerprint, full_hash, vault_path,
                    deleted_at, status, is_directory, symlink_target, link_type
             FROM entries
             WHERE status = 'PRESERVED'
             ORDER BY deleted_at DESC {}",
            limit_clause
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
        let sql = "SELECT id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                          file_size, mode, uid, gid, quick_fingerprint, full_hash, vault_path,
                          deleted_at, status, is_directory, symlink_target, link_type
                   FROM entries WHERE id = ?1";

        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query_map(params![id], |row| Self::row_to_record(row))?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn find_by_filename(&self, name: &str) -> Result<Vec<EntryRecord>> {
        let sql = "SELECT id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                          file_size, mode, uid, gid, quick_fingerprint, full_hash, vault_path,
                          deleted_at, status, is_directory, symlink_target, link_type
                   FROM entries
                   WHERE status = 'PRESERVED' AND (filename = ?1 OR original_path LIKE ?2)
                   ORDER BY deleted_at DESC";

        let search_pattern = format!("%{}", name);
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![name, search_pattern], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn mark_restored(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE entries SET status = 'RESTORED' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_purged(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE entries SET status = 'PURGED' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn get_expired(&self, days: u32) -> Result<Vec<EntryRecord>> {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let sql = "SELECT id, dev_major, dev_minor, mnt_id, inode_no, original_path, filename,
                          file_size, mode, uid, gid, quick_fingerprint, full_hash, vault_path,
                          deleted_at, status, is_directory, symlink_target, link_type
                   FROM entries
                   WHERE status = 'PRESERVED' AND deleted_at < ?1
                   ORDER BY deleted_at ASC";

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![cutoff.to_rfc3339()], |row| Self::row_to_record(row))?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<EntryRecord> {
        let deleted_at_str: String = row.get(14)?;
        let deleted_at = DateTime::parse_from_rfc3339(&deleted_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

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
            full_hash: row.get(12)?,
            vault_path: row.get(13)?,
            deleted_at,
            status: row.get(15)?,
            is_directory: row.get(16)?,
            symlink_target: row.get(17)?,
            link_type: row.get(18)?,
        })
    }
}
