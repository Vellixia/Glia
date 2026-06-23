//! glia-db — SurrealDB integration for Glia.
//!
//! Implements V1 (embedded in CLI, persistent disk), V10 (graph edges for
//! stack-aware RAG), V16 (local:: namespace + LWW sync foundation).
//!
//! Schema:
//! ```text
//! tool --REQUIRES--> auth
//! skill --APPLIES_TO--> stack
//! user --HAS_ACCESS_TO--> tool
//! ```

use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use thiserror::Error;

/// Errors from the DB layer.
#[derive(Debug, Error)]
pub enum DbError {
    /// SurrealDB internal error.
    #[error("surrealdb: {0}")]
    Surreal(#[from] surrealdb::Error),
    /// Connection failed.
    #[error("connect: {0}")]
    Connect(String),
    /// Record not found.
    #[error("not found: {0}")]
    NotFound(String),
    /// Serialization error.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

/// A tool entry in the graph (e.g., `linear-create-issue`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Display name.
    pub name: String,
    /// Tool category (e.g., `issue-tracker`, `filesystem`).
    pub category: String,
    /// Whether this tool runs locally or via Hub sandbox.
    pub local: bool,
    /// JSON schema for params validation.
    pub params_schema: serde_json::Value,
    /// Updated timestamp (for LWW, V16).
    pub updated_at: String,
}

/// An auth entry (e.g., `linear_oauth`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    /// Auth type (`oauth`, `api_key`, `database`).
    pub auth_type: String,
    /// Provider name.
    pub provider: String,
    /// Updated timestamp.
    pub updated_at: String,
}

/// A skill document (e.g., `supabase-auth-rules`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill content (markdown chunks or rules).
    pub content: String,
    /// Source file path (for citation, V4).
    pub source: String,
    /// Vector embedding (f32 array).
    pub embedding: Vec<f32>,
    /// Updated timestamp (for LWW).
    pub updated_at: String,
}

/// A tech stack entry (e.g., `nextjs`, `supabase`).
///
/// Note: the record key IS the stack id (e.g., `stack:nextjs`), so the
/// `id` is the Rust struct's record key, not a stored field. Only `name`
/// is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    /// Display name.
    pub name: String,
}

/// Row returned by `SELECT in FROM needs_auth WHERE out = $cred`.
#[derive(Debug, Deserialize)]
struct EdgeInRow {
    /// The `in` record id.
    #[serde(rename = "in")]
    pub subject: RecordId,
}

/// Edge: tool --needs_auth--> cred.
///
/// SurrealDB's relation wire format requires `in` and `out` keys; this
/// struct serializes its fields to those names via serde renames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedsAuthEdge {
    /// `in` record id (tool).
    #[serde(rename = "in")]
    pub subject: RecordId,
    /// `out` record id (cred).
    #[serde(rename = "out")]
    pub object: RecordId,
}

/// Edge: skill --applies_to_stack--> stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliesToStackEdge {
    /// `in` record id (skill).
    #[serde(rename = "in")]
    pub subject: RecordId,
    /// `out` record id (stack).
    #[serde(rename = "out")]
    pub object: RecordId,
}

/// Connection mode — embedded (CLI) or remote (Hub).
pub enum Connection {
    /// Embedded in-process with SurrealKV disk persistence (pure Rust, no C++).
    Embedded(std::path::PathBuf),
    /// Remote SurrealDB server via WebSocket.
    Remote(String),
    /// In-memory (for tests).
    Memory,
}

/// Glia database handle. Wraps a SurrealDB client (unified Any engine).
pub struct GliaDb {
    db: Surreal<Any>,
}

impl GliaDb {
    /// Connect to SurrealDB.
    ///
    /// Note: the Rust SDK's `query` API does NOT inherit the connection's
    /// session `use_ns/use_db` setting, so every raw query must be prefixed
    /// with `USE NS glia DB glia;`. We still call `use_ns/use_db` here so
    /// the typed `select`/`upsert` API works.
    pub async fn connect(conn: Connection) -> Result<Self, DbError> {
        let db = match conn {
            Connection::Embedded(path) => {
                tracing::info!(path = ?path, "connecting embedded surrealkv");
                let path_str = format!("surrealkv://{}", path.display());
                surrealdb::engine::any::connect(&path_str).await?
            }
            Connection::Remote(addr) => {
                tracing::info!(addr = %addr, "connecting remote ws");
                surrealdb::engine::any::connect(format!("ws://{addr}")).await?
            }
            Connection::Memory => {
                tracing::info!("connecting in-memory");
                surrealdb::engine::any::connect("memory").await?
            }
        };

        db.use_ns("glia").use_db("glia").await?;

        Ok(Self { db })
    }

    /// Initialize namespace, database, and schema.
    ///
    /// The `use_ns/use_db` already set in `connect` applies to typed API
    /// commands (`Create`, `Insert`, `Select`, etc.), so we use typed
    /// `query` for `DEFINE TABLE` statements. Each `DEFINE TABLE` is sent
    /// as a single-statement typed `query` call so the server-side session
    /// (set by `connect`) is in effect.
    pub async fn init_schema(&self) -> Result<(), DbError> {
        let defines = [
            "DEFINE TABLE IF NOT EXISTS tool;",
            "DEFINE TABLE IF NOT EXISTS cred;",
            "DEFINE TABLE IF NOT EXISTS skill;",
            "DEFINE TABLE IF NOT EXISTS stack;",
            "DEFINE TABLE IF NOT EXISTS needs_auth TYPE RELATION FROM tool TO cred;",
            "DEFINE TABLE IF NOT EXISTS applies_to_stack TYPE RELATION FROM skill TO stack;",
        ];
        for stmt in defines {
            self.db.query(stmt).await?;
        }
        Ok(())
    }

    /// Upsert a tool record.
    pub async fn upsert_tool(&self, id: &str, tool: Tool) -> Result<(), DbError> {
        let _: Option<Tool> = self.db.upsert(("tool", id)).content(tool).await?;
        Ok(())
    }

    /// Upsert an auth record.
    pub async fn upsert_auth(&self, id: &str, auth: Auth) -> Result<(), DbError> {
        let _: Option<Auth> = self.db.upsert(("cred", id)).content(auth).await?;
        Ok(())
    }

    /// Upsert a skill record. Supports `local::` namespacing (V16).
    pub async fn upsert_skill(&self, id: &str, skill: Skill) -> Result<(), DbError> {
        let _: Option<Skill> = self.db.upsert(("skill", id)).content(skill).await?;
        Ok(())
    }

    /// Upsert a stack record.
    pub async fn upsert_stack(&self, id: &str, stack: Stack) -> Result<(), DbError> {
        let _: Option<Stack> = self.db.upsert(("stack", id)).content(stack).await?;
        Ok(())
    }

    /// Create a `needs_auth` edge: tool → cred.
    ///
    /// Uses the typed `Command::InsertRelation` path, which carries the
    /// connection's `use_ns/use_db` session — unlike the `query()` API
    /// in SurrealDB Rust SDK 2.6, which uses a separate session and
    /// silently writes to the root namespace.
    pub async fn relate_tool_requires_auth(
        &self,
        tool_id: &str,
        auth_id: &str,
    ) -> Result<(), DbError> {
        let edge = NeedsAuthEdge {
            subject: ("tool", tool_id).into(),
            object: ("cred", auth_id).into(),
        };
        // Use table-resource `insert` (not record-id) so the SDK doesn't
        // clobber the struct's `in`/`out` fields with an `id` field.
        let edges: Vec<NeedsAuthEdge> = self.db.insert("needs_auth").relation(vec![edge]).await?;
        tracing::debug!(count = edges.len(), "inserted needs_auth edges");
        Ok(())
    }

    /// Create an `applies_to_stack` edge: skill → stack.
    pub async fn relate_skill_applies_to_stack(
        &self,
        skill_id: &str,
        stack_id: &str,
    ) -> Result<(), DbError> {
        let edge = AppliesToStackEdge {
            subject: ("skill", skill_id).into(),
            object: ("stack", stack_id).into(),
        };
        let edges: Vec<AppliesToStackEdge> = self
            .db
            .insert("applies_to_stack")
            .relation(vec![edge])
            .await?;
        tracing::debug!(count = edges.len(), "inserted applies_to_stack edges");
        Ok(())
    }

    /// Query tools that require a given auth.
    ///
    /// SurrealDB Rust SDK 2.6's `query` API uses a separate session from
    /// the typed API. We re-assert `use_ns/use_db` on the connection
    /// immediately before the query, then read all `needs_auth` edges and
    /// filter in Rust. This is correct but O(N) in edges; replace with
    /// a graph index when graph size warrants it.
    pub async fn tools_requiring_auth(&self, auth_id: &str) -> Result<Vec<Tool>, DbError> {
        // Re-assert session for the query API.
        self.db.use_ns("glia").use_db("glia").await?;
        let cred_thing: RecordId = ("cred", auth_id).into();
        let mut result = self
            .db
            .query("SELECT in FROM needs_auth WHERE out = $cred_thing")
            .bind(("cred_thing", cred_thing))
            .await?;
        let edges: Vec<EdgeInRow> = result.take(0)?;
        if edges.is_empty() {
            return Ok(Vec::new());
        }
        let mut tools = Vec::new();
        for edge in edges {
            if edge.subject.table() == "tool"
                && let Ok(id) = String::try_from(edge.subject.key().clone())
                && let Some(tool) = self.db.select(("tool", id.as_str())).await?
            {
                tools.push(tool);
            }
        }
        Ok(tools)
    }

    /// Query skills that apply to a given stack.
    pub async fn skills_for_stack(&self, stack_id: &str) -> Result<Vec<Skill>, DbError> {
        // Re-assert session for the query API.
        self.db.use_ns("glia").use_db("glia").await?;
        let stack_thing: RecordId = ("stack", stack_id).into();
        let mut result = self
            .db
            .query("SELECT in FROM applies_to_stack WHERE out = $stack_thing")
            .bind(("stack_thing", stack_thing))
            .await?;
        let edges: Vec<EdgeInRow> = result.take(0)?;
        if edges.is_empty() {
            return Ok(Vec::new());
        }
        let mut skills = Vec::new();
        for edge in edges {
            if edge.subject.table() == "skill"
                && let Ok(id) = String::try_from(edge.subject.key().clone())
                && let Some(skill) = self.db.select(("skill", id.as_str())).await?
            {
                skills.push(skill);
            }
        }
        Ok(skills)
    }

    /// Count `applies_to_stack` edges where `in = skill::<skill_id>`.
    /// Used by `glia-synth` to weight skills by graph centrality (V19).
    pub async fn count_applies_to_stack_for(&self, skill_id: &str) -> Result<usize, DbError> {
        self.db.use_ns("glia").use_db("glia").await?;
        let skill_thing: RecordId = ("skill", skill_id).into();
        let mut result = self
            .db
            .query("SELECT count() AS n FROM applies_to_stack WHERE in = $skill_thing GROUP ALL")
            .bind(("skill_thing", skill_thing))
            .await?;
        #[derive(serde::Deserialize)]
        struct Row {
            n: i64,
        }
        let rows: Vec<Row> = result.take(0)?;
        Ok(rows.first().map(|r| r.n as usize).unwrap_or(0))
    }

    /// Get a skill by ID.
    pub async fn get_skill(&self, skill_id: &str) -> Result<Option<Skill>, DbError> {
        let skill: Option<Skill> = self.db.select(("skill", skill_id)).await?;
        Ok(skill)
    }

    /// List every skill in the database.
    pub async fn list_skills(&self) -> Result<Vec<Skill>, DbError> {
        let skills: Vec<Skill> = self.db.select("skill").await?;
        Ok(skills)
    }

    /// List every skill in the database with its record id.
    /// Used by `glia-sync` for bidirectional sync (T22).
    pub async fn list_skills_with_ids(&self) -> Result<Vec<(String, Skill)>, DbError> {
        self.db.use_ns("glia").use_db("glia").await?;
        #[derive(Deserialize)]
        struct Row {
            id: RecordId,
            #[serde(flatten)]
            skill: Skill,
        }
        let rows: Vec<Row> = self.db.select("skill").await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                String::try_from(r.id.key().clone())
                    .ok()
                    .map(|id| (id, r.skill))
            })
            .collect())
    }

    /// List every cred id in the database.
    pub async fn list_cred_ids(&self) -> Result<Vec<String>, DbError> {
        #[derive(Deserialize)]
        struct Row {
            id: RecordId,
        }
        let rows: Vec<Row> = self.db.select("cred").await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| String::try_from(r.id.key().clone()).ok())
            .collect())
    }

    /// Get a tool by ID.
    pub async fn get_tool(&self, tool_id: &str) -> Result<Option<Tool>, DbError> {
        let tool: Option<Tool> = self.db.select(("tool", tool_id)).await?;
        Ok(tool)
    }

    /// Get an auth/cred by ID.
    pub async fn get_auth(&self, auth_id: &str) -> Result<Option<Auth>, DbError> {
        let auth: Option<Auth> = self.db.select(("cred", auth_id)).await?;
        Ok(auth)
    }

    /// Check if a skill ID is local-namespaced (V16).
    pub fn is_local_skill(skill_id: &str) -> bool {
        skill_id.starts_with("local::")
    }

    /// Raw query escape hatch.
    ///
    /// In SurrealDB Rust SDK 2.6, the `query` API uses a different session
    /// from the typed `select`/`upsert`/`insert` API. The typed API
    /// honors `use_ns/use_db` set on connect; the `query` API does not.
    /// Always include `USE NS glia DB glia;` at the start of any raw
    /// query string, or prefer the typed API methods on this type.
    pub async fn query(&self, q: &str) -> Result<surrealdb::Response, DbError> {
        tracing::debug!(query = %q, "executing raw surreal query");
        self.db.query(q).await.map_err(Into::into)
    }
}
