//! glia-cache — Redis caching layer for Glia (C5).
//!
//! Provides a `Cache` trait with two implementations:
//! - `InMemoryCache` — for tests and dev. `HashMap` + per-entry expiry.
//! - `RedisCache` — production. `redis` crate with `ConnectionManager`
//!   for automatic reconnection pooling. `rustls`-only TLS (matching
//!   glia-sandbox's choice).
//!
//! Design:
//! - Keys are application-namespaced strings: `oauth::creds::linear` not
//!   raw `linear`. Avoids collisions when the cache is shared.
//! - TTL is per-entry, set on put. Redis `SET key value EX <ttl>`.
//! - In-memory cleanup is lazy: expired entries are evicted on `get`
//!   (and on a background sweep task, spawned on first `put`).
//! - All errors route through `CacheError`. No panics on network loss —
//!   `redis` returns errors which bubble up.
//!
//! OAuth access tokens use the 15-minute TTL (T14: prevents redundant
//! exchange calls across parallel `glia_action` executions).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Default TTL for OAuth access tokens (T14 spec: 15 minutes).
pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

/// Cache key builders. Single source of truth so we don't typo namespaces.
pub mod keys {
    /// OAuth access token for a given credential id (e.g., `linear`).
    pub fn oauth_access_token(cred_id: &str) -> String {
        format!("oauth::creds::{}::access", cred_id)
    }

    /// OAuth refresh token for a given credential id.
    pub fn oauth_refresh_token(cred_id: &str) -> String {
        format!("oauth::creds::{}::refresh", cred_id)
    }

    /// Synthesis result keyed by SHA-256 of the query string.
    pub fn synth_result(query_hash: &str) -> String {
        format!("synth::result::{}", query_hash)
    }

    /// Cached credentials lookup for a user.
    pub fn auth_creds(user_id: &str) -> String {
        format!("auth::user::{}::creds", user_id)
    }

    /// Wrap a user query into a cache key (SHA-256 hex).
    pub fn hash_query(query: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(query.as_bytes());
        let digest = h.finalize();
        digest.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// A typed value wrapper. Lets us round-trip JSON in the cache and
/// distinguish a present-but-expired value from a missing one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedValue<T> {
    /// The wrapped value.
    pub value: T,
    /// When this entry was created (RFC-3339). Stored for diagnostics.
    pub created_at: String,
}

impl<T> CachedValue<T> {
    /// Wrap a value with the current time.
    pub fn now(value: T) -> Self {
        Self {
            value,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Errors from cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// Redis network/protocol error.
    #[error("redis: {0}")]
    Redis(String),
    /// I/O error from in-memory backend (rare; only on serialization).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// Operation timed out.
    #[error("timeout after {0:?}")]
    Timeout(Duration),
}

/// Pluggable cache backend.
#[async_trait]
pub trait Cache: Send + Sync {
    /// Get raw bytes for a key. Returns `None` if missing or expired.
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError>;

    /// Put raw bytes with a TTL.
    async fn put_bytes(&self, key: &str, value: &[u8], ttl: Duration) -> Result<(), CacheError>;

    /// Delete a key. Idempotent (no error if missing).
    async fn delete(&self, key: &str) -> Result<(), CacheError>;

    /// Health check.
    async fn ping(&self) -> Result<(), CacheError>;
}

/// Free function: typed get (JSON deserialize).
pub async fn get_typed<C: Cache + ?Sized, T: for<'de> Deserialize<'de>>(
    cache: &C,
    key: &str,
) -> Result<Option<T>, CacheError> {
    match cache.get_bytes(key).await? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

/// Free function: typed put (JSON serialize).
pub async fn put_typed<C: Cache + ?Sized, T: Serialize>(
    cache: &C,
    key: &str,
    value: &T,
    ttl: Duration,
) -> Result<(), CacheError> {
    let bytes = serde_json::to_vec(value)?;
    cache.put_bytes(key, &bytes, ttl).await
}

// ---------------- InMemoryCache ----------------

/// In-memory cache entry.
struct Entry {
    /// Raw bytes.
    value: Vec<u8>,
    /// When this entry expires.
    expires_at: Instant,
}

impl Entry {
    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// In-memory cache. Process-local, no network. For tests and dev.
pub struct InMemoryCache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    sweep: Mutex<Option<JoinHandle<()>>>,
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryCache {
    /// Build a new in-memory cache with no background sweeper.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            sweep: Mutex::new(None),
        }
    }

    /// Spawn a background task that evicts expired entries every `interval`.
    /// Safe to call multiple times — only the first call spawns.
    pub async fn start_sweeper(&self, interval: Duration) {
        let mut guard = self.sweep.lock().await;
        if guard.is_some() {
            return;
        }
        let inner = self.inner.clone();
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let mut map = inner.lock().await;
                map.retain(|_, e| !e.is_expired());
            }
        });
        *guard = Some(handle);
    }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let mut map = self.inner.lock().await;
        if let Some(entry) = map.get(key) {
            if entry.is_expired() {
                map.remove(key);
                return Ok(None);
            }
            return Ok(Some(entry.value.clone()));
        }
        Ok(None)
    }

    async fn put_bytes(&self, key: &str, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        let mut map = self.inner.lock().await;
        map.insert(
            key.into(),
            Entry {
                value: value.to_vec(),
                expires_at: Instant::now() + ttl,
            },
        );
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        self.inner.lock().await.remove(key);
        Ok(())
    }

    async fn ping(&self) -> Result<(), CacheError> {
        Ok(())
    }
}

// ---------------- RedisCache ----------------

/// Redis-backed cache. Uses `ConnectionManager` for auto-reconnect.
pub struct RedisCache {
    conn: redis::aio::ConnectionManager,
}

impl RedisCache {
    /// Connect to Redis at the given URL (e.g., `redis://127.0.0.1:6379/0`).
    pub async fn connect(url: impl AsRef<str>) -> Result<Self, CacheError> {
        let client =
            redis::Client::open(url.as_ref()).map_err(|e| CacheError::Redis(e.to_string()))?;
        let conn = redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let mut conn = self.conn.clone();
        let val: Option<Vec<u8>> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(val)
    }

    async fn put_bytes(&self, key: &str, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let ttl_secs = ttl.as_secs().max(1);
        let _: () = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let _: i64 = redis::cmd("DEL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(())
    }

    async fn ping(&self) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        if pong != "PONG" {
            return Err(CacheError::Redis(format!(
                "unexpected ping response: {}",
                pong
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn in_memory_put_get() {
        let c = InMemoryCache::new();
        c.put_bytes("k", b"v", DEFAULT_TTL).await.unwrap();
        assert_eq!(c.get_bytes("k").await.unwrap(), Some(b"v".to_vec()));
    }

    #[tokio::test]
    async fn in_memory_missing_key() {
        let c = InMemoryCache::new();
        assert!(c.get_bytes("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_ttl_expires() {
        let c = InMemoryCache::new();
        c.put_bytes("k", b"v", Duration::from_millis(20))
            .await
            .unwrap();
        assert!(c.get_bytes("k").await.unwrap().is_some());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(c.get_bytes("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_delete() {
        let c = InMemoryCache::new();
        c.put_bytes("k", b"v", DEFAULT_TTL).await.unwrap();
        c.delete("k").await.unwrap();
        assert!(c.get_bytes("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_sweeper_evicts() {
        let c = InMemoryCache::new();
        c.start_sweeper(Duration::from_millis(20)).await;
        c.put_bytes("k", b"v", Duration::from_millis(10))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let map = c.inner.lock().await;
        assert!(!map.contains_key("k"), "sweeper should have evicted k");
    }

    #[tokio::test]
    async fn typed_round_trip() {
        let c = InMemoryCache::new();
        let val = CachedValue::now(vec![1u32, 2, 3]);
        put_typed(&c, "k", &val, DEFAULT_TTL).await.unwrap();
        let back: CachedValue<Vec<u32>> = get_typed(&c, "k").await.unwrap().unwrap();
        assert_eq!(back.value, vec![1, 2, 3]);
    }

    #[test]
    fn key_builders_namespaced() {
        assert_eq!(
            keys::oauth_access_token("linear"),
            "oauth::creds::linear::access"
        );
        assert_eq!(
            keys::oauth_refresh_token("linear"),
            "oauth::creds::linear::refresh"
        );
        assert!(keys::synth_result("abc").starts_with("synth::result::abc"));
        assert_eq!(keys::auth_creds("u1"), "auth::user::u1::creds");
    }

    #[test]
    fn query_hash_deterministic() {
        let h1 = keys::hash_query("hello");
        let h2 = keys::hash_query("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn default_ttl_is_15_minutes() {
        assert_eq!(DEFAULT_TTL, Duration::from_secs(900));
    }

    #[tokio::test]
    async fn ping_succeeds_on_in_memory() {
        let c = InMemoryCache::new();
        c.ping().await.unwrap();
    }

    // Redis tests are opt-in: only run if REDIS_URL is set.
    #[tokio::test]
    async fn redis_round_trip_if_available() {
        let url = match std::env::var("REDIS_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("REDIS_URL not set, skipping redis tests");
                return;
            }
        };
        let c = match RedisCache::connect(&url).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Redis unavailable ({}), skipping", e);
                return;
            }
        };
        c.ping().await.unwrap();
        let key = format!("glia::test::{}", keys::hash_query("ping"));
        c.put_bytes(&key, b"hello", Duration::from_secs(5))
            .await
            .unwrap();
        let back = c.get_bytes(&key).await.unwrap();
        assert_eq!(back.as_deref(), Some(&b"hello"[..]));
        c.delete(&key).await.unwrap();
    }

    #[tokio::test]
    async fn empty_string_key_put_get() {
        let c = InMemoryCache::new();
        c.put_bytes("", b"empty-key-val", DEFAULT_TTL).await.unwrap();
        assert_eq!(
            c.get_bytes("").await.unwrap(),
            Some(b"empty-key-val".to_vec())
        );
    }

    #[tokio::test]
    async fn unicode_key_put_get() {
        let c = InMemoryCache::new();
        c.put_bytes("oauth::creds::héllo::access", b"v", DEFAULT_TTL)
            .await
            .unwrap();
        assert_eq!(
            c.get_bytes("oauth::creds::héllo::access").await.unwrap(),
            Some(b"v".to_vec())
        );
    }

    #[tokio::test]
    async fn empty_value_zero_bytes_roundtrip() {
        let c = InMemoryCache::new();
        c.put_bytes("k", b"", DEFAULT_TTL).await.unwrap();
        assert_eq!(c.get_bytes("k").await.unwrap(), Some(vec![]));
    }

    #[tokio::test]
    async fn ttl_zero_in_memory_immediate_expiry() {
        let c = InMemoryCache::new();
        c.put_bytes("k", b"v", Duration::ZERO).await.unwrap();
        // TTL=0 means expires_at = now. Any get after this point returns None.
        // There may be a tiny window where Instant::now() == expires_at, so
        // we sleep a tiny bit to ensure we're past.
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(c.get_bytes("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_missing_key_idempotent_no_error() {
        let c = InMemoryCache::new();
        // Deleting a key that was never set should succeed.
        c.delete("never-existed").await.unwrap();
        assert!(c.get_bytes("never-existed").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sweeper_with_no_expired_keeps_all() {
        let c = InMemoryCache::new();
        c.start_sweeper(Duration::from_millis(20)).await;
        c.put_bytes("k1", b"v1", DEFAULT_TTL).await.unwrap();
        c.put_bytes("k2", b"v2", DEFAULT_TTL).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        // Both should still be present (not expired).
        assert!(c.get_bytes("k1").await.unwrap().is_some());
        assert!(c.get_bytes("k2").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn start_sweeper_twice_spawns_once() {
        let c = InMemoryCache::new();
        c.start_sweeper(Duration::from_secs(60)).await;
        c.start_sweeper(Duration::from_secs(60)).await;
        // Should not panic or create duplicate sweepers.
        let guard = c.sweep.lock().await;
        assert!(guard.is_some());
    }

    #[tokio::test]
    async fn concurrent_set_get_same_key() {
        use std::sync::Arc;
        let c = Arc::new(InMemoryCache::new());
        let mut handles = Vec::new();
        for i in 0..10 {
            let cc = c.clone();
            handles.push(tokio::spawn(async move {
                cc.put_bytes("shared", format!("v{i}").as_bytes(), DEFAULT_TTL)
                    .await
                    .unwrap();
                cc.get_bytes("shared").await.unwrap()
            }));
        }
        for h in handles {
            let _ = h.await.unwrap();
        }
        // Final value is some value from one of the writers.
        assert!(c.get_bytes("shared").await.unwrap().is_some());
    }

    #[test]
    fn hash_query_empty_string_deterministic() {
        let h1 = keys::hash_query("");
        let h2 = keys::hash_query("");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn hash_query_unicode_deterministic() {
        let h1 = keys::hash_query("héllo 🐍");
        let h2 = keys::hash_query("héllo 🐍");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn empty_cred_id_key_has_double_colon() {
        // format!("oauth::creds::{}::access", "") = "oauth::creds::::access"
        let key = keys::oauth_access_token("");
        assert_eq!(key, "oauth::creds::::access");
    }

    #[tokio::test]
    async fn get_typed_corrupted_bytes_returns_serde_error() {
        let c = InMemoryCache::new();
        // Put raw invalid JSON bytes.
        c.put_bytes("bad", b"{not valid json", DEFAULT_TTL).await.unwrap();
        let result: Result<Option<CachedValue<String>>, _> = get_typed(&c, "bad").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CacheError::Serde(_)
        ));
    }
}
