//! glia-hub — WebSocket gateway and control plane for Glia.
//!
//! Implements V2: remote-intent → CLI proxies via WS → Hub Gateway.
//! Exposes `WS /gateway` (bidirectional) and `REST /healthz` (200).
//!
//! Security: `/gateway` requires `Authorization: Bearer <GLIA_HUB_TOKEN>`
//! when a token is configured; `/healthz` and `/oauth/callback` are always
//! unauthenticated (callback is hit by the provider redirect, not the CLI).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{ConnectInfo, Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ───────────────────── OAuth shared state ─────────────────────

/// A pending OAuth flow (keyed by state token in `OAuthState::flows`).
#[derive(Debug, Clone)]
struct PendingFlow {
    cred_id: String,
    provider_id: String,
    redirect_uri: String,
}

/// Shared state injected into every OAuth handler via Axum `State`.
#[derive(Clone)]
pub struct OAuthState {
    flows: Arc<Mutex<HashMap<String, PendingFlow>>>,
    bao: Arc<dyn glia_bao::OpenBao>,
}

// ───────────────────── Request / Response bodies ─────────────────────

#[derive(serde::Deserialize)]
struct ActionRequest {
    intent: String,
    #[serde(default)]
    stack: Option<String>,
}

#[derive(serde::Deserialize)]
struct RegisterProviderRequest {
    id: String,
    name: String,
    auth_url: String,
    token_url: String,
    client_id: String,
    client_secret: String,
    #[serde(default)]
    scopes: Vec<String>,
}

#[derive(serde::Deserialize)]
struct StartOAuthRequest {
    cred_id: String,
    provider_id: String,
    /// Hub's own base URL so the callback redirect_uri can be constructed.
    callback_base: String,
}

#[derive(serde::Serialize)]
struct StartOAuthResponse {
    state: String,
    redirect_url: String,
}

#[derive(serde::Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    state: String,
}

// ───────────────────── Helpers ─────────────────────

/// Unix-timestamp string — used for `updated_at` LWW fields.
fn now_unix_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// Percent-encode a string for use as a URL query parameter value.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let hi = byte >> 4;
                let lo = byte & 0xF;
                out.push(
                    char::from_digit(hi as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit(lo as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// Generate a non-guessable state token for OAuth CSRF protection.
///
/// Uses SystemTime nanos + PID + monotonic counter. Not cryptographically
/// random — replace with `getrandom` before any public Hub exposure.
fn generate_state() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let count = CTR.fetch_add(1, Ordering::Relaxed);
    format!("{pid:08x}{nanos:08x}{count:016x}")
}

/// Read `GLIA_HELIX_URL` + `GLIA_HELIX_TOKEN` and build a HelixClient.
fn helix_from_env() -> Result<glia_helix::HelixClient, StatusCode> {
    let url = std::env::var("GLIA_HELIX_URL").unwrap_or_else(|_| "http://127.0.0.1:6969".into());
    let token = std::env::var("GLIA_HELIX_TOKEN").ok();
    glia_helix::HelixClient::connect(Some(&url), token.as_deref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ───────────────────── Router ─────────────────────

/// Build the Hub Axum router.
///
/// `hub_token` gates every route except `/healthz` and `/oauth/callback`.
/// `bao` is the OpenBao backend used by OAuth handlers; defaults to
/// `StubOpenBao` when `None` (safe for tests, no secrets stored).
pub fn hub_router(hub_token: Option<String>, bao: Option<Arc<dyn glia_bao::OpenBao>>) -> Router {
    let oauth_state = OAuthState {
        flows: Arc::new(Mutex::new(HashMap::new())),
        bao: bao
            .unwrap_or_else(|| Arc::new(glia_bao::StubOpenBao::new("stub-root", "glia-transit"))),
    };
    let token = Arc::new(hub_token);
    Router::new()
        .route("/gateway", get(gateway_handler))
        .route("/healthz", get(healthz_handler))
        .route("/action", post(action_handler))
        .route("/oauth/provider", post(register_provider_handler))
        .route("/oauth/start", post(start_oauth_handler))
        .route("/oauth/callback", get(oauth_callback_handler))
        .route("/oauth/status/{cred_id}", get(oauth_status_handler))
        .with_state(oauth_state)
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let token = token.clone();
            async move { bearer_gate(req, next, token).await }
        }))
}

// ───────────────────── Middleware ─────────────────────

/// Constant-time bearer gate.
///
/// Exempt: `/healthz` (health checks), `/oauth/callback` (provider redirect
/// arrives from the browser — no Authorization header available).
async fn bearer_gate(
    req: Request,
    next: Next,
    expected: Arc<Option<String>>,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/healthz" || path == "/oauth/callback" {
        return Ok(next.run(req).await);
    }
    let Some(expected_token) = expected.as_deref() else {
        return Ok(next.run(req).await);
    };
    let provided = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");

    let authed: bool = if provided.len() == expected_token.len() {
        provided.as_bytes().ct_eq(expected_token.as_bytes()).into()
    } else {
        let _ = expected_token.as_bytes().ct_eq(expected_token.as_bytes());
        false
    };

    if authed {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

// ───────────────────── Handlers ─────────────────────

async fn healthz_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// `POST /action` — Hub action dispatcher.
async fn action_handler(
    axum::extract::Json(body): axum::extract::Json<ActionRequest>,
) -> impl IntoResponse {
    let client = match helix_from_env() {
        Ok(c) => c,
        Err(s) => return (s, "helix connect failed").into_response(),
    };
    let embedder = match glia_embed::Embedder::try_new() {
        Some(e) => Arc::new(e),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "embed model assets not available",
            )
                .into_response();
        }
    };
    let executor = Arc::new(glia_action::StubExecutor {
        response: "hub-dispatched".into(),
    });
    let action = glia_action::Action::new(client, embedder, executor);
    let intent = glia_action::Intent {
        query: body.intent,
        stack: body.stack,
    };
    match action.run(intent).await {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(json) => axum::response::Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(json))
                .unwrap(),
            Err(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("serialize: {e}")).into_response()
            }
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("action: {e}")).into_response(),
    }
}

/// `POST /oauth/provider` — Register an OAuth provider.
///
/// Stores non-secret fields in HelixDB and `client_secret` in OpenBao.
/// Also upserts an `Auth` record so `dep_check` can discover the credential.
async fn register_provider_handler(
    State(oauth): State<OAuthState>,
    axum::extract::Json(req): axum::extract::Json<RegisterProviderRequest>,
) -> impl IntoResponse {
    let client = match helix_from_env() {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "helix connect failed").into_response();
        }
    };

    let provider = glia_helix::Provider {
        name: req.name,
        auth_url: req.auth_url,
        token_url: req.token_url,
        client_id: req.client_id,
        scopes: req.scopes,
        updated_at: now_unix_ts(),
    };
    if let Err(e) = client.upsert_provider(&req.id, provider).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("helix upsert_provider: {e}"),
        )
            .into_response();
    }

    let auth = glia_helix::Auth {
        auth_type: "oauth".into(),
        provider: req.id.clone(),
        updated_at: now_unix_ts(),
    };
    if let Err(e) = client.upsert_auth(&req.id, auth).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("helix upsert_auth: {e}"),
        )
            .into_response();
    }

    let secret = glia_bao::Secret::single("client_secret", req.client_secret.clone());
    let path = format!("secret/data/providers/{}", req.id);
    if let Err(e) = oauth.bao.kv_put(&path, &secret).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("bao kv_put client_secret: {e}"),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        axum::Json(serde_json::json!({ "ok": true, "id": req.id })),
    )
        .into_response()
}

/// `POST /oauth/start` — Begin an OAuth authorization flow.
///
/// Looks up the provider, generates a CSRF state token, stores a pending flow,
/// and returns the redirect URL the CLI should open in the browser.
async fn start_oauth_handler(
    State(oauth): State<OAuthState>,
    axum::extract::Json(req): axum::extract::Json<StartOAuthRequest>,
) -> impl IntoResponse {
    let client = match helix_from_env() {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "helix connect failed").into_response();
        }
    };

    let provider = match client.get_provider(&req.provider_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                format!("provider '{}' not found", req.provider_id),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("helix get_provider: {e}"),
            )
                .into_response();
        }
    };

    let state_token = generate_state();
    let redirect_uri = format!("{}/oauth/callback", req.callback_base.trim_end_matches('/'));

    {
        let mut flows = oauth.flows.lock().await;
        flows.insert(
            state_token.clone(),
            PendingFlow {
                cred_id: req.cred_id,
                provider_id: req.provider_id,
                redirect_uri: redirect_uri.clone(),
            },
        );
    }

    let scopes = provider.scopes.join(" ");
    let redirect_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
        provider.auth_url,
        url_encode(&provider.client_id),
        url_encode(&redirect_uri),
        url_encode(&scopes),
        url_encode(&state_token),
    );

    (
        StatusCode::OK,
        axum::Json(StartOAuthResponse {
            state: state_token,
            redirect_url,
        }),
    )
        .into_response()
}

/// `GET /oauth/callback?code=X&state=Y` — Provider redirect target.
///
/// Exchanges the authorization code for tokens, stores them in OpenBao, and
/// removes the pending flow. The browser sees a plain-text success message.
async fn oauth_callback_handler(
    State(oauth): State<OAuthState>,
    Query(q): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let flow = {
        let flows = oauth.flows.lock().await;
        flows.get(&q.state).cloned()
    };
    let flow = match flow {
        Some(f) => f,
        None => return (StatusCode::BAD_REQUEST, "unknown state parameter").into_response(),
    };

    let client = match helix_from_env() {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "helix connect failed").into_response();
        }
    };

    let provider = match client.get_provider(&flow.provider_id).await {
        Ok(Some(p)) => p,
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "provider lookup failed").into_response(),
    };

    let secret_path = format!("secret/data/providers/{}", flow.provider_id);
    let provider_secret = match oauth.bao.kv_get(&secret_path).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("bao read client_secret: {e}"),
            )
                .into_response();
        }
    };
    let client_secret = match provider_secret.get_str("client_secret") {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "no client_secret in OpenBao",
            )
                .into_response();
        }
    };

    let http = reqwest::Client::new();
    let token_resp = http
        .post(&provider.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &q.code),
            ("redirect_uri", &flow.redirect_uri),
            ("client_id", &provider.client_id),
            ("client_secret", &client_secret),
        ])
        .send()
        .await;

    let token_resp = match token_resp {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("token exchange request: {e}"),
            )
                .into_response();
        }
    };

    if !token_resp.status().is_success() {
        let body = token_resp.text().await.unwrap_or_default();
        return (
            StatusCode::BAD_GATEWAY,
            format!("token exchange failed: {body}"),
        )
            .into_response();
    }

    let token_data: serde_json::Value = match token_resp.json().await {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("token parse: {e}")).into_response(),
    };

    let mut secret_map = serde_json::Map::new();
    for field in ["access_token", "refresh_token", "token_type"] {
        if let Some(v) = token_data.get(field).and_then(|v| v.as_str()) {
            secret_map.insert(field.into(), v.into());
        }
    }
    let token_secret = glia_bao::Secret::new(secret_map);
    let token_path = format!("secret/data/oauth/{}", flow.cred_id);
    if let Err(e) = oauth.bao.kv_put(&token_path, &token_secret).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("bao store token: {e}"),
        )
            .into_response();
    }

    oauth.flows.lock().await.remove(&q.state);
    (
        StatusCode::OK,
        "Authorization complete! You can close this window.",
    )
        .into_response()
}

/// `GET /oauth/status/:cred_id` — Check if an OAuth credential is ready.
async fn oauth_status_handler(
    State(oauth): State<OAuthState>,
    Path(cred_id): Path<String>,
) -> impl IntoResponse {
    let path = format!("secret/data/oauth/{}", cred_id);
    let ready = oauth.bao.kv_get(&path).await.is_ok();
    axum::Json(serde_json::json!({ "cred_id": cred_id, "ready": ready })).into_response()
}

/// `GET /gateway` — WebSocket upgrade.
async fn gateway_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!(%addr, "ws upgrade");
    ws.on_upgrade(move |socket| handle_connection(socket, addr))
}

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
            _ => {}
        }
    }
    let _ = sink.close().await;
}

/// Bind the Hub server to `addr` and serve until shutdown.
///
/// `bao` is the OpenBao backend for OAuth token storage. Pass `None` to use
/// the in-memory `StubOpenBao` (suitable for local dev without OpenBao running).
pub async fn serve(
    addr: SocketAddr,
    hub_token: Option<String>,
    bao: Option<Arc<dyn glia_bao::OpenBao>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "glia-hub listening");
    axum::serve(
        listener,
        hub_router(hub_token, bao).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
