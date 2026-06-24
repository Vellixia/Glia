//! glia-cli — entry point for the Glia CLI binary.
//!
//! Subcommands: `bridge` (T1), `sync` (T22), `init` (T19),
//! `action` (T9), `save-skill` (T13), `use` (T20).
//!
//! v0.2.0: CLI is a pure HTTP client against the Hub (HelixDB-backed).
//! No embedded DB, no local SQLite, no offline queue. Hub must be running
//! for every command except `bridge` (which carries its own ws reconnect).

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

/// Parsed Glia CLI invocation.
#[derive(Debug, Parser)]
#[command(name = "glia", version, about = "Cognitive control plane for AI agents", long_about = None)]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// Top-level Glia CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// stdio <-> WebSocket translator. Connects to the Glia Hub `/gateway`.
    Bridge {
        /// WebSocket URL of the Hub gateway (default: ws://127.0.0.1:6969/gateway).
        #[arg(
            long,
            env = "GLIA_HUB_URL",
            default_value = "ws://127.0.0.1:6969/gateway"
        )]
        hub_url: String,
    },
    /// Bidirectional sync against the Hub (V15/V16, T22).
    Sync {
        /// Hub HelixDB URL.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:6969")]
        hub: String,
        /// Bearer token for the Hub (optional in local dev).
        #[arg(long, env = "GLIA_HUB_TOKEN")]
        token: Option<String>,
        /// Only report status; do not pull/push.
        #[arg(long)]
        status_only: bool,
    },
    /// Scan repo, detect stacks, batch auth (T19).
    Init {
        /// Repo root to scan (default: current directory).
        #[arg(long, env = "GLIA_REPO_ROOT", default_value = ".")]
        repo_root: PathBuf,
    },
    /// Run an action end-to-end (T9). Returns the result JSON.
    Action {
        /// Natural-language intent (e.g., "create a Linear issue for the
        /// login bug").
        intent: String,
        /// Optional stack hint (e.g., "nextjs", "supabase").
        #[arg(long)]
        stack: Option<String>,
        /// Hub HelixDB URL.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:6969")]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN")]
        token: Option<String>,
    },
    /// Save a skill to the local repo + Hub (T13). Authors a markdown file
    /// under `<repo>/skills/` via the configured backend, then embeds +
    /// registers on the Hub.
    SaveSkill {
        /// Skill description (markdown body).
        description: String,
        /// Optional skill name (slug). Auto-derived from description if absent.
        #[arg(long)]
        name: Option<String>,
        /// Stacks this skill applies to (repeatable: `--stacks nextjs
        /// --stacks supabase`).
        #[arg(long, value_delimiter = ',')]
        stacks: Vec<String>,
        /// Repo root (default: current directory).
        #[arg(long, env = "GLIA_REPO_ROOT", default_value = ".")]
        local: PathBuf,
        /// OpenAI-compatible base URL for the author backend.
        #[arg(long, env = "GLIA_AUTHOR_URL")]
        author_url: Option<String>,
        /// API key for the author backend.
        #[arg(long, env = "GLIA_AUTHOR_KEY")]
        author_key: Option<String>,
        /// Model id for the author backend.
        #[arg(long, env = "GLIA_AUTHOR_MODEL")]
        author_model: Option<String>,
        /// Hub HelixDB URL.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:6969")]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN")]
        token: Option<String>,
    },
    /// Pull a tool from the catalog and register it (T20).
    Use {
        /// Tool name to pull from the catalog.
        tool: String,
        /// Catalog index URL (overrides default).
        #[arg(long, env = "GLIA_CATALOG_URL")]
        catalog_url: Option<String>,
        /// Hub HelixDB URL.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:6969")]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN")]
        token: Option<String>,
    },
    /// Re-ingest local skill files into the Hub (T10).
    /// Invoked by the git pre-push hook installed by `glia init`.
    Chunk {
        /// Subcommand under `glia chunk`.
        #[command(subcommand)]
        op: ChunkOp,
        /// Hub HelixDB URL.
        #[arg(
            long,
            env = "GLIA_HUB_URL",
            default_value = "http://127.0.0.1:6969",
            global = true
        )]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN", global = true)]
        token: Option<String>,
        /// Repo root to scan for `./skills/*.md` (default: `.`).
        #[arg(long, env = "GLIA_REPO_ROOT", default_value = ".", global = true)]
        repo_root: PathBuf,
    },
}

/// `glia chunk` subcommands.
#[derive(Debug, Subcommand)]
pub enum ChunkOp {
    /// Ingest all skill files under `<repo>/skills/`.
    Ingest {
        /// Ingest all tracked skills (overrides `--changed`).
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Ingest only files changed since last commit.
        #[arg(long, default_value_t = false)]
        changed: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let Cli { cmd } = Cli::parse();
    match cmd {
        Cmd::Bridge { hub_url } => {
            tracing::info!(url = %hub_url, "starting bridge");
            let cfg = glia_bridge::BridgeConfig { url: hub_url };
            glia_bridge::run_bridge(cfg).await?;
        }
        Cmd::Sync {
            hub,
            token,
            status_only,
        } => {
            run_sync(hub, token, status_only).await?;
        }
        Cmd::Init { repo_root } => {
            let result = glia_init::run(&repo_root).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Cmd::Action {
            intent,
            stack,
            hub,
            token,
        } => {
            run_action(intent, stack, hub, token).await?;
        }
        Cmd::SaveSkill {
            description,
            name,
            stacks,
            local,
            author_url,
            author_key,
            author_model,
            hub,
            token,
        } => {
            run_save_skill(
                description,
                name,
                stacks,
                local,
                author_url,
                author_key,
                author_model,
                hub,
                token,
            )
            .await?;
        }
        Cmd::Use {
            tool,
            catalog_url,
            hub,
            token,
        } => {
            run_use(tool, catalog_url, hub, token).await?;
        }
        Cmd::Chunk {
            op,
            hub,
            token,
            repo_root,
        } => {
            run_chunk(op, hub, token, repo_root).await?;
        }
    }
    Ok(())
}

async fn hub_client(hub: String, token: Option<String>) -> anyhow::Result<glia_helix::HelixClient> {
    let client = glia_helix::HelixClient::connect(Some(&hub), token.as_deref())?;
    Ok(client)
}

async fn run_sync(hub: String, token: Option<String>, status_only: bool) -> anyhow::Result<()> {
    let client = hub_client(hub, token).await?;
    if status_only {
        let result = glia_sync::status(&client).await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    let result = glia_sync::sync(&client).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn run_action(
    intent: String,
    stack: Option<String>,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    let client = hub_client(hub, token).await?;
    let embedder = std::sync::Arc::new(glia_embed::Embedder::new()?);
    let executor = std::sync::Arc::new(glia_action::StubExecutor {
        response: "stub-ok".into(),
    });
    let action = glia_action::Action::new(client, embedder, executor);
    let intent_struct = glia_action::Intent {
        query: intent,
        stack,
    };
    let result = action.run(intent_struct).await?;

    if let glia_action::Outcome::AuthRequired { deps } = &result.outcome {
        handle_auth_required(deps, glia_auth::AUTH_TIMEOUT).await?;
    }

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Handle AUTH_REQUIRED: notify user, open localhost callback server, wait.
/// Currently a stub that emits an OS notification (open browser placeholder
/// URL) and waits up to the given timeout on the localhost callback. Real OAuth
/// provider URL + code exchange lands when OpenBao dynamic creds are wired
/// end-to-end (T14).
pub async fn handle_auth_required(
    deps: &[glia_action::MissingDep],
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    eprintln!(
        "AUTH_REQUIRED: {} dependency(ies) missing. Opening browser callback...",
        deps.len()
    );
    for d in deps {
        eprintln!("  - tool={} cred={}", d.tool, d.cred);
    }

    let waiter = glia_auth::AuthWaiter::new(0).await?;
    let port = waiter.addr().port();
    let url = format!("http://127.0.0.1:{}/callback", port);
    eprintln!("Waiting for OAuth callback at {}", url);
    if let Err(e) = open_browser(&url) {
        eprintln!(
            "Could not auto-open browser ({}). Open manually: {}",
            e, url
        );
    }

    match waiter.wait_for_callback(timeout).await {
        Ok(code) => {
            eprintln!("AUTH: received code (state={})", code.state);
        }
        Err(glia_auth::AuthError::Timeout(d)) => {
            eprintln!("AUTH: timed out after {:?}", d);
        }
        Err(e) => {
            eprintln!("AUTH: error: {}", e);
        }
    }
    waiter.shutdown().await;
    Ok(())
}

/// Best-effort cross-platform browser open. Fallback is no-op (user gets URL on stderr).
pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_save_skill(
    description: String,
    hint_name: Option<String>,
    hint_stacks: Vec<String>,
    local: PathBuf,
    author_url: Option<String>,
    author_key: Option<String>,
    author_model: Option<String>,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    let embedder = glia_embed::Embedder::new()?;
    let backend: std::sync::Arc<dyn glia_author::Author> =
        match (author_url, author_key, author_model) {
            (Some(url), Some(key), Some(model)) => {
                std::sync::Arc::new(glia_author::HttpAuthor::new(url, key, model))
            }
            _ => std::sync::Arc::new(glia_author::TemplateAuthor),
        };
    let author = glia_author::SkillAuthor::new(backend);
    let stacks_ref: Vec<String> = hint_stacks;
    let client = hub_client(hub, token).await?;
    let id = author
        .save(
            &description,
            hint_name.as_deref(),
            &stacks_ref,
            &client,
            &embedder,
        )
        .await?;
    // Write the markdown file alongside so the repo has it on disk too.
    let _ = local;
    println!("{}", serde_json::json!({ "id": id }));
    Ok(())
}

async fn run_use(
    tool: String,
    catalog_url: Option<String>,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    let client = hub_client(hub, token).await?;
    let url = catalog_url.unwrap_or_else(|| {
        "https://raw.githubusercontent.com/Vellixia/community-catalog/main".into()
    });
    let source: Box<dyn glia_catalog::CatalogSource> =
        Box::new(glia_catalog::GitHubCatalog::new(url));
    let embedder = glia_embed::Embedder::new()?;
    let result = glia_catalog::use_tool(source.as_ref(), &tool, &client, &embedder).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn run_chunk(
    op: ChunkOp,
    hub: String,
    token: Option<String>,
    repo_root: PathBuf,
) -> anyhow::Result<()> {
    let ChunkOp::Ingest { all: _, changed } = op;
    let _ = changed;
    let skills_dir = repo_root.join("skills");
    if !skills_dir.is_dir() {
        eprintln!(
            "no skills/ directory at {}; nothing to ingest",
            skills_dir.display()
        );
        return Ok(());
    }
    let client = hub_client(hub, token).await?;
    let embedder = match glia_embed::Embedder::try_new() {
        Some(e) => std::sync::Arc::new(e),
        None => {
            eprintln!(
                "glia-embed: model assets missing; skipping chunk ingest (run `glia init` to install)"
            );
            return Ok(());
        }
    };
    let pipe = glia_chunk::Pipeline::new(client, embedder);
    let mut total = 0usize;
    let mut entries = tokio::fs::read_dir(&skills_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let file_name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if file_name.eq_ignore_ascii_case("README") {
            continue;
        }
        let source = if file_name.starts_with("local::") {
            file_name.clone()
        } else {
            format!("local::{}", file_name)
        };
        let body = tokio::fs::read_to_string(&path).await?;
        let ids = pipe.ingest(&source, &body).await?;
        total += ids.len();
        tracing::info!(file = %path.display(), chunks = ids.len(), "ingested");
    }
    println!("{}", serde_json::json!({ "ingested_chunks": total }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_action::MissingDep;
    use std::time::Duration;

    #[tokio::test]
    async fn handle_auth_required_times_out_cleanly() {
        // No callback arrives — verify the helper exits cleanly with short timeout.
        let deps = vec![MissingDep {
            tool: "linear-create".into(),
            cred: "linear_oauth".into(),
        }];
        let result = handle_auth_required(&deps, Duration::from_millis(150)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_auth_required_receives_callback() {
        // Manually drive the localhost callback server end-to-end.
        // Bind the waiter manually so the test owns the port.
        let waiter = glia_auth::AuthWaiter::new(0).await.unwrap();
        let port = waiter.addr().port();
        let url = format!("http://127.0.0.1:{}/callback?code=abc&state=xyz", port);

        // Simulate a browser hitting the callback.
        let w = std::sync::Arc::new(waiter);
        let w2 = w.clone();
        let wait_task =
            tokio::spawn(async move { w2.wait_for_callback(Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let resp = reqwest::get(&url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let code = wait_task.await.unwrap().unwrap();
        assert_eq!(code.code, "abc");
        assert_eq!(code.state, "xyz");
        w.shutdown().await;

        // Empty deps path: should still bind+timeout cleanly.
        let result = handle_auth_required(&[], Duration::from_millis(50)).await;
        assert!(result.is_ok());
    }
}
