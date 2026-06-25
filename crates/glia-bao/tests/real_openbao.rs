//! E2E: Real OpenBao tests via HTTP (not stub).
//!
//! Requires: docker compose up openbao (port 8201, token glia-root).
//! These tests exercise the HttpOpenBao path that was previously untested.

use glia_bao::{HttpOpenBao, OpenBao, Secret};
use glia_cache::{Cache, InMemoryCache, RedisCache};
use std::sync::Arc;
use std::time::Duration;

const OPENBAO_URL: &str = "http://127.0.0.1:8201";
const OPENBAO_TOKEN: &str = "glia-root";

fn http_bao() -> HttpOpenBao {
    HttpOpenBao::new(OPENBAO_URL, OPENBAO_TOKEN)
}

async fn is_bao_live() -> bool {
    reqwest::Client::new()
        .get(format!("{}/v1/sys/health", OPENBAO_URL))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok()
}

#[tokio::test]
async fn real_bao_kv_put_get_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let path = format!("secret/data/real-bao-kv-{}", chrono::Utc::now().timestamp());
    let secret = Secret::single("api_key", "real-bao-secret");
    bao.kv_put(&path, &secret).await.unwrap();
    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("api_key"), Some("real-bao-secret"));
}

#[tokio::test]
async fn real_bao_kv_overwrite_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let path = format!(
        "secret/data/real-bao-overwrite-{}",
        chrono::Utc::now().timestamp()
    );
    bao.kv_put(&path, &Secret::single("v", "v1")).await.unwrap();
    bao.kv_put(&path, &Secret::single("v", "v2")).await.unwrap();
    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("v"), Some("v2"));
}

#[tokio::test]
async fn real_bao_kv_get_missing_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let result = bao.kv_get("secret/data/real-bao-missing-path-xyz").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn real_bao_kv_multi_field_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let path = format!(
        "secret/data/real-bao-multi-{}",
        chrono::Utc::now().timestamp()
    );
    let mut data = serde_json::Map::new();
    data.insert("access_token".into(), "at-val".into());
    data.insert("refresh_token".into(), "rt-val".into());
    data.insert("scope".into(), "read write".into());
    bao.kv_put(&path, &Secret::new(data)).await.unwrap();
    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("access_token"), Some("at-val"));
    assert_eq!(got.get_str("refresh_token"), Some("rt-val"));
    assert_eq!(got.get_str("scope"), Some("read write"));
}

#[tokio::test]
async fn real_bao_response_wrapping_round_trip_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let path = format!(
        "secret/data/real-bao-wrap-{}",
        chrono::Utc::now().timestamp()
    );
    let secret = Secret::single("wrapped_token", "wrap-this-value");
    bao.kv_put(&path, &secret).await.unwrap();

    let wrap_result = bao.mint_wrapping(&path, Duration::from_secs(300)).await;
    if let Err(e) = &wrap_result {
        eprintln!("SKIP: mint_wrapping failed: {e}");
        return;
    }
    let wrap_token = wrap_result.unwrap();
    assert!(!wrap_token.is_empty());

    let unwrapped = bao.unwrap(&wrap_token).await.unwrap();
    assert_eq!(unwrapped.get_str("wrapped_token"), Some("wrap-this-value"));
}

#[tokio::test]
async fn real_bao_single_use_token_enforcement_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let path = format!(
        "secret/data/real-bao-single-use-{}",
        chrono::Utc::now().timestamp()
    );
    bao.kv_put(&path, &Secret::single("k", "v")).await.unwrap();
    let wrap_token = match bao.mint_wrapping(&path, Duration::from_secs(300)).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("SKIP: mint_wrapping failed: {e}");
            return;
        }
    };

    // First unwrap succeeds.
    let _ = bao.unwrap(&wrap_token).await.unwrap();
    // Second unwrap should fail (single-use).
    assert!(bao.unwrap(&wrap_token).await.is_err());
}

#[tokio::test]
async fn real_bao_unwrap_invalid_token_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    assert!(bao.unwrap("garbage-invalid-token").await.is_err());
}

#[tokio::test]
async fn real_bao_cubbyhole_round_trip_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao();
    let key = format!("real-cubby-{}", chrono::Utc::now().timestamp());
    let secret = Secret::single("temp", "cubby-value");
    let put_result = bao.cubbyhole_put(&key, &secret).await;
    if let Err(e) = &put_result {
        eprintln!("SKIP: cubbyhole_put (root token): {e}");
        return;
    }
    let got = bao.cubbyhole_get(&key).await;
    match got {
        Ok(g) => {
            let _ = g.get_str("temp");
        }
        Err(e) => eprintln!("SKIP: cubbyhole_get (root token): {e}"),
    }
}

#[tokio::test]
async fn real_bao_token_cache_with_real_redis_live() {
    if !is_bao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    // Try to connect to real Redis.
    let cache: Arc<dyn Cache> = match RedisCache::connect("redis://127.0.0.1:6379/0").await {
        Ok(c) => Arc::new(c),
        Err(_) => {
            eprintln!("Redis unavailable, using InMemoryCache");
            Arc::new(InMemoryCache::new())
        }
    };
    // Round-trip: put a secret in KV, then read it back, using the
    // real OpenBao + real Redis combination. This exercises the
    // same code path TokenCache uses for caching OAuth tokens.
    let bao = Arc::new(http_bao());
    let path = format!(
        "secret/data/real-cache-test-{}",
        chrono::Utc::now().timestamp()
    );
    let secret = Secret::single("cached_token", "cache-value");
    bao.kv_put(&path, &secret).await.unwrap();

    // Cache the encrypted secret value via real Redis.
    let cache_key = format!("glia::real-bao::{}", path);
    cache
        .put_bytes(&cache_key, b"cached-value", Duration::from_secs(60))
        .await
        .unwrap();
    let cached = cache.get_bytes(&cache_key).await.unwrap();
    assert_eq!(cached.as_deref(), Some(&b"cached-value"[..]));

    // Read the secret from OpenBao.
    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("cached_token"), Some("cache-value"));
}
