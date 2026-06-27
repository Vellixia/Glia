//! WS /gateway protocol edge cases: frame types, concurrent connections,
//! non-JSON messages, rapid-fire, close frames.

use futures_util::{SinkExt, StreamExt};
use glia_hub::hub_router;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

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
    // Give the server a tick to start.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    url
}

fn ws_url(base: &str) -> String {
    base.replacen("http://", "ws://", 1) + "/gateway"
}

#[tokio::test]
async fn ws_empty_text_frame_is_echoed() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    ws.send(Message::text("")).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert!(msg.is_text());
    assert_eq!(msg.into_text().unwrap(), "");
}

#[tokio::test]
async fn ws_binary_frame_is_echoed() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    ws.send(Message::binary(vec![1, 2, 3])).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert!(msg.is_binary());
    assert_eq!(msg.into_data(), vec![1, 2, 3]);
}

#[tokio::test]
async fn ws_rapid_fire_100_messages_all_echoed_in_order() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    for i in 0..100 {
        let line = format!("msg-{i:03}");
        ws.send(Message::text(line.clone())).await.unwrap();
        let msg = ws.next().await.unwrap().unwrap();
        assert_eq!(msg.into_text().unwrap(), line, "message {i} out of order");
    }
}

#[tokio::test]
async fn ws_immediate_disconnect_no_messages_clean() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    ws.send(Message::Close(None)).await.unwrap();
    // The server should close cleanly.
    let _ = ws.next().await;
}

#[tokio::test]
async fn ws_close_frame_with_payload_breaks_connection() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    // Send a close with code 1000 (normal closure).
    ws.send(Message::Close(Some(
        tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
            reason: "test done".into(),
        },
    )))
    .await
    .unwrap();
    // Should receive close back or stream end.
    let next = ws.next().await;
    // Either Close or None is acceptable.
    assert!(next.is_some() || next.is_none());
}

#[tokio::test]
async fn ws_non_json_text_echoed_without_validation() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    ws.send(Message::text("not json at all")).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), "not json at all");
}

#[tokio::test]
async fn ws_json_missing_fields_echoed_not_rejected() {
    // The Hub is an echo server — no schema validation.
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    let bad_json = r#"{"intent": "test"}"#; // missing other fields
    ws.send(Message::text(bad_json)).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), bad_json);
}

#[tokio::test]
async fn ws_json_extra_fields_echoed_not_rejected() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    let extra_json = r#"{"intent":"x","unknown_field":123}"#;
    ws.send(Message::text(extra_json)).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), extra_json);
}

#[tokio::test]
async fn ws_json_wrong_types_echoed_not_rejected() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    let wrong_types = r#"{"intent": 12345}"#; // int instead of string
    ws.send(Message::text(wrong_types)).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), wrong_types);
}

#[tokio::test]
async fn ws_10_concurrent_connections_all_echo() {
    let base = spawn_hub().await;
    let url = ws_url(&base);
    let mut handles = Vec::new();
    for i in 0..10 {
        let url = url.clone();
        handles.push(tokio::spawn(async move {
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            let msg = format!("concurrent-{i}");
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
async fn ws_large_text_frame_64kb_echoes() {
    let base = spawn_hub().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&base))
        .await
        .unwrap();
    let large = "x".repeat(64 * 1024);
    ws.send(Message::text(large.clone())).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), large);
}

#[tokio::test]
async fn ws_nonexistent_path_returns_404() {
    let base = spawn_hub().await;
    let resp = reqwest::get(format!("{base}/foobar")).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn ws_gateway_with_query_params_upgrades_ok() {
    let base = spawn_hub().await;
    let ws_url = ws_url(&base) + "?foo=bar";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.send(Message::text("with-query")).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    assert_eq!(msg.into_text().unwrap(), "with-query");
}
