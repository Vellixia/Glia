//! glia-bridge — stdio <-> WebSocket translator.
//!
//! Implements V2: remote-intent → CLI proxies via WS → Hub Gateway.
//! Reads lines from stdin, forwards as text messages over WebSocket;
//! receives WebSocket text messages, writes to stdout.
//!
//! Used by `glia bridge` (T1). The Hub Gateway (T4) is the WS server peer.

use std::io;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

/// Errors returned by [`run_bridge`].
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// WebSocket connect or handshake failed.
    #[error("ws connect: {0}")]
    Connect(String),
    /// stdin read failed.
    #[error("stdin: {0}")]
    Stdin(String),
    /// stdout write failed.
    #[error("stdout: {0}")]
    Stdout(String),
    /// WebSocket read/write failed.
    #[error("ws io: {0}")]
    Ws(String),
}

/// Configuration for [`run_bridge`].
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// `ws://host:port/path` or `wss://…`. Must point at the Hub `/gateway`.
    pub url: String,
    /// Bearer token forwarded as `Authorization: Bearer <token>`.
    pub bearer: Option<String>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            url: "ws://127.0.0.1:3000/gateway".to_string(),
            bearer: None,
        }
    }
}

/// Connect to the Hub WebSocket and pump stdin↔WS until either side closes.
///
/// Returns `Ok(())` when the WebSocket closes cleanly; returns `Err` on
/// connect, read, or write failure. stdin EOF is treated as a clean shutdown
/// (we drop the write half but keep draining WS→stdout until the server
/// closes).
pub async fn run_bridge(cfg: BridgeConfig) -> Result<(), BridgeError> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run_bridge_with_io(cfg, stdin, stdout).await
}

/// Like [`run_bridge`] but intercepts [`glia_proto::ServerFrame::ConfigChanged`]
/// frames before they reach stdout and sends them to `config_tx`.
///
/// All other frames are forwarded to stdout unchanged, preserving MCP
/// passthrough. The caller typically spawns a task that reads from the
/// receiving end and re-renders agent config files.
pub async fn run_bridge_with_handler(
    cfg: BridgeConfig,
    config_tx: Option<tokio::sync::mpsc::Sender<glia_proto::ServerFrame>>,
) -> Result<(), BridgeError> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run_bridge_internal(cfg, stdin, stdout, config_tx).await
}

/// Test-friendly variant of [`run_bridge`] that takes arbitrary IO.
/// Used by integration tests with `tokio::io::DuplexStream` to exercise
/// the full stdin/stdout pipeline without a real terminal.
pub async fn run_bridge_with_io<R, W>(
    cfg: BridgeConfig,
    stdin: R,
    stdout: W,
) -> Result<(), BridgeError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    run_bridge_internal(cfg, stdin, stdout, None).await
}

/// Internal implementation shared by all `run_bridge*` variants.
async fn run_bridge_internal<R, W>(
    cfg: BridgeConfig,
    stdin: R,
    stdout: W,
    config_tx: Option<tokio::sync::mpsc::Sender<glia_proto::ServerFrame>>,
) -> Result<(), BridgeError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = cfg
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| BridgeError::Connect(e.to_string()))?;
    if let Some(ref token) = cfg.bearer {
        use tokio_tungstenite::tungstenite::http::{HeaderValue, header::AUTHORIZATION};
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
            request.headers_mut().insert(AUTHORIZATION, value);
        }
    }
    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| BridgeError::Connect(e.to_string()))?;
    info!(url = %cfg.url, "ws connected");
    let (ws_sink, mut ws_stream) = ws.split();

    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    // stdout writer: drains channel, writes to stdout.
    let mut stdout = stdout;
    let stdout_task = tokio::spawn(async move {
        while let Some(buf) = stdout_rx.recv().await {
            if let Err(e) = stdout.write_all(&buf).await {
                error!(error = %e, "stdout write");
                return Err(BridgeError::Stdout(e.to_string()));
            }
            if let Err(e) = stdout.flush().await {
                error!(error = %e, "stdout flush");
                return Err(BridgeError::Stdout(e.to_string()));
            }
        }
        Ok(())
    });

    // WS → stdout pump. ConfigChanged frames are intercepted when a handler
    // channel is provided; all other frames pass through to stdout unchanged.
    let ws_to_stdout_tx = stdout_tx.clone();
    let ws_to_stdout = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "ws recv");
                    return Err(BridgeError::Ws(e.to_string()));
                }
            };
            if msg.is_close() {
                debug!("ws close");
                break;
            }
            if msg.is_binary() || msg.is_text() {
                let bytes = msg.into_data().to_vec();
                // Intercept ConfigChanged before forwarding to stdout.
                if let Some(ref tx) = config_tx
                    && let Ok(frame) = serde_json::from_slice::<glia_proto::ServerFrame>(&bytes)
                    && matches!(frame, glia_proto::ServerFrame::ConfigChanged { .. })
                {
                    let _ = tx.try_send(frame);
                    continue;
                }
                let _ = ws_to_stdout_tx.send(bytes).await;
            }
        }
        Ok(())
    });

    // stdin → WS pump. stdin EOF drops the write half; WS read side keeps
    // running so the server can flush pending responses.
    let mut reader = BufReader::new(stdin);
    let mut buf = String::new();
    let (stdin_done_tx, stdin_done_rx) = oneshot::channel::<()>();
    let mut ws_sink_for_stdin = std::mem::ManuallyDrop::new(ws_sink);
    let stdin_to_ws = tokio::spawn(async move {
        loop {
            buf.clear();
            let n = reader.read_line(&mut buf).await;
            match n {
                Ok(0) => {
                    debug!("stdin eof");
                    let _ = ws_sink_for_stdin.close().await;
                    let _ = stdin_done_tx.send(());
                    return Ok(());
                }
                Ok(_) => {
                    let line = buf.trim_end_matches('\n');
                    if line.is_empty() {
                        continue;
                    }
                    if let Err(e) = ws_sink_for_stdin
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            line.to_string().into(),
                        ))
                        .await
                    {
                        error!(error = %e, "ws send");
                        let _ = stdin_done_tx.send(());
                        return Err(BridgeError::Ws(e.to_string()));
                    }
                }
                Err(e) => {
                    error!(error = %e, "stdin read");
                    let _ = stdin_done_tx.send(());
                    return Err(BridgeError::Stdin(e.to_string()));
                }
            }
        }
    });

    // Wait for stdin EOF (clean) or WS error (propagate) — whichever first.
    let _ = stdin_done_rx.await;
    let stdin_result = stdin_to_ws.await.unwrap_or(Ok(()));
    let ws_result = ws_to_stdout.await.unwrap_or(Ok(()));
    drop(stdout_tx);
    let _stdout_result = stdout_task.await.unwrap_or(Ok(()));

    match (stdin_result, ws_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(e)) => Err(e),
        (Err(e), Ok(())) => Err(e),
        (Err(e), Err(_)) => Err(e),
    }
}

/// Re-export for tests: open a raw stdin pipe that [`run_bridge`] can read.
pub fn open_test_stdin(
    buf: &[u8],
) -> io::Result<(tokio::io::DuplexStream, tokio::io::DuplexStream)> {
    let (a, b) = tokio::io::duplex(1024);
    let _ = buf;
    Ok((a, b))
}

/// Error returned by [`call_once`].
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// WebSocket connect or handshake failed.
    #[error("ws connect: {0}")]
    Connect(String),
    /// Frame serialisation failed.
    #[error("serialize: {0}")]
    Serialize(#[from] serde_json::Error),
    /// WebSocket send/recv failed.
    #[error("ws io: {0}")]
    Ws(String),
    /// Connection closed before the Hub sent a response frame.
    #[error("connection closed before response")]
    ClosedEarly,
}

/// Connect to the Hub at `url`, send one [`glia_proto::ClientFrame`], and
/// await the first [`glia_proto::ServerFrame`] response.
///
/// The connection is closed cleanly after the first response is received.
/// Use this for one-shot RPC-style action requests; for the long-lived MCP
/// bridge use [`run_bridge`] instead.
///
/// `bearer` is forwarded as `Authorization: Bearer <token>` when present.
pub async fn call_once(
    url: &str,
    frame: &glia_proto::ClientFrame,
    bearer: Option<&str>,
) -> Result<glia_proto::ServerFrame, FrameError> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = url
        .into_client_request()
        .map_err(|e| FrameError::Connect(e.to_string()))?;

    if let Some(token) = bearer {
        let value =
            tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!("Bearer {token}"))
                .unwrap_or_else(|_| {
                    tokio_tungstenite::tungstenite::http::HeaderValue::from_static("")
                });
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            value,
        );
    }

    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| FrameError::Connect(e.to_string()))?;

    let json = serde_json::to_string(frame)?;
    ws.send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
        .await
        .map_err(|e| FrameError::Ws(e.to_string()))?;

    loop {
        match ws.next().await {
            None => return Err(FrameError::ClosedEarly),
            Some(Err(e)) => return Err(FrameError::Ws(e.to_string())),
            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                let server_frame: glia_proto::ServerFrame =
                    serde_json::from_str(&text).map_err(FrameError::Serialize)?;
                let _ = ws.close(None).await;
                return Ok(server_frame);
            }
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                return Err(FrameError::ClosedEarly);
            }
            Some(Ok(_)) => continue,
        }
    }
}
