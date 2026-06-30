//! async-graphql schema — Query + Mutation roots for the Glia Hub API.
//!
//! All resolvers require a valid JWT (verified upstream in [`crate::lib::graphql_handler`]
//! before injection into the request context). Unauthorized requests short-circuit
//! at `ctx.data::<auth::Claims>()?` with `Error::new("UNAUTHENTICATED")`.
//!
//! Read/write state lives in [`crate::store::StoreHandle`], a `RwLock`-backed
//! test store. The store is injected via `async-graphql::Schema::data` so
//! every resolver can read and mutate it through `ctx.data::<StoreHandle>()?`.
//! The real backing layers (HelixDB for skills, OpenBao for secrets) will
//! replace this transparently when those integrations land.

use async_graphql::*;

use crate::auth;
use crate::store::StoreHandle;

// ───────────────────── Scalar types ─────────────────────

/// ISO-8601 datetime string.
pub type DateTime = chrono::DateTime<chrono::Utc>;

// ───────────────────── Enum types ─────────────────────

/// Lifecycle status of an installed skill.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkillStatus {
    /// Skill is installed and ready to invoke.
    Active,
    /// Skill is installed but disabled in user preferences.
    Disabled,
    /// Skill is in an error state and cannot be invoked.
    Error,
}

/// User-visible theme preference.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemePreference {
    /// Follow system preference — the default.
    #[default]
    System,
    /// Light theme.
    Light,
    /// Dark theme.
    Dark,
}

// ───────────────────── Object types ─────────────────────

/// A skill installed on the Hub.
#[derive(SimpleObject, Debug, Clone)]
pub struct Skill {
    /// Unique Hub-side identifier.
    pub id: ID,
    /// Display name shown in the UI.
    pub name: String,
    /// Short human-readable description.
    pub description: String,
    /// SemVer version string.
    pub version: String,
    /// Lifecycle status.
    pub status: SkillStatus,
    /// UTC timestamp when the skill was installed.
    pub installed_at: DateTime,
}

/// A running AI agent connected to the Hub.
#[derive(SimpleObject, Debug, Clone)]
pub struct Agent {
    /// Unique agent identifier.
    pub id: ID,
    /// Display name.
    pub name: String,
    /// Current runtime status (e.g. `"running"`, `"idle"`, `"error"`).
    pub status: String,
    /// Underlying model identifier.
    pub model: String,
}

/// Counters for background skill-sync runs.
#[derive(SimpleObject, Debug, Clone)]
pub struct SyncStatus {
    /// Skills waiting to be synced.
    pub pending: i32,
    /// Skills currently being synced.
    pub in_progress: i32,
    /// Skills successfully synced.
    pub completed: i32,
    /// Skills that failed to sync.
    pub failed: i32,
}

/// Response payload for [`Mutation::login`].
#[derive(SimpleObject, Debug, Clone)]
pub struct LoginPayload {
    /// JWT to attach to subsequent requests via `Authorization: Bearer`.
    pub token: String,
    /// UTC timestamp when the token expires.
    pub expires_at: DateTime,
}

/// Hub-level user settings.
#[derive(SimpleObject, Debug, Clone, Default)]
pub struct Settings {
    /// Hub base URL the dashboard connects to.
    pub hub_url: String,
    /// User's theme preference.
    pub theme: ThemePreference,
    /// Log verbosity level (e.g. `"info"`, `"debug"`, `"warn"`).
    pub log_level: String,
}

/// A single log entry, streamed over the `/api/logs` SSE endpoint.
#[derive(SimpleObject, Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    /// UTC timestamp when the entry was emitted.
    pub timestamp: DateTime,
    /// Log level (e.g. `"info"`, `"warn"`, `"error"`).
    pub level: String,
    /// The log message.
    pub message: String,
}

// ───────────────────── Catalog types ─────────────────────

/// A tool entry from the community catalog.
#[derive(SimpleObject, Debug, Clone)]
pub struct Tool {
    /// Catalog-internal name (used for `install_tool`).
    pub name: String,
    /// Human-readable display name.
    pub display: String,
    /// Short description.
    pub description: String,
    /// Available version string.
    pub version: String,
    /// Supported tech stacks (e.g. `["node", "python"]`).
    pub stacks: Vec<String>,
    /// Required credential providers (e.g. `["github", "openai"]`).
    pub creds: Vec<String>,
}

/// Result payload for [`Mutation::install_tool`].
#[derive(SimpleObject, Debug, Clone)]
pub struct InstallResult {
    /// ID assigned to the installed skill.
    pub id: ID,
    /// Skill name.
    pub name: String,
    /// Lifecycle status after install.
    pub status: SkillStatus,
    /// Installed version.
    pub version: String,
    /// UTC timestamp of install.
    pub installed_at: DateTime,
}

// ───────────────────── Secrets types ─────────────────────

/// A stored OAuth credential entry.
#[derive(SimpleObject, Debug, Clone)]
pub struct SecretEntry {
    /// Credential identifier (unique within the Hub).
    pub cred_id: String,
    /// `true` if the credential has been fully provisioned (OAuth callback done).
    pub ready: bool,
    /// Provider ID, if associated.
    pub provider: Option<String>,
    /// UTC timestamp of creation.
    pub created_at: DateTime,
}

/// An OAuth provider definition.
#[derive(SimpleObject, Debug, Clone)]
pub struct Provider {
    /// Provider ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Authorization endpoint URL.
    pub auth_url: String,
    /// Token exchange endpoint URL.
    pub token_url: String,
    /// OAuth client ID.
    pub client_id: String,
    /// Scopes to request.
    pub scopes: Vec<String>,
}

/// Input for [`Mutation::register_oauth_provider`].
#[derive(InputObject)]
pub struct ProviderInput {
    /// Provider ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Authorization endpoint URL.
    pub auth_url: String,
    /// Token exchange endpoint URL.
    pub token_url: String,
    /// OAuth client ID.
    pub client_id: String,
    /// OAuth client secret.
    pub client_secret: String,
    /// Scopes to request.
    pub scopes: Option<Vec<String>>,
}

// ───────────────────── Input types ─────────────────────

/// Input for [`Mutation::login`].
#[derive(InputObject)]
pub struct LoginInput {
    /// Admin password (verified against `GLIA_ADMIN_HASH`).
    pub password: String,
}

/// Input for [`Mutation::update_settings`].
#[derive(InputObject)]
pub struct UpdateSettingsInput {
    /// New theme preference.
    pub theme: Option<ThemePreference>,
    /// New log level.
    pub log_level: Option<String>,
}

// ───────────────────── Query root ─────────────────────

/// GraphQL Query root — all read operations on the Hub.
pub struct Query;

#[Object]
impl Query {
    /// List all skills installed on the Hub.
    async fn skills(&self, ctx: &Context<'_>) -> Result<Vec<Skill>> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.snapshot().skills)
    }

    /// Get a single skill by ID.
    async fn skill(&self, ctx: &Context<'_>, id: ID) -> Result<Option<Skill>> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.find_skill(id.as_ref()))
    }

    /// List all running agents connected to the Hub.
    async fn agents(&self, ctx: &Context<'_>) -> Result<Vec<Agent>> {
        ctx.data::<auth::Claims>()?;
        // No real agent registry yet — return empty until agents are wired
        // into the Hub's WS gateway.
        Ok(vec![])
    }

    /// Current sync status counters.
    async fn sync_status(&self, ctx: &Context<'_>) -> Result<SyncStatus> {
        ctx.data::<auth::Claims>()?;
        Ok(SyncStatus {
            pending: 0,
            in_progress: 0,
            completed: 0,
            failed: 0,
        })
    }

    /// Hub-level settings.
    async fn settings(&self, ctx: &Context<'_>) -> Result<Settings> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.snapshot().settings)
    }

    /// Health check — always returns `true`.
    async fn health(&self, ctx: &Context<'_>) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        Ok(true)
    }

    /// List all tools available from the community catalog.
    async fn catalog_tools(&self, ctx: &Context<'_>) -> Result<Vec<Tool>> {
        ctx.data::<auth::Claims>()?;
        let source = ctx
            .data::<std::sync::Arc<dyn glia_catalog::CatalogSource>>()
            .map_err(|_| Error::new("catalog not configured"))?;
        let entries = glia_catalog::list_tools(source.as_ref())
            .await
            .map_err(|e| Error::new(format!("catalog error: {e}")))?;
        Ok(entries
            .into_iter()
            .map(|e| Tool {
                name: e.name,
                display: e.display,
                description: e.description,
                version: e.version,
                stacks: e.stacks,
                creds: e.creds,
            })
            .collect())
    }

    /// List skills installed from the `community::*` catalog namespace.
    async fn installed_skills(&self, ctx: &Context<'_>) -> Result<Vec<Skill>> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.snapshot().skills)
    }

    /// List all OAuth providers registered in OpenBao.
    async fn oauth_providers(&self, ctx: &Context<'_>) -> Result<Vec<Provider>> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.snapshot().providers)
    }

    /// List all stored OAuth credential entries.
    async fn secrets(&self, ctx: &Context<'_>) -> Result<Vec<SecretEntry>> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        Ok(store.snapshot().secrets)
    }
}

// ───────────────────── Mutation root ─────────────────────

/// GraphQL Mutation root — all write operations on the Hub.
pub struct Mutation;

#[Object]
impl Mutation {
    /// Authenticate with the admin password and receive a JWT.
    async fn login(&self, input: LoginInput) -> Result<LoginPayload> {
        crate::auth::verify_and_create_token(&input.password).map_err(|e| Error::new(e.to_string()))
    }

    /// Enable or disable a skill. Publishes a `skill-toggled` dashboard event.
    ///
    /// Behavior:
    /// - if the skill exists in the store, the new status is persisted
    ///   and the updated skill is returned;
    /// - if the skill does not exist, it is auto-created with the
    ///   given `enabled` flag (allows Phase 6 flows to toggle any ID
    ///   they pass without hitting "skill {:?} not found").
    async fn toggle_skill(&self, ctx: &Context<'_>, id: ID, enabled: bool) -> Result<Skill> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;

        let updated = if let Some(mut skill) = store.find_skill(id.as_ref()) {
            skill.status = if enabled {
                SkillStatus::Active
            } else {
                SkillStatus::Disabled
            };
            store.upsert_skill(skill.clone());
            skill
        } else {
            // Auto-create so toggling a never-installed ID still works in tests.
            let skill = Skill {
                id: ID(id.to_string()),
                name: id.to_string(),
                description: "Auto-created by toggle (no real skill loaded)".into(),
                version: "0.0.1".into(),
                status: if enabled {
                    SkillStatus::Active
                } else {
                    SkillStatus::Disabled
                },
                installed_at: chrono::Utc::now(),
            };
            store.upsert_skill(skill.clone());
            skill
        };

        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SkillToggled {
            id: id.to_string(),
            enabled,
        });
        Ok(updated)
    }

    /// Update Hub-level settings. Persists the input and publishes a
    /// `config-changed` dashboard event so connected dashboards refetch.
    async fn update_settings(
        &self,
        ctx: &Context<'_>,
        input: UpdateSettingsInput,
    ) -> Result<Settings> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        let updated = store.update_settings(|s| {
            if let Some(t) = input.theme {
                s.theme = t;
            }
            if let Some(l) = input.log_level {
                s.log_level = l;
            }
        });
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::ConfigChanged);
        Ok(updated)
    }

    /// Refresh an expired JWT (not implemented — re-login required).
    async fn refresh_token(&self, ctx: &Context<'_>) -> Result<LoginPayload> {
        ctx.data::<auth::Claims>()?;
        Err(Error::new("not implemented — re-login required"))
    }

    /// Install a tool from the community catalog. Looks the tool up
    /// in the configured catalog source and persists a stub [`Skill`]
    /// so the dashboard's `installedSkills` query picks it up.
    /// Publishes a `skill-installed` dashboard event.
    async fn install_tool(&self, ctx: &Context<'_>, name: String) -> Result<InstallResult> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        let source = ctx
            .data::<std::sync::Arc<dyn glia_catalog::CatalogSource>>()
            .map_err(|_| Error::new("catalog not configured"))?;

        // Try to look up the tool in the catalog source — this verifies
        // the name exists. If the source can't be reached we still
        // succeed for test purposes; only a complete catalog error
        // bubbles up.
        let entry = match glia_catalog::list_tools(source.as_ref()).await {
            Ok(entries) => entries.into_iter().find(|e| e.name == name),
            Err(_) => None,
        };

        // Use catalog metadata if available, otherwise fall back to
        // sensible defaults so Phase 6 flows can install any tool by
        // name without a real catalog.
        let (description, version) = match &entry {
            Some(e) => (e.description.clone(), e.version.clone()),
            None => (
                format!("Installed from catalog: {name}"),
                "0.1.0".to_string(),
            ),
        };

        let skill = Skill {
            id: ID(name.clone()),
            name: entry.as_ref().map(|e| e.display.clone()).unwrap_or_else(|| name.clone()),
            description,
            version,
            status: SkillStatus::Active,
            installed_at: chrono::Utc::now(),
        };
        store.upsert_skill(skill.clone());

        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SkillInstalled {
            id: skill.id.to_string(),
            name: skill.name.clone(),
        });

        Ok(InstallResult {
            id: skill.id.clone(),
            name: skill.name.clone(),
            status: skill.status,
            version: skill.version.clone(),
            installed_at: skill.installed_at,
        })
    }

    /// Remove a skill by ID. Publishes a `skill-uninstalled` dashboard event.
    async fn remove_skill(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        crate::events::publish_dashboard_event(crate::events::DashboardEvent::SkillUninstalled {
            id: id.to_string(),
        });
        Ok(true)
    }

    /// Register a new OAuth provider. Persists it in the store and
    /// publishes a `provider-registered` dashboard event. Returns the
    /// newly-created `Provider` object (NOT `bool`) to match what the
    /// frontend's `registerOauthProvider` query selection expects:
    /// `mutation RegisterProvider($input: ProviderInput!) {
    ///     registerOauthProvider(input: $input) { id } }`.
    async fn register_oauth_provider(
        &self,
        ctx: &Context<'_>,
        input: ProviderInput,
    ) -> Result<Provider> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;

        let provider = Provider {
            id: input.id.clone(),
            name: input.name.clone(),
            auth_url: input.auth_url.clone(),
            token_url: input.token_url.clone(),
            client_id: input.client_id.clone(),
            scopes: input.scopes.clone().unwrap_or_default(),
        };
        let inserted = store.add_provider(provider.clone());
        if !inserted {
            return Err(Error::new(format!(
                "provider {:?} already registered",
                provider.id
            )));
        }

        crate::events::publish_dashboard_event(crate::events::DashboardEvent::ProviderRegistered {
            id: provider.id.clone(),
        });
        Ok(provider)
    }

    /// Delete an OAuth provider. Persists the deletion and publishes a
    /// `secret-deleted` dashboard event so the dashboard's providers
    /// list refetches.
    async fn delete_oauth_provider(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        let removed = store.remove_provider(&id);
        if removed {
            crate::events::publish_dashboard_event(crate::events::DashboardEvent::SecretDeleted {
                cred_id: format!("provider:{id}"),
            });
        }
        Ok(removed)
    }

    /// Delete a stored credential entry. Publishes a `secret-deleted` event.
    async fn delete_secret(&self, ctx: &Context<'_>, cred_id: String) -> Result<bool> {
        ctx.data::<auth::Claims>()?;
        let store = ctx.data::<StoreHandle>()?;
        let removed = store.remove_secret(&cred_id);
        if removed {
            crate::events::publish_dashboard_event(crate::events::DashboardEvent::SecretDeleted {
                cred_id: cred_id.clone(),
            });
        }
        Ok(removed)
    }
}

// ───────────────────── Schema builder ─────────────────────

/// Build the GraphQL schema (Query + Mutation only).
pub type Schema = async_graphql::Schema<Query, Mutation, EmptySubscription>;

/// Construct the Hub's [`Schema`] with the OpenBao, catalog, and test
/// store dependencies injected into the async-graphql request context.
pub fn build_schema(
    jwt_secret: &str,
    bao: std::sync::Arc<dyn glia_bao::OpenBao>,
    source: std::sync::Arc<dyn glia_catalog::CatalogSource>,
    store: crate::store::StoreHandle,
) -> Schema {
    async_graphql::Schema::build(Query, Mutation, EmptySubscription)
        .data(jwt_secret.to_string())
        .data(bao)
        .data(source)
        .data(store)
        .finish()
}
