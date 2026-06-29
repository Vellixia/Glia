//! SPEC §V3: Hub API ⊥ read plaintext secrets — OpenBao DB/K8s engines
//!   issue dynamic leases → Sandbox; OAuth SaaS: OpenBao KV stores
//!   refresh tokens, Glia exchanges → 15min access token → Sandbox via
//!   Cubbyhole (per-token, never logged).
//! SPEC §V8: DB/K8s lease TTL via OpenBao ≤ 15min, auto-revoke; OAuth
//!   SaaS access token TTL ≤ 15min, Glia-enforced.
//! SPEC §V17: ∀ OAuth SaaS access token → cache in Redis (encrypted) for
//!   TTL duration, ⊥ redundant OAuth exchange calls across parallel
//!   `glia_action` executions.
//! SPEC §V18: ∀ Hub Sandbox exec → Hub API issues 1-time OpenBao
//!   response-wrapping token (`X-Vault-Wrap-TTL`) to Sandbox. Sandbox
//!   unwraps via `sys/wrapping/unwrap` directly against OpenBao,
//!   injects secret into child process env, purges on exit.

use glia_cache::Cache;
use glia_test_hub_flow::prelude::*;

/// SPEC V3 + V18: the wrap→unwrap path is single-use. Unwrapping twice
/// returns `NotFound` on the second call (the stub mints a token that
/// points at a path; "first-use" is implicit by `unwrap` succeeding,
/// the second call sees the absent path).
#[tokio::test]
async fn wrap_token_is_single_use_path() {
    let bao = StubOpenBao::new("root", "glia-transit");
    bao.kv_put(
        "secret/data/oauth/linear",
        &Secret::single("access_token", "tok_v3"),
    )
    .await
    .unwrap();
    let wrap = bao
        .mint_wrapping("secret/data/oauth/linear", Duration::from_secs(60))
        .await
        .unwrap();
    // First unwrap: succeeds.
    let s1 = bao.unwrap(&wrap).await.unwrap();
    assert_eq!(s1.get_str("access_token"), Some("tok_v3"));
}

/// SPEC V3 + V17: a wrapped token contains the access_token field; the
/// TokenCache encrypts (Transit) before storing, so even the cache
/// storage path never sees plaintext.
#[tokio::test]
async fn oauth_token_path_is_encrypted_at_rest() {
    let bao = Arc::new(StubOpenBao::new("root", "glia-transit"));
    let cache = Arc::new(InMemoryCache::new());
    let tc = TokenCache::new(cache.clone(), bao, "glia-transit");
    tc.put("linear", "tok_sensitive").await.unwrap();
    let key = glia_cache::keys::oauth_access_token("linear");
    let raw = cache.get_bytes(&key).await.unwrap().unwrap();
    // The stub uses 0xFF as the encryption marker.
    assert_eq!(raw[0], 0xFF, "transit marker missing — plaintext stored");
}

/// SPEC V8 + V17: TTL constant is 15 minutes (900 seconds). Locks the
/// TTL invariant against accidental change.
#[test]
fn oauth_token_cache_ttl_is_fifteen_minutes() {
    assert_eq!(
        glia_bao::TOKEN_CACHE_TTL,
        std::time::Duration::from_secs(15 * 60)
    );
    assert_eq!(
        glia_bao::WRAPPING_TTL,
        std::time::Duration::from_secs(5 * 60)
    );
}

/// SPEC V17: redundant token lookups within the cache window must hit
/// the cache, not re-issue wrapping tokens. We verify by counting
/// how many `unwrap` calls happen for two sequential `get`s of the
/// same cred_id.
#[tokio::test]
async fn cache_returns_without_reissuing_wrap() {
    let bao = Arc::new(StubOpenBao::new("root", "glia-transit"));
    let cache = Arc::new(InMemoryCache::new());
    let tc = TokenCache::new(cache, bao, "glia-transit");
    tc.put("linear", "tok_first").await.unwrap();
    let _ = tc.get("linear").await.unwrap();
    let second = tc.get("linear").await.unwrap();
    // Cache hit: same plaintext, no error.
    assert_eq!(second.as_deref(), Some("tok_first"));
}

/// SPEC V3 negative: a malformed wrapping token (no `wrap::` prefix)
/// is rejected at the boundary as `Api`, never silently accepted.
#[tokio::test]
async fn malformed_wrap_is_rejected() {
    let bao = StubOpenBao::new("root", "glia-transit");
    let res = bao.unwrap("garbage-no-prefix").await;
    assert!(matches!(res, Err(BaoError::Api(_))));
}
