//! E2E: Hub live WebSocket + HTTP tests.
//! The Hub is spawned in-process (no Docker needed for the Hub itself).

mod common;

use common::spawn_hub;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn hub_healthz_live() {
    let (base, _handle) = spawn_hub().await;
    let resp = reqwest::get(&format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn hub_ws_echo_text_live() {
    let (base, _handle) = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.send(Message::text("e2e live echo")).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), "e2e live echo");
}

#[tokio::test]
async fn hub_ws_echo_binary_live() {
    let (base, _handle) = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.send(Message::binary(vec![0xDE, 0xAD, 0xBE, 0xEF]))
        .await
        .unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_data(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[tokio::test]
async fn hub_ws_rapid_fire_live() {
    let (base, _handle) = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    for i in 0..50 {
        let msg = format!("e2e-msg-{i}");
        ws.send(Message::text(msg.clone())).await.unwrap();
        let echo = ws.next().await.unwrap().unwrap();
        assert_eq!(echo.into_text().unwrap(), msg);
    }
}

#[tokio::test]
async fn hub_ws_concurrent_connections_live() {
    let (base, _handle) = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let mut handles = Vec::new();
    for i in 0..20 {
        let url = ws_url.clone();
        handles.push(tokio::spawn(async move {
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            let msg = format!("e2e-concurrent-{i}");
            ws.send(Message::text(msg.clone())).await.unwrap();
            let echo = ws.next().await.unwrap().unwrap();
            assert_eq!(echo.into_text().unwrap(), msg);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn hub_healthz_concurrent_live() {
    let (base, _handle) = spawn_hub().await;
    let client = reqwest::Client::new();
    let mut handles = Vec::new();
    for _ in 0..100 {
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
async fn hub_404_for_unknown_path_live() {
    let (base, _handle) = spawn_hub().await;
    let resp = reqwest::get(&format!("{base}/api/v1/nonexistent"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn hub_healthz_method_not_allowed_live() {
    let (base, _handle) = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn hub_ws_large_frame_live() {
    let (base, _handle) = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let large = "x".repeat(256 * 1024); // 256KB
    ws.send(Message::text(large.clone())).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), large);
}
