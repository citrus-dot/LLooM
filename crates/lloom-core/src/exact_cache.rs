//! SQLite exact-response cache owned by the Rust host.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ExactCache {
    path: PathBuf,
    ttl_secs: i64,
    conn: Mutex<Connection>,
}

impl ExactCache {
    pub fn open(path: impl AsRef<Path>, ttl_secs: i64) -> rusqlite::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS exact_cache (
               key TEXT PRIMARY KEY, model TEXT NOT NULL, response TEXT NOT NULL,
               conv_id TEXT, created_at INTEGER NOT NULL, hits INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_exact_created ON exact_cache(created_at);",
        )?;
        Ok(Self {
            path,
            ttl_secs,
            conn: Mutex::new(conn),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn lookup(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT response, created_at FROM exact_cache WHERE key = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((response, created_at)) = row else {
            return Ok(None);
        };
        if self.ttl_secs > 0 && now() - created_at > self.ttl_secs {
            conn.execute("DELETE FROM exact_cache WHERE key = ?1", params![key])?;
            return Ok(None);
        }
        conn.execute(
            "UPDATE exact_cache SET hits = hits + 1 WHERE key = ?1",
            params![key],
        )?;
        Ok(Some(response))
    }

    pub fn store(
        &self,
        key: &str,
        model: &str,
        response: &str,
        conv_id: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO exact_cache (key, model, response, conv_id, created_at, hits)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)
             ON CONFLICT(key) DO UPDATE SET model=excluded.model, response=excluded.response,
               conv_id=excluded.conv_id, created_at=excluded.created_at, hits=0",
            params![key, model, response, conv_id, now()],
        )?;
        Ok(())
    }

    pub fn sweep(&self, max_entries: usize) -> rusqlite::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut removed = 0usize;
        if self.ttl_secs > 0 {
            removed += conn.execute(
                "DELETE FROM exact_cache WHERE created_at < ?1",
                params![now() - self.ttl_secs],
            )?;
        }
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM exact_cache", [], |r| r.get(0))?;
        let count = count.max(0) as usize;
        if count > max_entries {
            removed += conn.execute(
                "DELETE FROM exact_cache WHERE key IN
                 (SELECT key FROM exact_cache ORDER BY created_at ASC LIMIT ?1)",
                params![(count - max_entries) as i64],
            )?;
        }
        Ok(removed)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lloom-{label}-{}-{}.sqlite3",
            std::process::id(),
            now()
        ))
    }

    #[test]
    fn stores_hits_and_evicts_oldest() {
        let path = temp_path("exact");
        let cache = ExactCache::open(&path, 3600).unwrap();
        cache.store("a", "m", "A", None).unwrap();
        cache.store("b", "m", "B", Some("c")).unwrap();
        assert_eq!(cache.lookup("a").unwrap().as_deref(), Some("A"));
        assert_eq!(cache.sweep(1).unwrap(), 1);
        let remaining = usize::from(cache.lookup("a").unwrap().is_some())
            + usize::from(cache.lookup("b").unwrap().is_some());
        assert_eq!(remaining, 1);
        drop(cache);
        let _ = std::fs::remove_file(path);
    }
}
