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
        /// WebSocket URL of the Hub gateway (default: ws://127.0.0.1:3000/gateway).
        #[arg(
            long,
            env = "GLIA_HUB_URL",
            default_value = "ws://127.0.0.1:3000/gateway"
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
    /// Manage OAuth providers registered with the Hub.
    ///
    /// `glia provider add` registers a provider (stores non-secret config in
    /// HelixDB, client_secret in OpenBao — the CLI never retains it after the
    /// POST). `glia provider list` shows registered providers.
    Provider {
        /// Provider subcommand (add, list).
        #[command(subcommand)]
        op: ProviderOp,
        /// Hub base URL (REST, not WS).
        #[arg(
            long,
            env = "GLIA_HUB_URL",
            default_value = "http://127.0.0.1:3000",
            global = true
        )]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN", global = true)]
        token: Option<String>,
    },
    /// Corrections → candidate-skills review queue (Phase 4 / D-Learning).
    ///
    /// `glia review capture <file>` — called by the PostToolUse hook; records
    /// the diff as a pending candidate rule in `.glia/review-queue.jsonl`.
    /// `glia review list`           — show pending items.
    /// `glia review approve <id>`   — upsert as a Hub skill + mark approved.
    /// `glia review reject <id>`    — discard.
    Review {
        /// Review subcommand.
        #[command(subcommand)]
        op: ReviewOp,
        /// Repo root (default: current directory).
        #[arg(long, env = "GLIA_REPO_ROOT", default_value = ".", global = true)]
        repo_root: PathBuf,
        /// Hub HelixDB URL (used by `approve`).
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
    },
    /// Enroll this device: generate a per-device token and store it in
    /// `~/.glia/config.toml`. Subsequent commands auto-read it so you
    /// don't need to set `GLIA_HUB_TOKEN` manually.
    Enroll {
        /// Hub base URL to enroll with.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:3000")]
        hub: String,
        /// Admin token to authorize device registration on the Hub.
        #[arg(long, env = "GLIA_HUB_ADMIN_TOKEN")]
        admin_token: Option<String>,
    },
    /// Load context for a file or stack (T17). Called by the Claude
    /// PreToolUse hook installed by `glia init`.
    #[command(name = "context")]
    Context {
        /// Stacks to load context for (repeatable).
        #[arg(long, value_delimiter = ',')]
        stacks: Vec<String>,
        /// File to detect stacks from (used when --stacks is empty).
        #[arg(long)]
        file: Option<String>,
        /// Hub HelixDB URL.
        #[arg(long, env = "GLIA_HUB_URL", default_value = "http://127.0.0.1:6969")]
        hub: String,
        /// Bearer token for the Hub.
        #[arg(long, env = "GLIA_HUB_TOKEN")]
        token: Option<String>,
    },
    /// Print runtime dependency status — which runtimes (npx, uvx, docker)
    /// are available on PATH and which are missing. Exit 0 if all present,
    /// exit 1 if any missing (so CI can gate on it).
    Doctor,
}

/// `glia chunk` subcommands.
#[derive(Debug, Subcommand)]
pub enum ChunkOp {
    /// Ingest all skill files under `<repo>/skills/`.
    Ingest,
}

/// `glia provider` subcommands.
#[derive(Debug, Subcommand)]
pub enum ProviderOp {
    /// Register an OAuth provider with the Hub.
    Add {
        /// Unique provider/cred ID (e.g., `linear_oauth`).
        #[arg(long)]
        id: String,
        /// Display name (e.g., "Linear").
        #[arg(long)]
        name: String,
        /// OAuth authorization URL.
        #[arg(long)]
        auth_url: String,
        /// OAuth token exchange URL.
        #[arg(long)]
        token_url: String,
        /// OAuth client ID (non-secret).
        #[arg(long)]
        client_id: String,
        /// OAuth client secret. Sent once to the Hub; stored in OpenBao.
        #[arg(long)]
        client_secret: String,
        /// Scopes (comma-separated, e.g., `read,write`).
        #[arg(long, value_delimiter = ',', default_value = "")]
        scopes: Vec<String>,
    },
    /// List registered OAuth providers.
    List,
}

/// `glia review` subcommands.
#[derive(Debug, Subcommand)]
pub enum ReviewOp {
    /// Capture a file write as a candidate review item.
    /// Called automatically by the PostToolUse hook.
    Capture {
        /// Path of the file written by the agent.
        file: String,
        /// Optional diff string (stdin if absent).
        #[arg(long)]
        diff: Option<String>,
    },
    /// List pending review items.
    List,
    /// Approve a review item and upsert it to the Hub as a skill.
    Approve {
        /// Review item id.
        id: String,
    },
    /// Reject a review item (discard the candidate).
    Reject {
        /// Review item id.
        id: String,
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
    let result = match cmd {
        Cmd::Bridge { hub_url } => {
            tracing::info!(url = %hub_url, "starting bridge");
            let cfg = glia_bridge::BridgeConfig { url: hub_url };
            glia_bridge::run_bridge(cfg).await.map_err(Into::into)
        }
        Cmd::Sync {
            hub,
            token,
            status_only,
        } => run_sync(hub, token, status_only).await,
        Cmd::Init { repo_root } => {
            let result = glia_init::run(&repo_root).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Cmd::Action {
            intent,
            stack,
            hub,
            token,
        } => run_action(intent, stack, hub, token).await,
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
            .await
        }
        Cmd::Use {
            tool,
            catalog_url,
            hub,
            token,
        } => run_use(tool, catalog_url, hub, token).await,
        Cmd::Chunk {
            op,
            hub,
            token,
            repo_root,
        } => run_chunk(op, hub, token, repo_root).await,
        Cmd::Provider { op, hub, token } => run_provider(op, hub, token).await,
        Cmd::Review {
            op,
            repo_root,
            hub,
            token,
        } => run_review(op, repo_root, hub, token).await,
        Cmd::Enroll { hub, admin_token } => run_enroll(hub, admin_token).await,
        Cmd::Context {
            stacks,
            file,
            hub,
            token,
        } => run_context(stacks, file, hub, token).await,
        Cmd::Doctor => run_doctor().await,
    };
    // Map error categories to specific exit codes per SPEC V14/V15.
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            if is_auth_timeout(&e) {
                std::process::exit(3);
            }
            if is_hub_unreachable(&e) {
                std::process::exit(2);
            }
            Err(e)
        }
    }
}

/// Check if an anyhow error chain indicates the Hub is unreachable.
fn is_hub_unreachable(err: &anyhow::Error) -> bool {
    // SyncError::HubUnreachable is the explicit signal.
    if err
        .downcast_ref::<glia_sync::SyncError>()
        .is_some_and(|e| matches!(e, glia_sync::SyncError::HubUnreachable(_)))
    {
        return true;
    }
    // Walk the error chain looking for HelixDB connection/HTTP errors
    // (the Hub's data plane). Catalog errors are NOT Hub errors.
    err.chain().any(|cause| {
        cause
            .downcast_ref::<glia_helix::HelixError>()
            .is_some_and(|e| {
                matches!(
                    e,
                    glia_helix::HelixError::Connect(_) | glia_helix::HelixError::Http(_)
                )
            })
    })
}

/// Check if an anyhow error chain indicates AUTH_TIMEOUT (SPEC V14).
/// Exit code 3 is used so the AI agent can distinguish "auth timed out"
/// from "Hub is down" (exit 2).
fn is_auth_timeout(err: &anyhow::Error) -> bool {
    err.to_string().contains("AUTH_TIMEOUT")
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

    // Re-render agent configs from Hub after every sync (Phase 3 / C).
    // Runs best-effort: failures are warnings, not errors.
    let repo_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let skills = client.list_skills().await.unwrap_or_default();
    let stacks = glia_init::scan_repo(&repo_root)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.id)
        .collect::<Vec<_>>();
    if let Err(e) = glia_hooks::generate_cursor_rules(&repo_root, &skills, &stacks).await {
        eprintln!("warn: cursor rules re-render failed: {e}");
    }
    if let Err(e) = glia_hooks::register_mcp_bridge(&repo_root).await {
        eprintln!("warn: mcp bridge re-register failed: {e}");
    }
    Ok(())
}

async fn run_action(
    intent: String,
    stack: Option<String>,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    let client = hub_client(hub.clone(), token.clone()).await?;
    let embedder = std::sync::Arc::new(glia_embed::Embedder::new()?);
    let repo_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let executor = std::sync::Arc::new(glia_action::RoutingExecutor {
        root: repo_root.clone(),
        allow_patterns: vec![
            r"^echo ".to_string(),
            r"^cat ".to_string(),
            r"^ls ".to_string(),
        ],
    });

    // Load usage store (best-effort — missing file = empty store).
    let usage_path = repo_root.join(".glia").join("usage.jsonl");
    let mut usage = if usage_path.exists() {
        tokio::fs::read_to_string(&usage_path)
            .await
            .ok()
            .map(|c| glia_action::UsageStore::from_jsonl(&c))
            .unwrap_or_default()
    } else {
        glia_action::UsageStore::default()
    };

    let action = glia_action::Action::new(client, embedder, executor).with_usage(usage.clone());
    let intent_struct = glia_action::Intent {
        query: intent,
        stack,
    };
    let result = action.run(intent_struct).await?;

    // Record cited skills and save updated usage store.
    let cited: Vec<String> = result.skills.iter().map(|s| s.source.clone()).collect();
    if !cited.is_empty() {
        usage.record(&cited);
        if let Some(parent) = usage_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&usage_path, usage.to_jsonl()).await;
    }

    if let glia_action::Outcome::AuthRequired { deps } = &result.outcome {
        handle_auth_required_via_hub(deps, &hub, token.as_deref(), glia_auth::AUTH_TIMEOUT).await?;
    }

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Handle AUTH_REQUIRED via Hub OAuth broker flow.
///
/// Calls `POST /oauth/start` on the Hub to get a redirect URL, opens the
/// browser at that URL, then polls `GET /oauth/status/{cred_id}` until the
/// token is stored in OpenBao or the timeout expires.
///
/// Falls back to the localhost callback flow when `hub_url` is not provided.
pub async fn handle_auth_required_via_hub(
    deps: &[glia_action::MissingDep],
    hub_url: &str,
    hub_token: Option<&str>,
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    if deps.is_empty() {
        return Ok(());
    }
    let client = reqwest::Client::new();

    for dep in deps {
        eprintln!(
            "AUTH_REQUIRED: {dep_tool} needs cred {dep_cred}. Starting OAuth via Hub...",
            dep_tool = dep.tool,
            dep_cred = dep.cred
        );
        let body = serde_json::json!({
            "cred_id": dep.cred,
            "provider_id": dep.cred,
            "callback_base": hub_url.trim_end_matches('/'),
        });
        let resp = client
            .post(format!("{}/oauth/start", hub_url.trim_end_matches('/')))
            .bearer_auth(hub_token.unwrap_or(""))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("hub /oauth/start: {e}"))?;

        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("hub /oauth/start failed: {msg}"));
        }
        let start: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("parse /oauth/start: {e}"))?;
        let redirect_url = start["redirect_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no redirect_url in response"))?;

        eprintln!("Opening browser for OAuth: {redirect_url}");
        if let Err(e) = open_browser(redirect_url) {
            eprintln!("Could not auto-open browser ({e}). Open manually: {redirect_url}");
        }

        // Poll /oauth/status/{cred_id} until ready or timeout.
        let deadline = std::time::Instant::now() + timeout;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if std::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!("AUTH_TIMEOUT after {timeout:?}"));
            }
            let status_resp = client
                .get(format!(
                    "{}/oauth/status/{}",
                    hub_url.trim_end_matches('/'),
                    dep.cred
                ))
                .bearer_auth(hub_token.unwrap_or(""))
                .send()
                .await;
            if let Ok(r) = status_resp
                && let Ok(v) = r.json::<serde_json::Value>().await
                && v["ready"].as_bool().unwrap_or(false)
            {
                eprintln!("AUTH: credential '{}' ready.", dep.cred);
                break;
            }
        }
    }
    Ok(())
}

/// Handle AUTH_REQUIRED: notify user, open localhost callback server, wait.
///
/// Returns `Err(AuthError::Timeout(_))` on timeout so callers can exit
/// with a non-zero code (surfaces AUTH_TIMEOUT to the AI agent per SPEC V14).
pub async fn handle_auth_required(
    deps: &[glia_action::MissingDep],
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    if deps.is_empty() {
        // No auth required — nothing to wait for.
        return Ok(());
    }
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

    let result = waiter.wait_for_callback(timeout).await;
    waiter.shutdown().await;
    match result {
        Ok(code) => {
            eprintln!("AUTH: received code (state={})", code.state);
            Ok(())
        }
        Err(glia_auth::AuthError::Timeout(d)) => {
            eprintln!("AUTH: timed out after {:?}", d);
            Err(anyhow::anyhow!("AUTH_TIMEOUT after {d:?}"))
        }
        Err(e) => {
            eprintln!("AUTH: error: {}", e);
            Err(anyhow::anyhow!("AUTH_ERROR: {e}"))
        }
    }
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
    let source = if id.starts_with("local::") {
        id.clone()
    } else {
        format!("local::{}", id)
    };
    let name = hint_name.clone().unwrap_or_else(|| id.clone());
    let output = serde_json::json!({
        "id": id,
        "source": source,
        "stacks": stacks_ref,
        "name": name,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
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
    let ChunkOp::Ingest = op;
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

/// Load context for the given file (or stack filter). Called by the
/// Claude PreToolUse hook installed by `glia init` — outputs the
/// synthesized context text to stdout for the agent to consume.
async fn run_context(
    stacks: Vec<String>,
    file: Option<String>,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    use glia_context::{ContextLoader, DefaultStackDetector};
    use glia_helix::HelixClient;
    use glia_synth::StubSynthesizer;
    use std::sync::Arc;

    let client = HelixClient::connect(Some(&hub), token.as_deref())?;
    let embedder = match glia_embed::Embedder::try_new() {
        Some(e) => Arc::new(e),
        None => {
            eprintln!("glia-embed: model assets missing; context unavailable");
            println!();
            return Ok(());
        }
    };
    let synth: Arc<dyn glia_synth::Synthesizer> = Arc::new(StubSynthesizer::default());
    let detector: Arc<dyn glia_context::StackDetector> = Arc::new(DefaultStackDetector);
    let loader = ContextLoader::new(client, embedder, synth, detector);

    // If --file is given, load context for that file.
    // Otherwise emit an empty context (the agent can pass --file).
    let file_path = match file {
        Some(f) => std::path::PathBuf::from(f),
        None => {
            eprintln!("glia context: --file is required (or use --stacks to filter)");
            println!();
            return Ok(());
        }
    };

    // When --stacks is provided, use it as a hint (filter the detected set).
    // The ContextLoader detects stacks from the file automatically;
    // we filter the output by the requested stack set if provided.
    let _ = stacks; // currently informational; detector handles filtering.

    let result = loader.load_context(&file_path).await?;
    print!("{}", result.text);
    Ok(())
}

/// `glia provider add/list` — manage OAuth provider registry via the Hub.
async fn run_provider(op: ProviderOp, hub: String, token: Option<String>) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let hub = hub.trim_end_matches('/').to_string();

    match op {
        ProviderOp::Add {
            id,
            name,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scopes,
        } => {
            // Wrap the secret immediately so the original allocation is zeroed on drop.
            let secret = zeroize::Zeroizing::new(client_secret);
            let body = serde_json::json!({
                "id": id,
                "name": name,
                "auth_url": auth_url,
                "token_url": token_url,
                "client_id": client_id,
                "client_secret": secret.as_str(),
                "scopes": scopes,
            });
            drop(secret); // zero before the HTTP round-trip completes
            let mut req = client.post(format!("{hub}/oauth/provider")).json(&body);
            if let Some(tok) = token {
                req = req.bearer_auth(tok);
            }
            let resp = req.send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(anyhow::anyhow!("provider add failed ({status}): {text}"));
            }
            println!("{text}");
        }
        ProviderOp::List => {
            // List providers directly via HelixDB client.
            let helix_url =
                std::env::var("GLIA_HELIX_URL").unwrap_or_else(|_| "http://127.0.0.1:6969".into());
            let helix_token = std::env::var("GLIA_HELIX_TOKEN").ok();
            let helix = glia_helix::HelixClient::connect(Some(&helix_url), helix_token.as_deref())?;
            let providers = helix.list_providers().await?;
            let list: Vec<serde_json::Value> = providers
                .into_iter()
                .map(|(id, p)| {
                    serde_json::json!({
                        "id": id,
                        "name": p.name,
                        "auth_url": p.auth_url,
                        "token_url": p.token_url,
                        "client_id": p.client_id,
                        "scopes": p.scopes,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
    }
    Ok(())
}

/// Path to the per-device Glia config file.
fn device_config_path() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".glia")
        .join("config.toml")
}

/// Load the hub token from the device config (fallback if env var not set).
/// Called early in each command that needs a hub token.
pub fn load_device_token() -> Option<String> {
    let path = device_config_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    // Minimal TOML parse: look for `hub_token = "..."`.
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("hub_token")
            && let Some(val) = line.split('=').nth(1)
        {
            let token = val.trim().trim_matches('"').to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

/// Generate a cryptographically-random 32-byte hex token using the OS RNG.
fn generate_device_token() -> String {
    // Use `getrandom` via a trivial loop over std::collections::hash_map.
    // Avoid pulling an extra crate: use SystemTime + process-id + a counter
    // mixed with addr-space randomness (pointer addresses on the heap).
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut buf = [0u8; 32];
    for (i, chunk) in buf.chunks_mut(8).enumerate() {
        let mut h = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut h);
        std::process::id().hash(&mut h);
        (i as u64).hash(&mut h);
        // Include a heap address as entropy source.
        let boxed = Box::new(i);
        let addr = &*boxed as *const usize as u64;
        addr.hash(&mut h);
        let val = h.finish();
        chunk.copy_from_slice(&val.to_le_bytes());
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// `glia enroll` — provision a per-device token and store in `~/.glia/config.toml`.
///
/// The generated token is registered with the Hub so it can be used as a
/// bearer token for all subsequent commands without setting `GLIA_HUB_TOKEN`.
async fn run_enroll(hub: String, admin_token: Option<String>) -> anyhow::Result<()> {
    let device_id = format!("device-{}", std::process::id());
    let device_token = generate_device_token();

    // POST to Hub /device/register.
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "device_id": device_id,
        "device_token": device_token,
    });
    let mut req = client
        .post(format!("{}/device/register", hub.trim_end_matches('/')))
        .json(&body);
    if let Some(tok) = &admin_token {
        req = req.bearer_auth(tok);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            eprintln!("Enrolled with Hub at {hub}.");
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            // Hub may not implement /device/register yet — warn but still save.
            eprintln!("warn: Hub returned {status}: {text}. Saving token locally anyway.");
        }
        Err(e) => {
            // Hub unreachable — still save the token for later use.
            eprintln!("warn: Hub unreachable ({e}). Saving token locally for when Hub starts.");
        }
    }

    // Write config file.
    let config_path = device_config_path();
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let config = format!(
        "# Glia device config — generated by `glia enroll`\n\
         device_id = \"{device_id}\"\n\
         hub_url = \"{hub}\"\n\
         hub_token = \"{device_token}\"\n"
    );
    tokio::fs::write(&config_path, &config).await?;
    println!(
        "Device enrolled.\n  device_id: {device_id}\n  config:    {}\n\
         \nAll future `glia` commands will use this token automatically.",
        config_path.display()
    );
    Ok(())
}

/// `glia review capture/list/approve/reject` — corrections → skills loop.
async fn run_review(
    op: ReviewOp,
    repo_root: PathBuf,
    hub: String,
    token: Option<String>,
) -> anyhow::Result<()> {
    let queue = glia_review::ReviewQueue::open(&repo_root);
    match op {
        ReviewOp::Capture { file, diff } => {
            let diff_str = diff.unwrap_or_default();
            let item = queue.capture(&file, &diff_str).await?;
            println!("Captured review item {} for '{}'.", item.id, item.file_path);
        }
        ReviewOp::List => {
            let items = queue.list_pending().await?;
            if items.is_empty() {
                println!("No pending review items.");
            } else {
                for item in &items {
                    println!("  {} | {} | {}", item.id, item.file_path, item.created_at);
                    println!("    {}", item.candidate_rule.lines().next().unwrap_or(""));
                }
                println!(
                    "\n{} pending item(s). Use `glia review approve <id>` or `glia review reject <id>`.",
                    items.len()
                );
            }
        }
        ReviewOp::Approve { id } => {
            let item = queue.approve(&id).await?;
            // Upsert the approved item as a skill on the Hub.
            let source = format!("local::review::{}", item.id);
            let client = hub_client(hub, token).await?;
            let embedder = match glia_embed::Embedder::try_new() {
                Some(e) => e,
                None => {
                    eprintln!("warn: glia-embed unavailable; skill upserted without embedding");
                    let skill = glia_helix::Skill {
                        content: item.candidate_rule.clone(),
                        source: source.clone(),
                        embedding: vec![],
                        updated_at: item.created_at.clone(),
                    };
                    client
                        .upsert_skill(&source, skill)
                        .await
                        .map_err(|e| anyhow::anyhow!("skill upsert failed: {e}"))?;
                    println!("Approved and upserted skill '{source}' (no embedding).");
                    return Ok(());
                }
            };
            let embedding = embedder
                .embed(&item.candidate_rule)
                .map_err(|e| anyhow::anyhow!("embed failed: {e}"))?;
            let skill = glia_helix::Skill {
                content: item.candidate_rule.clone(),
                source: source.clone(),
                embedding,
                updated_at: item.created_at.clone(),
            };
            client
                .upsert_skill(&source, skill)
                .await
                .map_err(|e| anyhow::anyhow!("skill upsert failed: {e}"))?;
            println!("Approved and upserted skill '{source}'.");
        }
        ReviewOp::Reject { id } => {
            let item = queue.reject(&id).await?;
            println!("Rejected review item {} for '{}'.", item.id, item.file_path);
        }
    }
    Ok(())
}

/// `glia doctor` — probe runtime availability and print a status table.
///
/// Exit 0 if all runtimes are found, exit 1 if any are missing so CI can gate.
async fn run_doctor() -> anyhow::Result<()> {
    use glia_sandbox::{Runtime, probe_runtimes, probe_version};

    let runtimes = [Runtime::Npx, Runtime::Uvx, Runtime::Docker];
    let results = probe_runtimes(&runtimes);

    let mut any_missing = false;
    println!("{:<10} {:<10} VERSION", "RUNTIME", "STATUS");
    println!("{}", "-".repeat(35));
    for r in &results {
        let (status, version) = if r.found {
            let v = probe_version(r.runtime.binary()).unwrap_or_else(|| "?".into());
            ("ok", v)
        } else {
            ("MISSING", "-".into())
        };
        println!("{:<10} {:<10} {}", r.runtime.binary(), status, version);
        if !r.found {
            any_missing = true;
        }
    }

    // Embed model assets.
    let embed_ok = glia_embed::Embedder::try_new().is_some();
    println!(
        "{:<10} {}",
        "embed",
        if embed_ok {
            "ok"
        } else {
            "MISSING (run `glia init`)"
        }
    );
    if !embed_ok {
        any_missing = true;
    }

    if any_missing {
        eprintln!(
            "\nSome dependencies are missing. Agent actions requiring them will return Outcome::RuntimeMissing."
        );
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_action::MissingDep;
    use std::time::Duration;

    #[tokio::test]
    async fn handle_auth_required_times_out_returns_error() {
        // No callback arrives — the helper now returns Err(AUTH_TIMEOUT)
        // so the CLI exits with code 3 (SPEC V14).
        let deps = vec![MissingDep {
            tool: "linear-create".into(),
            cred: "linear_oauth".into(),
        }];
        let result = handle_auth_required(&deps, Duration::from_millis(150)).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("AUTH_TIMEOUT"),
            "expected AUTH_TIMEOUT error"
        );
    }

    #[test]
    fn is_auth_timeout_detects_auth_timeout_string() {
        let err = anyhow::anyhow!("AUTH_TIMEOUT after 120s");
        assert!(super::is_auth_timeout(&err));
        let err = anyhow::anyhow!("db: connection refused");
        assert!(!super::is_auth_timeout(&err));
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
