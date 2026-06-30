//! In-memory backing store for GraphQL resolvers.
//!
//! This module provides a simple state container that powers the
//! test/dashboard flows. It is NOT a production store — the real
//! backing layers (HelixDB for skills, OpenBao for secrets) will
//! replace this once those integrations are wired. The intent is
//! that live-UI tests have a round-trippable target end-to-end so
//! that the dashboard can render real rows + events, instead of
//! hitting the hard-coded stubs in `schema.rs`.
//!
//! State is held in `RwLock`s (cheap, single-process Hub) and reset
//! whenever a fresh schema is built (which only happens at Hub boot).

use std::sync::{Arc, RwLock};

use crate::schema::{Provider, SecretEntry, Settings, Skill, SkillStatus, ThemePreference};

/// All mutable resolver state lives here.
#[derive(Debug, Default)]
pub struct TestStore {
    /// Persisted skills (installed from the catalog or auto-created via toggle).
    pub skills: Vec<Skill>,
    /// Persisted OAuth providers (registered via `registerOauthProvider`).
    pub providers: Vec<Provider>,
    /// Persisted credential entries (deleted via `deleteSecret`, added by `add_secret`).
    pub secrets: Vec<SecretEntry>,
    /// Current settings (mutated via `updateSettings`).
    pub settings: Settings,
}

impl TestStore {
    /// Construct a new, empty test store with default settings.
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            providers: Vec::new(),
            secrets: Vec::new(),
            settings: Settings {
                hub_url: "http://127.0.0.1:3000".into(),
                theme: ThemePreference::System,
                log_level: "info".into(),
            },
        }
    }
}

/// State passed to async-graphql resolvers via `ctx.data::<StoreHandle>()`.
#[derive(Clone)]
pub struct StoreHandle(pub Arc<RwLock<TestStore>>);

impl Default for StoreHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreHandle {
    /// Build a fresh handle backed by an empty in-memory store.
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(TestStore::new())))
    }

    /// Clone the current store contents so resolvers can return them
    /// without holding the read lock across an `.await`.
    pub fn snapshot(&self) -> TestStoreSnapshot {
        let r = self.0.read().expect("TestStore poisoned");
        TestStoreSnapshot {
            skills: r.skills.clone(),
            providers: r.providers.clone(),
            secrets: r.secrets.clone(),
            settings: r.settings.clone(),
        }
    }

    /// Apply `mutate` to the current settings and return the updated value.
    pub fn update_settings<F>(&self, mutate: F) -> Settings
    where
        F: FnOnce(&mut Settings),
    {
        let mut w = self.0.write().expect("TestStore poisoned");
        mutate(&mut w.settings);
        w.settings.clone()
    }

    /// Insert a provider by ID; returns `false` if it already exists.
    pub fn add_provider(&self, provider: Provider) -> bool {
        let mut w = self.0.write().expect("TestStore poisoned");
        if w.providers.iter().any(|p| p.id == provider.id) {
            return false;
        }
        w.providers.push(provider);
        true
    }

    /// Remove a provider by ID; returns `true` if one was removed.
    pub fn remove_provider(&self, id: &str) -> bool {
        let mut w = self.0.write().expect("TestStore poisoned");
        let before = w.providers.len();
        w.providers.retain(|p| p.id != id);
        before != w.providers.len()
    }

    /// Append a credential entry.
    pub fn add_secret(&self, entry: SecretEntry) {
        let mut w = self.0.write().expect("TestStore poisoned");
        w.secrets.push(entry);
    }

    /// Remove a credential entry; returns `true` if one was removed.
    pub fn remove_secret(&self, cred_id: &str) -> bool {
        let mut w = self.0.write().expect("TestStore poisoned");
        let before = w.secrets.len();
        w.secrets.retain(|s| s.cred_id != cred_id);
        before != w.secrets.len()
    }

    /// Upsert a skill by ID.
    pub fn upsert_skill(&self, skill: Skill) {
        let mut w = self.0.write().expect("TestStore poisoned");
        if let Some(existing) = w.skills.iter_mut().find(|s| s.id == skill.id) {
            *existing = skill;
        } else {
            w.skills.push(skill);
        }
    }

    /// Look up a skill by ID; clones so callers can return it without holding the lock.
    pub fn find_skill(&self, id: &str) -> Option<Skill> {
        let r = self.0.read().expect("TestStore poisoned");
        r.skills.iter().find(|s| s.id == id).cloned()
    }
}

/// Cheap clone of the store contents (used for resolver return values).
#[derive(Debug, Clone)]
pub struct TestStoreSnapshot {
    /// Snapshot of skills at the time of the read.
    pub skills: Vec<Skill>,
    /// Snapshot of providers at the time of the read.
    pub providers: Vec<Provider>,
    /// Snapshot of secrets at the time of the read.
    pub secrets: Vec<SecretEntry>,
    /// Snapshot of settings at the time of the read.
    pub settings: Settings,
}

impl SkillStatus {
    /// Convenience for `toggleSkill`: maps a `Boolean!` flag to a status.
    pub fn from_bool(enabled: bool) -> Self {
        if enabled {
            SkillStatus::Active
        } else {
            SkillStatus::Disabled
        }
    }
}
