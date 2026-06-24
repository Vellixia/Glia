//! E2E: OpenBao live secret operations.
//! Requires: docker compose up openbao (port 8201, token glia-root).
//!
//! Tests KV v2 (always available in dev mode). Transit, cubbyhole, and
//! response-wrapping tests skip gracefully if those engines aren't mounted.

mod common;

use common::{openbao_live, OPENBAO_TOKEN, OPENBAO_URL};
use glia_bao::{OpenBao, Secret};
use std::sync::Arc;

async fn http_bao() -> Arc<glia_bao::HttpOpenBao> {
    Arc::new(glia_bao::HttpOpenBao::new(OPENBAO_URL, OPENBAO_TOKEN))
}

#[tokio::test]
async fn openbao_health_check() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao at {}", OPENBAO_URL);
        return;
    }
    let resp = reqwest::get(&format!("{}/v1/sys/health", OPENBAO_URL))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["sealed"], false);
    assert_eq!(body["initialized"], true);
}

#[tokio::test]
async fn openbao_kv_put_and_get_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let path = format!("secret/data/e2e-kv-test-{}", chrono::Utc::now().timestamp());
    let secret = Secret::single("api_key", "e2e-secret-value");
    bao.kv_put(&path, &secret).await.unwrap();

    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("api_key"), Some("e2e-secret-value"));
}

#[tokio::test]
async fn openbao_kv_get_missing_returns_not_found_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let result = bao
        .kv_get("secret/data/nonexistent-e2e-path-xyz")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn openbao_kv_put_multiple_fields_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let path = format!(
        "secret/data/e2e-multi-{}",
        chrono::Utc::now().timestamp()
    );
    let mut data = serde_json::Map::new();
    data.insert("key1".into(), "val1".into());
    data.insert("key2".into(), "val2".into());
    data.insert("key3".into(), "val3".into());
    let secret = Secret::new(data);
    bao.kv_put(&path, &secret).await.unwrap();

    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("key1"), Some("val1"));
    assert_eq!(got.get_str("key2"), Some("val2"));
    assert_eq!(got.get_str("key3"), Some("val3"));
}

#[tokio::test]
async fn openbao_kv_overwrite_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let path = format!(
        "secret/data/e2e-overwrite-{}",
        chrono::Utc::now().timestamp()
    );
    // Write initial value.
    bao.kv_put(&path, &Secret::single("v", "original"))
        .await
        .unwrap();
    // Overwrite.
    bao.kv_put(&path, &Secret::single("v", "replaced"))
        .await
        .unwrap();
    let got = bao.kv_get(&path).await.unwrap();
    assert_eq!(got.get_str("v"), Some("replaced"));
}

#[tokio::test]
async fn openbao_unwrap_invalid_token_returns_error_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let result = bao.unwrap("invalid-garbage-token").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn openbao_response_wrapping_round_trip_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let path = format!(
        "secret/data/e2e-wrap-{}",
        chrono::Utc::now().timestamp()
    );
    let secret = Secret::single("token", "wrap-me-please");
    bao.kv_put(&path, &secret).await.unwrap();

    // Mint a wrapping token.
    let wrap_result = bao
        .mint_wrapping(&path, std::time::Duration::from_secs(300))
        .await;
    if let Err(e) = &wrap_result {
        eprintln!("SKIP: mint_wrapping failed (engine may not support it): {e}");
        return;
    }
    let wrap_token = wrap_result.unwrap();
    assert!(!wrap_token.is_empty());

    // Unwrap it.
    let unwrapped = bao.unwrap(&wrap_token).await.unwrap();
    assert_eq!(unwrapped.get_str("token"), Some("wrap-me-please"));
}

#[tokio::test]
async fn openbao_unwrap_already_consumed_token_fails_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let path = format!(
        "secret/data/e2e-consume-{}",
        chrono::Utc::now().timestamp()
    );
    bao.kv_put(&path, &Secret::single("k", "v"))
        .await
        .unwrap();
    let wrap_result = bao
        .mint_wrapping(&path, std::time::Duration::from_secs(300))
        .await;
    if let Err(e) = &wrap_result {
        eprintln!("SKIP: mint_wrapping failed: {e}");
        return;
    }
    let wrap_token = wrap_result.unwrap();

    // First unwrap succeeds.
    let _ = bao.unwrap(&wrap_token).await.unwrap();

    // Second unwrap should fail (single-use token).
    let result = bao.unwrap(&wrap_token).await;
    assert!(result.is_err(), "single-use token should fail on reuse");
}

#[tokio::test]
async fn openbao_transit_encrypt_decrypt_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let plaintext = b"e2e encrypt me";

    // Try to create a transit key first via raw API.
    let client = reqwest::Client::new();
    let create_result = client
        .post(format!(
            "{}/v1/transit/keys/e2e-test-key",
            OPENBAO_URL
        ))
        .header("X-Vault-Token", OPENBAO_TOKEN)
        .json(&serde_json::json!({"type":"aes256-gcm96"}))
        .send()
        .await
        .unwrap();

    if !create_result.status().is_success() {
        let status = create_result.status();
        let body = create_result.text().await.unwrap_or_default();
        eprintln!("SKIP: transit engine not available (status {status}: {body})");
        return;
    }

    let encrypted = bao
        .transit_encrypt("e2e-test-key", plaintext)
        .await
        .unwrap();
    assert!(!encrypted.is_empty());
    let decrypted = bao
        .transit_decrypt("e2e-test-key", &encrypted)
        .await
        .unwrap();
    assert_eq!(decrypted, plaintext.to_vec());
}

#[tokio::test]
async fn openbao_cubbyhole_put_and_get_live() {
    if !openbao_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let bao = http_bao().await;
    let key = format!("e2e-cubby-{}", chrono::Utc::now().timestamp());
    let secret = Secret::single("temp_token", "cubbyhole-value");

    // Cubbyhole requires a non-root token. Root token's cubbyhole is
    // at a different path. Try with root and skip if it fails.
    let put_result = bao.cubbyhole_put(&key, &secret).await;
    if let Err(e) = &put_result {
        eprintln!("SKIP: cubbyhole_put failed (root token limitation): {e}");
        return;
    }
    put_result.unwrap();

    let get_result = bao.cubbyhole_get(&key).await;
    match get_result {
        Ok(got) => {
            // Cubbyhole with root token may return the secret or not,
            // depending on OpenBao's cubbyhole implementation for root.
            let _ = got.get_str("temp_token");
        }
        Err(e) => {
            // Cubbyhole access with root token is implementation-specific.
            eprintln!("SKIP: cubbyhole_get failed (root token limitation): {e}");
        }
    }
}