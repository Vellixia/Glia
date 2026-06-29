//! glia-hub binary entry point.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = glia_hub::config::AppConfig::from_env()?;
    tracing::info!("AppConfig loaded");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let bind: SocketAddr = std::env::var("GLIA_HUB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let hub_token = std::env::var("GLIA_HUB_TOKEN").ok();
    let bao_url = std::env::var("GLIA_BAO_URL").ok();
    let bao_token = std::env::var("GLIA_BAO_TOKEN").ok();
    let bao: Option<std::sync::Arc<dyn glia_bao::OpenBao>> = match (bao_url, bao_token) {
        (Some(url), Some(token)) => {
            tracing::info!("OpenBao configured");
            Some(std::sync::Arc::new(glia_bao::HttpOpenBao::new(url, token)))
        }
        _ => {
            tracing::warn!("GLIA_BAO_URL/TOKEN not set — using in-memory stub (dev only)");
            None
        }
    };
    tracing::info!(%bind, auth = hub_token.is_some(), "glia-hub starting");

    let catalog_source: std::sync::Arc<dyn glia_catalog::CatalogSource> = std::sync::Arc::new(
        glia_catalog::GitHubCatalog::new(std::env::var("GLIA_CATALOG_URL").unwrap_or_else(|_| {
            "https://raw.githubusercontent.com/Vellixia/community-catalog/main".into()
        })),
    );

    // Mount GraphQL + SSE routes alongside the existing Hub REST/WS routes.
    let bao_for_api: std::sync::Arc<dyn glia_bao::OpenBao> = bao
        .clone()
        .unwrap_or_else(|| std::sync::Arc::new(glia_bao::StubOpenBao::new("dev", "dev-key")));
    let api_routes = glia_hub_api::routes(
        std::sync::Arc::new(config.jwt_secret.to_string()),
        bao_for_api,
        catalog_source,
    );
    tracing::info!("GraphQL API mounted at /graphql, /graphql/ws, /api/logs");

    // Run the server with graceful shutdown on SIGINT/SIGTERM.
    let server = glia_hub::serve(bind, hub_token, bao, Some(api_routes));
    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = server => {
            result.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        _ = shutdown => {
            tracing::info!("received SIGINT, shutting down gracefully");
        }
    }
    Ok(())
}
