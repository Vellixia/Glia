//! Integration tests for glia-hub. Verifies V2: WS /gateway accepts
//! connections and echoes, and REST /healthz returns 200.

use futures_util::{SinkExt, StreamExt};
use glia_hub::hub_router;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Spawn the hub on a random port, return the base URL.
async fn spawn_hub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    tokio::spawn(async move {
        axum::serve(
            listener,
            hub_router().into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    url
}

#[tokio::test]
async fn healthz_returns_200() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn ws_gateway_echoes_text() {
    let base = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";

    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.send(Message::Text("hello hub".into())).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    assert!(msg.is_text());
    assert_eq!(msg.into_text().unwrap(), "hello hub");
}

#[tokio::test]
async fn ws_gateway_echoes_multiple() {
    let base = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";

    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    for i in 0..5 {
        let line = format!("msg-{i}");
        ws.send(Message::Text(line.clone().into())).await.unwrap();
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg.into_text().unwrap(), line);
    }
}
