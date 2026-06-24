//! glia-sync — sync against the Hub (HelixDB-backed, v0.2.0).
//!
//! Hub-authoritative LWW (V16): the record with the later `updated_at`
//! wins. Local-namespaced skills (`local::*`) are owned by the CLI/repo
//! and live on the Hub too — they can be re-pushed at any time.
//!
//! v0.2.0: no local embedded DB. The CLI is a pure HTTP client; sync
//! talks to the Hub via `HelixClient`. Local-only state = un-ingested
//! skill files in `<repo>/skills/`; those are pushed via
//! `glia chunk ingest`.
//!
//! Sync flow:
//! 1. `status` — fetch remote skills and report diffs (per-id LWW).
//! 2. `sync` — pull remote-newer/remote-only to Hub-side state (idempotent
//!    since Hub is the source of truth).
//! 3. CLI side: `glia chunk ingest` pushes local files into the Hub.

use serde::{Deserialize, Serialize};

use glia_helix::HelixClient;

/// Errors from sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// DB operation failed.
    #[error("db: {0}")]
    Db(#[from] glia_helix::HelixError),
    /// Hub unreachable.
    #[error("hub unreachable: {0}")]
    HubUnreachable(String),
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
    /// Local-namespaced skill — owned by repo, push when changed.
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

/// Bidirectional sync engine against the Hub.
pub struct SyncEngine {
    client: HelixClient,
}

impl SyncEngine {
    /// Build a new sync engine against a Hub HelixDB instance.
    pub fn new(client: HelixClient) -> Self {
        Self { client }
    }

    /// Compute the diff between local (provided) and remote (Hub).
    ///
    /// `local` = the set of skill ids + updated_at the CLI knows about
    /// (typically read from `<repo>/skills/*.md` frontmatter or supplied
    /// empty for pure Hub-state inspection).
    pub async fn status(&self, local: &[(String, String)]) -> Result<Vec<SyncDiff>, SyncError> {
        let remote_skills = self.client.list_skills_with_ids().await?;
        let mut diffs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (id, local_updated) in local {
            seen.insert(id.clone());
            if HelixClient::is_local_skill(id) {
                diffs.push(SyncDiff {
                    id: id.clone(),
                    status: SkillSyncStatus::LocalNamespace,
                    local_updated: Some(local_updated.clone()),
                    remote_updated: None,
                });
                continue;
            }
            let rs = remote_skills.iter().find(|(r_id, _)| r_id == id);
            match rs {
                None => diffs.push(SyncDiff {
                    id: id.clone(),
                    status: SkillSyncStatus::LocalOnly,
                    local_updated: Some(local_updated.clone()),
                    remote_updated: None,
                }),
                Some((_, r)) => {
                    if local_updated == &r.updated_at {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::InSync,
                            local_updated: Some(local_updated.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    } else if local_updated > &r.updated_at {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::LocalNewer,
                            local_updated: Some(local_updated.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    } else {
                        diffs.push(SyncDiff {
                            id: id.clone(),
                            status: SkillSyncStatus::RemoteNewer,
                            local_updated: Some(local_updated.clone()),
                            remote_updated: Some(r.updated_at.clone()),
                        });
                    }
                }
            }
        }
        for (id, r) in &remote_skills {
            if !seen.contains(id) {
                if HelixClient::is_local_skill(id) {
                    diffs.push(SyncDiff {
                        id: id.clone(),
                        status: SkillSyncStatus::LocalNamespace,
                        local_updated: None,
                        remote_updated: Some(r.updated_at.clone()),
                    });
                } else {
                    diffs.push(SyncDiff {
                        id: id.clone(),
                        status: SkillSyncStatus::RemoteOnly,
                        local_updated: None,
                        remote_updated: Some(r.updated_at.clone()),
                    });
                }
            }
        }
        Ok(diffs)
    }

    /// Pull remote-newer and remote-only skills (idempotent against Hub).
    /// In single-gateway mode this is effectively a no-op confirmation:
    /// the Hub already holds its own state.
    pub async fn pull(&self, local: &[(String, String)]) -> Result<usize, SyncError> {
        let diffs = self.status(local).await?;
        let count = diffs
            .iter()
            .filter(|d| {
                d.status == SkillSyncStatus::RemoteNewer || d.status == SkillSyncStatus::RemoteOnly
            })
            .count();
        Ok(count)
    }

    /// Full sync: pull then push. With Hub as source of truth, this
    /// returns counts without mutating Hub-side state (mutations happen
    /// via `glia chunk ingest` / `glia save-skill`).
    pub async fn sync(&self, local: &[(String, String)]) -> Result<SyncResult, SyncError> {
        let pulled = self.pull(local).await?;
        let diffs = self.status(local).await?;
        let mut pushed = 0;
        let mut skipped = 0;
        for d in &diffs {
            if d.status == SkillSyncStatus::LocalNewer || d.status == SkillSyncStatus::LocalOnly {
                pushed += 1;
            } else if d.status == SkillSyncStatus::InSync
                || d.status == SkillSyncStatus::LocalNamespace
            {
                skipped += 1;
            }
        }
        Ok(SyncResult {
            pulled,
            pushed,
            skipped,
            hub_reachable: true,
        })
    }
}

/// Compute Hub-only status (no local state).
pub async fn status(client: &HelixClient) -> Result<Vec<SyncDiff>, SyncError> {
    SyncEngine::new(client.clone()).status(&[]).await
}

/// Full sync against the Hub with no local state.
pub async fn sync(client: &HelixClient) -> Result<SyncResult, SyncError> {
    SyncEngine::new(client.clone()).sync(&[]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn try_client() -> Option<HelixClient> {
        let client = HelixClient::connect(None, None).ok()?;
        if client.ping().await.is_err() {
            return None;
        }
        Some(client)
    }

    #[tokio::test]
    async fn status_empty_hub() {
        let Some(client) = try_client().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let engine = SyncEngine::new(client);
        let diffs = engine.status(&[]).await.unwrap();
        // Hub may have data; we just verify it returns without panicking.
        let _ = diffs;
    }

    #[tokio::test]
    async fn local_only_reports_push() {
        let Some(client) = try_client().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let engine = SyncEngine::new(client);
        let local = vec![(
            "never-seen.md".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
        )];
        let diffs = engine.status(&local).await.unwrap();
        assert!(
            diffs
                .iter()
                .any(|d| d.id == "never-seen.md" && d.status == SkillSyncStatus::LocalOnly)
        );
    }

    #[tokio::test]
    async fn local_namespace_skipped() {
        let Some(client) = try_client().await else {
            eprintln!("SKIP: no helixdb");
            return;
        };
        let engine = SyncEngine::new(client);
        let local = vec![(
            "local::my-rule.md".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
        )];
        let diffs = engine.status(&local).await.unwrap();
        assert!(diffs.iter().any(|d| d.id == "local::my-rule.md"
            && d.status == SkillSyncStatus::LocalNamespace));
    }

    #[test]
    fn sync_result_serializes_round_trip() {
        let r = SyncResult {
            pulled: 1,
            pushed: 2,
            skipped: 3,
            hub_reachable: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: SyncResult = serde_json::from_str(&s).unwrap();
        assert_eq!(back.pulled, 1);
        assert_eq!(back.pushed, 2);
        assert_eq!(back.skipped, 3);
        assert!(back.hub_reachable);
    }

    #[test]
    fn sync_diff_serializes_round_trip() {
        let d = SyncDiff {
            id: "x".into(),
            status: SkillSyncStatus::InSync,
            local_updated: Some("a".into()),
            remote_updated: Some("a".into()),
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: SyncDiff = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, "x");
        assert_eq!(back.status, SkillSyncStatus::InSync);
    }
}
