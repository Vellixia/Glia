//! glia-hub — WebSocket gateway and control plane for Glia.
//!
//! Implements V2: remote-intent → CLI proxies via WS → Hub Gateway.
//! Exposes `WS /gateway` (bidirectional) and `REST /healthz` (200).

use std::net::SocketAddr;

use axum::{
    Router,
    extract::ConnectInfo,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info, warn};

/// Build the Hub Axum router with `/gateway` and `/healthz` routes.
pub fn hub_router() -> Router {
    Router::new()
        .route("/gateway", get(gateway_handler))
        .route("/healthz", get(healthz_handler))
}

/// `GET /healthz` → 200 OK with plain text body.
async fn healthz_handler() -> impl IntoResponse {
    (axum::http::StatusCode::OK, "ok")
}

/// `GET /gateway` — WebSocket upgrade. Echoes messages back to the client.
///
/// v1: simple echo. T9 will replace this with the unified `glia_action`
/// engine that queries SurrealDB, validates schemas, and routes to sandbox.
async fn gateway_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!(%addr, "ws upgrade");
    ws.on_upgrade(move |socket| handle_connection(socket, addr))
}

/// Per-connection handler: pump messages both ways, echo text/binary.
async fn handle_connection(socket: WebSocket, addr: SocketAddr) {
    let (mut sink, mut stream) = socket.split();
    while let Some(msg) = stream.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(%addr, error = %e, "ws recv error");
                break;
            }
        };
        match msg {
            Message::Close(_) => {
                debug!(%addr, "ws close");
                break;
            }
            Message::Text(_) | Message::Binary(_) => {
                if let Err(e) = sink.send(msg).await {
                    warn!(%addr, error = %e, "ws send error");
                    break;
                }
            }
            _ => {} // Ping/Pong handled by axum internally
        }
    }
    let _ = sink.close().await;
}

/// Bind the Hub server to `addr` and serve until shutdown.
pub async fn serve(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "glia-hub listening");
    axum::serve(
        listener,
        hub_router().into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
