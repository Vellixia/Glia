//! E2E: Redis live cache operations.
//! Requires: docker compose up redis (port 6379).

mod common;

use common::{redis_live, REDIS_URL};
use glia_cache::{Cache, RedisCache};
use std::time::Duration;

#[tokio::test]
async fn redis_health_check() {
    if !redis_live().await {
        eprintln!("SKIP: no redis at {}", REDIS_URL);
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    cache.ping().await.unwrap();
}

#[tokio::test]
async fn redis_put_get_delete_round_trip_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    let key = format!("glia::e2e::{}", chrono::Utc::now().timestamp());

    cache.put_bytes(&key, b"e2e-value", Duration::from_secs(60)).await.unwrap();
    let got = cache.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"e2e-value"[..]));

    cache.delete(&key).await.unwrap();
    let after_delete = cache.get_bytes(&key).await.unwrap();
    assert!(after_delete.is_none());
}

#[tokio::test]
async fn redis_ttl_expiry_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    let key = format!("glia::e2e::ttl-{}", chrono::Utc::now().timestamp());

    // Put with 1-second TTL (minimum for Redis).
    cache.put_bytes(&key, b"expires-soon", Duration::from_secs(1)).await.unwrap();
    assert!(cache.get_bytes(&key).await.unwrap().is_some());

    // Wait for expiry.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(cache.get_bytes(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn redis_missing_key_returns_none_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    let result = cache.get_bytes("glia::e2e::nonexistent-xyz").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn redis_delete_missing_key_idempotent_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    // Deleting a non-existent key should succeed (idempotent).
    cache.delete("glia::e2e::never-existed").await.unwrap();
}

#[tokio::test]
async fn redis_large_value_round_trip_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    let key = format!("glia::e2e::large-{}", chrono::Utc::now().timestamp());
    let large_value = vec![0xABu8; 100_000]; // 100KB

    cache.put_bytes(&key, &large_value, Duration::from_secs(60)).await.unwrap();
    let got = cache.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(large_value.as_slice()));
    cache.delete(&key).await.unwrap();
}

#[tokio::test]
async fn redis_unicode_key_round_trip_live() {
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = RedisCache::connect(REDIS_URL).await.unwrap();
    let key = format!("glia::e2e::héllo::日本語-{}", chrono::Utc::now().timestamp());

    cache.put_bytes(&key, b"unicode-key-value", Duration::from_secs(60)).await.unwrap();
    let got = cache.get_bytes(&key).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"unicode-key-value"[..]));
    cache.delete(&key).await.unwrap();
}

#[tokio::test]
async fn redis_concurrent_writes_live() {
    use std::sync::Arc;
    if !redis_live().await {
        eprintln!("SKIP: no redis");
        return;
    }
    let cache = Arc::new(RedisCache::connect(REDIS_URL).await.unwrap());
    let mut handles = Vec::new();
    for i in 0..10 {
        let c = cache.clone();
        handles.push(tokio::spawn(async move {
            let key = format!("glia::e2e::conc-{}-{}", chrono::Utc::now().timestamp(), i);
            c.put_bytes(&key, format!("v{i}").as_bytes(), Duration::from_secs(30))
                .await
                .unwrap();
            let got = c.get_bytes(&key).await.unwrap();
            assert!(got.is_some());
            c.delete(&key).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}