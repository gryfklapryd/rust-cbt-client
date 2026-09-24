//! Penyimpanan lokal (SQLite). Semua jawaban disimpan segera setelah berubah,
//! sehingga ujian bisa dilanjutkan bila aplikasi tertutup / listrik padam.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::error::AppResult;

const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE IF NOT EXISTS config (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Satu baris per versi paket. Paket lama tetap disimpan selama masih dirujuk attempt.
CREATE TABLE IF NOT EXISTS packages (
  package_id    TEXT PRIMARY KEY,
  schedule_id   TEXT NOT NULL,
  version       INTEGER NOT NULL,
  checksum      TEXT NOT NULL,
  json          TEXT NOT NULL,
  downloaded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS packages_schedule_idx ON packages(schedule_id, version);
CREATE TABLE IF NOT EXISTS attempts (
  id              TEXT PRIMARY KEY,
  schedule_id     TEXT NOT NULL,
  package_id      TEXT NOT NULL,
  participant_id  TEXT NOT NULL,
  status          TEXT NOT NULL,
  sequence        INTEGER NOT NULL DEFAULT 1,
  started_at      TEXT NOT NULL,
  finished_at     TEXT,
  deadline        TEXT NOT NULL,
  plan            TEXT NOT NULL,
  option_orders   TEXT NOT NULL,
  current_index   INTEGER NOT NULL DEFAULT 0,
  violation_count INTEGER NOT NULL DEFAULT 0,
  synced_sequence INTEGER NOT NULL DEFAULT 0,
  sync_error      TEXT,
  UNIQUE (schedule_id, participant_id)
);
CREATE TABLE IF NOT EXISTS answers (
  attempt_id   TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
  question_id  TEXT NOT NULL,
  response     TEXT,
  answered_at  TEXT,
  time_spent   INTEGER NOT NULL DEFAULT 0,
  flagged      INTEGER NOT NULL DEFAULT 0,
  change_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (attempt_id, question_id)
);
CREATE TABLE IF NOT EXISTS events (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  attempt_id TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
  type       TEXT NOT NULL,
  at         TEXT NOT NULL,
  data       TEXT
);
CREATE TABLE IF NOT EXISTS attachments (
  id          TEXT PRIMARY KEY,
  attempt_id  TEXT NOT NULL,
  question_id TEXT NOT NULL,
  path        TEXT NOT NULL,
  name        TEXT NOT NULL,
  mime        TEXT NOT NULL,
  size        INTEGER NOT NULL,
  uploaded    INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS batches (
  id          TEXT PRIMARY KEY,
  created_at  TEXT NOT NULL,
  attempts    TEXT NOT NULL,
  status      TEXT NOT NULL,
  response    TEXT
);
"#];

pub struct Db {
    pub conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(sql)?;
            conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        }
        Ok(Db { conn })
    }

    pub fn get_config(&self, key: &str) -> AppResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM config WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_config(&self, key: &str, value: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_config(&self, key: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM config WHERE key = ?1", [key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_idempotent_migration() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_config("a").unwrap(), None);
        db.set_config("a", "1").unwrap();
        db.set_config("a", "2").unwrap();
        assert_eq!(db.get_config("a").unwrap().as_deref(), Some("2"));
        // Menjalankan init ulang tidak mengulang migrasi.
        let v: i64 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
    }
}
