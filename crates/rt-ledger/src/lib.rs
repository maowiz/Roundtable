//! SQLite ledger (backend plan section 4).
//!
//! - One writer connection behind a mutex; readers open their own read-only connections.
//! - WAL with synchronous=FULL, foreign keys on, busy timeout 5 s.
//! - Numbered migrations with a pre-migration backup through the online backup API.
//! - Papers, envelopes and views are immutable except inside `maintenance()`.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub mod repo;
pub mod runs;

/// Re-exported so dependants use the same SQLite bindings.
pub use rusqlite;

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database integrity check failed: {0}")]
    Corrupt(String),
    #[error("unsupported database version {found}, this app supports up to {supported}")]
    Downgrade { found: i64, supported: i64 },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("lock poisoned")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, LedgerError>;

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("migrations/0001_initial.sql"))];
pub const SCHEMA_VERSION: i64 = 1;

pub struct Ledger {
    path: PathBuf,
    writer: Mutex<Connection>,
}

/// Where the pre-migration backup went, if a migration ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReport {
    pub migrated_from: i64,
    pub migrated_to: i64,
    pub backup: Option<PathBuf>,
}

impl Ledger {
    /// Open (or create) the ledger at `path`, run integrity check and migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<(Ledger, OpenReport)> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        configure(&conn)?;
        // integrity before touching anything
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(LedgerError::Corrupt(check));
        }
        let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(LedgerError::Downgrade { found, supported: SCHEMA_VERSION });
        }
        let mut report = OpenReport { migrated_from: found, migrated_to: found, backup: None };
        if found < SCHEMA_VERSION {
            if found > 0 {
                report.backup = Some(backup_before_migration(&conn, &path, found)?);
            }
            for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > found) {
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", version)?;
                tx.commit()?;
                report.migrated_to = *version;
            }
        }
        Ok((Ledger { path, writer: Mutex::new(conn) }, report))
    }

    /// In-memory ledger for tests.
    pub fn in_memory() -> Result<Ledger> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        for (version, sql) in MIGRATIONS {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
        Ok(Ledger { path: PathBuf::from(":memory:"), writer: Mutex::new(conn) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Exclusive access to the single writer connection.
    pub fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer.lock().map_err(|_| LedgerError::Poisoned)
    }

    /// A read-only connection for queries that must not block the writer. File-backed only.
    pub fn reader(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Ok(conn)
    }

    /// Run `f` with immutability triggers disabled, inside one transaction (DATA-01).
    /// This is the only path that may delete papers, envelopes or views.
    pub fn maintenance<T>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.writer()?;
        let tx = conn.transaction()?;
        tx.execute("UPDATE maintenance SET enabled = 1 WHERE id = 1", [])?;
        let out = f(&tx);
        tx.execute("UPDATE maintenance SET enabled = 0 WHERE id = 1", [])?;
        match out {
            Ok(v) => {
                tx.commit()?;
                Ok(v)
            }
            Err(e) => {
                tx.rollback()?;
                Err(e)
            }
        }
    }

    /// Consistent online backup to `dest` (PRD 12.2).
    pub fn backup_to(&self, dest: impl AsRef<Path>) -> Result<()> {
        let conn = self.writer()?;
        let mut dst = Connection::open(dest.as_ref())?;
        let bk = rusqlite::backup::Backup::new(&conn, &mut dst)?;
        bk.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<()> {
        let conn = self.writer()?;
        let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check == "ok" {
            Ok(())
        } else {
            Err(LedgerError::Corrupt(check))
        }
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "secure_delete", "ON")?;
    Ok(())
}

fn backup_before_migration(conn: &Connection, path: &Path, from: i64) -> Result<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new(".")).join("backups");
    std::fs::create_dir_all(&dir)?;
    let stamp = now_compact();
    let dest = dir.join(format!("pre-migration-v{from}-{stamp}.db"));
    let mut dst = Connection::open(&dest)?;
    let bk = rusqlite::backup::Backup::new(conn, &mut dst)?;
    bk.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
    Ok(dest)
}

/// RFC 3339 UTC timestamp used for every `*_at` column.
pub fn now() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap()
}

fn now_compact() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(), t.second())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn opens_with_wal_and_full_sync_and_migrates_to_current_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtable.db");
        let (ledger, report) = Ledger::open(&path).unwrap();
        assert_eq!(report, OpenReport { migrated_from: 0, migrated_to: SCHEMA_VERSION, backup: None });
        let conn = ledger.writer().unwrap();
        let jm: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        let sync: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(jm.to_lowercase(), "wal");
        assert_eq!(sync, 2, "synchronous=FULL");
        assert_eq!(fk, 1);
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn reopening_is_idempotent_and_rejects_downgrades() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db");
        {
            let (_l, r) = Ledger::open(&path).unwrap();
            assert_eq!(r.migrated_to, SCHEMA_VERSION);
        }
        let (_l, r) = Ledger::open(&path).unwrap();
        assert_eq!(r.migrated_from, SCHEMA_VERSION);
        assert!(r.backup.is_none());
        drop(_l);
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", SCHEMA_VERSION + 10).unwrap();
        }
        assert!(matches!(Ledger::open(&path), Err(LedgerError::Downgrade { .. })));
    }

    #[test]
    fn immutability_triggers_block_normal_deletes_but_maintenance_allows_them() {
        let ledger = Ledger::in_memory().unwrap();
        {
            let c = ledger.writer().unwrap();
            c.execute_batch(&format!(
                "INSERT INTO runs(id,state,stage,question,panel_snapshot_json,limits_json,price_catalog_version,schema_version,app_version,adapter_versions_json,seed,created_at)
                 VALUES('r1','answering','answer','q','{{}}','{{}}','p1','1','0.1','{{}}','s','{now}');
                 INSERT INTO logical_calls(id,run_id,stage,reviewer_seat,state,created_at,updated_at) VALUES('c1','r1','answer',0,'in_flight','{now}','{now}');
                 INSERT INTO attempts(id,logical_call_id,ordinal,kind,request_digest,serialized_body_redacted,dispatched_at) VALUES('a1','c1',1,'primary','d','{{}}','{now}');
                 INSERT INTO papers(id,run_id,kind,author_seat,schema_version,body_json,digest,attempt_id,created_at) VALUES('p1','r1','answer',0,'answer.v1','{{}}','d','a1','{now}');",
                now = now()
            ))
            .unwrap();
            let err = c.execute("DELETE FROM papers WHERE id='p1'", []).unwrap_err();
            assert!(err.to_string().contains("immutable"));
            let err = c.execute("UPDATE papers SET body_json='{}' WHERE id='p1'", []).unwrap_err();
            assert!(err.to_string().contains("immutable"));
        }
        ledger
            .maintenance(|tx| {
                tx.execute("DELETE FROM runs WHERE id='r1'", [])?; // cascades to papers
                Ok(())
            })
            .unwrap();
        let c = ledger.writer().unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM papers", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        let flag: i64 = c.query_row("SELECT enabled FROM maintenance", [], |r| r.get(0)).unwrap();
        assert_eq!(flag, 0, "maintenance flag is reset after the transaction");
    }

    #[test]
    fn maintenance_rolls_back_and_resets_flag_on_error() {
        let ledger = Ledger::in_memory().unwrap();
        let r: Result<()> = ledger.maintenance(|_tx| Err(LedgerError::NotFound("x".into())));
        assert!(r.is_err());
        let c = ledger.writer().unwrap();
        let flag: i64 = c.query_row("SELECT enabled FROM maintenance", [], |r| r.get(0)).unwrap();
        assert_eq!(flag, 0);
    }

    #[test]
    fn online_backup_produces_a_readable_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (ledger, _) = Ledger::open(dir.path().join("r.db")).unwrap();
        ledger.writer().unwrap().execute("INSERT INTO settings(key,json,updated_at) VALUES('theme','\"dark\"',?1)", params![now()]).unwrap();
        let dest = dir.path().join("copy.db");
        ledger.backup_to(&dest).unwrap();
        let c = Connection::open(&dest).unwrap();
        let v: String = c.query_row("SELECT json FROM settings WHERE key='theme'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "\"dark\"");
    }
}
