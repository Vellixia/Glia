//! Shared helpers for the cli-flow SPEC-driven test suite.
//!
//! Spawn the `glia` CLI binary (built by the glia-cli crate) and
//! capture stdout / stderr / exit code. The Hub is forced to an
//! unreachable port via `GLIA_HUB_URL` so the dominant exit path
//! is `HUB_UNREACHABLE` (SPEC V15).

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Result of a CLI invocation in tests.
#[derive(Debug, Clone)]
pub struct CliRun {
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout (trimmed).
    pub stdout: String,
    /// Captured stderr (trimmed).
    pub stderr: String,
}

/// Resolve the path to the `glia` binary built by `cargo build`.
///
/// Search strategy:
///   1. `CARGO_BIN_EXE_glia` (set by cargo for the crate that owns the binary).
///   2. `<CARGO_TARGET_DIR>/debug/glia[.exe]` (Windows: `.exe` suffix).
///   3. Walk up from the current exe looking for `target/debug/glia[.exe]`.
///   4. `<cwd>/target/debug/glia[.exe]`.
///
/// The bit is that glia-cli is a different crate than glia-test-cli-flow,
/// so `CARGO_BIN_EXE_glia` is NOT set when running `cargo test -p
/// glia-test-cli-flow`. We must discover it manually.
pub fn glia_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_glia") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }

    // Pick the right binary name based on platform.
    let bin_name = if cfg!(windows) { "glia.exe" } else { "glia" };

    // Build candidate paths: workspace-root relative, and a few upward walks.
    let target_subdir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());

    let mut candidates: Vec<PathBuf> = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    candidates.push(cwd.join(&target_subdir).join("debug").join(bin_name));
    candidates.push(cwd.join("target").join("debug").join(bin_name));

    // Walk upward looking for `target/debug/glia[.exe]`.
    let mut p = cwd.as_path();
    for _ in 0..6 {
        let path = p.join(&target_subdir).join("debug").join(bin_name);
        candidates.push(path);
        match p.parent() {
            Some(parent) => p = parent,
            None => break,
        }
    }

    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| {
            panic!(
                "glia binary not found; build with `cargo build -p glia-cli` first. \
                 Searched: {:?}",
                cwd
            )
        })
}

/// Spawn `glia` with the given args and capture exit code + output.
///
/// Setting `GLIA_HUB_URL=http://127.0.0.1:1` (an unreachable port)
/// guarantees the Hub can't be reached — that's how the exit-code
/// tests inject a Hub-down scenario deterministically.
pub fn spawn_cli(args: &[&str]) -> CliRun {
    let bin = glia_bin();
    let child = Command::new(&bin)
        .args(args)
        .env("GLIA_HUB_URL", "http://127.0.0.1:1")
        .env("GLIA_HUB_TOKEN", "")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {}", bin.display(), e));

    let out = child.wait_with_output().expect("glia wait failed");
    CliRun {
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    }
}
