//! glia-hub binary entry point.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let bind: SocketAddr = std::env::var("GLIA_HUB_BIND")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;
    tracing::info!(%bind, "glia-hub starting");

    // Run the server with graceful shutdown on SIGINT/SIGTERM.
    let server = glia_hub::serve(bind);
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
