//! glia-helix — HelixDB integration for Glia.
//!
//! Replaces the SurrealDB-backed `glia-db` (v0.1.0). Single client type,
//! `HelixClient`, that talks HTTP to a HelixDB instance on `localhost:6969`
//! by default (configurable via `GLIA_HELIX_URL`). Hub embeds a HelixDB
//! container; CLI is a pure HTTP consumer.
//!
//! Implements V1 (Hub-authoritative persistent storage), V10 (graph edges
//! for stack-aware RAG), V16 (local:: namespace), V19 (synthesis reweight).
//!
//! Schema:
//! ```text
//! tool  --REQUIRES--> cred
//! skill --APPLIES_TO--> stack
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from the HelixDB layer.
#[derive(Debug, Error)]
pub enum HelixError {
    /// Connection failed.
    #[error("connect: {0}")]
    Connect(String),
    /// HTTP transport error.
    #[error("http: {0}")]
    Http(String),
    /// Serialization error.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// Record not found.
    #[error("not found: {0}")]
    NotFound(String),
    /// Query error.
    #[error("query: {0}")]
    Query(String),
}

/// Result alias for HelixDB operations.
pub type HelixResult<T> = Result<T, HelixError>;

/// A tool entry in the graph (e.g., `linear-create-issue`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Auth {
    /// Auth type (`oauth`, `api_key`, `database`).
    pub auth_type: String,
    /// Provider name.
    pub provider: String,
    /// Updated timestamp.
    pub updated_at: String,
}

/// A skill document (e.g., `supabase-auth-rules`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
/// The record key IS the stack id (e.g., `nextjs`), so only `name` is stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stack {
    /// Display name.
    pub name: String,
}

/// Glia HelixDB client. Wraps the HelixDB HTTP API.
#[derive(Clone)]
pub struct HelixClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl std::fmt::Debug for HelixClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelixClient")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

impl HelixClient {
    /// Connect to a HelixDB instance.
    ///
    /// `url` = instance base URL (e.g. `http://127.0.0.1:6969`). Pass
    /// `None` to default to `http://127.0.0.1:6969`. `api_key` attaches
    /// an `Authorization: Bearer <key>` header to every request.
    pub fn connect(url: Option<&str>, api_key: Option<&str>) -> HelixResult<Self> {
        let base_url = url
            .map(String::from)
            .unwrap_or_else(|| "http://127.0.0.1:6969".to_string());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| HelixError::Connect(e.to_string()))?;
        Ok(Self {
            base_url,
            api_key: api_key.map(String::from),
            http,
        })
    }

    /// Health probe: hits a Glia-specific query endpoint. Returns Ok only
    /// if the server is HelixDB-compatible AND has Glia queries deployed
    /// (otherwise returns Err so tests don't run against the wrong server).
    pub async fn ping(&self) -> HelixResult<()> {
        match self.query_raw("list_skills", serde_json::json!({})).await {
            Ok(_) => Ok(()),
            Err(HelixError::Query(msg)) if msg.contains("query not found") => Err(
                HelixError::Connect("helix server reachable but Glia schema not deployed".into()),
            ),
            Err(HelixError::Query(msg)) if msg.starts_with("404") => Err(HelixError::Connect(
                "helix server reachable but Glia schema not deployed".into(),
            )),
            Err(HelixError::Http(msg)) => Err(HelixError::Connect(msg)),
            Err(e) => Err(e),
        }
    }

    /// Run a single `#[register]`-style dynamic query against the instance.
    ///
    /// `name` is the query name (matches `HelixQuery::name()` of the DSL builder).
    /// `body` is the serialized JSON body Helix expects.
    /// Returns the raw `serde_json::Value` so callers can shape it.
    pub async fn query_raw(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> HelixResult<serde_json::Value> {
        let url = format!("{}/v1/{}", self.base_url, name);
        let mut req = self.http.post(&url).json(&body);
        if let Some(k) = &self.api_key {
            req = req.bearer_auth(k);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| HelixError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HelixError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(HelixError::Query(format!("{status}: {text}")));
        }
        serde_json::from_str(&text).map_err(HelixError::from)
    }

    // ───────────────────────── entity writes ─────────────────────────

    /// Upsert a tool record.
    pub async fn upsert_tool(&self, id: &str, tool: Tool) -> HelixResult<()> {
        let body = serde_json::json!({ "id": id, "tool": tool });
        self.query_raw("upsert_tool", body).await?;
        Ok(())
    }

    /// Upsert an auth (cred) record.
    pub async fn upsert_auth(&self, id: &str, auth: Auth) -> HelixResult<()> {
        let body = serde_json::json!({ "id": id, "auth": auth });
        self.query_raw("upsert_auth", body).await?;
        Ok(())
    }

    /// Upsert a skill record. Supports `local::` namespacing (V16).
    pub async fn upsert_skill(&self, id: &str, skill: Skill) -> HelixResult<()> {
        let body = serde_json::json!({ "id": id, "skill": skill });
        self.query_raw("upsert_skill", body).await?;
        Ok(())
    }

    /// Upsert a stack record.
    pub async fn upsert_stack(&self, id: &str, stack: Stack) -> HelixResult<()> {
        let body = serde_json::json!({ "id": id, "stack": stack });
        self.query_raw("upsert_stack", body).await?;
        Ok(())
    }

    /// Create a `needs_auth` edge: tool → cred.
    pub async fn relate_tool_requires_auth(&self, tool_id: &str, auth_id: &str) -> HelixResult<()> {
        let body = serde_json::json!({ "tool_id": tool_id, "auth_id": auth_id });
        self.query_raw("relate_tool_requires_auth", body).await?;
        Ok(())
    }

    /// Create an `applies_to_stack` edge: skill → stack.
    pub async fn relate_skill_applies_to_stack(
        &self,
        skill_id: &str,
        stack_id: &str,
    ) -> HelixResult<()> {
        let body = serde_json::json!({ "skill_id": skill_id, "stack_id": stack_id });
        self.query_raw("relate_skill_applies_to_stack", body)
            .await?;
        Ok(())
    }

    // ───────────────────────── entity reads ──────────────────────────

    /// Get a tool by ID.
    pub async fn get_tool(&self, tool_id: &str) -> HelixResult<Option<Tool>> {
        let body = serde_json::json!({ "id": tool_id });
        let v = self.query_raw("get_tool", body).await?;
        Ok(serde_json::from_value(v).ok())
    }

    /// Get an auth/cred by ID.
    pub async fn get_auth(&self, auth_id: &str) -> HelixResult<Option<Auth>> {
        let body = serde_json::json!({ "id": auth_id });
        let v = self.query_raw("get_auth", body).await?;
        Ok(serde_json::from_value(v).ok())
    }

    /// Get a skill by ID.
    pub async fn get_skill(&self, skill_id: &str) -> HelixResult<Option<Skill>> {
        let body = serde_json::json!({ "id": skill_id });
        let v = self.query_raw("get_skill", body).await?;
        Ok(serde_json::from_value(v).ok())
    }

    /// List every skill in the database.
    pub async fn list_skills(&self) -> HelixResult<Vec<Skill>> {
        let v = self.query_raw("list_skills", serde_json::json!({})).await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    /// List every skill with its record id (for sync, T22).
    pub async fn list_skills_with_ids(&self) -> HelixResult<Vec<(String, Skill)>> {
        let v = self
            .query_raw("list_skills_with_ids", serde_json::json!({}))
            .await?;
        let raw: Vec<serde_json::Value> = serde_json::from_value(v).unwrap_or_default();
        Ok(raw
            .into_iter()
            .filter_map(|row| {
                let id = row.get("id")?.as_str()?.to_string();
                let skill: Skill = serde_json::from_value(row.get("skill")?.clone()).ok()?;
                Some((id, skill))
            })
            .collect())
    }

    /// List every cred id (for sync, T22).
    pub async fn list_cred_ids(&self) -> HelixResult<Vec<String>> {
        let v = self
            .query_raw("list_cred_ids", serde_json::json!({}))
            .await?;
        let raw: Vec<serde_json::Value> = serde_json::from_value(v).unwrap_or_default();
        Ok(raw
            .into_iter()
            .filter_map(|row| row.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect())
    }

    /// Query tools that require a given auth.
    pub async fn tools_requiring_auth(&self, auth_id: &str) -> HelixResult<Vec<Tool>> {
        let body = serde_json::json!({ "auth_id": auth_id });
        let v = self.query_raw("tools_requiring_auth", body).await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    /// Query skills that apply to a given stack.
    pub async fn skills_for_stack(&self, stack_id: &str) -> HelixResult<Vec<Skill>> {
        let body = serde_json::json!({ "stack_id": stack_id });
        let v = self.query_raw("skills_for_stack", body).await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    /// Count `applies_to_stack` edges where `in = skill::<skill_id>`.
    /// Used by `glia-synth` for V19 reweight.
    pub async fn count_applies_to_stack_for(&self, skill_id: &str) -> HelixResult<usize> {
        let body = serde_json::json!({ "skill_id": skill_id });
        let v = self.query_raw("count_applies_to_stack_for", body).await?;
        let n = v
            .get("n")
            .and_then(|n| n.as_i64())
            .ok_or_else(|| HelixError::Query("missing n in response".into()))?;
        Ok(n.max(0) as usize)
    }

    /// Check if a skill ID is local-namespaced (V16).
    pub fn is_local_skill(skill_id: &str) -> bool {
        skill_id.starts_with("local::")
    }

    /// Base URL the client is talking to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Build a default HelixClient from env vars.
///
/// `GLIA_HELIX_URL` (default `http://127.0.0.1:6969`).
/// `GLIA_HELIX_TOKEN` (optional bearer token).
pub fn from_env() -> HelixResult<HelixClient> {
    let url = std::env::var("GLIA_HELIX_URL").ok();
    let key = std::env::var("GLIA_HELIX_TOKEN").ok();
    HelixClient::connect(url.as_deref(), key.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_namespace_detection() {
        assert!(HelixClient::is_local_skill("local::use-zustand"));
        assert!(!HelixClient::is_local_skill("supabase-auth-rules"));
        assert!(!HelixClient::is_local_skill("community::linear"));
    }

    #[tokio::test]
    async fn connect_succeeds() {
        let client = HelixClient::connect(Some("http://127.0.0.1:1"), None).unwrap();
        assert_eq!(client.base_url(), "http://127.0.0.1:1");
    }
}
