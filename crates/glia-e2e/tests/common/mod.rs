//! Common helpers for e2e tests.
//!
//! Probes real Docker services (HelixDB, OpenBao, Redis) and provides
//! spawn helpers for the Hub. Tests soft-skip if a service is unreachable.

#![allow(dead_code, missing_docs)]

use std::time::Duration;

/// HelixDB base URL (matches docker-compose.yml port 6969).
pub const HELIX_URL: &str = "http://127.0.0.1:6969";
/// OpenBao base URL (host port 8201 → container 8200).
pub const OPENBAO_URL: &str = "http://127.0.0.1:8201";
/// OpenBao dev-mode root token.
pub const OPENBAO_TOKEN: &str = "glia-root";
/// Redis connection URL.
pub const REDIS_URL: &str = "redis://127.0.0.1:6379/0";

/// Probe a URL with a GET request. Returns true if reachable within timeout.
pub async fn probe_http(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    client.get(url).send().await.is_ok()
}

/// Probe Redis by trying to connect.
pub async fn probe_redis(url: &str) -> bool {
    glia_cache::RedisCache::connect(url).await.is_ok()
}

/// Check if HelixDB is live and has Glia schema deployed.
pub async fn helix_with_schema() -> Option<glia_helix::HelixClient> {
    let client = glia_helix::HelixClient::connect(Some(HELIX_URL), None).ok()?;
    if client.ping().await.is_err() {
        eprintln!("SKIP: helixdb reachable but Glia schema not deployed (SPEC B7)");
        return None;
    }
    Some(client)
}

/// Check if HelixDB is live at all (even without Glia schema).
pub async fn helix_live() -> Option<glia_helix::HelixClient> {
    let client = glia_helix::HelixClient::connect(Some(HELIX_URL), None).ok()?;
    // Try a raw HTTP call — even if ping fails (no schema), the server is up.
    if probe_http(&format!("{}/health", HELIX_URL)).await {
        Some(client)
    } else {
        None
    }
}

/// Check if OpenBao is live and unsealed.
pub async fn openbao_live() -> bool {
    probe_http(&format!("{}/v1/sys/health", OPENBAO_URL)).await
}

/// Check if Redis is live.
pub async fn redis_live() -> bool {
    probe_redis(REDIS_URL).await
}

/// Spawn the Hub on an ephemeral port. Returns (base_url, join_handle).
pub async fn spawn_hub() -> (String, tokio::task::JoinHandle<()>) {
    use glia_hub::hub_router;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            hub_router().into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    // Wait for the server to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    (url, handle)
}

/// Create a temp directory with a skills/ folder containing test markdown.
pub fn temp_repo_with_skills() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("test-skill.md"),
        "## Auth\nNever use service_role in client code.\n## RLS\nEnable RLS on all tables.\n",
    )
    .unwrap();
    tmp
}