//! glia-bao — OpenBao client + Predictive Pre-Auth + Redis OAuth token cache (T14).
//!
//! Implements V3 (all secret injection via wrapping tokens), V8 (OpenBao
//! deploy), V17 (Transit for encryption-at-rest).
//!
//! Four engines:
//! - **DB secrets** — dynamic Supabase/Postgres creds (role-based).
//! - **KV v2** — OAuth refresh tokens (long-lived, per-cred).
//! - **Cubbyhole** — per-exec access tokens (single-use, short TTL).
//! - **Transit** — encryption-at-rest for cached token values.
//!
//! `TokenCache` wraps `glia_cache::Cache` and encrypts values via Transit
//! before storing. TTL is 15min (OAuth access token lifetime).
//!
//! `PredictivePreAuth` pre-mints a wrapping token for a cred before the
//! user prompt returns `AUTH_REQUIRED`, so the sandbox can unwrap without
//! a round-trip to OpenBao.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use glia_cache::Cache;

/// Default TTL for OAuth access tokens cached in Redis (T14: 15 minutes).
pub const TOKEN_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Default TTL for wrapping tokens (single-use, 5 minutes).
pub const WRAPPING_TTL: Duration = Duration::from_secs(5 * 60);

/// Errors from OpenBao operations.
#[derive(Debug, thiserror::Error)]
pub enum BaoError {
    /// HTTP / network error.
    #[error("http: {0}")]
    Http(String),
    /// OpenBao returned an error response.
    #[error("bao: {0}")]
    Api(String),
    /// Secret not found at the given path.
    #[error("not found: {0}")]
    NotFound(String),
    /// JSON (de)serialization failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// Cache error.
    #[error("cache: {0}")]
    Cache(#[from] glia_cache::CacheError),
    /// Encryption / decryption failed.
    #[error("crypto: {0}")]
    Crypto(String),
}

/// Opaque secret data (already unwrapped from a wrapping token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    /// Key-value pairs.
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl Secret {
    /// Build from a key-value map.
    pub fn new(data: serde_json::Map<String, serde_json::Value>) -> Self {
        Self { data }
    }

    /// Build from a single key=value.
    pub fn single(key: &str, value: impl Into<serde_json::Value>) -> Self {
        let mut map = serde_json::Map::new();
        map.insert(key.into(), value.into());
        Self { data: map }
    }

    /// Get a string field.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(|v| v.as_str())
    }
}

/// Dynamic DB credentials (from DB secrets engine).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicCreds {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
}

/// Pluggable OpenBao backend.
#[async_trait]
pub trait OpenBao: Send + Sync {
    /// Unwrap a wrapping token → `Secret`. Single-use.
    async fn unwrap(&self, wrapping_token: &str) -> Result<Secret, BaoError>;

    /// Mint a wrapping token for a secret at `path`. Returns the token.
    async fn mint_wrapping(&self, path: &str, ttl: Duration) -> Result<String, BaoError>;

    /// Read dynamic DB creds for a role (DB secrets engine).
    async fn db_read_creds(&self, role: &str) -> Result<DynamicCreds, BaoError>;

    /// KV v2: read a secret at `path`.
    async fn kv_get(&self, path: &str) -> Result<Secret, BaoError>;

    /// KV v2: write a secret at `path`.
    async fn kv_put(&self, path: &str, secret: &Secret) -> Result<(), BaoError>;

    /// Cubbyhole: store a per-exec secret (single-use token).
    async fn cubbyhole_put(&self, key: &str, secret: &Secret) -> Result<(), BaoError>;

    /// Cubbyhole: read a per-exec secret.
    async fn cubbyhole_get(&self, key: &str) -> Result<Secret, BaoError>;

    /// Transit: encrypt plaintext with the named key.
    async fn transit_encrypt(&self, key_name: &str, plaintext: &[u8]) -> Result<Vec<u8>, BaoError>;

    /// Transit: decrypt ciphertext with the named key.
    async fn transit_decrypt(&self, key_name: &str, ciphertext: &[u8])
    -> Result<Vec<u8>, BaoError>;
}

// ---------------- StubOpenBao ----------------

/// In-memory stub for tests. No network. Secrets stored in `HashMap`.
pub struct StubOpenBao {
    root_token: String,
    kv: Mutex<std::collections::HashMap<String, Secret>>,
    cubbyhole: Mutex<std::collections::HashMap<String, Secret>>,
    db_roles: Mutex<std::collections::HashMap<String, DynamicCreds>>,
}

impl StubOpenBao {
    /// Build a new stub with a given root token and transit key name.
    pub fn new(root_token: impl Into<String>, _transit_key: impl Into<String>) -> Self {
        Self {
            root_token: root_token.into(),
            kv: Mutex::new(std::collections::HashMap::new()),
            cubbyhole: Mutex::new(std::collections::HashMap::new()),
            db_roles: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Pre-load a DB role with creds (for test setup).
    pub async fn set_db_role(&self, role: &str, creds: DynamicCreds) {
        self.db_roles.lock().await.insert(role.into(), creds);
    }

    /// Pre-load a KV secret (for test setup).
    pub async fn set_kv(&self, path: &str, secret: Secret) {
        self.kv.lock().await.insert(path.into(), secret);
    }

    /// Root token (for diagnostics).
    pub fn root_token(&self) -> &str {
        &self.root_token
    }
}

#[async_trait]
impl OpenBao for StubOpenBao {
    async fn unwrap(&self, wrapping_token: &str) -> Result<Secret, BaoError> {
        // Stub: wrapping token encodes the path as `wrap::<path>`.
        let path = wrapping_token
            .strip_prefix("wrap::")
            .ok_or_else(|| BaoError::Api("invalid wrapping token".into()))?;
        // Try KV first, then cubbyhole.
        if let Some(s) = self.kv.lock().await.get(path) {
            return Ok(s.clone());
        }
        if let Some(s) = self.cubbyhole.lock().await.get(path) {
            return Ok(s.clone());
        }
        Err(BaoError::NotFound(format!("wrap target: {}", path)))
    }

    async fn mint_wrapping(&self, path: &str, _ttl: Duration) -> Result<String, BaoError> {
        Ok(format!("wrap::{}", path))
    }

    async fn db_read_creds(&self, role: &str) -> Result<DynamicCreds, BaoError> {
        self.db_roles
            .lock()
            .await
            .get(role)
            .cloned()
            .ok_or_else(|| BaoError::NotFound(format!("db role: {}", role)))
    }

    async fn kv_get(&self, path: &str) -> Result<Secret, BaoError> {
        self.kv
            .lock()
            .await
            .get(path)
            .cloned()
            .ok_or_else(|| BaoError::NotFound(format!("kv: {}", path)))
    }

    async fn kv_put(&self, path: &str, secret: &Secret) -> Result<(), BaoError> {
        self.kv.lock().await.insert(path.into(), secret.clone());
        Ok(())
    }

    async fn cubbyhole_put(&self, key: &str, secret: &Secret) -> Result<(), BaoError> {
        self.cubbyhole
            .lock()
            .await
            .insert(key.into(), secret.clone());
        Ok(())
    }

    async fn cubbyhole_get(&self, key: &str) -> Result<Secret, BaoError> {
        self.cubbyhole
            .lock()
            .await
            .get(key)
            .cloned()
            .ok_or_else(|| BaoError::NotFound(format!("cubbyhole: {}", key)))
    }

    async fn transit_encrypt(
        &self,
        _key_name: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, BaoError> {
        // Stub: "encrypt" = prefix with 0xFF marker + plaintext.
        let mut out = vec![0xFF];
        out.extend_from_slice(plaintext);
        Ok(out)
    }

    async fn transit_decrypt(
        &self,
        _key_name: &str,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, BaoError> {
        // Stub: strip 0xFF marker.
        if ciphertext.is_empty() || ciphertext[0] != 0xFF {
            return Err(BaoError::Crypto("invalid ciphertext".into()));
        }
        Ok(ciphertext[1..].to_vec())
    }
}

// ---------------- HttpOpenBao ----------------

/// HTTP OpenBao client. Talks to the OpenBao REST API.
pub struct HttpOpenBao {
    /// Base URL (e.g., `http://127.0.0.1:8200`).
    pub base_url: String,
    /// Root token — private and zeroized on drop so it never lingers in heap.
    token: Zeroizing<String>,
    client: reqwest::Client,
}

impl HttpOpenBao {
    /// Build a new HTTP client.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: Zeroizing::new(token.into()),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, BaoError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .client
            .get(&url)
            .header("X-Vault-Token", self.token.as_str())
            .send()
            .await
            .map_err(|e| BaoError::Http(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| BaoError::Http(e.to_string()))?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(BaoError::NotFound(path.into()));
        }
        if !status.is_success() {
            return Err(BaoError::Api(format!("{}: {}", status, body)));
        }
        Ok(serde_json::from_str(&body)?)
    }

    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, BaoError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .client
            .post(&url)
            .header("X-Vault-Token", self.token.as_str())
            .json(body)
            .send()
            .await
            .map_err(|e| BaoError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BaoError::Http(e.to_string()))?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(BaoError::NotFound(path.into()));
        }
        if !status.is_success() {
            return Err(BaoError::Api(format!("{}: {}", status, text)));
        }
        if text.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// Extract `data.data` from a KV v2 response.
    fn extract_kv_data(resp: &serde_json::Value) -> Option<Secret> {
        let data = resp.get("data")?.get("data")?.as_object()?;
        Some(Secret { data: data.clone() })
    }
}

#[async_trait]
impl OpenBao for HttpOpenBao {
    async fn unwrap(&self, wrapping_token: &str) -> Result<Secret, BaoError> {
        let body = serde_json::json!({ "token": wrapping_token });
        let resp = self.post_json("/v1/sys/wrapping/unwrap", &body).await?;
        let data = resp
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| BaoError::Api("unwrap: no data".into()))?;
        Ok(Secret { data: data.clone() })
    }

    async fn mint_wrapping(&self, path: &str, ttl: Duration) -> Result<String, BaoError> {
        let body = serde_json::json!({
            "response_wrapping_ttl": ttl.as_secs(),
            "response_wrapping_path": path,
        });
        let resp = self.post_json("/v1/sys/wrapping/wrap", &body).await?;
        resp.get("wrap_info")
            .and_then(|w| w.get("token"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| BaoError::Api("mint_wrapping: no token".into()))
    }

    async fn db_read_creds(&self, role: &str) -> Result<DynamicCreds, BaoError> {
        let resp = self
            .get_json(&format!("/v1/database/creds/{}", role))
            .await?;
        let data = resp
            .get("data")
            .ok_or_else(|| BaoError::Api(format!("db_read_creds: no data for {}", role)))?;
        let username = data
            .get("username")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaoError::Api("db_read_creds: no username".into()))?;
        let password = data
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaoError::Api("db_read_creds: no password".into()))?;
        Ok(DynamicCreds {
            username: username.into(),
            password: password.into(),
        })
    }

    async fn kv_get(&self, path: &str) -> Result<Secret, BaoError> {
        let resp = self
            .get_json(&format!("/v1/{}/data", path.trim_start_matches('/')))
            .await?;
        Self::extract_kv_data(&resp)
            .ok_or_else(|| BaoError::Api(format!("kv_get: no data at {}", path)))
    }

    async fn kv_put(&self, path: &str, secret: &Secret) -> Result<(), BaoError> {
        let body = serde_json::json!({ "data": secret.data });
        self.post_json(&format!("/v1/{}/data", path.trim_start_matches('/')), &body)
            .await?;
        Ok(())
    }

    async fn cubbyhole_put(&self, key: &str, secret: &Secret) -> Result<(), BaoError> {
        let body = serde_json::json!({ "data": secret.data });
        self.post_json(&format!("/v1/cubbyhole/{}", key), &body)
            .await?;
        Ok(())
    }

    async fn cubbyhole_get(&self, key: &str) -> Result<Secret, BaoError> {
        let resp = self.get_json(&format!("/v1/cubbyhole/{}", key)).await?;
        let data = resp
            .get("data")
            .and_then(|d| d.as_object())
            .ok_or_else(|| BaoError::Api("cubbyhole_get: no data".into()))?;
        Ok(Secret { data: data.clone() })
    }

    async fn transit_encrypt(&self, key_name: &str, plaintext: &[u8]) -> Result<Vec<u8>, BaoError> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(plaintext);
        let body = serde_json::json!({ "plaintext": b64 });
        let resp = self
            .post_json(&format!("/v1/transit/encrypt/{}", key_name), &body)
            .await?;
        let ciphertext_b64 = resp
            .get("data")
            .and_then(|d| d.get("ciphertext"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| BaoError::Api("transit_encrypt: no ciphertext".into()))?;
        // OpenBao returns "vault:v1:..." prefixed strings; for raw bytes we
        // decode the base64 portion after the prefix. If there's no prefix,
        // decode the whole thing.
        let raw = ciphertext_b64
            .splitn(2, ':')
            .last()
            .unwrap_or(ciphertext_b64);
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|e| BaoError::Crypto(e.to_string()))
    }

    async fn transit_decrypt(
        &self,
        key_name: &str,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, BaoError> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(ciphertext);
        // OpenBao expects the "vault:v1:..." prefix; we prepend a placeholder.
        let ciphertext_str = format!("vault:v1:{}", b64);
        let body = serde_json::json!({ "ciphertext": ciphertext_str });
        let resp = self
            .post_json(&format!("/v1/transit/decrypt/{}", key_name), &body)
            .await?;
        let plaintext_b64 = resp
            .get("data")
            .and_then(|d| d.get("plaintext"))
            .and_then(|p| p.as_str())
            .ok_or_else(|| BaoError::Api("transit_decrypt: no plaintext".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(plaintext_b64)
            .map_err(|e| BaoError::Crypto(e.to_string()))
    }
}

// ---------------- TokenCache ----------------

/// Redis-backed cache for OAuth access tokens, encrypted via Transit.
///
/// Flow:
/// 1. `put(cred_id, token)` → transit_encrypt(token) → cache.put(key, ciphertext, 15min)
/// 2. `get(cred_id)` → cache.get(key) → transit_decrypt(ciphertext) → token
///
/// Prevents redundant OAuth exchange calls across parallel `glia_action`
/// executions (T14).
pub struct TokenCache {
    cache: Arc<dyn Cache>,
    bao: Arc<dyn OpenBao>,
    transit_key: String,
}

impl TokenCache {
    /// Build a new token cache.
    pub fn new(
        cache: Arc<dyn Cache>,
        bao: Arc<dyn OpenBao>,
        transit_key: impl Into<String>,
    ) -> Self {
        Self {
            cache,
            bao,
            transit_key: transit_key.into(),
        }
    }

    /// Cache an OAuth access token for a cred. Encrypts via Transit.
    pub async fn put(&self, cred_id: &str, token: &str) -> Result<(), BaoError> {
        let ciphertext = self
            .bao
            .transit_encrypt(&self.transit_key, token.as_bytes())
            .await?;
        let key = glia_cache::keys::oauth_access_token(cred_id);
        self.cache
            .put_bytes(&key, &ciphertext, TOKEN_CACHE_TTL)
            .await?;
        Ok(())
    }

    /// Retrieve a cached OAuth access token. Returns `None` if missing
    /// or expired.
    pub async fn get(&self, cred_id: &str) -> Result<Option<String>, BaoError> {
        let key = glia_cache::keys::oauth_access_token(cred_id);
        let ciphertext = match self.cache.get_bytes(&key).await? {
            Some(c) => c,
            None => return Ok(None),
        };
        let plaintext = self
            .bao
            .transit_decrypt(&self.transit_key, &ciphertext)
            .await?;
        let token = String::from_utf8(plaintext).map_err(|e| BaoError::Crypto(e.to_string()))?;
        Ok(Some(token))
    }

    /// Delete a cached token (e.g., on 401 from provider).
    pub async fn delete(&self, cred_id: &str) -> Result<(), BaoError> {
        let key = glia_cache::keys::oauth_access_token(cred_id);
        self.cache.delete(&key).await?;
        Ok(())
    }
}

// ---------------- PredictivePreAuth ----------------

/// Predictive Pre-Auth: pre-mint wrapping tokens for creds that a remote
/// intent will likely need, before the user prompt returns `AUTH_REQUIRED`.
///
/// Given a list of cred ids (from `glia_action`'s `dep_check`), pre-mint
/// a wrapping token for each and cache the unwrapped secret in Redis
/// (encrypted via Transit) for the sandbox to consume.
///
/// V17: reduces perceived latency by doing the OpenBao round-trip
/// concurrently with the user's prompt processing.
pub struct PredictivePreAuth {
    bao: Arc<dyn OpenBao>,
    token_cache: Arc<TokenCache>,
}

/// Result of a pre-auth attempt for a single cred.
#[derive(Debug, Clone)]
pub struct PreAuthResult {
    /// Cred id.
    pub cred_id: String,
    /// Wrapping token (for the sandbox to unwrap, or for fallback).
    pub wrapping_token: String,
    /// Whether the token was successfully cached.
    pub cached: bool,
}

impl PredictivePreAuth {
    /// Build a new pre-auth orchestrator.
    pub fn new(bao: Arc<dyn OpenBao>, token_cache: Arc<TokenCache>) -> Self {
        Self { bao, token_cache }
    }

    /// Pre-mint wrapping tokens for a list of cred ids.
    ///
    /// For each cred:
    /// 1. Mint a wrapping token (5min TTL).
    /// 2. Unwrap to get the secret.
    /// 3. Extract the access token field and cache it via `TokenCache`.
    ///
    /// Errors for individual creds don't fail the whole batch — the
    /// caller can retry on-demand.
    pub async fn pre_auth(&self, cred_ids: &[String]) -> Vec<PreAuthResult> {
        let mut results = Vec::with_capacity(cred_ids.len());
        for cred_id in cred_ids {
            let result = self.pre_auth_one(cred_id).await;
            results.push(result);
        }
        results
    }

    /// Pre-auth a single cred.
    async fn pre_auth_one(&self, cred_id: &str) -> PreAuthResult {
        let path = format!("secret/data/oauth/{}", cred_id);
        let wrapping_token = match self.bao.mint_wrapping(&path, WRAPPING_TTL).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(cred = cred_id, err = %e, "pre_auth: mint_wrapping failed");
                return PreAuthResult {
                    cred_id: cred_id.into(),
                    wrapping_token: String::new(),
                    cached: false,
                };
            }
        };
        let secret = match self.bao.unwrap(&wrapping_token).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(cred = cred_id, err = %e, "pre_auth: unwrap failed");
                return PreAuthResult {
                    cred_id: cred_id.into(),
                    wrapping_token,
                    cached: false,
                };
            }
        };
        let token = match secret.get_str("access_token") {
            Some(t) => t.to_string(),
            None => {
                tracing::warn!(cred = cred_id, "pre_auth: no access_token in secret");
                return PreAuthResult {
                    cred_id: cred_id.into(),
                    wrapping_token,
                    cached: false,
                };
            }
        };
        match self.token_cache.put(cred_id, &token).await {
            Ok(()) => PreAuthResult {
                cred_id: cred_id.into(),
                wrapping_token,
                cached: true,
            },
            Err(e) => {
                tracing::warn!(cred = cred_id, err = %e, "pre_auth: cache put failed");
                PreAuthResult {
                    cred_id: cred_id.into(),
                    wrapping_token,
                    cached: false,
                }
            }
        }
    }
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use glia_cache::InMemoryCache;

    fn stub() -> StubOpenBao {
        StubOpenBao::new("root", "glia-transit")
    }

    fn secret(_path: &str, token: &str) -> Secret {
        let mut s = Secret::new(serde_json::Map::new());
        s.data.insert("access_token".into(), token.into());
        s
    }

    #[tokio::test]
    async fn stub_kv_round_trip() {
        let bao = stub();
        let s = secret("oauth/linear", "tok_123");
        bao.kv_put("secret/data/oauth/linear", &s).await.unwrap();
        let back = bao.kv_get("secret/data/oauth/linear").await.unwrap();
        assert_eq!(back.get_str("access_token"), Some("tok_123"));
    }

    #[tokio::test]
    async fn stub_cubbyhole_round_trip() {
        let bao = stub();
        let s = secret("exec", "tok_exec");
        bao.cubbyhole_put("exec-1", &s).await.unwrap();
        let back = bao.cubbyhole_get("exec-1").await.unwrap();
        assert_eq!(back.get_str("access_token"), Some("tok_exec"));
    }

    #[tokio::test]
    async fn stub_db_read_creds() {
        let bao = stub();
        bao.set_db_role(
            "supabase-readonly",
            DynamicCreds {
                username: "glia_user".into(),
                password: "secret".into(),
            },
        )
        .await;
        let creds = bao.db_read_creds("supabase-readonly").await.unwrap();
        assert_eq!(creds.username, "glia_user");
        assert_eq!(creds.password, "secret");
    }

    #[tokio::test]
    async fn stub_db_read_creds_not_found() {
        let bao = stub();
        let err = bao.db_read_creds("nope").await.unwrap_err();
        assert!(matches!(err, BaoError::NotFound(_)));
    }

    #[tokio::test]
    async fn stub_transit_encrypt_decrypt_round_trip() {
        let bao = stub();
        let plaintext = b"hello world";
        let ct = bao
            .transit_encrypt("glia-transit", plaintext)
            .await
            .unwrap();
        assert_eq!(ct[0], 0xFF);
        let pt = bao.transit_decrypt("glia-transit", &ct).await.unwrap();
        assert_eq!(pt, plaintext);
    }

    #[tokio::test]
    async fn stub_transit_decrypt_invalid() {
        let bao = stub();
        let err = bao
            .transit_decrypt("glia-transit", &[0x00, 0x01])
            .await
            .unwrap_err();
        assert!(matches!(err, BaoError::Crypto(_)));
    }

    #[tokio::test]
    async fn stub_mint_and_unwrap() {
        let bao = stub();
        let s = secret("linear", "tok_abc");
        bao.kv_put("secret/data/oauth/linear", &s).await.unwrap();
        let wrap = bao
            .mint_wrapping("secret/data/oauth/linear", WRAPPING_TTL)
            .await
            .unwrap();
        assert!(wrap.starts_with("wrap::"));
        let unwrapped = bao.unwrap(&wrap).await.unwrap();
        assert_eq!(unwrapped.get_str("access_token"), Some("tok_abc"));
    }

    #[tokio::test]
    async fn token_cache_put_get_round_trip() {
        let bao = Arc::new(stub());
        let cache = Arc::new(InMemoryCache::new());
        let tc = TokenCache::new(cache, bao, "glia-transit");
        tc.put("linear", "tok_123").await.unwrap();
        let back = tc.get("linear").await.unwrap().unwrap();
        assert_eq!(back, "tok_123");
    }

    #[tokio::test]
    async fn token_cache_get_missing() {
        let bao = Arc::new(stub());
        let cache = Arc::new(InMemoryCache::new());
        let tc = TokenCache::new(cache, bao, "glia-transit");
        assert!(tc.get("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn token_cache_delete() {
        let bao = Arc::new(stub());
        let cache = Arc::new(InMemoryCache::new());
        let tc = TokenCache::new(cache, bao, "glia-transit");
        tc.put("linear", "tok_123").await.unwrap();
        tc.delete("linear").await.unwrap();
        assert!(tc.get("linear").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn token_cache_values_are_encrypted_at_rest() {
        // The stub "encrypts" by prepending a 0xFF marker. Real HttpOpenBao
        // uses Transit. This test verifies the marker is present, proving
        // the TokenCache invoked the bao backend rather than storing raw.
        let bao = Arc::new(stub());
        let cache = Arc::new(InMemoryCache::new());
        let tc = TokenCache::new(cache.clone(), bao, "glia-transit");
        tc.put("linear", "sensitive_token").await.unwrap();
        let key = glia_cache::keys::oauth_access_token("linear");
        let raw = cache.get_bytes(&key).await.unwrap().unwrap();
        assert_eq!(raw[0], 0xFF, "transit marker missing");
    }

    #[tokio::test]
    async fn pre_auth_caches_token() {
        let bao = Arc::new(stub());
        // Pre-load the KV secret that pre_auth will unwrap.
        let s = secret("linear", "tok_pre");
        bao.kv_put("secret/data/oauth/linear", &s).await.unwrap();
        let cache = Arc::new(InMemoryCache::new());
        let tc = Arc::new(TokenCache::new(cache.clone(), bao.clone(), "glia-transit"));
        let ppa = PredictivePreAuth::new(bao, tc);
        let results = ppa.pre_auth(&["linear".into()]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].cached);
        assert!(!results[0].wrapping_token.is_empty());
        // Token is now in the cache.
        let key = glia_cache::keys::oauth_access_token("linear");
        assert!(cache.get_bytes(&key).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn pre_auth_handles_missing_secret() {
        let bao = Arc::new(stub());
        let cache = Arc::new(InMemoryCache::new());
        let tc = Arc::new(TokenCache::new(cache.clone(), bao.clone(), "glia-transit"));
        let ppa = PredictivePreAuth::new(bao, tc);
        let results = ppa.pre_auth(&["nonexistent".into()]).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].cached);
    }

    #[tokio::test]
    async fn pre_auth_batch_partial_success() {
        let bao = Arc::new(stub());
        // Only "good" has a secret.
        let s = secret("good", "tok_good");
        bao.kv_put("secret/data/oauth/good", &s).await.unwrap();
        let cache = Arc::new(InMemoryCache::new());
        let tc = Arc::new(TokenCache::new(cache.clone(), bao.clone(), "glia-transit"));
        let ppa = PredictivePreAuth::new(bao, tc);
        let results = ppa.pre_auth(&["good".into(), "bad".into()]).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.cred_id == "good" && r.cached));
        assert!(results.iter().any(|r| r.cred_id == "bad" && !r.cached));
    }

    #[test]
    fn ttl_constants() {
        assert_eq!(TOKEN_CACHE_TTL, Duration::from_secs(900));
        assert_eq!(WRAPPING_TTL, Duration::from_secs(300));
    }

    #[test]
    fn secret_get_str() {
        let s = Secret::single("token", "abc");
        assert_eq!(s.get_str("token"), Some("abc"));
        assert_eq!(s.get_str("missing"), None);
    }

    #[tokio::test]
    async fn kv_put_empty_key() {
        let bao = stub();
        let s = Secret::single("K", "V");
        bao.kv_put("", &s).await.unwrap();
        let got = bao.kv_get("").await.unwrap();
        assert_eq!(got.get_str("K"), Some("V"));
    }

    #[tokio::test]
    async fn kv_put_empty_secret() {
        let bao = stub();
        let empty = Secret::new(serde_json::Map::new());
        bao.kv_put("empty-secret", &empty).await.unwrap();
        let got = bao.kv_get("empty-secret").await.unwrap();
        assert!(got.data.is_empty());
    }

    #[tokio::test]
    async fn kv_get_missing_returns_not_found() {
        let bao = stub();
        let result = bao.kv_get("never-set").await;
        assert!(matches!(result, Err(BaoError::NotFound(_))));
    }

    #[tokio::test]
    async fn cubbyhole_get_missing_returns_not_found() {
        let bao = stub();
        let result = bao.cubbyhole_get("nonexistent-token").await;
        assert!(matches!(result, Err(BaoError::NotFound(_))));
    }

    #[tokio::test]
    async fn transit_encrypt_empty_plaintext() {
        let bao = stub();
        let encrypted = bao.transit_encrypt("glia-transit", b"").await.unwrap();
        assert!(!encrypted.is_empty());
        let decrypted = bao.transit_decrypt("glia-transit", &encrypted).await.unwrap();
        assert!(decrypted.is_empty());
    }

    #[tokio::test]
    async fn stub_unwrap_missing_prefix_returns_api_error() {
        let bao = stub();
        let result = bao.unwrap("garbage-no-prefix").await;
        assert!(result.is_err());
    }

    #[test]
    fn bao_error_display() {
        let e = BaoError::NotFound("x".into());
        assert!(format!("{}", e).contains("not found"));
        let e = BaoError::Crypto("x".into());
        assert!(format!("{}", e).contains("crypto"));
        let e = BaoError::Api("x".into());
        assert!(format!("{}", e).contains("bao"));
    }

    #[test]
    fn secret_single_and_new() {
        let s = Secret::single("k", "v");
        assert_eq!(s.get_str("k"), Some("v"));
        let empty = Secret::new(serde_json::Map::new());
        assert!(empty.data.is_empty());
    }

    #[tokio::test]
    async fn pre_auth_secret_without_access_token_field() {
        let bao = Arc::new(stub());
        let s = Secret::single("other_field", "val");
        bao.kv_put("secret/data/oauth/no-at", &s).await.unwrap();
        let cache = Arc::new(InMemoryCache::new());
        let tc = Arc::new(TokenCache::new(cache.clone(), bao.clone(), "glia-transit"));
        let ppa = PredictivePreAuth::new(bao, tc);
        let results = ppa.pre_auth(&["no-at".into()]).await;
        assert!(!results[0].cached);
    }
}
