//! glia-init — `glia init` repo scan + stack detect + batch auth (T19, V8).
//!
//! Scans a repo directory for tech stack markers, registers detected
//! stacks in the DB, and prompts for batch auth setup (OAuth/API keys).
//!
//! Stack detection heuristics:
//! - `package.json` → Node.js (check deps for next/react/supabase/etc.)
//! - `Cargo.toml` → Rust
//! - `pyproject.toml` / `requirements.txt` → Python
//! - `go.mod` → Go
//! - `*.sql` migrations → Supabase/Postgres
//!
//! Auth batch: for each detected remote tool, check if a cred exists;
//! if not, mark as "needs auth" for the caller to handle.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Errors from init.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parse failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// DB operation failed.
    #[error("db: {0}")]
    Db(String),
}

/// A detected stack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedStack {
    /// Stack id (e.g., `nextjs`, `rust`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Evidence (file that triggered detection).
    pub evidence: String,
}

/// Result of `glia init`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    /// Detected stacks.
    pub stacks: Vec<DetectedStack>,
    /// Creds that need auth setup.
    pub pending_auth: Vec<String>,
    /// Scanned file count.
    pub files_scanned: usize,
    /// Paths written by hook install (empty if hooks skipped).
    #[serde(default)]
    pub hooks_installed: Vec<String>,
    /// `true` if no git repo found at `repo_root` (hooks skipped).
    #[serde(default)]
    pub git_repo_missing: bool,
}

/// Scan a repo for tech stacks.
pub async fn scan_repo(repo_root: &Path) -> Result<Vec<DetectedStack>, InitError> {
    let mut stacks = Vec::new();
    // package.json
    let pkg_json = repo_root.join("package.json");
    if pkg_json.is_file() {
        let content = tokio::fs::read_to_string(&pkg_json).await?;
        let pkg: serde_json::Value = serde_json::from_str(&content)?;
        let deps = pkg.get("dependencies").and_then(|d| d.as_object());
        let dev_deps = pkg.get("devDependencies").and_then(|d| d.as_object());
        let has_dep = |name: &str| -> bool {
            deps.map(|d| d.contains_key(name)).unwrap_or(false)
                || dev_deps.map(|d| d.contains_key(name)).unwrap_or(false)
        };
        if has_dep("next") {
            stacks.push(DetectedStack {
                id: "nextjs".into(),
                name: "Next.js".into(),
                evidence: "package.json: next".into(),
            });
        }
        if has_dep("react") {
            stacks.push(DetectedStack {
                id: "react".into(),
                name: "React".into(),
                evidence: "package.json: react".into(),
            });
        }
        if has_dep("@supabase/supabase-js") {
            stacks.push(DetectedStack {
                id: "supabase".into(),
                name: "Supabase".into(),
                evidence: "package.json: @supabase/supabase-js".into(),
            });
        }
        if has_dep("stripe") {
            stacks.push(DetectedStack {
                id: "stripe".into(),
                name: "Stripe".into(),
                evidence: "package.json: stripe".into(),
            });
        }
        if stacks.is_empty() {
            stacks.push(DetectedStack {
                id: "node".into(),
                name: "Node.js".into(),
                evidence: "package.json".into(),
            });
        }
    }
    // Cargo.toml
    let cargo_toml = repo_root.join("Cargo.toml");
    if cargo_toml.is_file() {
        stacks.push(DetectedStack {
            id: "rust".into(),
            name: "Rust".into(),
            evidence: "Cargo.toml".into(),
        });
    }
    // pyproject.toml / requirements.txt
    let pyproject = repo_root.join("pyproject.toml");
    let requirements = repo_root.join("requirements.txt");
    if pyproject.is_file() || requirements.is_file() {
        let ev = if pyproject.is_file() {
            "pyproject.toml"
        } else {
            "requirements.txt"
        };
        stacks.push(DetectedStack {
            id: "python".into(),
            name: "Python".into(),
            evidence: ev.into(),
        });
    }
    // go.mod
    let go_mod = repo_root.join("go.mod");
    if go_mod.is_file() {
        stacks.push(DetectedStack {
            id: "go".into(),
            name: "Go".into(),
            evidence: "go.mod".into(),
        });
    }
    // Supabase migrations dir
    let supa_migrations = repo_root.join("supabase").join("migrations");
    if supa_migrations.is_dir() && !stacks.iter().any(|s| s.id == "supabase") {
        stacks.push(DetectedStack {
            id: "supabase".into(),
            name: "Supabase".into(),
            evidence: "supabase/migrations/".into(),
        });
    }
    Ok(stacks)
}

/// Count files in a repo (up to a limit).
pub async fn count_files(repo_root: &Path, limit: usize) -> Result<usize, InitError> {
    let mut count = 0;
    count_files_recursive(repo_root, &mut count, limit).await?;
    Ok(count)
}

async fn count_files_recursive(
    dir: &Path,
    count: &mut usize,
    limit: usize,
) -> Result<(), InitError> {
    if *count >= limit {
        return Ok(());
    }
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip hidden + common ignore dirs.
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            Box::pin(count_files_recursive(&path, count, limit)).await?;
        } else {
            *count += 1;
            if *count >= limit {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Map detected stacks to cred ids that need auth.
pub fn pending_auth_for_stacks(stacks: &[DetectedStack]) -> Vec<String> {
    let mut creds = Vec::new();
    for s in stacks {
        match s.id.as_str() {
            "supabase" => {
                creds.push("supabase".into());
            }
            "stripe" => {
                creds.push("stripe".into());
            }
            "nextjs" | "react" => {
                // Often paired with OAuth providers.
            }
            _ => {}
        }
    }
    creds
}

/// Run full `glia init` scan + hook install.
pub async fn run(repo_root: &Path) -> Result<InitResult, InitError> {
    let stacks = scan_repo(repo_root).await?;
    let files_scanned = count_files(repo_root, 1000).await?;
    let pending_auth = pending_auth_for_stacks(&stacks);
    let (hooks_installed, git_repo_missing) = install_hooks(repo_root).await?;
    Ok(InitResult {
        stacks,
        pending_auth,
        files_scanned,
        hooks_installed,
        git_repo_missing,
    })
}

/// Install git pre-push + Cursor/Claude hooks for the repo.
/// Returns the list of paths written + whether a git repo was found.
pub async fn install_hooks(repo_root: &Path) -> Result<(Vec<String>, bool), InitError> {
    let mut written = Vec::new();
    let git_dir = repo_root.join(".git");
    let git_repo_missing = !git_dir.is_dir();
    if !git_repo_missing {
        match glia_chunk::git::install_pre_push(repo_root) {
            Ok(path) => written.push(path.to_string_lossy().into_owned()),
            Err(e) => tracing::warn!(error = %e, "pre-push install failed"),
        }
    }
    let stack_names: Vec<String> = {
        let stacks = scan_repo(repo_root).await?;
        stacks.into_iter().map(|s| s.id).collect()
    };
    // v0.2.0: CLI has no local DB; pull skill list from the Hub. Falls
    // back to empty if the Hub is unreachable — hooks are still emitted
    // (Cursor rules + Claude hooks) without skill citations.
    let hub_url =
        std::env::var("GLIA_HUB_URL").unwrap_or_else(|_| "http://127.0.0.1:6969".to_string());
    let hub_token = std::env::var("GLIA_HUB_TOKEN").ok();
    let skills: Vec<glia_helix::Skill> =
        match glia_helix::HelixClient::connect(Some(&hub_url), hub_token.as_deref()) {
            Ok(client) => match client.list_skills().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "hub skill list failed; hooks for empty set");
                    Vec::new()
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "hub unreachable; hooks for empty set");
                Vec::new()
            }
        };
    match glia_hooks::generate_cursor_rules(repo_root, &skills, &stack_names).await {
        Ok(paths) => written.extend(paths),
        Err(e) => tracing::warn!(error = %e, "cursor rules failed"),
    }
    match glia_hooks::generate_claude_hooks(repo_root, &stack_names).await {
        Ok(path) => written.push(path),
        Err(e) => tracing::warn!(error = %e, "claude hooks failed"),
    }
    Ok((written, git_repo_missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    async fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(path, content).await.unwrap();
    }

    fn tmp() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        std::env::temp_dir().join(format!("glia-init-{}-{}", pid, nanos))
    }

    #[tokio::test]
    async fn scan_nextjs_repo() {
        let dir = tmp();
        write(
            &dir.join("package.json"),
            r#"{"dependencies":{"next":"14","react":"18"}}"#,
        )
        .await;
        let stacks = scan_repo(&dir).await.unwrap();
        assert!(stacks.iter().any(|s| s.id == "nextjs"));
        assert!(stacks.iter().any(|s| s.id == "react"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn scan_rust_repo() {
        let dir = tmp();
        write(
            &dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1\"\n",
        )
        .await;
        let stacks = scan_repo(&dir).await.unwrap();
        assert!(stacks.iter().any(|s| s.id == "rust"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn scan_supabase_migrations() {
        let dir = tmp();
        write(&dir.join("supabase/migrations/001.sql"), "-- migration\n").await;
        let stacks = scan_repo(&dir).await.unwrap();
        assert!(stacks.iter().any(|s| s.id == "supabase"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn scan_python_repo() {
        let dir = tmp();
        write(&dir.join("requirements.txt"), "flask\nrequests\n").await;
        let stacks = scan_repo(&dir).await.unwrap();
        assert!(stacks.iter().any(|s| s.id == "python"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn scan_empty_repo() {
        let dir = tmp();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let stacks = scan_repo(&dir).await.unwrap();
        assert!(stacks.is_empty());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn count_files_skips_node_modules() {
        let dir = tmp();
        write(&dir.join("package.json"), "{}").await;
        write(&dir.join("node_modules/x/index.js"), "x").await;
        write(&dir.join("src/index.ts"), "x").await;
        let count = count_files(&dir, 100).await.unwrap();
        assert_eq!(count, 2); // package.json + src/index.ts
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn count_files_respects_limit() {
        let dir = tmp();
        for i in 0..50 {
            write(&dir.join(format!("f{}.ts", i)), "x").await;
        }
        let count = count_files(&dir, 10).await.unwrap();
        assert!(count <= 11);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn pending_auth_supabase() {
        let stacks = vec![DetectedStack {
            id: "supabase".into(),
            name: "Supabase".into(),
            evidence: "x".into(),
        }];
        let creds = pending_auth_for_stacks(&stacks);
        assert!(creds.contains(&"supabase".to_string()));
    }

    #[test]
    fn pending_auth_empty_for_rust() {
        let stacks = vec![DetectedStack {
            id: "rust".into(),
            name: "Rust".into(),
            evidence: "x".into(),
        }];
        let creds = pending_auth_for_stacks(&stacks);
        assert!(creds.is_empty());
    }

    #[tokio::test]
    async fn run_full_init() {
        let dir = tmp();
        write(
            &dir.join("package.json"),
            r#"{"dependencies":{"next":"14","@supabase/supabase-js":"2"}}"#,
        )
        .await;
        write(&dir.join("src/app/page.tsx"), "x").await;
        let result = run(&dir).await.unwrap();
        assert!(result.stacks.iter().any(|s| s.id == "nextjs"));
        assert!(result.stacks.iter().any(|s| s.id == "supabase"));
        assert!(result.pending_auth.contains(&"supabase".to_string()));
        assert!(result.files_scanned >= 2);
        assert!(result.git_repo_missing);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn install_hooks_writes_files() {
        let dir = tmp();
        tokio::fs::create_dir_all(dir.join(".git/hooks"))
            .await
            .unwrap();
        write(
            &dir.join("package.json"),
            r#"{"dependencies":{"next":"14"}}"#,
        )
        .await;
        let (written, git_missing) = install_hooks(&dir).await.unwrap();
        assert!(!git_missing);
        assert!(written.iter().any(|p| p.contains("pre-push")));
        assert!(
            written
                .iter()
                .any(|p| p.contains(".claude") || p.contains("claude"))
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn install_hooks_with_skill_writes_cursor_rule() {
        let dir = tmp();
        tokio::fs::create_dir_all(dir.join(".git/hooks"))
            .await
            .unwrap();
        // v0.2.0: skill list now comes from the Hub. Test falls back to
        // empty skills if the Hub is unreachable — hooks are still
        // written, just without skill citations.
        let (written, _) = install_hooks(&dir).await.unwrap();
        assert!(written.iter().any(|p| p.contains(".cursor")));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn install_hooks_skips_pre_push_without_git() {
        let dir = tmp();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let (written, git_missing) = install_hooks(&dir).await.unwrap();
        assert!(git_missing);
        assert!(!written.iter().any(|p| p.contains("pre-push")));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
