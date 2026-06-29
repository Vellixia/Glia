use std::sync::Arc;

/// Centralized configuration loaded from environment / `.env`.
#[derive(Clone)]
pub struct AppConfig {
    /// Secret key for signing and verifying JWT tokens.
    pub jwt_secret: Arc<String>,
    /// Argon2id hash of the admin password.
    pub admin_password_hash: Arc<String>,
    /// HelixDB base URL.
    pub helix_url: String,
    /// Redis connection URL.
    pub redis_url: String,
}

impl AppConfig {
    /// Load configuration from the process environment.
    ///
    /// Calls `dotenvy::dotenv()` first so a root `.env` file is picked up
    /// automatically. Existing `std::env::var(...)` calls inherit these
    /// values without changes.
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        Ok(Self {
            jwt_secret: Arc::new(std::env::var("GLIA_JWT_SECRET")?),
            admin_password_hash: Arc::new(std::env::var("GLIA_ADMIN_HASH")?),
            helix_url: std::env::var("GLIA_HELIX_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),
            redis_url: std::env::var("GLIA_REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".into()),
        })
    }
}
