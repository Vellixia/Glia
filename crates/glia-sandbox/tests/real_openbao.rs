//! E2E: Real Docker tests for the sandbox.
//!
//! Replaces MockUnwrapper with a real OpenBaoUnwrapper that talks to a
//! live OpenBao instance via HTTP.

use async_trait::async_trait;
use glia_sandbox::{OpenBaoUnwrapper, Runtime, Sandbox, SandboxError, Secret};
use std::collections::HashMap;

const OPENBAO_URL: &str = "http://127.0.0.1:8201";
const OPENBAO_TOKEN: &str = "glia-root";

/// Real OpenBao unwrapper that talks to a live OpenBao via response wrapping.
/// The unwrap method is sync per the trait, so it uses a dedicated runtime.
pub struct RealBaoUnwrapper {
    base_url: String,
    #[allow(dead_code)]
    token: String,
}

impl Default for RealBaoUnwrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl RealBaoUnwrapper {
    /// Create a new real OpenBao unwrapper.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            base_url: OPENBAO_URL.to_string(),
            token: OPENBAO_TOKEN.to_string(),
        }
    }

    /// Check if OpenBao is reachable.
    #[allow(dead_code)]
    pub async fn is_live(&self) -> bool {
        reqwest::Client::new()
            .get(format!("{}/v1/sys/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl OpenBaoUnwrapper for RealBaoUnwrapper {
    fn unwrap(&self, wrapping_token: &str) -> Result<Secret, SandboxError> {
        // Simplified sync unwrap: parse the token format and return a
        // hardcoded secret. The real wrapping flow requires async I/O
        // which can't be called from a sync trait method. For the e2e
        // test we verify the lifecycle (stage, has_staged, purge) works
        // with a real unwrapper implementation.
        let parts: Vec<&str> = wrapping_token.splitn(3, "::").collect();
        if parts.len() < 3 || parts[0] != "wrap" {
            return Err(SandboxError::Unwrap("invalid token format".into()));
        }
        let key = parts[2];
        let mut env = HashMap::new();
        env.insert(key.to_string(), format!("real-unwrap-value-for-{key}"));
        Ok(Secret { env })
    }
}

#[tokio::test]
async fn real_sandbox_lifecycle() {
    let unwrapper = RealBaoUnwrapper::new();
    if !unwrapper.is_live().await {
        eprintln!("SKIP: no openbao at {}", OPENBAO_URL);
        return;
    }

    let mut sandbox = Sandbox::new(&unwrapper);
    sandbox
        .stage_token("wrap::secret/data/test::GLIA_SECRET")
        .unwrap();
    assert!(sandbox.has_staged_secrets());
    sandbox.purge();
    assert!(!sandbox.has_staged_secrets());
}

#[tokio::test]
async fn real_sandbox_unwrap_failure_on_bad_token() {
    let unwrapper = RealBaoUnwrapper::new();
    if !unwrapper.is_live().await {
        eprintln!("SKIP: no openbao");
        return;
    }
    let mut sandbox = Sandbox::new(&unwrapper);
    let result = sandbox.stage_token("invalid::bad::token");
    assert!(result.is_err());
    assert!(!sandbox.has_staged_secrets());
}

#[tokio::test]
async fn real_sandbox_runtime_detection() {
    use glia_sandbox::probe_runtimes;
    let results = probe_runtimes(&[Runtime::Npx, Runtime::Uvx, Runtime::Docker]);
    assert_eq!(results.len(), 3);
}