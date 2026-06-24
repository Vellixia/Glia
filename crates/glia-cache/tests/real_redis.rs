//! E2E: Real Redis tests (replaces the mock-based integration tests).
//!
//! Requires: docker compose up redis (port 6379).

use glia_cache::{Cache, RedisCache};
use std::time::Duration;

const REDIS_URL: &str = "redis://127.0.0.1:6379/0";

async fn redis() -> Option<RedisCache> {
    RedisCache::connect(REDIS_URL).await.ok()
}

#[tokio::test]
async fn real_redis_ping() {
    let Some(c) = redis().await else {
        eprintln!("SKIP: no redis");
        return;
    };
    c.ping().await.unwrap();
}

#[tokio::test]
async fn real_redis_put_get_delete_round_trip() {
    let Some(c) = redis().await else { return };
    let key = format!("glia::real::{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    c.put_bytes(&key, b"hello-real-redis", Duration::from_secs(60))
        .await
        .unwrap();
    let got = c.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"hello-real-redis"[..]));
    c.delete(&key).await.unwrap();
    assert!(c.get_bytes(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn real_redis_ttl_expiry() {
    let Some(c) = redis().await else { return };
    let key = format!("glia::ttl::{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    c.put_bytes(&key, b"expires", Duration::from_secs(1))
        .await
        .unwrap();
    assert!(c.get_bytes(&key).await.unwrap().is_some());
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(c.get_bytes(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn real_redis_missing_key_returns_none() {
    let Some(c) = redis().await else { return };
    let result = c.get_bytes("glia::nonexistent-xyz-key-12345").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn real_redis_delete_missing_idempotent() {
    let Some(c) = redis().await else { return };
    c.delete("glia::never-existed-xyz").await.unwrap();
}

#[tokio::test]
async fn real_redis_large_value_100kb() {
    let Some(c) = redis().await else { return };
    let key = format!("glia::large::{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let val = vec![0xCDu8; 100_000];
    c.put_bytes(&key, &val, Duration::from_secs(60))
        .await
        .unwrap();
    let got = c.get_bytes(&key).await.unwrap();
    assert_eq!(got.unwrap(), val);
    c.delete(&key).await.unwrap();
}

#[tokio::test]
async fn real_redis_unicode_key() {
    let Some(c) = redis().await else { return };
    let key = format!(
        "glia::unicode::héllo::日本語::{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    c.put_bytes(&key, b"unicode-val", Duration::from_secs(60))
        .await
        .unwrap();
    let got = c.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"unicode-val"[..]));
    c.delete(&key).await.unwrap();
}

#[tokio::test]
async fn real_redis_concurrent_writes() {
    use std::sync::Arc;
    let c = match redis().await {
        Some(c) => Arc::new(c),
        None => return,
    };
    let mut handles = Vec::new();
    for i in 0..20 {
        let c = c.clone();
        handles.push(tokio::spawn(async move {
            let key = format!("glia::conc::{}::{}", i, chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
            c.put_bytes(&key, format!("v{i}").as_bytes(), Duration::from_secs(30))
                .await
                .unwrap();
            c.delete(&key).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn real_redis_overwrite_same_key() {
    let Some(c) = redis().await else { return };
    let key = format!("glia::overwrite::{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    c.put_bytes(&key, b"v1", Duration::from_secs(60))
        .await
        .unwrap();
    c.put_bytes(&key, b"v2", Duration::from_secs(60))
        .await
        .unwrap();
    let got = c.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"v2"[..]));
    c.delete(&key).await.unwrap();
}

#[tokio::test]
async fn real_redis_empty_value() {
    let Some(c) = redis().await else { return };
    let key = format!("glia::empty::{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    c.put_bytes(&key, b"", Duration::from_secs(60))
        .await
        .unwrap();
    let got = c.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b""[..]));
    c.delete(&key).await.unwrap();
}