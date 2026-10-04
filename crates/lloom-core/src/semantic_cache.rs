//! Rust-native semantic response cache using FastEmbed and SQLite.

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static EMBEDDER: OnceLock<std::result::Result<Mutex<TextEmbedding>, String>> = OnceLock::new();

pub struct SemanticCache {
    conn: Connection,
    ttl_secs: i64,
    threshold: f32,
}

impl SemanticCache {
    pub fn open(path: impl AsRef<Path>, ttl_secs: i64, threshold: f32) -> rusqlite::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS semantic_cache (
               id INTEGER PRIMARY KEY AUTOINCREMENT, model TEXT NOT NULL,
               query TEXT NOT NULL, response TEXT NOT NULL, embedding BLOB NOT NULL,
               created_at INTEGER NOT NULL, hits INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_semantic_model_created
             ON semantic_cache(model, created_at);",
        )?;
        Ok(Self {
            conn,
            ttl_secs,
            threshold,
        })
    }

    pub fn embed(text: &str) -> std::result::Result<Vec<f32>, String> {
        let result = EMBEDDER.get_or_init(|| {
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().min(4))
                .ok();
            let mut options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
                .with_cache_dir(crate::config::data_dir().join("models/fastembed"))
                .with_show_download_progress(false);
            if let Some(threads) = threads {
                options = options.with_intra_threads(threads);
            }
            TextEmbedding::try_new(options)
                .map(Mutex::new)
                .map_err(|e| e.to_string())
        });
        let model = result.as_ref().map_err(Clone::clone)?;
        model
            .lock()
            .map_err(|_| "embedding model lock poisoned".to_string())?
            .embed(vec![text], None)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "embedding model returned no vector".to_string())
    }

    pub fn lookup(&self, model: &str, vector: &[f32]) -> rusqlite::Result<Option<(String, f32)>> {
        let cutoff = if self.ttl_secs > 0 {
            now() - self.ttl_secs
        } else {
            0
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, response, embedding FROM semantic_cache
             WHERE model = ?1 AND created_at >= ?2 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![model, cutoff], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut best: Option<(i64, String, f32)> = None;
        for row in rows {
            let (id, response, blob) = row?;
            let Some(candidate) = decode(&blob) else {
                continue;
            };
            let similarity = cosine(vector, &candidate);
            if similarity >= self.threshold && best.as_ref().is_none_or(|b| similarity > b.2) {
                best = Some((id, response, similarity));
            }
        }
        if let Some((id, response, similarity)) = best {
            self.conn.execute(
                "UPDATE semantic_cache SET hits = hits + 1 WHERE id = ?1",
                params![id],
            )?;
            Ok(Some((response, similarity)))
        } else {
            Ok(None)
        }
    }

    pub fn store(
        &self,
        model: &str,
        query: &str,
        response: &str,
        vector: &[f32],
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO semantic_cache(model, query, response, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![model, query, response, encode(vector), now()],
        )?;
        Ok(())
    }

    pub fn sweep(&self, max_entries: usize) -> rusqlite::Result<usize> {
        let mut removed = 0;
        if self.ttl_secs > 0 {
            removed += self.conn.execute(
                "DELETE FROM semantic_cache WHERE created_at < ?1",
                params![now() - self.ttl_secs],
            )?;
        }
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM semantic_cache", [], |r| r.get(0))?;
        if count > max_entries as i64 {
            removed += self.conn.execute(
                "DELETE FROM semantic_cache WHERE id IN
                 (SELECT id FROM semantic_cache ORDER BY created_at ASC LIMIT ?1)",
                params![count - max_entries as i64],
            )?;
        }
        Ok(removed)
    }
}

fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn decode(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    Some(
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
    )
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return -1.0;
    }
    let (mut dot, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        -1.0
    } else {
        dot / (aa.sqrt() * bb.sqrt())
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

    #[test]
    fn vector_roundtrip_and_cosine() {
        let v = vec![0.1, -0.2, 0.3];
        assert_eq!(decode(&encode(&v)).unwrap(), v);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
        assert_eq!(cosine(&v, &[1.0]), -1.0);
    }

    #[test]
    fn sqlite_lookup_honors_threshold() {
        let path = std::env::temp_dir().join(format!("lloom-semantic-{}.db", now()));
        let cache = SemanticCache::open(&path, 3600, 0.8).unwrap();
        cache.store("m", "q", "answer", &[1.0, 0.0]).unwrap();
        assert_eq!(cache.lookup("m", &[0.9, 0.1]).unwrap().unwrap().0, "answer");
        assert!(cache.lookup("m", &[0.0, 1.0]).unwrap().is_none());
        drop(cache);
        let _ = std::fs::remove_file(path);
    }
}
