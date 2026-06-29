use axum::{
    Extension,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use std::convert::Infallible;
use std::sync::LazyLock;
use tokio::sync::broadcast;

use crate::schema::LogEntry;

/// Broadcast channel for log entries — shared across all SSE subscribers.
static LOG_CHANNEL: LazyLock<broadcast::Sender<LogEntry>> = LazyLock::new(|| {
    let (tx, _) = broadcast::channel::<LogEntry>(256);
    tx
});

/// Subscribe to the log broadcast channel.
pub fn subscribe_log_stream() -> broadcast::Receiver<LogEntry> {
    LOG_CHANNEL.subscribe()
}

/// Publish a log entry to all SSE subscribers.
pub fn publish_log(entry: LogEntry) {
    let _ = LOG_CHANNEL.send(entry);
}

/// SSE endpoint handler — streams real-time log entries to the client.
///
/// Mount at `GET /api/logs` in the Hub router.
pub async fn log_stream_handler(
    Extension(_jwt_secret): Extension<std::sync::Arc<String>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = subscribe_log_stream();

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(entry) => {
                let event = Event::default()
                    .event("log")
                    .json_data(&entry)
                    .unwrap_or_else(|_| {
                        Event::default().data(serde_json::to_string(&entry).unwrap_or_default())
                    });
                Some((Ok(event), rx))
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("SSE log stream lagged by {n} entries");
                let event = Event::default()
                    .event("warning")
                    .data(format!("stream lagged by {n} entries"));
                Some((Ok(event), rx))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_channel_publish_subscribe() {
        let mut rx = subscribe_log_stream();
        let entry = LogEntry {
            timestamp: chrono::Utc::now(),
            level: "info".into(),
            message: "test log line".into(),
        };
        publish_log(entry.clone());
        let received = rx.recv().await.expect("should receive log entry");
        assert_eq!(received.message, "test log line");
    }
}
