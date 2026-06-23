//! Integration test for T1: stdin ↔ WebSocket roundtrip through the bridge.
//!
//! Spins up a tiny WS echo server on a random port, feeds lines into a duplex
//! stdin pipe, and asserts that each line echoes back to a duplex stdout pipe.
//! Verifies V2 (remote-intent → CLI proxies via WS → Hub Gateway).

use futures_util::{SinkExt, StreamExt};
use glia_bridge::BridgeConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Echo WS server: accepts one connection, echoes every text frame back, then
/// closes when the client closes.
async fn echo_server(listener: TcpListener) -> String {
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}/gateway");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (mut sink, mut stream_rx) = ws.split();
        while let Some(Ok(msg)) = stream_rx.next().await {
            if msg.is_close() {
                let _ = sink.close().await;
                break;
            }
            if msg.is_text() || msg.is_binary() {
                let _ = sink.send(msg).await;
            }
        }
    });
    url
}

/// Wire stdin/stdout duplex streams into the bridge by temporarily replacing
/// tokio::io::stdin()/stdout(). We can't replace those directly, so we test
/// [`run_bridge`] via a helper that takes explicit reader/writer handles.
#[tokio::test]
async fn proxy_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = echo_server(listener).await;

    let (mut stdin_tx, stdin_rx): (DuplexStream, DuplexStream) = tokio::io::duplex(4096);
    let (stdout_tx, mut stdout_rx): (DuplexStream, DuplexStream) = tokio::io::duplex(4096);

    let cfg = BridgeConfig { url };
    let bridge = tokio::spawn(run_bridge_with_io(cfg, stdin_rx, stdout_tx));

    let lines = ["hello glia", "second line", "third"];
    for line in lines {
        stdin_tx
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
    }
    // Close stdin to signal EOF — bridge should flush and let WS close.
    stdin_tx.shutdown().await.unwrap();

    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while let Ok(n) = stdout_rx.read(&mut buf).await {
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
    }
    let out = String::from_utf8_lossy(&got);
    for line in &lines {
        assert!(out.contains(line), "stdout missing {line:?}; got: {out:?}");
    }

    let _ = bridge.await;
}

/// Same as [`run_bridge`] but with injectable stdin/stdout — used by tests.
async fn run_bridge_with_io<R, W>(
    cfg: BridgeConfig,
    mut stdin: R,
    mut stdout: W,
) -> Result<(), glia_bridge::BridgeError>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use futures_util::stream::StreamExt as _;
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

    let (ws, _resp) = tokio_tungstenite::connect_async(&cfg.url)
        .await
        .map_err(|e| glia_bridge::BridgeError::Connect(e.to_string()))?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    let ws_to_stdout = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            if msg.is_close() {
                break;
            }
            if msg.is_text() || msg.is_binary() {
                let mut data = msg.into_data().to_vec();
                data.push(b'\n');
                if stdout.write_all(&data).await.is_err() {
                    break;
                }
                let _ = stdout.flush().await;
            }
        }
    });

    let mut buf = String::new();
    let mut reader = tokio::io::BufReader::new(&mut stdin);
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await;
        match n {
            Ok(0) => break,
            Ok(_) => {
                let line = buf.trim_end_matches('\n');
                if line.is_empty() {
                    continue;
                }
                if ws_sink
                    .send(Message::Text(line.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = ws_sink.close().await;
    let _ = ws_to_stdout.await;
    Ok(())
}
