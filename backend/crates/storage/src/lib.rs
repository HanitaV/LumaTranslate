//! SQLite persistence for translation cache (fronted by bounded in-memory LRU),
//! conversation history, saved capture regions, and user settings.

use chrono::{DateTime, Utc};
use luma_core::{AppSettings, CaptureRegion, HistoryEntry};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use thiserror::Error;

const TARGET_LANGUAGE_VI: &str = "vi";
const DEFAULT_MEMORY_CACHE_CAPACITY: usize = 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub source_text: String,
    pub source_language: String,
    pub provider: String,
    pub provider_config_hash: String,
}

impl CacheKey {
    pub fn new(
        source_text: &str,
        source_language: Option<&str>,
        provider: &str,
        provider_config_tag: &str,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(provider_config_tag.as_bytes());
        let hash_hex = format!("{:x}", hasher.finalize())[..16].to_string();
        Self {
            source_text: source_text.trim().to_string(),
            source_language: source_language.unwrap_or("auto").trim().to_lowercase(),
            provider: provider.trim().to_lowercase(),
            provider_config_hash: hash_hex,
        }
    }
}

struct BoundedMemoryCache {
    capacity: usize,
    order: VecDeque<CacheKey>,
    entries: HashMap<CacheKey, String>,
}

impl BoundedMemoryCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(16),
            order: VecDeque::with_capacity(capacity.max(16)),
            entries: HashMap::with_capacity(capacity.max(16)),
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<String> {
        let val = self.entries.get(key)?.clone();
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.clone());
        Some(val)
    }

    fn insert(&mut self, key: CacheKey, value: String) {
        if self.entries.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
        } else if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
    }

    fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
    }
}

pub struct LumaStore {
    db_path: PathBuf,
    conn: Mutex<Connection>,
    mem_cache: Mutex<BoundedMemoryCache>,
}

impl LumaStore {
    pub fn open_default() -> Result<Self, StorageError> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("LumaTranslate").join("data");
        Self::open_in_dir(&dir)
    }

    pub fn open_in_dir(data_dir: &Path) -> Result<Self, StorageError> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("luma_translate.db");
        Self::open_at_path(db_path, DEFAULT_MEMORY_CACHE_CAPACITY)
    }

    pub fn open_at_path(db_path: PathBuf, memory_cache_capacity: usize) -> Result<Self, StorageError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS translation_cache (
                source_text TEXT NOT NULL,
                source_language TEXT NOT NULL,
                target_language TEXT NOT NULL,
                provider TEXT NOT NULL,
                provider_config_hash TEXT NOT NULL,
                vietnamese_translation TEXT NOT NULL,
                updated_at_utc TEXT NOT NULL,
                PRIMARY KEY (
                    source_text,
                    source_language,
                    target_language,
                    provider,
                    provider_config_hash
                )
            );

            CREATE TABLE IF NOT EXISTS conversation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp_utc TEXT NOT NULL,
                region_id TEXT NOT NULL,
                source_language TEXT NOT NULL,
                ocr_text TEXT NOT NULL,
                vietnamese_translation TEXT NOT NULL,
                provider TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_history_timestamp
                ON conversation_history(id DESC);

            CREATE TABLE IF NOT EXISTS saved_regions (
                id TEXT PRIMARY KEY,
                sort_order INTEGER NOT NULL,
                region_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS kv_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );
            ",
        )?;

        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
            mem_cache: Mutex::new(BoundedMemoryCache::new(memory_cache_capacity)),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn get_cached_translation(&self, key: &CacheKey) -> Result<Option<String>, StorageError> {
        if let Some(hit) = self.mem_cache.lock().get(key) {
            return Ok(Some(hit));
        }

        let found: Option<String> = {
            let conn = self.conn.lock();
            conn.query_row(
                "SELECT vietnamese_translation
                 FROM translation_cache
                 WHERE source_text = ?1
                   AND source_language = ?2
                   AND target_language = ?3
                   AND provider = ?4
                   AND provider_config_hash = ?5",
                params![
                    key.source_text,
                    key.source_language,
                    TARGET_LANGUAGE_VI,
                    key.provider,
                    key.provider_config_hash
                ],
                |row| row.get(0),
            )
            .optional()?
        };

        if let Some(ref val) = found {
            self.mem_cache.lock().insert(key.clone(), val.clone());
        }
        Ok(found)
    }

    pub fn put_cached_translation(&self, key: CacheKey, vietnamese: &str) -> Result<(), StorageError> {
        let now = Utc::now().to_rfc3339();
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO translation_cache (
                    source_text,
                    source_language,
                    target_language,
                    provider,
                    provider_config_hash,
                    vietnamese_translation,
                    updated_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(
                    source_text,
                    source_language,
                    target_language,
                    provider,
                    provider_config_hash
                ) DO UPDATE SET
                    vietnamese_translation = excluded.vietnamese_translation,
                    updated_at_utc = excluded.updated_at_utc",
                params![
                    key.source_text,
                    key.source_language,
                    TARGET_LANGUAGE_VI,
                    key.provider,
                    key.provider_config_hash,
                    vietnamese,
                    now
                ],
            )?;
        }
        self.mem_cache.lock().insert(key, vietnamese.to_string());
        Ok(())
    }

    pub fn clear_cache(&self) -> Result<(), StorageError> {
        self.mem_cache.lock().clear();
        self.conn.lock().execute("DELETE FROM translation_cache", [])?;
        Ok(())
    }

    pub fn append_history(
        &self,
        region_id: &str,
        source_language: &str,
        ocr_text: &str,
        vietnamese_translation: &str,
        provider: &str,
    ) -> Result<HistoryEntry, StorageError> {
        let ts = Utc::now();
        let ts_str = ts.to_rfc3339();
        let id = {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO conversation_history (
                    timestamp_utc,
                    region_id,
                    source_language,
                    ocr_text,
                    vietnamese_translation,
                    provider
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    ts_str,
                    region_id,
                    source_language,
                    ocr_text,
                    vietnamese_translation,
                    provider
                ],
            )?;
            conn.last_insert_rowid()
        };

        Ok(HistoryEntry {
            id,
            timestamp_utc: ts,
            region_id: region_id.to_string(),
            source_language: source_language.to_string(),
            ocr_text: ocr_text.to_string(),
            vietnamese_translation: vietnamese_translation.to_string(),
            provider: provider.to_string(),
        })
    }

    pub fn query_history(
        &self,
        search_query: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let clamped_limit = limit.clamp(1, 500) as i64;
        let offset_i64 = offset as i64;
        let conn = self.conn.lock();

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HistoryEntry> {
            let raw_ts: String = row.get(1)?;
            let parsed_ts = DateTime::parse_from_rfc3339(&raw_ts)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(HistoryEntry {
                id: row.get(0)?,
                timestamp_utc: parsed_ts,
                region_id: row.get(2)?,
                source_language: row.get(3)?,
                ocr_text: row.get(4)?,
                vietnamese_translation: row.get(5)?,
                provider: row.get(6)?,
            })
        };

        if let Some(q) = search_query.map(str::trim).filter(|s| !s.is_empty()) {
            let pattern = format!("%{q}%");
            let mut stmt = conn.prepare(
                "SELECT id, timestamp_utc, region_id, source_language, ocr_text, vietnamese_translation, provider
                 FROM conversation_history
                 WHERE ocr_text LIKE ?1 OR vietnamese_translation LIKE ?1
                 ORDER BY id DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(params![pattern, clamped_limit, offset_i64], map_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp_utc, region_id, source_language, ocr_text, vietnamese_translation, provider
                 FROM conversation_history
                 ORDER BY id DESC
                 LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![clamped_limit, offset_i64], map_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn clear_history(&self) -> Result<(), StorageError> {
        self.conn
            .lock()
            .execute("DELETE FROM conversation_history", [])?;
        Ok(())
    }

    pub fn save_regions(&self, regions: &[CaptureRegion]) -> Result<(), StorageError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM saved_regions", [])?;
        for (idx, region) in regions.iter().enumerate() {
            let json = serde_json::to_string(region)?;
            tx.execute(
                "INSERT INTO saved_regions (id, sort_order, region_json) VALUES (?1, ?2, ?3)",
                params![region.id, idx as i64, json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_regions(&self) -> Result<Vec<CaptureRegion>, StorageError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT region_json FROM saved_regions ORDER BY sort_order ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            let json_str = r?;
            out.push(serde_json::from_str(&json_str)?);
        }
        Ok(out)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        let json = serde_json::to_string(settings)?;
        self.conn.lock().execute(
            "INSERT INTO kv_settings (key, value_json) VALUES ('app_settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![json],
        )?;
        Ok(())
    }

    pub fn load_settings(&self) -> Result<Option<AppSettings>, StorageError> {
        let raw: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT value_json FROM kv_settings WHERE key = 'app_settings'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_cache_and_history_persist_search_and_clear() {
        let temp_dir = std::env::temp_dir().join(format!("luma_store_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let db_file = temp_dir.join("test.db");

        {
            let store = LumaStore::open_at_path(db_file.clone(), 32).unwrap();
            let key = CacheKey::new("また会えて嬉しい。", Some("ja"), "mymemory", "v1");
            assert_eq!(store.get_cached_translation(&key).unwrap(), None);

            store
                .put_cached_translation(key.clone(), "Rất vui khi được gặp lại bạn.")
                .unwrap();
            assert_eq!(
                store.get_cached_translation(&key).unwrap().as_deref(),
                Some("Rất vui khi được gặp lại bạn.")
            );

            store
                .append_history(
                    "region-1",
                    "ja",
                    "また会えて嬉しい。",
                    "Rất vui khi được gặp lại bạn.",
                    "MyMemory",
                )
                .unwrap();
            store
                .append_history("region-2", "en", "Good morning", "Chào buổi sáng", "MyMemory")
                .unwrap();
        }

        // Reopen DB to verify disk persistence beyond in-memory LRU
        let reopened = LumaStore::open_at_path(db_file, 32).unwrap();
        let key = CacheKey::new("また会えて嬉しい。", Some("ja"), "mymemory", "v1");
        assert_eq!(
            reopened.get_cached_translation(&key).unwrap().as_deref(),
            Some("Rất vui khi được gặp lại bạn.")
        );

        let all = reopened.query_history(None, 50, 0).unwrap();
        assert_eq!(all.len(), 2);

        let filtered = reopened.query_history(Some("gặp lại"), 50, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].region_id, "region-1");

        reopened.clear_cache().unwrap();
        assert_eq!(reopened.get_cached_translation(&key).unwrap(), None);

        reopened.clear_history().unwrap();
        assert!(reopened.query_history(None, 50, 0).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
