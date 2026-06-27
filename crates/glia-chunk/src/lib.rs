//! glia-chunk — skill chunking + ingestion pipeline.
//!
//! Implements T10 (chunking pipeline + Git pre-push hook) and lays the
//! storage foundation for T11 (LLM synthesis) and T13 (`glia_save_skill`).
//!
//! V6: every skill chunk is embedded via the local MiniLM embedder
//! before persisting. V11: chunks keep a `source` field for citation
//! (V4). V16: chunks preserve the `local::` namespace prefix.
//!
//! v0.2.0: pipeline talks to the Hub via `HelixClient` (HTTP). Each chunk
//! upserted as a `skill` record with id `{source}::{i}`.
//!
//! Pipeline:
//! 1. `chunker::split` — markdown split on `## ` headings, then by
//!    paragraph if still over `max_chars`.
//! 2. `pipeline::ingest` — for each chunk, embed via `glia-embed`,
//!    upsert via the HelixClient (`POST /v1/upsert_skill`).
//! 3. `git::install_pre_push` — write `.git/hooks/pre-push` that calls
//!    `glia chunk ingest --changed` before push.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;

use glia_embed::Embedder;
use glia_helix::{HelixClient, Skill};

/// Errors from chunking / ingestion.
#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    /// DB operation failed.
    #[error("db: {0}")]
    Db(#[from] glia_helix::HelixError),
    /// Embedder failed.
    #[error("embed: {0}")]
    Embed(#[from] glia_embed::EmbedError),
    /// Git hook I/O failed.
    #[error("git hook: {0}")]
    Git(String),
    /// IO error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// A single chunk of a skill document.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    /// Sequential index in the source document (0-based).
    pub index: usize,
    /// Section heading (e.g., `## Auth Rules`), empty if no heading.
    pub heading: String,
    /// Markdown body (heading excluded).
    pub body: String,
}

impl Chunk {
    /// Concatenate heading + body for embedding. No markdown chars.
    pub fn text(&self) -> String {
        if self.heading.is_empty() {
            self.body.clone()
        } else {
            format!("{}\n{}", self.heading, self.body)
        }
    }
}

/// Markdown chunker. Splits on `## ` headings, then refines oversized
/// sections by paragraph.
pub struct Chunker {
    /// Soft character limit per chunk. Chunks may exceed this by up to
    /// one section's worth of content.
    pub max_chars: usize,
}

impl Default for Chunker {
    fn default() -> Self {
        Self { max_chars: 800 }
    }
}

impl Chunker {
    /// Build a chunker with the given soft limit.
    pub fn with_max_chars(max_chars: usize) -> Self {
        Self { max_chars }
    }

    /// Split markdown into chunks. Headings (`## `) are the primary
    /// split point; oversized sections are further split on blank lines.
    pub fn split(&self, md: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut current_heading = String::new();
        let mut current_body = String::new();
        let mut index = 0;

        for line in md.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                // Flush prior section.
                if !current_body.trim().is_empty() || !current_heading.is_empty() {
                    for c in self.refine(&current_heading, &current_body) {
                        chunks.push(Chunk {
                            index,
                            heading: c.0,
                            body: c.1,
                        });
                        index += 1;
                    }
                }
                current_heading = heading.trim().to_string();
                current_body.clear();
            } else {
                current_body.push_str(line);
                current_body.push('\n');
            }
        }
        // Trailing section.
        if !current_body.trim().is_empty() || !current_heading.is_empty() {
            for c in self.refine(&current_heading, &current_body) {
                chunks.push(Chunk {
                    index,
                    heading: c.0,
                    body: c.1,
                });
                index += 1;
            }
        }
        chunks
    }

    /// Refine a single section: if its text exceeds `max_chars`, split
    /// on blank lines. Otherwise return one chunk.
    fn refine(&self, heading: &str, body: &str) -> Vec<(String, String)> {
        let text = if heading.is_empty() {
            body.to_string()
        } else {
            format!("{}\n{}", heading, body)
        };
        if text.chars().count() <= self.max_chars {
            return vec![(heading.to_string(), body.to_string())];
        }
        // Split on blank lines.
        let mut out = Vec::new();
        let mut buf = String::new();
        for para in body.split("\n\n") {
            if buf.chars().count() + para.chars().count() > self.max_chars && !buf.is_empty() {
                out.push((heading.to_string(), buf.trim().to_string()));
                buf.clear();
            }
            buf.push_str(para);
            buf.push_str("\n\n");
        }
        if !buf.trim().is_empty() {
            out.push((heading.to_string(), buf.trim().to_string()));
        }
        if out.is_empty() {
            out.push((heading.to_string(), body.to_string()));
        }
        out
    }
}

/// Ingestion pipeline. Holds shared deps.
pub struct Pipeline {
    client: HelixClient,
    embedder: Arc<Embedder>,
    chunker: Chunker,
}

impl Pipeline {
    /// Build a pipeline with the default chunker.
    pub fn new(client: HelixClient, embedder: Arc<Embedder>) -> Self {
        Self {
            client,
            embedder,
            chunker: Chunker::default(),
        }
    }

    /// Override the chunker.
    pub fn with_chunker(mut self, chunker: Chunker) -> Self {
        self.chunker = chunker;
        self
    }

    /// Ingest one document. Returns the list of upserted chunk ids.
    ///
    /// The chunk id format is `{source}::{i}` so a single skill file
    /// (e.g., `supabase-auth.md`) yields `supabase-auth.md::0`,
    /// `supabase-auth.md::1`, ...; the `local::` prefix on the source
    /// (V16) is preserved transparently.
    pub async fn ingest(&self, source: &str, content: &str) -> Result<Vec<String>, ChunkError> {
        let chunks = self.chunker.split(content);
        let mut ids = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let id = format!("{}::{}", source, chunk.index);
            let text = chunk.text();
            let embedding = self.embedder.embed(&text)?;
            let skill = Skill {
                content: text,
                source: id.clone(),
                embedding,
                updated_at: Utc::now().to_rfc3339(),
                usage_count: 0,
            };
            self.client.upsert_skill(&id, skill).await?;
            ids.push(id);
        }
        Ok(ids)
    }
}

/// Git pre-push hook installer.
pub mod git {
    use super::{ChunkError, Path, PathBuf};

    const HOOK_BODY: &str = "#!/bin/sh\n# Installed by `glia chunk install-hook`.\n# Re-ingests every tracked skill file under ./skills/ before push.\nset -e\nif command -v glia >/dev/null 2>&1; then\n  glia chunk ingest --all 2>&1 || true\nelse\n  echo \"glia: CLI not on PATH; skipping skill ingestion.\" >&2\nfi\n";

    /// Install the pre-push hook at `<repo>/.git/hooks/pre-push`.
    /// Returns the path written.
    pub fn install_pre_push(repo_root: &Path) -> Result<PathBuf, ChunkError> {
        let hooks_dir = repo_root.join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir)?;
        let hook_path = hooks_dir.join("pre-push");
        std::fs::write(&hook_path, HOOK_BODY)?;
        // On Unix, mark executable. No-op on Windows.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&hook_path)?.permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&hook_path, perm)?;
        }
        Ok(hook_path)
    }

    /// Get a short label for the hook (for display).
    pub fn hook_name() -> &'static str {
        "pre-push"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunker_splits_on_h2() {
        let c = Chunker::default();
        let md = "## Auth\nNever use service_role.\n## Storage\nAlways enable RLS.\n";
        let chunks = c.split(md);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading, "Auth");
        assert_eq!(chunks[1].heading, "Storage");
    }

    #[test]
    fn chunker_handles_no_headings() {
        let c = Chunker::default();
        let md = "Just some text without headings.\nMore text.\n";
        let chunks = c.split(md);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_empty());
    }

    #[test]
    fn chunker_refines_oversized_section() {
        let c = Chunker::with_max_chars(100);
        let mut md = String::from("## Big\n");
        for i in 0..20 {
            md.push_str(&format!("paragraph {} with some words.\n\n", i));
        }
        let chunks = c.split(&md);
        assert!(
            chunks.len() > 1,
            "expected refinement, got {} chunks",
            chunks.len()
        );
        for ch in &chunks {
            assert!(
                ch.text().chars().count() <= 200,
                "chunk too big: {}",
                ch.text().chars().count()
            );
        }
    }

    #[test]
    fn chunk_text_concatenates_heading_and_body() {
        let c = Chunk {
            index: 0,
            heading: "Auth".into(),
            body: "use OAuth".into(),
        };
        assert_eq!(c.text(), "Auth\nuse OAuth");
    }

    async fn try_helix() -> Option<HelixClient> {
        let c = HelixClient::connect(None, None).ok()?;
        if c.ping().await.is_ok() {
            Some(c)
        } else {
            None
        }
    }

    #[tokio::test]
    async fn ingest_persists_chunks_with_embedding() {
        let Some(emb) = Embedder::try_new().map(Arc::new) else {
            eprintln!("SKIP: embed model assets missing");
            return;
        };
        let Some(client) = try_helix().await else {
            eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
            return;
        };
        let pipe = Pipeline::new(client.clone(), emb);
        let md = "## Auth\nNever use service_role.\n## Storage\nEnable RLS.\n";
        let ids = pipe.ingest("supabase-auth.md", md).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "supabase-auth.md::0");
        assert_eq!(ids[1], "supabase-auth.md::1");

        let got = client.get_skill("supabase-auth.md::0").await.unwrap();
        assert!(got.is_some());
        let s = got.unwrap();
        assert_eq!(s.embedding.len(), 384);
    }

    #[tokio::test]
    async fn ingest_preserves_local_namespace() {
        let Some(emb) = Embedder::try_new().map(Arc::new) else {
            eprintln!("SKIP: embed model assets missing");
            return;
        };
        let Some(client) = try_helix().await else {
            eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
            return;
        };
        let pipe = Pipeline::new(client, emb);
        let ids = pipe
            .ingest(
                "local::use-zustand",
                "## State\nUse Zustand for global state.",
            )
            .await
            .unwrap();
        assert_eq!(ids[0], "local::use-zustand::0");
        assert!(HelixClient::is_local_skill(&ids[0]));
    }

    #[tokio::test]
    async fn ingest_links_skill_to_stack() {
        let Some(emb) = Embedder::try_new().map(Arc::new) else {
            eprintln!("SKIP: embed model assets missing");
            return;
        };
        let Some(client) = try_helix().await else {
            eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
            return;
        };
        client
            .upsert_stack(
                "nextjs",
                glia_helix::Stack {
                    name: "Next.js".into(),
                },
            )
            .await
            .unwrap();
        let pipe = Pipeline::new(client.clone(), emb);
        pipe.ingest("nextjs::hooks", "## Hooks\nUse useState, useEffect.")
            .await
            .unwrap();
        client
            .relate_skill_applies_to_stack("nextjs::hooks::0", "nextjs")
            .await
            .unwrap();
        let skills = client.skills_for_stack("nextjs").await.unwrap();
        assert_eq!(skills.len(), 1);
        assert!(skills[0].content.contains("useState"));
    }

    #[test]
    fn install_hook_writes_executable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = git::install_pre_push(tmp.path()).unwrap();
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("glia chunk ingest"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perm = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(perm.mode() & 0o111, 0o111, "hook should be executable");
        }
    }

    #[test]
    fn empty_markdown_returns_empty_vec() {
        let c = Chunker::default();
        let chunks = c.split("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn only_headings_no_body_emits_empty_body_chunks() {
        let c = Chunker::default();
        let chunks = c.split("## A\n## B\n");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading, "A");
        assert!(chunks[0].body.trim().is_empty());
    }

    #[test]
    fn h3_not_treated_as_split_point() {
        let c = Chunker::default();
        let chunks = c.split("### sub\ntext\n## main\nbody\n");
        // `### sub` should be body, `## main` should split.
        // First chunk: heading="" (from before first ## ), body="### sub\ntext"
        // But "### sub\ntext\n" has no `## ` prefix line → all goes to
        // first chunk with empty heading. Then "## main" splits.
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].heading, "main");
    }

    #[test]
    fn h1_not_treated_as_split_point() {
        let c = Chunker::default();
        let chunks = c.split("# title\n## section\nbody\n");
        // "# title" → not `## ` prefix → body of first chunk.
        // "## section" → splits.
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].heading, "section");
    }

    #[test]
    fn h6_headings_treated_as_body_not_split() {
        let c = Chunker::default();
        let chunks = c.split("###### deep\nbody\n");
        // "###### deep" is NOT "## " prefix → treated as body.
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_empty());
    }

    #[test]
    fn trailing_heading_no_body_emitted() {
        let c = Chunker::default();
        let chunks = c.split("## First\nbody\n## Last\n");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].heading, "Last");
        assert!(chunks[1].body.trim().is_empty());
    }

    #[test]
    fn windows_line_endings_handled() {
        let c = Chunker::default();
        let chunks = c.split("## A\r\nbody text\r\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, "A");
        // Body may contain \r — documents the gap.
        assert!(chunks[0].body.contains("body text"));
    }

    #[test]
    fn only_frontmatter_no_content() {
        let c = Chunker::default();
        let chunks = c.split("---\ntitle: x\n---\n");
        // No `## ` → one chunk with empty heading.
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_empty());
    }

    #[test]
    fn max_chars_zero_refine_falls_back() {
        let c = Chunker::with_max_chars(0);
        // Every section is > 0 chars → refine tries, but single paragraph
        // can't split → returns full section.
        let chunks = c.split("## A\nsingle paragraph\n");
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn install_hook_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let hook_dir = tmp.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(hook_dir.join("pre-push"), "old content").unwrap();
        let path = git::install_pre_push(tmp.path()).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("glia chunk ingest"));
        assert!(!body.contains("old content"));
    }

    #[test]
    fn unicode_chunk_heading() {
        let c = Chunker::default();
        let md = "## 标题\n中文内容\n## Another\nbody\n";
        let chunks = c.split(md);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].heading, "标题");
    }

    #[test]
    fn unicode_chunk_emoji_in_body() {
        let c = Chunker::default();
        let md = "## Test 🎫\nbody with 日本語 and émojis\n## Next\nmore\n";
        let chunks = c.split(md);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].body.contains("日本語"));
    }
}
