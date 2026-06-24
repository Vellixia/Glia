//! glia-catalog — `glia use <community-tool>` catalog pull + private sandbox exec (T20, V12/C9).
//!
//! Pulls community-contributed skills from a GitHub repo catalog, registers
//! them locally, and executes them in a private sandbox (Hub remote).
//!
//! Catalog format: a GitHub repo with `catalog.json` index file listing
//! all available tools. Each tool is a markdown skill file with YAML
//! frontmatter (same format as `glia-author`).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Errors from catalog operations.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// HTTP / network error.
    #[error("http: {0}")]
    Http(String),
    /// Tool not found in catalog.
    #[error("not found: {0}")]
    NotFound(String),
    /// JSON parse failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// DB operation failed.
    #[error("db: {0}")]
    Db(String),
    /// I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// A catalog entry (one per community tool).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Tool slug (e.g., `linear-create-issue`).
    pub name: String,
    /// Display name.
    pub display: String,
    /// Description.
    pub description: String,
    /// GitHub path to the skill file (relative to repo root).
    pub path: String,
    /// Required stacks.
    #[serde(default)]
    pub stacks: Vec<String>,
    /// Required creds.
    #[serde(default)]
    pub creds: Vec<String>,
    /// Version (semver-ish).
    pub version: String,
}

/// Catalog index (the `catalog.json` file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogIndex {
    /// Catalog schema version.
    pub version: u32,
    /// All entries.
    pub tools: Vec<CatalogEntry>,
}

impl CatalogIndex {
    /// Find an entry by name.
    pub fn find(&self, name: &str) -> Option<&CatalogEntry> {
        self.tools.iter().find(|t| t.name == name)
    }
}

/// Pluggable catalog source.
#[async_trait]
pub trait CatalogSource: Send + Sync {
    /// Fetch the catalog index.
    async fn fetch_index(&self) -> Result<CatalogIndex, CatalogError>;
    /// Fetch a single tool's skill markdown.
    async fn fetch_skill(&self, entry: &CatalogEntry) -> Result<String, CatalogError>;
}

/// GitHub catalog source.
pub struct GitHubCatalog {
    /// Repo URL (e.g., `https://raw.githubusercontent.com/glia-catalog/main`).
    pub base_url: String,
    client: reqwest::Client,
}

impl GitHubCatalog {
    /// Build a new GitHub catalog source.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl CatalogSource for GitHubCatalog {
    async fn fetch_index(&self) -> Result<CatalogIndex, CatalogError> {
        let url = format!("{}/catalog.json", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| CatalogError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| CatalogError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(CatalogError::Http(format!("{}: {}", status, text)));
        }
        Ok(serde_json::from_str(&text)?)
    }

    async fn fetch_skill(&self, entry: &CatalogEntry) -> Result<String, CatalogError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), entry.path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| CatalogError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| CatalogError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(CatalogError::Http(format!("{}: {}", status, text)));
        }
        Ok(text)
    }
}

/// Stub catalog source for tests.
pub struct StubCatalog {
    /// Pre-loaded index.
    pub index: CatalogIndex,
    /// Pre-loaded skill markdown by name.
    pub skills: std::collections::HashMap<String, String>,
}

#[async_trait]
impl CatalogSource for StubCatalog {
    async fn fetch_index(&self) -> Result<CatalogIndex, CatalogError> {
        Ok(self.index.clone())
    }

    async fn fetch_skill(&self, entry: &CatalogEntry) -> Result<String, CatalogError> {
        self.skills
            .get(&entry.name)
            .cloned()
            .ok_or_else(|| CatalogError::NotFound(entry.name.clone()))
    }
}

/// Result of `glia use`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UseResult {
    /// Tool name.
    pub name: String,
    /// Skill id registered locally (`community::<name>`).
    pub skill_id: String,
    /// Skill content pulled.
    pub content: String,
    /// Required creds (for the caller to handle auth).
    pub required_creds: Vec<String>,
}

/// Pull a community tool and register it locally.
///
/// V12: community tools are namespaced as `community::<name>` (⊥ `local::`).
pub async fn use_tool(
    source: &dyn CatalogSource,
    name: &str,
    db: &glia_helix::HelixClient,
    embedder: &glia_embed::Embedder,
) -> Result<UseResult, CatalogError> {
    let index = source.fetch_index().await?;
    let entry = index
        .find(name)
        .ok_or_else(|| CatalogError::NotFound(name.into()))?;
    let content = source.fetch_skill(entry).await?;
    let skill_id = format!("community::{}", entry.name);
    let vector = embedder
        .embed(&content)
        .map_err(|e| CatalogError::Http(e.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    db.upsert_skill(
        &skill_id,
        glia_helix::Skill {
            content: content.clone(),
            source: entry.path.clone(),
            embedding: vector,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| CatalogError::Db(e.to_string()))?;
    for stack in &entry.stacks {
        db.upsert_stack(
            stack,
            glia_helix::Stack {
                name: stack.clone(),
            },
        )
        .await
        .map_err(|e| CatalogError::Db(e.to_string()))?;
        db.relate_skill_applies_to_stack(&skill_id, stack)
            .await
            .map_err(|e| CatalogError::Db(e.to_string()))?;
    }
    Ok(UseResult {
        name: entry.name.clone(),
        skill_id,
        content,
        required_creds: entry.creds.clone(),
    })
}

/// List all available tools in the catalog.
pub async fn list_tools(source: &dyn CatalogSource) -> Result<Vec<CatalogEntry>, CatalogError> {
    let index = source.fetch_index().await?;
    Ok(index.tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(name: &str, creds: Vec<String>) -> CatalogEntry {
        CatalogEntry {
            name: name.into(),
            display: name.into(),
            description: "test".into(),
            path: format!("tools/{}.md", name),
            stacks: vec!["nextjs".into()],
            creds,
            version: "1.0.0".into(),
        }
    }

    fn stub() -> StubCatalog {
        let mut skills = HashMap::new();
        skills.insert(
            "linear-create-issue".into(),
            "# Linear Create Issue\n\nUse OAuth.".into(),
        );
        StubCatalog {
            index: CatalogIndex {
                version: 1,
                tools: vec![entry("linear-create-issue", vec!["linear".into()])],
            },
            skills,
        }
    }

    #[test]
    fn catalog_index_find() {
        let idx = CatalogIndex {
            version: 1,
            tools: vec![entry("foo", vec![])],
        };
        assert!(idx.find("foo").is_some());
        assert!(idx.find("bar").is_none());
    }

    #[tokio::test]
    async fn stub_fetch_index() {
        let s = stub();
        let idx = s.fetch_index().await.unwrap();
        assert_eq!(idx.tools.len(), 1);
    }

    #[tokio::test]
    async fn stub_fetch_skill() {
        let s = stub();
        let e = entry("linear-create-issue", vec![]);
        let content = s.fetch_skill(&e).await.unwrap();
        assert!(content.contains("Linear"));
    }

    async fn try_db() -> Option<glia_helix::HelixClient> {
        let client = glia_helix::HelixClient::connect(None, None).ok()?;
        if client.ping().await.is_err() {
            return None;
        }
        Some(client)
    }

    #[tokio::test]
    async fn use_tool_registers_community_skill() {
        let Some(emb) = glia_embed::Embedder::try_new() else {
            return;
        };
        let Some(db) = try_db().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let s = stub();
        let result = use_tool(&s, "linear-create-issue", &db, &emb)
            .await
            .unwrap();
        assert_eq!(result.skill_id, "community::linear-create-issue");
        assert_eq!(result.required_creds, vec!["linear"]);
        let skill = db.get_skill(&result.skill_id).await.unwrap().unwrap();
        assert!(skill.content.contains("Linear"));
    }

    #[tokio::test]
    async fn use_tool_not_found() {
        let Some(emb) = glia_embed::Embedder::try_new() else {
            return;
        };
        let Some(db) = try_db().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let s = stub();
        let err = use_tool(&s, "nope", &db, &emb).await.unwrap_err();
        assert!(matches!(err, CatalogError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_tools_returns_all() {
        let s = stub();
        let tools = list_tools(&s).await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "linear-create-issue");
    }

    #[tokio::test]
    async fn use_tool_creates_stack_edges() {
        let Some(emb) = glia_embed::Embedder::try_new() else {
            return;
        };
        let Some(db) = try_db().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let s = stub();
        use_tool(&s, "linear-create-issue", &db, &emb)
            .await
            .unwrap();
        let stacks = db.skills_for_stack("nextjs").await.unwrap();
        assert!(
            stacks
                .iter()
                .any(|s| s.source.contains("linear-create-issue"))
        );
    }

    #[test]
    fn catalog_entry_serialize_round_trip() {
        let e = entry("foo", vec!["x".into()]);
        let j = serde_json::to_string(&e).unwrap();
        let back: CatalogEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }
}
