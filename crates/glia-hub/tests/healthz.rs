//! /healthz endpoint edge cases: methods, trailing slash, query params,
//! concurrent checks, response headers.

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

#[tokio::test]
async fn healthz_post_returns_405() {
    let base = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn healthz_trailing_slash_returns_404() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/healthz/")).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn healthz_query_params_ignored_returns_200() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/healthz?foo=bar"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn healthz_delete_returns_405() {
    let base = spawn_hub().await;
    let resp = reqwest::Client::new()
        .delete(format!("{base}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn healthz_put_returns_405() {
    let base = spawn_hub().await;
    let resp = reqwest::Client::new()
        .put(format!("{base}/healthz"))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn healthz_body_is_exactly_ok() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn healthz_50_concurrent_gets_all_200() {
    let base = spawn_hub().await;
    let client = reqwest::Client::new();
    let mut handles = Vec::new();
    for _ in 0..50 {
        let c = client.clone();
        let url = format!("{base}/healthz");
        handles.push(tokio::spawn(async move {
            let resp = c.get(&url).send().await.unwrap();
            assert_eq!(resp.status(), 200);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn nonexistent_path_returns_404() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/api/v1/something"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
