//! Rust-native semantic response cache using FastEmbed and SQLite.

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static EMBEDDER: OnceLock<std::result::Result<EmbedderPool, String>> = OnceLock::new();

/// P2-3：ONNX 会话不再共用一把全局锁——小容量实例池（最多 2 个会话），
/// 单实例独占式借出，剩余请求复用空闲槽或在下一个实例上排队。
/// 首次失败（如 x86_64 Mac 缺 ONNX Runtime dylib）会永久缓存错误并降级。
struct EmbedderPool {
    free: Mutex<std::collections::VecDeque<TextEmbedding>>,
    created: std::sync::atomic::AtomicUsize,
    max: usize,
}

fn new_embedder_session() -> std::result::Result<TextEmbedding, String> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .ok();
    let mut options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
        .with_cache_dir(crate::config::data_dir().join("models/fastembed"))
        .with_show_download_progress(false);
    if let Some(threads) = threads {
        options = options.with_intra_threads(threads);
    }
    TextEmbedding::try_new(options).map_err(|e| {
        let msg = e.to_string();
        // Intel Mac 构建走 ort load-dynamic：dylib 缺失时错误文本指向
        // dlopen 失败。聊天会静默降级无 L2，这里给一条可执行的修复提示。
        if cfg!(all(target_os = "macos", target_arch = "x86_64")) && msg.contains("libonnxruntime")
        {
            crate::logging::warn(
                "core",
                &format!(
                    "L2 语义缓存不可用：{msg}\n\
                     [core] Intel Mac 版需 ONNX Runtime：brew install onnxruntime\n\
                     [core] （不装也能用：L1 精确缓存与云端/本地模型路由不受影响）"
                ),
            );
        }
        msg
    })
}

/// 借出的会话句柄，Drop 时归还实例池。
struct EmbedderLease<'a>(&'a EmbedderPool, Option<TextEmbedding>);

impl Drop for EmbedderLease<'_> {
    fn drop(&mut self) {
        if let Some(model) = self.1.take() {
            if let Ok(mut free) = self.0.free.lock() {
                free.push_back(model);
            }
        }
    }
}

impl EmbedderPool {
    fn instance(&self) -> std::result::Result<TextEmbedding, String> {
        let mut tries = 0;
        loop {
            if let Ok(mut free) = self.free.lock() {
                if let Some(model) = free.pop_front() {
                    return Ok(model);
                }
            }
            // 未达实例上限 → 惰性创建新会话；否则小睡等待归还。
            let created = self.created.load(std::sync::atomic::Ordering::Relaxed);
            if created < self.max {
                if self
                    .created
                    .compare_exchange(
                        created,
                        created + 1,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return new_embedder_session();
                }
                continue;
            }
            tries += 1;
            if tries > 1200 {
                // 约 1 分钟仍无空闲会话：视为系统异常，放弃（调用方降级无缓存）。
                return Err("embedding pool exhausted".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

pub struct SemanticCache {
    conn: Mutex<Connection>,
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
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;
              CREATE TABLE IF NOT EXISTS semantic_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT, model TEXT NOT NULL,
                query TEXT NOT NULL, response TEXT NOT NULL, embedding BLOB NOT NULL,
                created_at INTEGER NOT NULL, hits INTEGER NOT NULL DEFAULT 0,
                system_hash TEXT NOT NULL DEFAULT '', temperature REAL NOT NULL DEFAULT 0,
                last_hit_at INTEGER NOT NULL DEFAULT 0
              );
             CREATE INDEX IF NOT EXISTS idx_semantic_model_created
             ON semantic_cache(model, created_at);",
        )?;
        // P2-7：复用 db::Db::ensure_columns 的幂等加列助手（不再手写 PRAGMA 探测）。
        crate::db::Db::ensure_columns(
            &conn,
            "semantic_cache",
            &[
                (
                    "system_hash",
                    "ALTER TABLE semantic_cache ADD COLUMN system_hash TEXT NOT NULL DEFAULT ''",
                ),
                (
                    "temperature",
                    "ALTER TABLE semantic_cache ADD COLUMN temperature REAL NOT NULL DEFAULT 0",
                ),
                (
                    "last_hit_at",
                    "ALTER TABLE semantic_cache ADD COLUMN last_hit_at INTEGER NOT NULL DEFAULT 0",
                ),
            ],
        )
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
            ttl_secs,
            threshold,
        })
    }

    pub fn embed(text: &str) -> std::result::Result<Vec<f32>, String> {
        let result = EMBEDDER.get_or_init(|| {
            // 首个会话急切创建：初始化失败（含 dylib 缺失提示）只发生一次并永久缓存。
            Ok(match new_embedder_session() {
                Ok(first) => EmbedderPool {
                    free: Mutex::new(std::collections::VecDeque::from(vec![first])),
                    created: std::sync::atomic::AtomicUsize::new(1),
                    max: 2,
                },
                Err(msg) => return Err(msg),
            })
        });
        let pool = result.as_ref().map_err(Clone::clone)?;
        let mut lease = EmbedderLease(pool, Some(pool.instance()?));
        lease
            .1
            .as_mut()
            .ok_or_else(|| "embedding lease already returned".to_string())?
            .embed(vec![text], None)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "embedding model returned no vector".to_string())
    }

    pub fn lookup(
        &self,
        model: &str,
        system_hash: &str,
        temperature: f64,
        vector: &[f32],
    ) -> rusqlite::Result<Option<(String, f32)>> {
        let cutoff = if self.ttl_secs > 0 {
            now() - self.ttl_secs
        } else {
            0
        };
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, response, embedding FROM semantic_cache
              WHERE model = ?1 AND created_at >= ?2 AND system_hash = ?3 AND temperature = ?4
              ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![model, cutoff, system_hash, temperature], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut best: Option<(i64, String, f32)> = None;
        let mut invalid_dimensions = Vec::new();
        for row in rows {
            let (id, response, blob) = row?;
            let Some(candidate) = decode(&blob) else {
                invalid_dimensions.push(id);
                continue;
            };
            if candidate.len() != vector.len() {
                invalid_dimensions.push(id);
                continue;
            }
            let similarity = cosine(vector, &candidate);
            if similarity >= self.threshold && best.as_ref().is_none_or(|b| similarity > b.2) {
                best = Some((id, response, similarity));
            }
        }
        drop(stmt);
        for id in invalid_dimensions {
            conn.execute("DELETE FROM semantic_cache WHERE id = ?1", params![id])?;
        }
        if let Some((id, response, similarity)) = best {
            conn.execute(
                "UPDATE semantic_cache SET hits = hits + 1, last_hit_at = ?2 WHERE id = ?1",
                params![id, now_millis()],
            )?;
            Ok(Some((response, similarity)))
        } else {
            Ok(None)
        }
    }

    pub fn store(
        &self,
        model: &str,
        system_hash: &str,
        temperature: f64,
        query: &str,
        response: &str,
        vector: &[f32],
    ) -> rusqlite::Result<()> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner()).execute(
            "INSERT INTO semantic_cache(model, system_hash, temperature, query, response, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![model, system_hash, temperature, query, response, encode(vector), now()],
        )?;
        Ok(())
    }

    pub fn sweep(&self, max_entries: usize) -> rusqlite::Result<usize> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut removed = 0;
        if self.ttl_secs > 0 {
            removed += conn.execute(
                "DELETE FROM semantic_cache WHERE created_at < ?1",
                params![now() - self.ttl_secs],
            )?;
        }
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM semantic_cache", [], |r| r.get(0))?;
        if count > max_entries as i64 {
            removed += conn.execute(
                "DELETE FROM semantic_cache WHERE id IN
                  (SELECT id FROM semantic_cache ORDER BY
                    CASE WHEN last_hit_at > 0 THEN last_hit_at ELSE created_at * 1000 END ASC,
                    id ASC LIMIT ?1)",
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

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
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
        cache
            .store("m", "system-A", 0.3, "q", "answer", &[1.0, 0.0])
            .unwrap();
        assert_eq!(
            cache
                .lookup("m", "system-A", 0.3, &[0.9, 0.1])
                .unwrap()
                .unwrap()
                .0,
            "answer"
        );
        assert!(cache
            .lookup("m", "system-B", 0.3, &[0.9, 0.1])
            .unwrap()
            .is_none());
        assert!(cache
            .lookup("m", "system-A", 0.5, &[0.9, 0.1])
            .unwrap()
            .is_none());
        assert!(cache
            .lookup("m", "system-A", 0.3, &[0.0, 1.0])
            .unwrap()
            .is_none());
        assert!(cache
            .lookup("m", "system-A", 0.3, &[1.0, 0.0, 0.0])
            .unwrap()
            .is_none());
        let remaining: i64 = cache
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM semantic_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "不同维度的旧向量应被移除");
        drop(cache);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn old_cache_rows_do_not_cross_system_boundaries_after_migration() {
        let path = std::env::temp_dir().join(format!(
            "lloom-semantic-legacy-{}-{}.db",
            std::process::id(),
            now()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE semantic_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT, model TEXT NOT NULL,
                query TEXT NOT NULL, response TEXT NOT NULL, embedding BLOB NOT NULL,
                created_at INTEGER NOT NULL, hits INTEGER NOT NULL DEFAULT 0
            );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO semantic_cache(model, query, response, embedding, created_at)
                VALUES('m', 'q', 'legacy', ?1, ?2)",
                params![encode(&[1.0, 0.0]), now()],
            )
            .unwrap();
        }
        let cache = SemanticCache::open(&path, 3600, 0.8).unwrap();
        assert!(cache
            .lookup("m", "system-A", 0.0, &[1.0, 0.0])
            .unwrap()
            .is_none());
        cache
            .store("m", "system-A", 0.3, "q", "safe", &[1.0, 0.0])
            .unwrap();
        assert_eq!(
            cache
                .lookup("m", "system-A", 0.3, &[1.0, 0.0])
                .unwrap()
                .unwrap()
                .0,
            "safe"
        );
        assert!(cache
            .lookup("m", "system-B", 0.3, &[1.0, 0.0])
            .unwrap()
            .is_none());
        drop(cache);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sweep_keeps_recently_accessed_entry() {
        let path = std::env::temp_dir().join(format!(
            "lloom-semantic-lru-{}-{}.db",
            std::process::id(),
            now()
        ));
        let cache = SemanticCache::open(&path, 3600, 0.8).unwrap();
        cache
            .store("old", "sys", 0.3, "a", "A", &[1.0, 0.0])
            .unwrap();
        cache
            .store("new", "sys", 0.3, "b", "B", &[0.0, 1.0])
            .unwrap();
        cache
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE semantic_cache SET created_at=?1 WHERE model='old'",
                params![now() - 100],
            )
            .unwrap();
        cache
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE semantic_cache SET created_at=?1 WHERE model='new'",
                params![now() - 1],
            )
            .unwrap();
        assert!(cache
            .lookup("old", "sys", 0.3, &[1.0, 0.0])
            .unwrap()
            .is_some());
        assert_eq!(cache.sweep(1).unwrap(), 1);
        let remaining: String = cache
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT model FROM semantic_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, "old");
        drop(cache);
        let _ = std::fs::remove_file(path);
    }
}
