//! glia-review — corrections → candidate-skills review queue (Phase 4 / D).
//!
//! When a developer overrides an agent's file write, the diff is captured
//! as a `ReviewItem` in `.glia/review-queue.jsonl`. The developer can then:
//!
//! - `glia review list`          — see pending candidate rules
//! - `glia review approve <id>`  — upsert candidate as a Hub skill
//! - `glia review reject <id>`   — discard the candidate
//!
//! Capture happens via a `glia review capture <file>` CLI call that is
//! wired as a PostToolUse hook by `glia-hooks::generate_claude_hooks`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Errors from the review queue.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    /// I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parse/serialize failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// Item not found in queue.
    #[error("not found: {0}")]
    NotFound(String),
    /// Hub operation failed.
    #[error("hub: {0}")]
    Hub(String),
}

/// Status of a review item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// Awaiting decision.
    Pending,
    /// Approved and upserted to the Hub as a skill.
    Approved,
    /// Discarded.
    Rejected,
}

/// A candidate skill derived from a developer correction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewItem {
    /// Unique id (UUIDv4).
    pub id: String,
    /// File path that triggered the capture.
    pub file_path: String,
    /// The raw diff (unified format) or description of the correction.
    pub diff: String,
    /// Distilled candidate rule content (derived from the diff).
    pub candidate_rule: String,
    /// RFC-3339 creation timestamp.
    pub created_at: String,
    /// Current status.
    pub status: ReviewStatus,
}

/// The review queue — reads/writes `.glia/review-queue.jsonl` in the repo.
pub struct ReviewQueue {
    path: PathBuf,
}

impl ReviewQueue {
    /// Open the queue for a repo root (creates `.glia/` dir if needed).
    pub fn open(repo_root: &Path) -> Self {
        Self {
            path: repo_root.join(".glia").join("review-queue.jsonl"),
        }
    }

    /// Capture a correction: add a new pending item to the queue.
    ///
    /// `file_path` — the file that was written by the agent.
    /// `diff`      — raw diff or textual description of the correction.
    pub async fn capture(&self, file_path: &str, diff: &str) -> Result<ReviewItem, ReviewError> {
        tokio::fs::create_dir_all(self.path.parent().unwrap()).await?;
        let item = ReviewItem {
            id: new_id(),
            file_path: file_path.to_owned(),
            diff: diff.to_owned(),
            candidate_rule: distill_rule(file_path, diff),
            created_at: now_rfc3339(),
            status: ReviewStatus::Pending,
        };
        let line = serde_json::to_string(&item)? + "\n";
        use tokio::io::AsyncWriteExt as _;
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        f.write_all(line.as_bytes()).await?;
        Ok(item)
    }

    /// Load all items from the queue file.
    pub async fn load_all(&self) -> Result<Vec<ReviewItem>, ReviewError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = tokio::fs::read_to_string(&self.path).await?;
        let mut items = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() {
                let item: ReviewItem = serde_json::from_str(line)?;
                items.push(item);
            }
        }
        Ok(items)
    }

    /// List pending items (status = Pending).
    pub async fn list_pending(&self) -> Result<Vec<ReviewItem>, ReviewError> {
        let all = self.load_all().await?;
        Ok(all
            .into_iter()
            .filter(|i| i.status == ReviewStatus::Pending)
            .collect())
    }

    /// Approve an item: set status to Approved and optionally upsert to Hub.
    ///
    /// Returns the approved item. The caller is responsible for pushing to Hub
    /// via `glia_helix::HelixClient::upsert_skill`.
    pub async fn approve(&self, id: &str) -> Result<ReviewItem, ReviewError> {
        self.update_status(id, ReviewStatus::Approved).await
    }

    /// Reject an item: set status to Rejected.
    pub async fn reject(&self, id: &str) -> Result<ReviewItem, ReviewError> {
        self.update_status(id, ReviewStatus::Rejected).await
    }

    /// Update the status of an item, rewriting the queue file.
    async fn update_status(
        &self,
        id: &str,
        new_status: ReviewStatus,
    ) -> Result<ReviewItem, ReviewError> {
        let mut items = self.load_all().await?;
        let pos = items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| ReviewError::NotFound(id.to_owned()))?;
        items[pos].status = new_status;
        let updated = items[pos].clone();
        self.write_all(&items).await?;
        Ok(updated)
    }

    /// Rewrite the queue file from the in-memory list.
    async fn write_all(&self, items: &[ReviewItem]) -> Result<(), ReviewError> {
        let mut content = String::new();
        for item in items {
            content.push_str(&serde_json::to_string(item)?);
            content.push('\n');
        }
        tokio::fs::write(&self.path, content).await?;
        Ok(())
    }
}

/// Distill a candidate rule from a diff.
/// Simple heuristic: extract the added lines as the rule body.
fn distill_rule(file_path: &str, diff: &str) -> String {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let added: Vec<&str> = diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l.trim_start_matches('+'))
        .collect();
    if added.is_empty() {
        format!("Correction in `{}`: prefer the developer's version.", file_path)
    } else {
        format!(
            "Correction in `{}` ({} file):\n\n```\n{}\n```",
            file_path,
            ext,
            added.join("\n")
        )
    }
}

/// Generate a short unique ID (timestamp + random suffix).
fn new_id() -> String {
    // Avoid pulling uuid as a dep in tests; use a timestamp-based id.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    // Thread id as a low-cost random suffix.
    format!("{:x}-{:x}", nanos, pid)
}

/// Current time as RFC-3339 string (UTC, second precision).
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO 8601 UTC.
    let (y, mo, d, h, mi, s) = secs_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Minimal secs-since-epoch → (year, month, day, hour, min, sec) in UTC.
/// No external date library needed.
fn secs_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mi = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // Gregorian calendar from Unix epoch (1970-01-01).
    let (y, mo, d) = days_to_ymd(days);
    (y, mo, d, h, mi, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("glia-review-{}-{}", std::process::id(), nanos))
    }

    #[tokio::test]
    async fn capture_creates_queue_file() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        let item = queue.capture("src/auth.ts", "+const x = 1;").await.unwrap();
        assert_eq!(item.status, ReviewStatus::Pending);
        assert!(item.candidate_rule.contains("auth.ts"));
        assert!(dir.join(".glia/review-queue.jsonl").exists());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn list_pending_returns_only_pending() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        queue.capture("a.ts", "+x").await.unwrap();
        let b = queue.capture("b.ts", "+y").await.unwrap();
        queue.reject(&b.id).await.unwrap();
        let pending = queue.list_pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].file_path, "a.ts");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn approve_changes_status() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        let item = queue.capture("x.rs", "+fn foo() {}").await.unwrap();
        let approved = queue.approve(&item.id).await.unwrap();
        assert_eq!(approved.status, ReviewStatus::Approved);
        // Verify persisted.
        let all = queue.load_all().await.unwrap();
        assert_eq!(all[0].status, ReviewStatus::Approved);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn reject_changes_status() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        let item = queue.capture("y.py", "+import x").await.unwrap();
        let rejected = queue.reject(&item.id).await.unwrap();
        assert_eq!(rejected.status, ReviewStatus::Rejected);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn update_status_not_found_returns_error() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        let result = queue.approve("no-such-id").await;
        assert!(matches!(result, Err(ReviewError::NotFound(_))));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn load_all_empty_when_no_file() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        let items = queue.load_all().await.unwrap();
        assert!(items.is_empty());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn multiple_captures_accumulate() {
        let dir = tmp();
        let queue = ReviewQueue::open(&dir);
        queue.capture("a.ts", "+1").await.unwrap();
        queue.capture("b.ts", "+2").await.unwrap();
        queue.capture("c.ts", "+3").await.unwrap();
        let all = queue.load_all().await.unwrap();
        assert_eq!(all.len(), 3);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn distill_rule_extracts_added_lines() {
        let rule = distill_rule("auth.ts", "+const token = getToken();\n+validateToken(token);");
        assert!(rule.contains("auth.ts"));
        assert!(rule.contains("getToken"));
    }

    #[test]
    fn distill_rule_empty_diff_returns_description() {
        let rule = distill_rule("x.rs", "no diff lines");
        assert!(rule.contains("x.rs"));
    }

    #[test]
    fn now_rfc3339_is_parseable() {
        let ts = now_rfc3339();
        assert!(ts.contains('T'));
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
    }

    #[test]
    fn new_id_is_not_empty() {
        let id = new_id();
        assert!(!id.is_empty());
        assert!(id.contains('-'));
    }

    #[test]
    fn review_error_display() {
        assert!(ReviewError::NotFound("x".into()).to_string().contains("not found"));
        assert!(ReviewError::Hub("y".into()).to_string().contains("hub"));
    }
}
