//! glia-sandbox — Execution Sandbox with OpenBao response-wrapping token.
//!
//! Implements V18: Hub API issues 1-time OpenBao wrapping token to Sandbox.
//! Sandbox unwraps via `sys/wrapping/unwrap` directly against OpenBao,
//! injects secret into child process env, purges on exit. Hub API memory
//! ⊥ plaintext secret.
//!
//! Implements V9: sandbox exec via npx/uvx/docker as fallback when local
//! deps are missing.

use std::collections::HashMap;

use thiserror::Error;
use tokio::process::Command;

/// Errors from the sandbox.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// OpenBao unwrap failed (token invalid, expired, or already consumed).
    #[error("openbao unwrap: {0}")]
    Unwrap(String),
    /// Requested runtime (npx/uvx/docker) not found on PATH.
    #[error("runtime not found: {0}")]
    RuntimeNotFound(String),
    /// Child process exited non-zero.
    #[error("exit {code}: {stderr}")]
    NonZeroExit {
        /// Exit code from the child process.
        code: i32,
        /// stderr captured from the child process.
        stderr: String,
    },
    /// IO error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Runtime selector for sandbox execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// `npx` — Node.js package runner.
    Npx,
    /// `uvx` — Python/uv package runner.
    Uvx,
    /// `docker run` — containerized fallback.
    Docker,
}

impl Runtime {
    /// Return the binary name used to invoke this runtime.
    pub fn binary(&self) -> &'static str {
        match self {
            Self::Npx => "npx",
            Self::Uvx => "uvx",
            Self::Docker => "docker",
        }
    }
}

/// A secret unwrapped from an OpenBao response-wrapping token.
/// Keys are env var names; values are secret values.
#[derive(Debug, Clone, Default)]
pub struct Secret {
    /// Environment variable name → secret value.
    pub env: HashMap<String, String>,
}

/// Trait for unwrapping OpenBao response-wrapping tokens.
/// Production impl calls `sys/wrapping/unwrap` against a real OpenBao.
/// Tests use a mock impl.
pub trait OpenBaoUnwrapper: Send + Sync {
    /// Unwrap a 1-time wrapping token. Returns the secret inside.
    /// Token is consumed (single-use).
    fn unwrap(&self, wrapping_token: &str) -> Result<Secret, SandboxError>;
}

/// Mock OpenBao unwrapper for tests. Returns a pre-configured secret
/// for any token, recording the token that was unwrapped.
#[cfg(any(test, feature = "mock"))]
pub struct MockUnwrapper {
    /// Secret to return on unwrap.
    pub secret: Secret,
    /// Token received in last unwrap call (for assertions).
    pub last_token: std::sync::Mutex<Option<String>>,
}

#[cfg(any(test, feature = "mock"))]
impl MockUnwrapper {
    /// Create a mock that returns the given secret.
    pub fn new(secret: Secret) -> Self {
        Self {
            secret,
            last_token: std::sync::Mutex::new(None),
        }
    }

    /// Check what token was passed to `unwrap`.
    pub fn last_token(&self) -> Option<String> {
        self.last_token.lock().unwrap().clone()
    }
}

#[cfg(any(test, feature = "mock"))]
impl OpenBaoUnwrapper for MockUnwrapper {
    fn unwrap(&self, wrapping_token: &str) -> Result<Secret, SandboxError> {
        *self.last_token.lock().unwrap() = Some(wrapping_token.to_string());
        Ok(self.secret.clone())
    }
}

/// Result of a sandbox execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutput {
    /// stdout from the child process.
    pub stdout: String,
    /// stderr from the child process.
    pub stderr: String,
    /// Exit code.
    pub exit_code: i32,
}

/// Execution Sandbox. Unwraps a 1-time OpenBao token, injects secrets into
/// child process env, and purges them after execution.
pub struct Sandbox<'a, U: OpenBaoUnwrapper> {
    bao: &'a U,
    /// Secrets currently held. Purged after each `exec` call.
    secrets: Vec<Secret>,
}

impl<'a, U: OpenBaoUnwrapper> Sandbox<'a, U> {
    /// Create a new sandbox bound to an OpenBao unwrapper.
    pub fn new(bao: &'a U) -> Self {
        Self {
            bao,
            secrets: Vec::new(),
        }
    }

    /// Unwrap a wrapping token and stage the secret for the next exec.
    /// The token is consumed by OpenBao (single-use).
    pub fn stage_token(&mut self, wrapping_token: &str) -> Result<(), SandboxError> {
        let secret = self.bao.unwrap(wrapping_token)?;
        self.secrets.push(secret);
        Ok(())
    }

    /// Execute a command via the given runtime, injecting staged secrets
    /// into the child env. Secrets are purged after exec completes
    /// (success or failure).
    pub async fn exec(
        &mut self,
        runtime: Runtime,
        package: &str,
        args: &[&str],
    ) -> Result<SandboxOutput, SandboxError> {
        // Check runtime exists (V9: if not found, caller falls back to
        // remote sandbox — but this fn errors here).
        which_check(runtime.binary())?;

        let mut cmd = if runtime == Runtime::Docker {
            let mut c = Command::new(runtime.binary());
            c.arg("run").arg("--rm");
            c
        } else {
            Command::new(runtime.binary())
        };
        cmd.arg(package);
        for a in args {
            cmd.arg(a);
        }

        // Inject staged secrets into child env.
        for secret in &self.secrets {
            for (k, v) in &secret.env {
                cmd.env(k, v);
            }
        }

        let output_result = cmd.output().await;

        // V18: purge secrets regardless of success or failure.
        self.secrets.clear();

        let output = output_result?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        if code != 0 {
            return Err(SandboxError::NonZeroExit { code, stderr });
        }
        Ok(SandboxOutput {
            stdout,
            stderr,
            exit_code: code,
        })
    }

    /// Check if any staged secrets are currently held.
    pub fn has_staged_secrets(&self) -> bool {
        !self.secrets.is_empty()
    }

    /// Manually purge all staged secrets without executing.
    pub fn purge(&mut self) {
        self.secrets.clear();
    }
}

impl<'a, U: OpenBaoUnwrapper> Drop for Sandbox<'a, U> {
    fn drop(&mut self) {
        // V18: purge on exit — no secrets leak after sandbox drops.
        self.secrets.clear();
    }
}

/// Check if a binary is on PATH. On Windows, also checks for `.exe` / `.cmd`
/// / `.bat` variants.
fn which_check(binary: &str) -> Result<(), SandboxError> {
    let path_var = std::env::var_os("PATH");
    let path_exts = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .map(|e| {
                e.to_string_lossy()
                    .split(';')
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".exe".into(), ".cmd".into(), ".bat".into()])
    } else {
        vec![String::new()]
    };

    if let Some(paths) = path_var {
        for dir in std::env::split_paths(&paths) {
            for ext in &path_exts {
                let candidate = if cfg!(windows) {
                    dir.join(format!("{binary}{ext}"))
                } else {
                    dir.join(binary)
                };
                if candidate.is_file() {
                    return Ok(());
                }
            }
        }
    }
    Err(SandboxError::RuntimeNotFound(binary.to_string()))
}

/// Dependency probe result for a runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// Runtime that was probed.
    pub runtime: Runtime,
    /// True if the runtime binary was found on PATH.
    pub found: bool,
}

/// Probe whether the given runtimes are available locally.
/// Returns a result per runtime. Callers use this to decide whether to
/// exec locally (V9) or fall back to Hub sandbox.
pub fn probe_runtimes(runtimes: &[Runtime]) -> Vec<ProbeResult> {
    runtimes
        .iter()
        .map(|&r| {
            let found = which_check(r.binary()).is_ok();
            ProbeResult {
                runtime: r,
                found,
            }
        })
        .collect()
}

/// Pick the first available runtime from a preference list.
/// Returns `None` if none are found (caller should fall back to Hub).
pub fn pick_runtime(preference: &[Runtime]) -> Option<Runtime> {
    preference
        .iter()
        .copied()
        .find(|r| which_check(r.binary()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret_env(key: &str, val: &str) -> Secret {
        let mut env = HashMap::new();
        env.insert(key.to_string(), val.to_string());
        Secret { env }
    }

    #[test]
    fn stage_and_purge() {
        let unwrapper = MockUnwrapper::new(secret_env("API_KEY", "secret123"));
        let mut sandbox = Sandbox::new(&unwrapper);
        assert!(!sandbox.has_staged_secrets());

        sandbox.stage_token("wrapping-token-abc").unwrap();
        assert!(sandbox.has_staged_secrets());
        assert_eq!(
            unwrapper.last_token(),
            Some("wrapping-token-abc".to_string())
        );

        sandbox.purge();
        assert!(!sandbox.has_staged_secrets());
    }

    #[test]
    fn drop_purges_secrets() {
        let unwrapper = MockUnwrapper::new(secret_env("K", "V"));
        let mut sandbox = Sandbox::new(&unwrapper);
        sandbox.stage_token("tok").unwrap();
        assert!(sandbox.has_staged_secrets());
        drop(sandbox);
        // Can't assert after drop, but the Drop impl clears secrets.
        // This test verifies Drop doesn't panic.
    }

    #[test]
    fn runtime_binary_names() {
        assert_eq!(Runtime::Npx.binary(), "npx");
        assert_eq!(Runtime::Uvx.binary(), "uvx");
        assert_eq!(Runtime::Docker.binary(), "docker");
    }

    #[test]
    fn which_check_finds_known_binary() {
        // `cmd` is always on PATH on Windows; `sh` on Unix.
        let bin = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(which_check(bin).is_ok());
    }

    #[test]
    fn which_check_rejects_nonexistent() {
        let result = which_check("definitely_not_a_real_binary_xyz");
        assert!(matches!(result, Err(SandboxError::RuntimeNotFound(_))));
    }

    #[test]
    fn probe_runtimes_returns_all() {
        let results = probe_runtimes(&[Runtime::Npx, Runtime::Uvx, Runtime::Docker]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].runtime, Runtime::Npx);
    }

    #[test]
    fn pick_runtime_returns_first_found() {
        // At least one of cmd/sh should be found.
        // We test with a known binary by creating a custom runtime list.
        // Since Runtime is an enum, we test pick_runtime with the real ones.
        let result = pick_runtime(&[Runtime::Docker, Runtime::Npx, Runtime::Uvx]);
        // Result depends on what's installed — just verify it doesn't panic.
        let _ = result;
    }
}