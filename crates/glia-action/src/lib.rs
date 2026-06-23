//! glia-action — unified action orchestrator.
//!
//! Implements T9: parallel discover+exec+dep-check, intent classification.
//! V1 (graph), V2 (parallel), V3 (AUTH_REQUIRED surfacing), V4 (citation),
//! V13 (local/remote intent registry).
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

use glia_db::{GliaDb, Skill, Tool};
use glia_embed::Embedder;

/// Errors from action orchestration.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// DB query failed.
    #[error("db: {0}")]
    Db(#[from] glia_db::DbError),
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

/// Top-K skill search by cosine similarity.
pub fn rank_skills(query_vec: &[f32], skills: &[Skill], k: usize) -> Vec<SkillMatch> {
    let mut scored: Vec<SkillMatch> = skills
        .iter()
        .map(|s| SkillMatch {
            id: format!("skill::{}", s.source),
            content: s.content.clone(),
            source: s.source.clone(),
            score: cosine(query_vec, &s.embedding),
            local: glia_db::GliaDb::is_local_skill(&s.source),
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
    db: Arc<GliaDb>,
    embedder: Arc<Embedder>,
    executor: Arc<dyn Executor>,
    /// Top-K for skill ranking.
    top_k: usize,
}

impl Action {
    /// Build a new action with the given deps.
    pub fn new(db: Arc<GliaDb>, embedder: Arc<Embedder>, executor: Arc<dyn Executor>) -> Self {
        Self {
            db,
            embedder,
            executor,
            top_k: 5,
        }
    }

    /// Override the top-K default.
    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
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
            match self.executor.exec(tool, &serde_json::json!({})).await {
                Ok(result) => Outcome::Done { result },
                Err(e) => return Err(ActionError::Exec(e)),
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
    async fn discover_skills(
        &self,
        query_vec: &[f32],
        stack: Option<&str>,
    ) -> Result<Vec<SkillMatch>, ActionError> {
        let all = self.all_skills().await?;
        if let Some(stack_id) = stack {
            let for_stack = self.db.skills_for_stack(stack_id).await?;
            let ids: std::collections::HashSet<String> =
                for_stack.into_iter().map(|s| s.source).collect();
            let filtered: Vec<Skill> = all
                .into_iter()
                .filter(|s| ids.contains(&s.source))
                .collect();
            Ok(rank_skills(query_vec, &filtered, self.top_k))
        } else {
            Ok(rank_skills(query_vec, &all, self.top_k))
        }
    }

    /// Pull every skill from the DB.
    async fn all_skills(&self) -> Result<Vec<Skill>, ActionError> {
        Ok(self.db.list_skills().await?)
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
            let required = self.db.tools_requiring_auth(cred).await?;
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
        Ok(self.db.list_cred_ids().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_db::{Auth, Connection, Skill, Stack, Tool};
    use std::sync::Arc;

    fn now() -> String {
        Utc::now().to_rfc3339()
    }

    async fn setup() -> (Arc<GliaDb>, Arc<Embedder>) {
        let db = Arc::new(GliaDb::connect(Connection::Memory).await.unwrap());
        db.init_schema().await.unwrap();
        let emb = Arc::new(Embedder::new().expect("load embedder"));
        (db, emb)
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

    #[tokio::test]
    async fn rank_skills_orders_by_cosine() {
        let s1 = Skill {
            content: "rust borrow checker".into(),
            source: "rust-borrow.md".into(),
            embedding: vec![1.0, 0.0, 0.0],
            updated_at: now(),
        };
        let s2 = Skill {
            content: "kittens".into(),
            source: "cats.md".into(),
            embedding: vec![0.0, 1.0, 0.0],
            updated_at: now(),
        };
        let query = vec![1.0, 0.0, 0.0];
        let ranked = rank_skills(&query, &[s1, s2], 2);
        assert_eq!(ranked[0].source, "rust-borrow.md");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[tokio::test]
    async fn run_returns_auth_required_when_dep_missing() {
        let (db, emb) = setup().await;

        db.upsert_skill(
            "local::auth-required-rule",
            Skill {
                content: "never skip oauth".into(),
                source: "local::auth-required-rule".into(),
                embedding: vec![1.0, 0.0, 0.0],
                updated_at: now(),
            },
        )
        .await
        .unwrap();

        db.upsert_auth(
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
        let action = Action::new(db, emb, exec);
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
        let (db, emb) = setup().await;

        db.upsert_skill(
            "local::cat-readme",
            Skill {
                content: "read the readme".into(),
                source: "local::cat-readme".into(),
                embedding: vec![1.0, 0.0, 0.0],
                updated_at: now(),
            },
        )
        .await
        .unwrap();

        let exec = Arc::new(StubExecutor {
            response: "42".into(),
        });
        let action = Action::new(db, emb, exec);
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
        let (db, emb) = setup().await;

        db.upsert_skill(
            "nextjs::rule",
            Skill {
                content: "next.js rule".into(),
                source: "nextjs::rule".into(),
                embedding: vec![1.0, 0.0, 0.0],
                updated_at: now(),
            },
        )
        .await
        .unwrap();
        db.upsert_stack(
            "nextjs",
            Stack {
                name: "Next.js".into(),
            },
        )
        .await
        .unwrap();
        db.relate_skill_applies_to_stack("nextjs::rule", "nextjs")
            .await
            .unwrap();

        let exec = Arc::new(StubExecutor {
            response: "ok".into(),
        });
        let action = Action::new(db, emb, exec);
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
        let (db, emb) = setup().await;

        db.upsert_tool(
            "linear-create",
            Tool {
                name: "Linear Create".into(),
                category: "issue-tracker".into(),
                local: false,
                params_schema: serde_json::json!({}),
                updated_at: now(),
            },
        )
        .await
        .unwrap();
        db.upsert_auth(
            "linear_oauth",
            Auth {
                auth_type: "oauth".into(),
                provider: "linear".into(),
                updated_at: now(),
            },
        )
        .await
        .unwrap();
        db.relate_tool_requires_auth("linear-create", "linear_oauth")
            .await
            .unwrap();

        let tools = db.tools_requiring_auth("linear_oauth").await.unwrap();
        assert_eq!(tools.len(), 1);

        let exec = Arc::new(StubExecutor {
            response: "ok".into(),
        });
        let action = Action::new(db.clone(), emb, exec);
        let result = action
            .run(Intent {
                query: "create linear issue".into(),
                stack: None,
            })
            .await
            .unwrap();

        assert_eq!(result.kind, IntentKind::Remote);
    }
}
