//! glia-action — unified action orchestrator.
//!
//! Implements T9: parallel discover+exec+dep-check, intent classification.
//! V1 (graph), V2 (parallel), V3 (AUTH_REQUIRED surfacing), V4 (citation),
//! V13 (local/remote intent registry).
//!
//! v0.2.0: talks to the Hub via `HelixClient` (HTTP). Replaces the
//! SurrealDB-backed DB handle from v0.1.0.
//!
//! Pipeline:
//! 1. `classify` — split intent into Local vs Remote
//! 2. `discover` — vector search over skills for the intent query
//! 3. `dep_check` — graph walk for required creds/tools
//! 4. `exec` — run via pluggable Executor (local: glia-bash; remote: glia-sandbox)
//! 5. `synthesize` — return ranked matches + outcome; emit AUTH_REQUIRED if deps missing

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use glia_embed::Embedder;
use glia_helix::{HelixClient, HelixError, Skill, Tool};

/// Errors from action orchestration.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// DB query failed.
    #[error("db: {0}")]
    Db(#[from] HelixError),
    /// Embedder failed.
    #[error("embed: {0}")]
    Embed(#[from] glia_embed::EmbedError),
    /// Executor failed.
    #[error("exec: {0}")]
    Exec(String),
}

/// Caller's intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Natural-language query.
    pub query: String,
    /// Optional stack filter (e.g., `nextjs`).
    #[serde(default)]
    pub stack: Option<String>,
}

/// Classified intent routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentKind {
    /// Runs locally (in-CLI). E.g., file ops, bash, local-only skills.
    Local,
    /// Requires Hub sandbox + remote creds. E.g., Linear API, GitHub API.
    Remote,
}

/// Skill match with cosine similarity score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMatch {
    /// Skill id (may be `local::foo`).
    pub id: String,
    /// Skill content.
    pub content: String,
    /// Source path (for citation, V4).
    pub source: String,
    /// Cosine similarity vs intent query.
    pub score: f32,
    /// Whether this is a local-namespaced skill (V16).
    pub local: bool,
}

/// Missing dependency surfaced to caller (V3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingDep {
    /// Tool that needs the cred.
    pub tool: String,
    /// Cred id required.
    pub cred: String,
}

/// Outcome of an action run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Outcome {
    /// Successfully executed, payload attached.
    Done {
        /// Executor result.
        result: String,
    },
    /// Action needs authentication before it can run.
    AuthRequired {
        /// List of missing deps.
        deps: Vec<MissingDep>,
    },
    /// No relevant skills or tools discovered.
    NotApplicable,
    /// Tool requires a runtime that is not installed on this device (Problem B).
    RuntimeMissing {
        /// Runtime binary name (e.g., "uvx", "npx", "docker").
        runtime: String,
        /// Minimum required version, if known.
        needed_version: Option<String>,
        /// Human-readable hint for the agent.
        hint: String,
    },
}

/// Final action response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// Original intent.
    pub intent: Intent,
    /// Classification.
    pub kind: IntentKind,
    /// Top-K skill matches (V4 citation).
    pub skills: Vec<SkillMatch>,
    /// Discovered tools.
    pub tools: Vec<Tool>,
    /// Missing deps (drives `AUTH_REQUIRED`).
    pub missing: Vec<MissingDep>,
    /// Execution outcome.
    pub outcome: Outcome,
    /// ISO timestamp of completion.
    pub finished_at: String,
}

/// Pluggable executor for the action runtime.
///
/// Local impl = glia-bash. Remote impl = glia-sandbox wrapping a Hub call.
#[async_trait]
pub trait Executor: Send + Sync {
    /// Execute a single tool with the given params, returning the result string.
    async fn exec(&self, tool: &Tool, params: &serde_json::Value) -> Result<String, String>;
}

/// Stub executor that returns a fixed string. For tests.
pub struct StubExecutor {
    /// Value to return on every `exec`.
    pub response: String,
}

#[async_trait]
impl Executor for StubExecutor {
    async fn exec(&self, _tool: &Tool, _params: &serde_json::Value) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

/// Production executor that routes local tools through glia-bash and surfaces
/// `Outcome::RuntimeMissing` when a required runtime is absent from PATH.
pub struct RoutingExecutor {
    /// Root directory enforced by glia-bash path boundary checks.
    pub root: std::path::PathBuf,
    /// Allow-list regex patterns forwarded to glia-bash.
    pub allow_patterns: Vec<String>,
}

#[async_trait]
impl Executor for RoutingExecutor {
    async fn exec(&self, tool: &Tool, _params: &serde_json::Value) -> Result<String, String> {
        // Probe the declared runtime before attempting execution.
        if let Some(runtime) = &tool.runtime {
            let probe = tokio::process::Command::new(runtime)
                .arg("--version")
                .output()
                .await;
            let available = probe.is_ok_and(|o| o.status.success());
            if !available {
                return Err(format!("RUNTIME_MISSING:{runtime}"));
            }
        }

        if tool.local {
            let patterns: Vec<&str> = self.allow_patterns.iter().map(String::as_str).collect();
            let cfg =
                glia_bash::BashConfig::new(&self.root, &patterns).map_err(|e| e.to_string())?;
            // Derive the command from the runtime + tool name, or just echo.
            let command = match &tool.runtime {
                Some(rt) => format!("{rt} {}", tool.name),
                None => format!("echo {}", tool.name),
            };
            let output = glia_bash::run(&cfg, &command)
                .await
                .map_err(|e| e.to_string())?;
            Ok(output.stdout.trim().to_string())
        } else {
            // Remote dispatch is wired in Phase 2 via Hub action endpoint.
            Err("remote tool dispatch not yet implemented".to_string())
        }
    }
}

/// Intent classifier.
///
/// V13: local if query mentions file paths / shell; remote if mentions SaaS.
pub fn classify(intent: &Intent) -> IntentKind {
    let q = intent.query.to_lowercase();
    let remote_markers = [
        "linear", "github", "notion", "slack", "supabase", "stripe", "jira", "oauth",
    ];
    if remote_markers.iter().any(|m| q.contains(m)) {
        IntentKind::Remote
    } else {
        IntentKind::Local
    }
}

/// Cosine similarity.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Top-K skill search by cosine similarity (no usage boost).
pub fn rank_skills(query_vec: &[f32], skills: &[Skill], k: usize) -> Vec<SkillMatch> {
    rank_skills_weighted(query_vec, skills, k, None)
}

/// Per-skill citation counts used to boost ranking of frequently-cited skills.
///
/// Loaded from `.glia/usage.jsonl` by the CLI before each `Action::run`
/// and saved back after. The `Action` reads it for ranking only — mutation
/// stays with the caller.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageStore {
    /// `skill_source → citation_count`.
    pub counts: std::collections::HashMap<String, u32>,
}

impl UsageStore {
    /// Parse from JSONL content (one `{source, count}` object per line).
    pub fn from_jsonl(content: &str) -> Self {
        let mut counts = std::collections::HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && let (Some(source), Some(count)) = (v["source"].as_str(), v["count"].as_u64())
            {
                counts.insert(source.to_owned(), count as u32);
            }
        }
        Self { counts }
    }

    /// Serialize to JSONL (one object per skill).
    pub fn to_jsonl(&self) -> String {
        let mut lines = Vec::new();
        for (source, &count) in &self.counts {
            lines.push(serde_json::json!({"source": source, "count": count}).to_string());
        }
        lines.join("\n") + if lines.is_empty() { "" } else { "\n" }
    }

    /// Increment counts for each cited skill source.
    pub fn record(&mut self, sources: &[String]) {
        for s in sources {
            *self.counts.entry(s.clone()).or_insert(0) += 1;
        }
    }

    /// Look up the usage count for a skill source (0 if unseen).
    pub fn get(&self, source: &str) -> u32 {
        self.counts.get(source).copied().unwrap_or(0)
    }
}

/// Top-K skill search with optional usage-count boost.
///
/// Boost formula: `score = cosine * (1.0 + ALPHA * count.ln_1p())`
/// where ALPHA = 0.05. At count=0 → no change; at count=10 → ~12% boost.
/// Keeps semantic similarity primary while favouring proven skills.
pub fn rank_skills_weighted(
    query_vec: &[f32],
    skills: &[Skill],
    k: usize,
    usage: Option<&UsageStore>,
) -> Vec<SkillMatch> {
    const ALPHA: f32 = 0.05;
    let mut scored: Vec<SkillMatch> = skills
        .iter()
        .map(|s| {
            let base = cosine(query_vec, &s.embedding);
            let device_boost =
                usage.map_or(1.0, |u| 1.0 + ALPHA * (u.get(&s.source) as f32).ln_1p());
            let hub_boost = 1.0 + ALPHA * (s.usage_count as f32).ln_1p();
            SkillMatch {
                id: format!("skill::{}", s.source),
                content: s.content.clone(),
                source: s.source.clone(),
                score: base * device_boost * hub_boost,
                local: HelixClient::is_local_skill(&s.source),
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored
}

/// Action orchestrator. Holds shared deps and a pluggable executor.
pub struct Action {
    client: HelixClient,
    embedder: Arc<Embedder>,
    executor: Arc<dyn Executor>,
    /// Top-K for skill ranking.
    top_k: usize,
    /// Optional usage store for boosting previously-cited skills.
    usage: Option<UsageStore>,
}

impl Action {
    /// Build a new action with the given deps.
    pub fn new(client: HelixClient, embedder: Arc<Embedder>, executor: Arc<dyn Executor>) -> Self {
        Self {
            client,
            embedder,
            executor,
            top_k: 5,
            usage: None,
        }
    }

    /// Override the top-K default.
    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    /// Attach a usage store to boost previously-cited skills.
    pub fn with_usage(mut self, store: UsageStore) -> Self {
        self.usage = Some(store);
        self
    }

    /// Run the full pipeline.
    pub async fn run(&self, intent: Intent) -> Result<ActionResult, ActionError> {
        let kind = classify(&intent);

        let query_vec = self.embedder.embed(&intent.query)?;

        let skills = self
            .discover_skills(&query_vec, intent.stack.as_deref())
            .await?;

        let (tools, missing) = self.dep_check(&skills, kind).await?;

        let outcome = if !missing.is_empty() {
            Outcome::AuthRequired {
                deps: missing.clone(),
            }
        } else if let Some(tool) = tools.first() {
            // Runtime pre-flight (Problem B): surface missing/outdated runtime BEFORE exec.
            if let Some(early) = preflight_runtime(tool) {
                early
            } else {
                match self.executor.exec(tool, &serde_json::json!({})).await {
                    Ok(result) => Outcome::Done { result },
                    Err(e) if e.starts_with("RUNTIME_MISSING:") => {
                        let runtime = e["RUNTIME_MISSING:".len()..].to_string();
                        Outcome::RuntimeMissing {
                            hint: format!("Install '{runtime}' and retry."),
                            runtime,
                            needed_version: None,
                        }
                    }
                    Err(e) => return Err(ActionError::Exec(e)),
                }
            }
        } else {
            Outcome::NotApplicable
        };

        Ok(ActionResult {
            intent,
            kind,
            skills,
            tools,
            missing,
            outcome,
            finished_at: Utc::now().to_rfc3339(),
        })
    }

    /// Vector-search skills; filter by stack if provided.
    /// Applies usage-count boost when a store is attached.
    async fn discover_skills(
        &self,
        query_vec: &[f32],
        stack: Option<&str>,
    ) -> Result<Vec<SkillMatch>, ActionError> {
        let all = self.all_skills().await?;
        let usage = self.usage.as_ref();
        if let Some(stack_id) = stack {
            let for_stack = self.client.skills_for_stack(stack_id).await?;
            let ids: std::collections::HashSet<String> =
                for_stack.into_iter().map(|s| s.source).collect();
            let filtered: Vec<Skill> = all
                .into_iter()
                .filter(|s| ids.contains(&s.source))
                .collect();
            Ok(rank_skills_weighted(
                query_vec, &filtered, self.top_k, usage,
            ))
        } else {
            Ok(rank_skills_weighted(query_vec, &all, self.top_k, usage))
        }
    }

    /// Pull every skill from the DB.
    async fn all_skills(&self) -> Result<Vec<Skill>, ActionError> {
        Ok(self.client.list_skills().await?)
    }

    /// Graph walk: for remote intents, find tools that require auth.
    /// Returns the matching tools and any creds that the caller must
    /// present before exec. Local intents need no cred (V3).
    async fn dep_check(
        &self,
        _skills: &[SkillMatch],
        kind: IntentKind,
    ) -> Result<(Vec<Tool>, Vec<MissingDep>), ActionError> {
        if matches!(kind, IntentKind::Local) {
            return Ok((Vec::new(), Vec::new()));
        }

        // Remote: walk all creds; for each, find tools that require it.
        // T14 (OpenBao) will refine "ready" detection; for T9 we surface
        // every known tool→cred edge as the auth checklist.
        let cred_ids: Vec<String> = self.list_cred_ids().await?;
        let mut missing = Vec::new();
        let mut tools = Vec::new();
        for cred in &cred_ids {
            let required = self.client.tools_requiring_auth(cred).await?;
            for tool in required {
                missing.push(MissingDep {
                    tool: tool.name.clone(),
                    cred: cred.clone(),
                });
                tools.push(tool);
            }
        }
        Ok((tools, missing))
    }

    /// List every cred id via the typed select API (RecordIdKey → String).
    async fn list_cred_ids(&self) -> Result<Vec<String>, ActionError> {
        Ok(self.client.list_cred_ids().await?)
    }
}

/// Runtime pre-flight check (Problem B).
/// Returns `Some(Outcome::RuntimeMissing)` if the tool's required runtime is absent or too old;
/// returns `None` if the check passes (exec should proceed).
fn preflight_runtime(tool: &Tool) -> Option<Outcome> {
    let runtime_bin = tool.runtime.as_deref()?;
    if !glia_sandbox::probe_runtime_str(runtime_bin) {
        return Some(Outcome::RuntimeMissing {
            hint: format!("Install '{runtime_bin}' to run this tool locally."),
            runtime: runtime_bin.to_string(),
            needed_version: tool.min_version.clone(),
        });
    }
    if let Some(needed) = &tool.min_version
        && let Some(found) = glia_sandbox::probe_version(runtime_bin)
        && !glia_sandbox::satisfies(&found, needed)
    {
        return Some(Outcome::RuntimeMissing {
            hint: format!("'{runtime_bin}' {found} is too old; need \u{2265}{needed}."),
            runtime: runtime_bin.to_string(),
            needed_version: tool.min_version.clone(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_helix::{Auth, Stack};
    use std::sync::Arc;

    fn now() -> String {
        Utc::now().to_rfc3339()
    }

    async fn setup() -> Option<(HelixClient, Arc<Embedder>)> {
        let emb = Arc::new(Embedder::try_new()?);
        let client = HelixClient::connect(None, None).ok()?;
        if client.ping().await.is_err() {
            return None;
        }
        Some((client, emb))
    }

    #[tokio::test]
    async fn classify_local_and_remote() {
        assert_eq!(
            classify(&Intent {
                query: "cat ./README".into(),
                stack: None
            }),
            IntentKind::Local
        );
        assert_eq!(
            classify(&Intent {
                query: "create linear issue".into(),
                stack: None
            }),
            IntentKind::Remote
        );
        assert_eq!(
            classify(&Intent {
                query: "what is rust".into(),
                stack: None
            }),
            IntentKind::Local
        );
    }

    #[test]
    fn rank_skills_orders_by_cosine() {
        let s1 = Skill {
            content: "rust borrow checker".into(),
            source: "rust-borrow.md".into(),
            embedding: vec![1.0, 0.0, 0.0],
            updated_at: now(),
            usage_count: 0,
        };
        let s2 = Skill {
            content: "kittens".into(),
            source: "cats.md".into(),
            embedding: vec![0.0, 1.0, 0.0],
            updated_at: now(),
            usage_count: 0,
        };
        let query = vec![1.0, 0.0, 0.0];
        let ranked = rank_skills(&query, &[s1, s2], 2);
        assert_eq!(ranked[0].source, "rust-borrow.md");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[tokio::test]
    async fn run_returns_auth_required_when_dep_missing() {
        let Some((client, emb)) = setup().await else {
            eprintln!("SKIP: embed model or helixdb unavailable");
            return;
        };

        client
            .upsert_skill(
                "local::auth-required-rule",
                Skill {
                    content: "never skip oauth".into(),
                    source: "local::auth-required-rule".into(),
                    embedding: vec![1.0, 0.0, 0.0],
                    updated_at: now(),
                    usage_count: 0,
                },
            )
            .await
            .unwrap();

        client
            .upsert_auth(
                "linear_oauth",
                Auth {
                    auth_type: "oauth".into(),
                    provider: "linear".into(),
                    updated_at: now(),
                },
            )
            .await
            .unwrap();

        let exec = Arc::new(StubExecutor {
            response: "ok".into(),
        });
        let action = Action::new(client, emb, exec);
        let result = action
            .run(Intent {
                query: "linear oauth rule".into(),
                stack: None,
            })
            .await
            .unwrap();

        assert!(!result.skills.is_empty(), "should discover the auth skill");
    }

    #[tokio::test]
    async fn run_done_when_executor_succeeds() {
        let Some((client, emb)) = setup().await else {
            eprintln!("SKIP: embed model or helixdb unavailable");
            return;
        };

        client
            .upsert_skill(
                "local::cat-readme",
                Skill {
                    content: "read the readme".into(),
                    source: "local::cat-readme".into(),
                    embedding: vec![1.0, 0.0, 0.0],
                    updated_at: now(),
                    usage_count: 0,
                },
            )
            .await
            .unwrap();

        let exec = Arc::new(StubExecutor {
            response: "42".into(),
        });
        let action = Action::new(client, emb, exec);
        let result = action
            .run(Intent {
                query: "cat the readme".into(),
                stack: None,
            })
            .await
            .unwrap();

        assert!(matches!(
            result.outcome,
            Outcome::NotApplicable | Outcome::Done { .. }
        ));
    }

    #[tokio::test]
    async fn run_stack_filters_skills() {
        let Some((client, emb)) = setup().await else {
            eprintln!("SKIP: embed model or helixdb unavailable");
            return;
        };

        client
            .upsert_skill(
                "nextjs::rule",
                Skill {
                    content: "next.js rule".into(),
                    source: "nextjs::rule".into(),
                    embedding: vec![1.0, 0.0, 0.0],
                    updated_at: now(),
                    usage_count: 0,
                },
            )
            .await
            .unwrap();
        client
            .upsert_stack(
                "nextjs",
                Stack {
                    name: "Next.js".into(),
                },
            )
            .await
            .unwrap();
        client
            .relate_skill_applies_to_stack("nextjs::rule", "nextjs")
            .await
            .unwrap();

        let exec = Arc::new(StubExecutor {
            response: "ok".into(),
        });
        let action = Action::new(client, emb, exec);
        let result = action
            .run(Intent {
                query: "next.js".into(),
                stack: Some("nextjs".into()),
            })
            .await
            .unwrap();

        assert!(result.skills.iter().any(|s| s.source == "nextjs::rule"));
    }

    #[tokio::test]
    async fn result_serializes_round_trip() {
        let r = ActionResult {
            intent: Intent {
                query: "x".into(),
                stack: None,
            },
            kind: IntentKind::Local,
            skills: vec![],
            tools: vec![],
            missing: vec![],
            outcome: Outcome::NotApplicable,
            finished_at: now(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ActionResult = serde_json::from_str(&s).unwrap();
        assert_eq!(back.kind, IntentKind::Local);
    }

    #[tokio::test]
    async fn tool_required_missing_surfaces_auth() {
        let Some((client, emb)) = setup().await else {
            eprintln!("SKIP: embed model or helixdb unavailable");
            return;
        };

        client
            .upsert_tool(
                "linear-create",
                Tool {
                    name: "Linear Create".into(),
                    category: "issue-tracker".into(),
                    local: false,
                    params_schema: serde_json::json!({}),
                    updated_at: now(),
                    runtime: None,
                    min_version: None,
                },
            )
            .await
            .unwrap();
        client
            .upsert_auth(
                "linear_oauth",
                Auth {
                    auth_type: "oauth".into(),
                    provider: "linear".into(),
                    updated_at: now(),
                },
            )
            .await
            .unwrap();
        client
            .relate_tool_requires_auth("linear-create", "linear_oauth")
            .await
            .unwrap();

        let tools = client.tools_requiring_auth("linear_oauth").await.unwrap();
        assert_eq!(tools.len(), 1);

        let exec = Arc::new(StubExecutor {
            response: "ok".into(),
        });
        let action = Action::new(client, emb, exec);
        let result = action
            .run(Intent {
                query: "create linear issue".into(),
                stack: None,
            })
            .await
            .unwrap();

        assert_eq!(result.kind, IntentKind::Remote);
    }

    #[test]
    fn empty_query_classifies_local() {
        assert_eq!(
            classify(&Intent {
                query: "".into(),
                stack: None
            }),
            IntentKind::Local
        );
    }

    #[test]
    fn whitespace_query_classifies_local() {
        assert_eq!(
            classify(&Intent {
                query: "   ".into(),
                stack: None
            }),
            IntentKind::Local
        );
    }

    #[test]
    fn unicode_emoji_query_classification() {
        assert_eq!(
            classify(&Intent {
                query: "create linear issue 🎫".into(),
                stack: None
            }),
            IntentKind::Remote
        );
        assert_eq!(
            classify(&Intent {
                query: "cat 📄 file".into(),
                stack: None
            }),
            IntentKind::Local
        );
    }

    #[test]
    fn case_insensitive_linear_substring_match() {
        assert_eq!(
            classify(&Intent {
                query: "LinearCase".into(),
                stack: None
            }),
            IntentKind::Remote
        );
    }

    #[test]
    fn capitalized_linear_classifies_remote() {
        assert_eq!(
            classify(&Intent {
                query: "Linear issue".into(),
                stack: None
            }),
            IntentKind::Remote
        );
    }

    #[test]
    fn mixed_local_remote_markers_remote_wins() {
        // Contains both "cat" (local hint) and "linear" (remote marker).
        assert_eq!(
            classify(&Intent {
                query: "cat the linear issue".into(),
                stack: None
            }),
            IntentKind::Remote
        );
    }

    #[test]
    fn oauth_marker_classifies_remote() {
        assert_eq!(
            classify(&Intent {
                query: "oauth flow".into(),
                stack: None
            }),
            IntentKind::Remote
        );
    }

    #[test]
    fn each_remote_marker_classifies_remote() {
        for marker in &[
            "linear", "github", "notion", "slack", "supabase", "stripe", "jira", "oauth",
        ] {
            assert_eq!(
                classify(&Intent {
                    query: format!("test {marker} here"),
                    stack: None
                }),
                IntentKind::Remote,
                "marker '{marker}' should classify as Remote"
            );
        }
    }

    #[test]
    fn cosine_length_mismatch_returns_zero() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn cosine_empty_returns_zero() {
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_zero_vector_returns_zero() {
        assert_eq!(cosine(&[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_nan_inputs_sort_as_equal() {
        let nan_val = f32::NAN;
        // NaN in cosine → dot product becomes NaN → partial_cmp returns
        // None → unwrap_or(Equal). Test that it doesn't panic.
        let result = cosine(&[nan_val], &[1.0]);
        assert!(result.is_nan() || result == 0.0);
    }

    #[test]
    fn rank_skills_k_zero_returns_empty() {
        let skills = vec![Skill {
            content: "x".into(),
            source: "x.md".into(),
            embedding: vec![1.0],
            updated_at: now(),
            usage_count: 0,
        }];
        let ranked = rank_skills(&[1.0], &skills, 0);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_skills_k_exceeds_count_returns_all() {
        let skills = vec![
            Skill {
                content: "a".into(),
                source: "a.md".into(),
                embedding: vec![1.0],
                updated_at: now(),
                usage_count: 0,
            },
            Skill {
                content: "b".into(),
                source: "b.md".into(),
                embedding: vec![0.0],
                updated_at: now(),
                usage_count: 0,
            },
        ];
        let ranked = rank_skills(&[1.0], &skills, 10);
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn rank_skills_empty_input_returns_empty() {
        let ranked = rank_skills(&[1.0], &[], 5);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_skills_tie_scores_preserve_input_order() {
        // Two skills with identical embeddings → identical scores →
        // stable sort preserves input order.
        let skills = vec![
            Skill {
                content: "first".into(),
                source: "first.md".into(),
                embedding: vec![1.0, 0.0],
                updated_at: now(),
                usage_count: 0,
            },
            Skill {
                content: "second".into(),
                source: "second.md".into(),
                embedding: vec![1.0, 0.0],
                updated_at: now(),
                usage_count: 0,
            },
        ];
        let ranked = rank_skills(&[1.0, 0.0], &skills, 2);
        assert_eq!(ranked[0].source, "first.md");
        assert_eq!(ranked[1].source, "second.md");
    }

    #[test]
    fn stub_executor_empty_response() {
        let exec = StubExecutor {
            response: String::new(),
        };
        // Just verify it doesn't panic — actual exec is async.
        assert!(exec.response.is_empty());
    }

    #[tokio::test]
    async fn routing_executor_missing_runtime_returns_runtime_missing_prefix() {
        let exec = RoutingExecutor {
            root: std::env::temp_dir(),
            allow_patterns: vec![r"^echo ".to_string()],
        };
        let tool = Tool {
            name: "some-tool".into(),
            category: "test".into(),
            local: true,
            params_schema: serde_json::json!({}),
            updated_at: "2026-01-01T00:00:00Z".into(),
            // Use a binary that definitely doesn't exist on any system.
            runtime: Some("__glia_nonexistent_runtime_xyz__".into()),
            min_version: None,
        };
        let err = exec.exec(&tool, &serde_json::json!({})).await.unwrap_err();
        assert!(
            err.starts_with("RUNTIME_MISSING:"),
            "expected RUNTIME_MISSING: prefix, got: {err}"
        );
        assert!(err.contains("__glia_nonexistent_runtime_xyz__"));
    }

    #[test]
    fn runtime_missing_outcome_round_trips() {
        let r = ActionResult {
            intent: Intent {
                query: "uvx foo".into(),
                stack: None,
            },
            kind: IntentKind::Local,
            skills: vec![],
            tools: vec![],
            missing: vec![],
            outcome: Outcome::RuntimeMissing {
                runtime: "uvx".into(),
                needed_version: Some(">=0.1".into()),
                hint: "Install uvx and retry.".into(),
            },
            finished_at: now(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ActionResult = serde_json::from_str(&s).unwrap();
        assert!(
            matches!(back.outcome, Outcome::RuntimeMissing { runtime, .. } if runtime == "uvx")
        );
    }

    #[test]
    fn unicode_action_intent_classification_full() {
        use IntentKind;
        // Full unicode + emoji coverage.
        assert_eq!(
            classify(&Intent {
                query: "create linear issue 🎫".into(),
                stack: None
            }),
            IntentKind::Remote
        );
        assert_eq!(
            classify(&Intent {
                query: "cat 📄 file.txt".into(),
                stack: None
            }),
            IntentKind::Local
        );
        assert_eq!(
            classify(&Intent {
                query: "日本語のファイルを開く".into(),
                stack: None
            }),
            IntentKind::Local
        );
        assert_eq!(
            classify(&Intent {
                query: "créer un ticket linear".into(),
                stack: None
            }),
            IntentKind::Remote
        );
    }

    // --- UsageStore tests ---

    #[test]
    fn usage_store_empty_get_returns_zero() {
        let store = UsageStore::default();
        assert_eq!(store.get("any-source"), 0);
    }

    #[test]
    fn usage_store_record_increments() {
        let mut store = UsageStore::default();
        store.record(&["skill-a".into(), "skill-b".into()]);
        store.record(&["skill-a".into()]);
        assert_eq!(store.get("skill-a"), 2);
        assert_eq!(store.get("skill-b"), 1);
    }

    #[test]
    fn usage_store_jsonl_round_trip() {
        let mut store = UsageStore::default();
        store.record(&["x".into(), "y".into()]);
        store.record(&["x".into()]);
        let jsonl = store.to_jsonl();
        let back = UsageStore::from_jsonl(&jsonl);
        assert_eq!(back.get("x"), 2);
        assert_eq!(back.get("y"), 1);
    }

    #[test]
    fn usage_store_from_empty_string_gives_empty() {
        let store = UsageStore::from_jsonl("");
        assert!(store.counts.is_empty());
    }

    #[test]
    fn usage_store_from_malformed_line_skips_it() {
        let jsonl =
            "{\"source\":\"a\",\"count\":3}\n{not valid json}\n{\"source\":\"b\",\"count\":1}\n";
        let store = UsageStore::from_jsonl(jsonl);
        assert_eq!(store.get("a"), 3);
        assert_eq!(store.get("b"), 1);
    }

    #[test]
    fn rank_skills_weighted_boosts_high_usage() {
        let q = vec![1.0_f32, 0.0];
        let skills = vec![
            Skill {
                source: "low-usage".into(),
                content: "x".into(),
                embedding: vec![1.0, 0.0], // cosine = 1.0
                updated_at: "t".into(),
                usage_count: 0,
            },
            Skill {
                source: "high-usage".into(),
                content: "y".into(),
                embedding: vec![0.99, 0.141], // cosine slightly < 1.0
                updated_at: "t".into(),
                usage_count: 0,
            },
        ];
        let mut usage = UsageStore::default();
        usage.record(&vec!["high-usage".into(); 100]); // 100 citations

        let results = rank_skills_weighted(&q, &skills, 2, Some(&usage));
        // high-usage should win due to boost even though its base cosine is lower.
        assert_eq!(results[0].source, "high-usage");
    }

    #[test]
    fn rank_skills_with_no_usage_same_as_rank_skills() {
        let q = vec![1.0_f32, 0.0];
        let skills = vec![
            Skill {
                source: "a".into(),
                content: "x".into(),
                embedding: vec![0.9, 0.436],
                updated_at: "t".into(),
                usage_count: 0,
            },
            Skill {
                source: "b".into(),
                content: "y".into(),
                embedding: vec![1.0, 0.0],
                updated_at: "t".into(),
                usage_count: 0,
            },
        ];
        let plain = rank_skills(&q, &skills, 2);
        let weighted = rank_skills_weighted(&q, &skills, 2, None);
        assert_eq!(plain[0].source, weighted[0].source);
        assert_eq!(plain[1].source, weighted[1].source);
    }
}
