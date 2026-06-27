//! glia-auth — AUTH_REQUIRED WS blocking + localhost OAuth callback (T15, V3/V14).
//!
//! When `glia_action` returns `Outcome::AuthRequired`, the CLI blocks the
//! WebSocket to the agent while opening a localhost HTTP server to catch
//! the OAuth redirect. A 120-second timeout produces `AUTH_TIMEOUT`.
//!
//! Flow:
//! 1. `AuthWaiter::new(port)` — bind a localhost Axum server on `127.0.0.1`.
//! 2. `wait_for_callback(timeout)` — `tokio::select!` between:
//!    a. The callback server receives `GET /callback?code=...&state=...`.
//!    b. The timeout elapses → `AuthError::Timeout`.
//! 3. On success, returns `AuthCode { code, state }`.
//! 4. The caller (glia-cli) exchanges the code with the provider and
//!    stores via OpenBao.
//!
//! The OS notification (open browser) is NOT in this crate — that's the
//! caller's responsibility. This crate only handles the callback + timeout.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Query;
use axum::response::IntoResponse;
use axum::routing::get;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

/// Default timeout for OAuth callback (V14: 120 seconds).
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(120);

/// Errors from auth waiting.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthError {
    /// Timed out waiting for OAuth callback.
    #[error("auth timeout after {0:?}")]
    Timeout(Duration),
    /// OAuth provider returned an error in the callback.
    #[error("oauth error: {0}")]
    OAuthError(String),
    /// Missing required query parameter.
    #[error("missing param: {0}")]
    MissingParam(&'static str),
    /// Server failed to bind or start.
    #[error("server: {0}")]
    Server(String),
    /// I/O error (kept as String for Clone compatibility).
    #[error("io: {0}")]
    Io(String),
}

/// The auth code + state returned by the OAuth provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCode {
    /// Authorization code.
    pub code: String,
    /// State parameter (CSRF protection).
    pub state: String,
}

/// State shared between the Axum handler and the waiter.
#[derive(Default)]
struct CallbackState {
    /// Filled by the handler when the callback arrives.
    result: Mutex<Option<Result<AuthCode, AuthError>>>,
    /// One-shot notifier: handler signals waiter.
    tx: Mutex<Option<oneshot::Sender<Result<AuthCode, AuthError>>>>,
}

/// Localhost OAuth callback server.
pub struct AuthWaiter {
    addr: SocketAddr,
    state: Arc<CallbackState>,
    server_handle: Mutex<Option<JoinHandle<()>>>,
}

impl AuthWaiter {
    /// Bind a callback server on `127.0.0.1:<port>`. Does NOT start
    /// accepting until `wait_for_callback` is called.
    pub async fn new(port: u16) -> Result<Self, AuthError> {
        let state = Arc::new(CallbackState::default());
        let app = Router::new()
            .route("/callback", get(callback_handler))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| AuthError::Io(e.to_string()))?;
        let addr = listener
            .local_addr()
            .map_err(|e| AuthError::Io(e.to_string()))?;
        tracing::info!(addr = %addr, "auth callback server bound");

        // Spawn the server immediately; it will wait for requests.
        let state_clone = state.clone();
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
            // If serve returns, signal any pending waiter with an error.
            let mut tx = state_clone.tx.lock().await;
            if let Some(sender) = tx.take() {
                let _ = sender.send(Err(AuthError::Server("server stopped".into())));
            }
        });

        Ok(Self {
            addr,
            state,
            server_handle: Mutex::new(Some(server_handle)),
        })
    }

    /// The address the server is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Block until the callback arrives or the timeout elapses.
    pub async fn wait_for_callback(&self, timeout: Duration) -> Result<AuthCode, AuthError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.state.tx.lock().await;
            *guard = Some(tx);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AuthError::Server("callback channel dropped".into())),
            Err(_) => Err(AuthError::Timeout(timeout)),
        }
    }

    /// Shut down the server (best-effort).
    pub async fn shutdown(&self) {
        if let Some(handle) = self.server_handle.lock().await.take() {
            handle.abort();
        }
    }
}

impl Drop for AuthWaiter {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.server_handle.try_lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
    }
}

/// Axum query params for `/callback`.
#[derive(Debug, serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Axum handler for `GET /callback`.
async fn callback_handler(
    Query(q): Query<CallbackQuery>,
    axum::extract::State(state): axum::extract::State<Arc<CallbackState>>,
) -> impl IntoResponse {
    let result = process_callback(q);
    let (status, body) = match &result {
        Ok(auth) => (
            axum::http::StatusCode::OK,
            format!(
                "Authorization received. Code: {} (state: {}). You can close this tab.",
                auth.code, auth.state
            ),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Auth error: {}", e),
        ),
    };
    // Store result and notify waiter.
    {
        let mut guard = state.result.lock().await;
        *guard = Some(result.clone());
    }
    {
        let mut tx = state.tx.lock().await;
        if let Some(sender) = tx.take() {
            let _ = sender.send(result);
        }
    }
    (status, body)
}

/// Process the callback query into an `AuthCode` or `AuthError`.
fn process_callback(q: CallbackQuery) -> Result<AuthCode, AuthError> {
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        return Err(AuthError::OAuthError(format!("{}: {}", err, desc)));
    }
    let code = q.code.ok_or(AuthError::MissingParam("code"))?;
    let state = q.state.ok_or(AuthError::MissingParam("state"))?;
    Ok(AuthCode { code, state })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn spawn_waiter(port: u16) -> AuthWaiter {
        AuthWaiter::new(port).await.unwrap()
    }

    #[tokio::test]
    async fn callback_returns_auth_code() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let waiter_clone = std::sync::Arc::new(waiter);
        let w = waiter_clone.clone();
        let wait_task = tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await });

        // Simulate OAuth redirect.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?code=test_code&state=test_state",
            port
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("test_code"));

        let auth = wait_task.await.unwrap().unwrap();
        assert_eq!(auth.code, "test_code");
        assert_eq!(auth.state, "test_state");
        waiter_clone.shutdown().await;
    }

    #[tokio::test]
    async fn callback_missing_code() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?state=x", port);
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        assert!(matches!(err, AuthError::MissingParam("code")));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn callback_oauth_error() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?error=access_denied&error_description=user+cancelled",
            port
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        assert!(matches!(err, AuthError::OAuthError(_)));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn timeout_returns_auth_timeout() {
        let waiter = spawn_waiter(0).await;
        let w = std::sync::Arc::new(waiter);
        let result = w.wait_for_callback(Duration::from_millis(100)).await;
        assert!(matches!(result, Err(AuthError::Timeout(_))));
        w.shutdown().await;
    }

    #[test]
    fn auth_timeout_is_120_seconds() {
        assert_eq!(AUTH_TIMEOUT, Duration::from_secs(120));
    }

    #[tokio::test]
    async fn addr_returns_bound_port() {
        let waiter = spawn_waiter(0).await;
        let addr = waiter.addr();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        w_shutdown(std::sync::Arc::new(waiter)).await;
    }

    async fn w_shutdown(w: std::sync::Arc<AuthWaiter>) {
        w.shutdown().await;
    }

    #[tokio::test]
    async fn callback_with_all_params() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?code=abc&state=xyz&extra=ignore",
            port
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let auth = wait_task.await.unwrap().unwrap();
        assert_eq!(auth.code, "abc");
        assert_eq!(auth.state, "xyz");
        w.shutdown().await;
    }

    #[tokio::test]
    async fn callback_missing_state_returns_missing_param_state() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?code=abc", port);
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        assert!(matches!(err, AuthError::MissingParam("state")));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn error_and_code_present_error_wins() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?error=denied&code=abc&state=x",
            port
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        assert!(matches!(err, AuthError::OAuthError(_)));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn error_without_description_uses_empty_default() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?error=denied", port);
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        // Should contain "denied: " (with empty description).
        let msg = format!("{}", err);
        assert!(msg.contains("denied"));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn url_encoded_code_decoded_correctly() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!(
            "http://127.0.0.1:{}/callback?code=abc%2Fdef%3D123&state=x",
            port
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let auth = wait_task.await.unwrap().unwrap();
        assert_eq!(auth.code, "abc/def=123");
        w.shutdown().await;
    }

    #[tokio::test]
    async fn very_long_code_accepted() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let long_code = "c".repeat(1000);
        let url = format!(
            "http://127.0.0.1:{}/callback?code={}&state=x",
            port, long_code
        );
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let auth = wait_task.await.unwrap().unwrap();
        assert_eq!(auth.code.len(), 1000);
        w.shutdown().await;
    }

    #[tokio::test]
    async fn post_callback_returns_405() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let url = format!("http://127.0.0.1:{}/callback?code=x&state=y", port);
        let resp = reqwest::Client::new().post(&url).send().await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::METHOD_NOT_ALLOWED);
        waiter.shutdown().await;
    }

    #[tokio::test]
    async fn wrong_path_returns_404() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let url = format!("http://127.0.0.1:{}/callback/extra", port);
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
        waiter.shutdown().await;
    }

    #[tokio::test]
    async fn missing_both_code_and_state_returns_missing_code() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);
        let wait_task = {
            let w = w.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let url = format!("http://127.0.0.1:{}/callback?foo=bar", port);
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 400);
        let err = wait_task.await.unwrap().unwrap_err();
        assert!(matches!(err, AuthError::MissingParam("code")));
        w.shutdown().await;
    }

    #[tokio::test]
    async fn multiple_waiters_different_ports_independent() {
        let waiter1 = spawn_waiter(0).await;
        let waiter2 = spawn_waiter(0).await;
        let port1 = waiter1.addr().port();
        let port2 = waiter2.addr().port();
        assert_ne!(port1, port2, "should get different ports");

        let w1 = std::sync::Arc::new(waiter1);
        let w2 = std::sync::Arc::new(waiter2);
        let wait1 = {
            let w = w1.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };
        let wait2 = {
            let w = w2.clone();
            tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
        };

        tokio::time::sleep(Duration::from_millis(50)).await;
        // Send callback to waiter1 only.
        let url = format!("http://127.0.0.1:{}/callback?code=a&state=b", port1);
        reqwest::get(&url).await.unwrap();

        let auth1 = wait1.await.unwrap().unwrap();
        assert_eq!(auth1.code, "a");

        // waiter2 should still be waiting — cancel it.
        w2.shutdown().await;
        drop(wait2);
        w1.shutdown().await;
    }

    #[tokio::test]
    async fn late_callback_after_timeout_is_noop() {
        let waiter = spawn_waiter(0).await;
        let port = waiter.addr().port();
        let w = std::sync::Arc::new(waiter);

        // Timeout first.
        let result = w.wait_for_callback(Duration::from_millis(50)).await;
        assert!(matches!(result, Err(AuthError::Timeout(_))));

        // Late callback arrives.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let url = format!("http://127.0.0.1:{}/callback?code=late&state=s", port);
        let resp = reqwest::get(&url).await.unwrap();
        // The callback handler still responds 200, but the waiter already
        // returned Timeout. The late callback is a no-op for the waiter.
        assert_eq!(resp.status(), 200);
        w.shutdown().await;
    }

    #[test]
    fn process_callback_empty_code_accepted() {
        // Documents that empty string code passes (Some("") is not None).
        let q = CallbackQuery {
            code: Some(String::new()),
            state: Some("x".to_string()),
            error: None,
            error_description: None,
        };
        let result = process_callback(q);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().code, "");
    }

    #[test]
    fn process_callback_empty_state_accepted() {
        let q = CallbackQuery {
            code: Some("abc".to_string()),
            state: Some(String::new()),
            error: None,
            error_description: None,
        };
        let result = process_callback(q);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().state, "");
    }

    #[test]
    fn process_callback_both_empty_accepted() {
        let q = CallbackQuery {
            code: Some(String::new()),
            state: Some(String::new()),
            error: None,
            error_description: None,
        };
        let result = process_callback(q);
        assert!(result.is_ok());
    }
}
