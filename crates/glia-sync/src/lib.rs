//! glia-sync — local-remote SurrealDB bidirectional sync + disconnect fallback (T22, V15/V16).
//!
//! Hub-authoritative LWW (V16): on conflict, the record with the later
//! `updated_at` wins. Local-only skills (namespace `local::`) are NEVER
//! overwritten by remote — they're preserved as `local::` on the Hub too.
//!
//! Sync flow:
//! 1. `pull` — fetch remote skills not present locally (or older).
//! 2. `push` — send local skills not present remotely (or older).
//! 3. `status` — report diffs without applying.
//!
//! Disconnect fallback (V15): if Hub is unreachable, mark `HUB_UNREACHABLE`
//! and queue changes for later sync.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use glia_db::GliaDb;

/// Errors from sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// DB operation failed.
    #[error("db: {0}")]
    Db(#[from] glia_db::DbError),
    /// Hub unreachable.
    #[error("hub unreachable")]
    HubUnreachable,
}

/// Sync status for a single skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillSyncStatus {
    /// Skill is identical on both sides.
    InSync,
    /// Local is newer — needs push.
    LocalNewer,
    /// Remote is newer — needs pull.
    RemoteNewer,
    /// Only exists locally — needs push.
    LocalOnly,
    /// Only exists remotely — needs pull.
    RemoteOnly,
    /// Local-namespaced skill — never synced (V16).
    LocalNamespace,
}

/// A diff entry from `status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDiff {
    /// Skill id.
    pub id: String,
    /// Status.
    pub status: SkillSyncStatus,
    /// Local updated_at (if exists).
    pub local_updated: Option<String>,
    /// Remote updated_at (if exists).
    pub remote_updated: Option<String>,
}

/// Result of a sync run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Skills pulled from remote.
    pub pulled: usize,
    /// Skills pushed to remote.
    pub pushed: usize,
    /// Skills skipped (in sync or local-namespace).
    pub skipped: usize,
    /// Whether Hub was reachable.
    pub hub_reachable: bool,
}

/// Bidirectional sync engine.
pub struct SyncEngine {
    local: Arc<GliaDb>,
    remote: Arc<GliaDb>,
}

impl SyncEngine {
    /// Build a new sync engine between a local and remote DB.
    pub fn new(local: Arc<GliaDb>, remote: Arc<GliaDb>) -> Self {
        Self { local, remote }
    }

    /// Compute the diff between local and remote.
    pub async fn status(&self) -> Result<Vec<SyncDiff>, SyncError> {
        let local_skills = self.local.list_skills_with_ids().await?;
        let remote_skills = self.remote.list_skills_with_ids().await?;
        let mut diffs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (id, ls) in &local_skills {
            seen.insert(id.clone());
            if glia_db::GliaDb::is_local_skill(id) {
                diffs.push(SyncDiff {
                    id: id.clone(),
                    status: SkillSyncStatus::LocalNamespace,
                    local_updated: Some(ls.updated_at.clone()),
                    remote_updated: None,
                });
                continue;
            }
            let rs = remote_skills.iter().find(|(r_id, _)| r_id == id);
            match rs {
                None => diffs.push(SyncDiff {
                    id: id.clone(),
                    status: SkillSyncStatus::LocalOnly,
                    local_updated: Some(ls.updated_at.clone()),
                    remote_updated: None,
                }),
                Some((_, r)) => {
                    if ls.updated_at == r.updated_at {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::InSync,
                            local_updated: Some(ls.updated_at.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    } else if ls.updated_at > r.updated_at {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::LocalNewer,
                            local_updated: Some(ls.updated_at.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    } else {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::RemoteNewer,
                            local_updated: Some(ls.updated_at.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    }
                }
            }
        }
        for (id, rs) in &remote_skills {
            if !seen.contains(id) {
                if glia_db::GliaDb::is_local_skill(id) {
                    diffs.push(SyncDiff {
                        id: id.clone(),
                        status: SkillSyncStatus::LocalNamespace,
                        local_updated: None,
                        remote_updated: Some(rs.updated_at.clone()),
                    });
                } else {
                    diffs.push(SyncDiff {
                        id: id.clone(),
                        status: SkillSyncStatus::RemoteOnly,
                        local_updated: None,
                        remote_updated: Some(rs.updated_at.clone()),
                    });
                }
            }
        }
        Ok(diffs)
    }

    /// Pull remote-newer and remote-only skills to local.
    pub async fn pull(&self) -> Result<usize, SyncError> {
        let diffs = self.status().await?;
        let mut count = 0;
        for d in diffs {
            if d.status == SkillSyncStatus::RemoteNewer || d.status == SkillSyncStatus::RemoteOnly {
                if let Some(remote_skill) = self.remote.get_skill(&d.id).await? {
                    self.local.upsert_skill(&d.id, remote_skill).await?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Push local-newer and local-only skills to remote.
    /// Local-namespaced skills are skipped (V16).
    pub async fn push(&self) -> Result<usize, SyncError> {
        let diffs = self.status().await?;
        let mut count = 0;
        for d in diffs {
            if d.status == SkillSyncStatus::LocalNewer || d.status == SkillSyncStatus::LocalOnly {
                if let Some(local_skill) = self.local.get_skill(&d.id).await? {
                    self.remote.upsert_skill(&d.id, local_skill).await?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Full bidirectional sync: pull then push.
    pub async fn sync(&self) -> Result<SyncResult, SyncError> {
        let pulled = self.pull().await?;
        let pushed = self.push().await?;
        let diffs = self.status().await?;
        let skipped = diffs
            .into_iter()
            .filter(|d| d.status == SkillSyncStatus::InSync || d.status == SkillSyncStatus::LocalNamespace)
            .count();
        Ok(SyncResult { pulled, pushed, skipped, hub_reachable: true })
    }
}

/// Compute the local-only status diff (used when Hub is unreachable).
/// All skills are reported as `LocalOnly` or `LocalNamespace` since remote cannot be queried.
pub async fn status_offline(local: Arc<GliaDb>) -> Result<Vec<SyncDiff>, SyncError> {
    let local_skills = local.list_skills_with_ids().await?;
    let diffs = local_skills
        .into_iter()
        .map(|(id, ls)| {
            let status = if GliaDb::is_local_skill(&id) {
                SkillSyncStatus::LocalNamespace
            } else {
                SkillSyncStatus::LocalOnly
            };
            SyncDiff {
                id,
                status,
                local_updated: Some(ls.updated_at),
                remote_updated: None,
            }
        })
        .collect();
    Ok(diffs)
}

/// Disconnect fallback: queues changes when Hub is unreachable.
pub struct DisconnectFallback {
    /// Pending changes (skill ids that need syncing).
    queue: Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl DisconnectFallback {
    /// Build a new fallback queue.
    pub fn new() -> Self {
        Self { queue: Arc::new(tokio::sync::Mutex::new(Vec::new())) }
    }

    /// Enqueue a skill for later sync.
    pub async fn enqueue(&self, skill_id: String) {
        self.queue.lock().await.push(skill_id);
    }

    /// Drain the queue (called when Hub reconnects).
    pub async fn drain(&self) -> Vec<String> {
        let mut q = self.queue.lock().await;
        std::mem::take(&mut *q)
    }

    /// Check if there are pending changes.
    pub async fn has_pending(&self) -> bool {
        !self.queue.lock().await.is_empty()
    }

    /// Pending count.
    pub async fn pending_count(&self) -> usize {
        self.queue.lock().await.len()
    }
}

impl Default for DisconnectFallback {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_db::{Connection, Skill};

    async fn empty_db() -> Arc<GliaDb> {
        let db = Arc::new(GliaDb::connect(Connection::Memory).await.unwrap());
        db.init_schema().await.unwrap();
        db
    }

    fn skill(content: &str, updated: &str) -> Skill {
        Skill {
            content: content.into(),
            source: "test.md".into(),
            embedding: vec![],
            updated_at: updated.into(),
        }
    }

    #[tokio::test]
    async fn status_empty_both() {
        let local = empty_db().await;
        let remote = empty_db().await;
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert!(diffs.is_empty());
    }

    #[tokio::test]
    async fn status_local_only() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("supabase-auth", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, SkillSyncStatus::LocalOnly);
    }

    #[tokio::test]
    async fn status_remote_only() {
        let local = empty_db().await;
        let remote = empty_db().await;
        remote.upsert_skill("supabase-auth", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, SkillSyncStatus::RemoteOnly);
    }

    #[tokio::test]
    async fn status_in_sync() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        remote.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs[0].status, SkillSyncStatus::InSync);
    }

    #[tokio::test]
    async fn status_local_newer() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("foo", skill("x", "2026-01-02")).await.unwrap();
        remote.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs[0].status, SkillSyncStatus::LocalNewer);
    }

    #[tokio::test]
    async fn status_remote_newer() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        remote.upsert_skill("foo", skill("x", "2026-01-02")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs[0].status, SkillSyncStatus::RemoteNewer);
    }

    #[tokio::test]
    async fn local_namespace_never_syncs() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("local::foo", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote);
        let diffs = engine.status().await.unwrap();
        assert_eq!(diffs[0].status, SkillSyncStatus::LocalNamespace);
        // Push should skip it.
        let pushed = engine.push().await.unwrap();
        assert_eq!(pushed, 0);
    }

    #[tokio::test]
    async fn pull_remote_only() {
        let local = empty_db().await;
        let remote = empty_db().await;
        remote.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local.clone(), remote);
        let pulled = engine.pull().await.unwrap();
        assert_eq!(pulled, 1);
        assert!(local.get_skill("foo").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn push_local_only() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("foo", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local, remote.clone());
        let pushed = engine.push().await.unwrap();
        assert_eq!(pushed, 1);
        assert!(remote.get_skill("foo").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn sync_bidirectional() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("local-only", skill("x", "2026-01-01")).await.unwrap();
        remote.upsert_skill("remote-only", skill("x", "2026-01-01")).await.unwrap();
        let engine = SyncEngine::new(local.clone(), remote.clone());
        let result = engine.sync().await.unwrap();
        assert_eq!(result.pulled, 1);
        assert_eq!(result.pushed, 1);
        assert!(local.get_skill("remote-only").await.unwrap().is_some());
        assert!(remote.get_skill("local-only").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn lww_remote_wins_on_conflict() {
        let local = empty_db().await;
        let remote = empty_db().await;
        local.upsert_skill("foo", skill("old", "2026-01-01")).await.unwrap();
        remote.upsert_skill("foo", skill("new", "2026-01-02")).await.unwrap();
        let engine = SyncEngine::new(local.clone(), remote);
        engine.pull().await.unwrap();
        let skill = local.get_skill("foo").await.unwrap().unwrap();
        assert_eq!(skill.content, "new");
    }

    #[tokio::test]
    async fn disconnect_fallback_queue() {
        let fb = DisconnectFallback::new();
        assert!(!fb.has_pending().await);
        fb.enqueue("foo".into()).await;
        fb.enqueue("bar".into()).await;
        assert!(fb.has_pending().await);
        assert_eq!(fb.pending_count().await, 2);
        let drained = fb.drain().await;
        assert_eq!(drained, vec!["foo", "bar"]);
        assert!(!fb.has_pending().await);
    }

    #[tokio::test]
    async fn sync_result_hub_reachable() {
        let local = empty_db().await;
        let remote = empty_db().await;
        let engine = SyncEngine::new(local, remote);
        let result = engine.sync().await.unwrap();
        assert!(result.hub_reachable);
    }
}