//! Integration tests for Hub OAuth endpoints (Phase 2).
//!
//! Tests use `StubOpenBao` (no real OpenBao) and confirm the stateful
//! in-memory flows + bao injection work correctly.

use glia_bao::OpenBao as _;
use glia_hub::hub_router;
use tokio::net::TcpListener;

async fn spawn_hub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router(None, None).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    url
}

async fn spawn_hub_with_bao(bao: std::sync::Arc<dyn glia_bao::OpenBao>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router(None, Some(bao))
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    url
}

// ─────────────────── /oauth/status tests ───────────────────

#[tokio::test]
async fn oauth_status_missing_cred_returns_not_ready() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/oauth/status/no-such-cred"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ready"], false);
    assert_eq!(body["cred_id"], "no-such-cred");
}

#[tokio::test]
async fn oauth_status_stored_cred_returns_ready() {
    let bao = std::sync::Arc::new(glia_bao::StubOpenBao::new("root", "transit"));
    let token_secret = glia_bao::Secret::single("access_token", "tok_abc");
    bao.kv_put("secret/data/oauth/linear_oauth", &token_secret)
        .await
        .unwrap();

    let base = spawn_hub_with_bao(bao).await;
    let resp = reqwest::get(format!("{base}/oauth/status/linear_oauth"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ready"], true);
    assert_eq!(body["cred_id"], "linear_oauth");
}

#[tokio::test]
async fn oauth_status_different_cred_not_ready() {
    let bao = std::sync::Arc::new(glia_bao::StubOpenBao::new("root", "transit"));
    let token_secret = glia_bao::Secret::single("access_token", "tok_abc");
    bao.kv_put("secret/data/oauth/linear_oauth", &token_secret)
        .await
        .unwrap();

    let base = spawn_hub_with_bao(bao).await;
    // Different cred_id — not stored.
    let resp = reqwest::get(format!("{base}/oauth/status/github_oauth"))
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ready"], false);
}

// ─────────────────── /oauth/callback tests ───────────────────

#[tokio::test]
async fn oauth_callback_unknown_state_returns_400() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!(
        "{base}/oauth/callback?code=abc&state=unknown_state"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn oauth_callback_is_bearer_exempt() {
    // Hub with a token — /oauth/callback must NOT require auth.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router(Some("secret".into()), None)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // No Authorization header → 400 (bad state), NOT 401.
    let resp = reqwest::get(format!("{base}/oauth/callback?code=x&state=no_such_state"))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "callback must be exempt from bearer gate"
    );
}

// ─────────────────── /oauth/start / /oauth/provider — bearer gate ───────────────────

#[tokio::test]
async fn oauth_start_requires_bearer_when_token_set() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router(Some("mysecret".into()), None)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // No auth → 401.
    let resp = reqwest::Client::new()
        .post(format!("{base}/oauth/start"))
        .json(&serde_json::json!({
            "cred_id": "x",
            "provider_id": "y",
            "callback_base": "http://x",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong bearer → 401.
    let resp = reqwest::Client::new()
        .post(format!("{base}/oauth/start"))
        .bearer_auth("wrong")
        .json(&serde_json::json!({
            "cred_id": "x",
            "provider_id": "y",
            "callback_base": "http://x",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn register_provider_requires_bearer_when_token_set() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router(Some("tok".into()), None)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/oauth/provider"))
        .json(&serde_json::json!({
            "id": "linear_oauth",
            "name": "Linear",
            "auth_url": "https://linear.app/oauth/authorize",
            "token_url": "https://api.linear.app/oauth/token",
            "client_id": "cli_123",
            "client_secret": "shhh",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
