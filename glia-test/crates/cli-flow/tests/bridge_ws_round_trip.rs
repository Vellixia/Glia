//! SPEC §T1: build the Rust CLI `bridge` cmd (tokio stdio<->WS).
//! SPEC §T4: build Hub WS Gateway (Axum).
//! SPEC §V14: `AUTH_REQUIRED` WS wait ≤ 120s, timeout → `AUTH_TIMEOUT`.
//!
//! These tests focus on the **WS protocol surface**: connection setup,
//! heartbeat, and close-handling — without exercising the full action
//! dispatcher (that requires a live HelixDB and is covered by
//! `crates/glia-e2e/tests/live_hub.rs`). The point here is to lock in
//! V14 (no silent hangs) and the gateway's contract surface.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;

/// Spawn the Hub router on an ephemeral port. Returns base URL.
///
/// Mirrors `glia-e2e::common::spawn_hub` so this crate doesn't depend on
/// glia-e2e internals (which are `mod common` and not published).
async fn spawn_hub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    tokio::spawn(async move {
        let router = glia_hub::hub_router(None, None)
            .into_make_service_with_connect_info::<std::net::SocketAddr>();
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    url
}

/// SPEC T1: `glia bridge --help` exits 0 — clap shape is valid.
#[test]
fn bridge_bridge_subcommand_help_exits_0() {
    let result = glia_test_cli_flow::spawn_cli(&["bridge", "--help"]);
    assert_eq!(
        result.exit_code, 0,
        "`glia bridge --help` should exit 0; got {} stderr={}",
        result.exit_code, result.stderr
    );
}

/// SPEC T4: the Hub's `/healthz` returns 200 over plain HTTP.
#[tokio::test]
async fn hub_healthz_returns_ok() {
    let base = spawn_hub().await;
    let resp = reqwest::get(&format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");
}

/// SPEC T4: the Hub accepts WebSocket upgrade requests on `/gateway`.
/// Verifies that the route is mounted and the upgrade handshake succeeds.
#[tokio::test]
async fn hub_gateway_ws_upgrade_accepted() {
    let base = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (_ws, response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws upgrade");
    // tokio-tungstenite's connect_async succeeded — that's the proof.
    // We drop the stream immediately (we don't need to exchange frames;
    // that's the domain of `glia-e2e/tests/live_hub.rs`).
    drop(_ws);
    drop(response);
}

/// SPEC V14: a malformed JSON frame must NOT silently hang the Hub.
/// Per the gateway handler, malformed input → `ServerFrame::Error { code:
/// "PARSE_ERROR" }`. The contract: client gets a reply within bounded
/// time. We use a 5s budget — generous, but finite.
#[tokio::test]
async fn hub_rejects_malformed_frame_with_error() {
    let base = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect");

    // Garbage JSON — gateway's serde_json::from_str will fail.
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "this-is-not-json".into(),
    ))
    .await
    .unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("V14: malformed frame must get a reply within 5s")
        .expect("ws stream ended")
        .expect("ws error");
    let text = match received {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        other => panic!("expected text frame, got {:?}", other),
    };
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let kind = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let code = parsed
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(kind, "error");
    assert_eq!(code, "PARSE_ERROR");
}

/// SPEC V14: the WS connection closes cleanly when the client sends a
/// `Message::Close`. The Hub must not hang on the close.
#[tokio::test]
async fn ws_close_round_trip_terminates() {
    let base = spawn_hub().await;
    let ws_url = base.replacen("http://", "ws://", 1) + "/gateway";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("connect");

    ws.send(tokio_tungstenite::tungstenite::Message::Close(None))
        .await
        .expect("send close");

    // After close, the next read returns None within bounded time.
    let result = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
    match result {
        Ok(None) => {}         // expected: stream ended cleanly
        Ok(Some(Ok(_))) => {}  // acceptable: server's close ack
        Ok(Some(Err(_))) => {} // acceptable: socket torn down
        Err(_) => panic!("V14: WS close must not silently hang"),
    }
}
