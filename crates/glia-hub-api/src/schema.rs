use async_graphql::*;

use crate::auth;

// ───────────────────── Scalar types ─────────────────────

/// ISO-8601 datetime string.
pub type DateTime = chrono::DateTime<chrono::Utc>;

// ───────────────────── Enum types ─────────────────────

#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkillStatus {
    Active,
    Disabled,
    Error,
}

#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePreference {
    Light,
    Dark,
    System,
}

// ───────────────────── Object types ─────────────────────

#[derive(SimpleObject, Debug, Clone)]
pub struct Skill {
    pub id: ID,
    pub name: String,
    pub description: String,
    pub version: String,
    pub status: SkillStatus,
    pub installed_at: DateTime,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Agent {
    pub id: ID,
    pub name: String,
    pub status: String,
    pub model: String,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct SyncStatus {
    pub pending: i32,
    pub in_progress: i32,
    pub completed: i32,
    pub failed: i32,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct LoginPayload {
    pub token: String,
    pub expires_at: DateTime,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Settings {
    pub hub_url: String,
    pub theme: ThemePreference,
    pub log_level: String,
}

#[derive(SimpleObject, Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: DateTime,
    pub level: String,
    pub message: String,
}

// ───────────────────── Catalog types ─────────────────────

#[derive(SimpleObject, Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub display: String,
    pub description: String,
    pub version: String,
    pub stacks: Vec<String>,
    pub creds: Vec<String>,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct InstallResult {
    pub id: ID,
    pub name: String,
    pub status: SkillStatus,
    pub version: String,
    pub installed_at: DateTime,
}

// ───────────────────── Secrets types ─────────────────────

#[derive(SimpleObject, Debug, Clone)]
pub struct SecretEntry {
    pub cred_id: String,
    pub ready: bool,
    pub provider: Option<String>,
    pub created_at: DateTime,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}

#[derive(InputObject)]
pub struct ProviderInput {
    pub id: String,
    pub name: String,
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Option<Vec<String>>,
}

// ───────────────────── Input types ─────────────────────

#[derive(InputObject)]
pub struct LoginInput {
    pub password: String,
}

#[derive(InputObject)]
pub struct UpdateSettingsInput {
    pub theme: Option<ThemePreference>,
    pub log_level: Option<String>,
}

// ───────────────────── Query root ─────────────────────

pub struct Query;

#[Object]
impl Query {
    /// List all skills in the Hub.
    async fn skills(&self, ctx: &Context<'_>) -> Result<Vec<Skill>> {
        ctx.data::<auth::Claims>()?;
        Ok(vec![
            Skill {
                id: "example-skill".into(),
                name: "Example Skill".into(),
                description: "A placeholder skill".into(),
                version: "0.1.0".into(),
                status: SkillStatus::Active,
                installed_at: chrono::Utc::now(),
            },
        ])
    }

    /// Get a single skill by ID.
    async fn skill(&self, ctx: &Context<'_>, id: ID) -> Result<Option<Skill>> {
        ctx.data::<auth::Claims>()?;
        let _ = id;
        Ok(None)
    }

    /// List all running agents.
    async fn agents(&self, ctx: &Context<'_>) -> Result<Vec<Agent>> {
        ctx.data::<auth::Claims>()?;
        Ok(vec![])
    }

    /// Current sync status between CLI and Hub.
    async fn sync_status(&self, ctx: &Context<'_>) -> Result<SyncStatus> {
        ctx.data::<auth::Claims>()?;
        Ok(SyncStatus {
            pending: 0,
            in_progress: 0,
            completed: 0,
            failed: 0,
        })
    }

    /// Hub settings.
    async fn settings(&self, ctx: &Context<'_>) -> Result<Settings> {
        ctx.data::<auth::Claims>()?;
        Ok(Settings {
            hub_url: "http://127.0.0.1:3000".into(),
            theme: ThemePreference::System,
            log_level: "info".into(),
        })
    }

    /// Health check — always returns true.
    async fn health(&self, ctx: &Context<'_>) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        Ok(true)
    }

    /// List all tools from the community catalog.
    async fn catalog_tools(&self, ctx: &Context<'_>) -> Result<Vec<Tool>> {
        ctx.data::<auth::Claims>()?;
        let source = ctx.data::<std::sync::Arc<dyn glia_catalog::CatalogSource>>()
            .map_err(|_| Error::new("catalog not configured"))?;
        let entries = glia_catalog::list_tools(source.as_ref())
            .await
            .map_err(|e| Error::new(format!("catalog error: {e}")))?;
        Ok(entries.into_iter().map(|e| Tool {
            name: e.name,
            display: e.display,
            description: e.description,
            version: e.version,
            stacks: e.stacks,
            creds: e.creds,
        }).collect())
    }

    /// List skills installed from the catalog (community::* namespace).
    async fn installed_skills(&self, ctx: &Context<'_>) -> Result<Vec<Skill>> {
        ctx.data::<auth::Claims>()?;
        // Stub — returns empty list until HelixDB is wired
        Ok(vec![])
    }

    /// List all OAuth providers registered in OpenBao.
    async fn oauth_providers(&self, ctx: &Context<'_>) -> Result<Vec<Provider>> {
        ctx.data::<auth::Claims>()?;
        Ok(vec![])
    }

    /// List all stored OAuth credential entries.
    async fn secrets(&self, ctx: &Context<'_>) -> Result<Vec<SecretEntry>> {
        ctx.data::<auth::Claims>()?;
        let _ = ctx;
        Ok(vec![])
    }
}

// ───────────────────── Mutation root ─────────────────────

pub struct Mutation;

#[Object]
impl Mutation {
    /// Authenticate with the admin password and receive a JWT.
    async fn login(&self, input: LoginInput) -> Result<LoginPayload> {
        crate::auth::verify_and_create_token(&input.password)
            .map_err(|e| Error::new(e.to_string()))
    }

    /// Enable or disable a skill.
    async fn toggle_skill(&self, ctx: &Context<'_>, id: ID, enabled: bool) -> Result<Skill> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SkillToggled {
            id: id.to_string(),
            enabled,
        });
        let _ = enabled;
        Err(Error::new(format!("skill {:?} not found", id)))
    }

    /// Update Hub settings.
    async fn update_settings(&self, ctx: &Context<'_>, input: UpdateSettingsInput) -> Result<Settings> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::ConfigChanged);
        let _ = input;
        Ok(Settings {
            hub_url: "http://127.0.0.1:3000".into(),
            theme: ThemePreference::System,
            log_level: "info".into(),
        })
    }

    /// Refresh an expired JWT.
    async fn refresh_token(&self, ctx: &Context<'_>) -> Result<LoginPayload> {
        ctx.data::<auth::Claims>()?;
        Err(Error::new("not implemented — re-login required"))
    }

    /// Install a tool from the community catalog.
    async fn install_tool(&self, ctx: &Context<'_>, name: String) -> Result<InstallResult> {
        ctx.data::<auth::Claims>()?;
        let source = ctx.data::<std::sync::Arc<dyn glia_catalog::CatalogSource>>()
            .map_err(|_| Error::new("catalog not configured"))?;
        let _ = source;
        Err(Error::new(format!("install not yet connected to HelixDB: {name}")))
    }

    /// Remove a skill by ID.
    async fn remove_skill(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SkillUninstalled {
            id: id.to_string(),
        });
        let _ = id;
        Ok(true)
    }

    /// Register a new OAuth provider.
    async fn register_oauth_provider(&self, ctx: &Context<'_>, input: ProviderInput) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::ProviderRegistered {
            id: input.id.clone(),
        });
        let _ = input;
        Ok(true)
    }

    /// Delete an OAuth provider.
    async fn delete_oauth_provider(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        let _ = id;
        Ok(true)
    }

    /// Delete a stored credential entry.
    async fn delete_secret(&self, ctx: &Context<'_>, cred_id: String) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SecretDeleted {
            cred_id,
        });
        Ok(true)
    }
}

// ───────────────────── Schema builder ─────────────────────

/// Build the GraphQL schema (Query + Mutation only).
pub type Schema = async_graphql::Schema<Query, Mutation, EmptySubscription>;

pub fn build_schema(
    jwt_secret: &str,
    bao: std::sync::Arc<dyn glia_bao::OpenBao>,
    source: std::sync::Arc<dyn glia_catalog::CatalogSource>,
) -> Schema {
    async_graphql::Schema::build(Query, Mutation, EmptySubscription)
        .data(jwt_secret.to_string())
        .data(bao)
        .data(source)
        .finish()
}
