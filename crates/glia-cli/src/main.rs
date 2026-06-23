//! glia-cli — entry point for the Glia CLI binary.
//!
//! Subcommands: `bridge` (T1), `sync` (T22), `init` (T19),
//! `action` (T9), `save-skill` (T13), `use` (T20).

use std::path::PathBuf;
use std::sync::Arc;

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
    /// Bidirectional sync between local and Hub SurrealDB (V15/V16, T22).
    Sync {
        /// Path to the local SurrealKV data directory.
        #[arg(long, env = "GLIA_LOCAL_DB", default_value = "./.glia/local.db")]
        local: PathBuf,
        /// Hub SurrealDB WebSocket URL.
        #[arg(long, env = "GLIA_HUB_DB", default_value = "ws://127.0.0.1:8000")]
        hub: String,
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
    /// Unified tool discover + skill fetch + exec (T9).
    Action {
        /// Natural-language intent query.
        #[arg(long)]
        intent: String,
        /// Optional stack filter (e.g., `nextjs`).
        #[arg(long)]
        stack: Option<String>,
        /// Path to the local SurrealKV data directory.
        #[arg(long, env = "GLIA_LOCAL_DB", default_value = "./.glia/local.db")]
        local: PathBuf,
    },
    /// Author and register a new local skill (T13).
    SaveSkill {
        /// Description of the skill to author.
        #[arg(long)]
        description: String,
        /// Optional hint name.
        #[arg(long)]
        name: Option<String>,
        /// Optional comma-separated stack hints (e.g., `nextjs,supabase`).
        #[arg(long, value_delimiter = ',')]
        stacks: Vec<String>,
        /// Path to the local SurrealKV data directory.
        #[arg(long, env = "GLIA_LOCAL_DB", default_value = "./.glia/local.db")]
        local: PathBuf,
        /// OpenAI-compatible base URL for LLM backend (default: stub).
        #[arg(long, env = "GLIA_AUTHOR_URL")]
        author_url: Option<String>,
        /// API key for LLM backend.
        #[arg(long, env = "GLIA_AUTHOR_KEY")]
        author_key: Option<String>,
        /// Model name for LLM backend.
        #[arg(long, env = "GLIA_AUTHOR_MODEL")]
        author_model: Option<String>,
    },
    /// Pull a community tool from the catalog and register it (T20).
    Use {
        /// Community tool name (e.g., `supabase-auth`).
        #[arg(long)]
        tool: String,
        /// Catalog base URL (GitHub raw catalog.json location).
        #[arg(long, env = "GLIA_CATALOG_URL")]
        catalog_url: Option<String>,
        /// Path to the local SurrealKV data directory.
        #[arg(long, env = "GLIA_LOCAL_DB", default_value = "./.glia/local.db")]
        local: PathBuf,
    },
    /// Re-ingest local skill files into the embedded SurrealDB (T10).
    /// Invoked by the git pre-push hook installed by `glia init`.
    Chunk {
        /// Subcommand under `glia chunk`.
        #[command(subcommand)]
        op: ChunkOp,
        /// Path to the local SurrealKV data directory.
        #[arg(long, env = "GLIA_LOCAL_DB", default_value = "./.glia/local.db", global = true)]
        local: PathBuf,
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
            local,
            hub,
            status_only,
        } => {
            run_sync(local, hub, status_only).await?;
        }
        Cmd::Init { repo_root } => {
            let result = glia_init::run(&repo_root).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Cmd::Action {
            intent,
            stack,
            local,
        } => {
            run_action(intent, stack, local).await?;
        }
        Cmd::SaveSkill {
            description,
            name,
            stacks,
            local,
            author_url,
            author_key,
            author_model,
        } => {
            run_save_skill(
                description,
                name,
                stacks,
                local,
                author_url,
                author_key,
                author_model,
            )
            .await?;
        }
        Cmd::Use {
            tool,
            catalog_url,
            local,
        } => {
            run_use(tool, catalog_url, local).await?;
        }
        Cmd::Chunk { op, local, repo_root } => {
            run_chunk(op, local, repo_root).await?;
        }
    }
    Ok(())
}

async fn ensure_local_db(local: PathBuf) -> anyhow::Result<Arc<glia_db::GliaDb>> {
    if let Some(parent) = local.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let db = Arc::new(glia_db::GliaDb::connect(glia_db::Connection::Embedded(local)).await?);
    db.init_schema().await?;
    Ok(db)
}

async fn run_sync(local: PathBuf, hub: String, status_only: bool) -> anyhow::Result<()> {
    let local_db = ensure_local_db(local).await?;
    let hub_db = match glia_db::GliaDb::connect(glia_db::Connection::Remote(hub.clone())).await {
        Ok(db) => Some(Arc::new(db)),
        Err(e) => {
            tracing::warn!(error = %e, "hub unreachable; running status only");
            None
        }
    };
    if status_only || hub_db.is_none() {
        let diffs = glia_sync::status_offline(local_db).await?;
        println!("{}", serde_json::to_string_pretty(&diffs)?);
        if hub_db.is_none() {
            eprintln!("HUB_UNREACHABLE: skipped pull/push; queued changes persist locally");
            std::process::exit(2);
        }
    } else if let Some(hub_db) = hub_db {
        let engine = glia_sync::SyncEngine::new(local_db, hub_db);
        let result = engine.sync().await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

async fn run_action(intent: String, stack: Option<String>, local: PathBuf) -> anyhow::Result<()> {
    let db = ensure_local_db(local).await?;
    let embedder = Arc::new(glia_embed::Embedder::new()?);
    let executor = Arc::new(glia_action::StubExecutor {
        response: "stub-ok".into(),
    });
    let action = glia_action::Action::new(db, embedder, executor);
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

async fn run_save_skill(
    description: String,
    hint_name: Option<String>,
    hint_stacks: Vec<String>,
    local: PathBuf,
    author_url: Option<String>,
    author_key: Option<String>,
    author_model: Option<String>,
) -> anyhow::Result<()> {
    let db = ensure_local_db(local).await?;
    let embedder = glia_embed::Embedder::new()?;
    let backend: Arc<dyn glia_author::Author> = match (author_url, author_key, author_model) {
        (Some(url), Some(key), Some(model)) => {
            Arc::new(glia_author::HttpAuthor::new(url, key, model))
        }
        _ => Arc::new(glia_author::TemplateAuthor),
    };
    let author = glia_author::SkillAuthor::new(backend);
    let stacks_ref: Vec<String> = hint_stacks;
    let id = author
        .save(
            &description,
            hint_name.as_deref(),
            &stacks_ref,
            &db,
            &embedder,
        )
        .await?;
    println!("{}", serde_json::json!({ "id": id }));
    Ok(())
}

async fn run_use(tool: String, catalog_url: Option<String>, local: PathBuf) -> anyhow::Result<()> {
    let db = ensure_local_db(local).await?;
    let embedder = glia_embed::Embedder::new()?;
    let url = catalog_url.unwrap_or_else(|| {
        "https://raw.githubusercontent.com/Vellixia/community-catalog/main".into()
    });
    let source: Box<dyn glia_catalog::CatalogSource> =
        Box::new(glia_catalog::GitHubCatalog::new(url));
    let result = glia_catalog::use_tool(source.as_ref(), &tool, &db, &embedder).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn run_chunk(op: ChunkOp, local: PathBuf, repo_root: PathBuf) -> anyhow::Result<()> {
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
    let db = ensure_local_db(local).await?;
    let embedder = match glia_embed::Embedder::try_new() {
        Some(e) => Arc::new(e),
        None => {
            eprintln!(
                "glia-embed: model assets missing; skipping chunk ingest (run `glia init` to install)"
            );
            return Ok(());
        }
    };
    let pipe = glia_chunk::Pipeline::new(db, embedder);
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
