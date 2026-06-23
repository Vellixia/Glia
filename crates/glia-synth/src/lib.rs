//! glia-synth — LLM synthesis for Glia.
//!
//! Implements T11: take the top-K skill matches from `glia-action` and
//! produce a ≤150-token answer with citations. V5 (synthesis ⊥ rewrite
//! rules — extract & cite only), V19 (prioritize by graph edge weight).
//!
//! Pipeline:
//! 1. `reweight` — boost matches whose source has more `applies_to_stack`
//!    edges in the graph (V19). Output stays sorted by adjusted score.
//! 2. `Synthesizer::synthesize` — hand the top-K matches + query to the
//!    configured LLM backend.
//! 3. `StubSynthesizer` — for tests/dev: concatenate matches, hard-trim
//!    to `MAX_TOKENS * 4` chars (≈ 150 tokens).
//! 4. `HttpSynthesizer` — POST to OpenAI-compatible `/chat/completions`
//!    with `max_tokens=150` and a system prompt enforcing "extract & cite".

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use glia_db::GliaDb;

/// Hard cap on synthesis output: 150 tokens (V5).
pub const MAX_TOKENS: u32 = 150;
/// Rough heuristic: 4 chars per token.
const CHARS_PER_TOKEN: usize = 4;
const MAX_CHARS: usize = (MAX_TOKENS as usize) * CHARS_PER_TOKEN;

/// One cited source in the synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    /// Source path of the cited chunk.
    pub source: String,
    /// Score AFTER reweighting (V19).
    pub score: f32,
}

/// Final synthesis output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Synthesis {
    /// The synthesized text. ≤ MAX_CHARS.
    pub text: String,
    /// Sources cited in the text (V4).
    pub citations: Vec<Citation>,
    /// Which LLM produced this.
    pub model: String,
}

/// Errors from synthesis.
#[derive(Debug, thiserror::Error)]
pub enum SynthError {
    /// HTTP / LLM endpoint failed.
    #[error("http: {0}")]
    Http(String),
    /// LLM returned a response we couldn't parse.
    #[error("parse: {0}")]
    Parse(String),
    /// Reweighting failed (DB error).
    #[error("db: {0}")]
    Db(#[from] Box<glia_db::DbError>),
}

impl From<glia_db::DbError> for SynthError {
    fn from(e: glia_db::DbError) -> Self {
        SynthError::Db(Box::new(e))
    }
}

/// A ranked skill match. Mirrors `glia_action::SkillMatch` so the
/// synthesizer doesn't depend on `glia-action` (avoids a cycle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMatch {
    /// Source path of the chunk.
    pub source: String,
    /// Content of the chunk.
    pub content: String,
    /// Cosine score from discover step.
    pub score: f32,
}

/// Pluggable LLM backend.
#[async_trait]
pub trait Synthesizer: Send + Sync {
    /// Produce a synthesis for the given query, given the top-K matches.
    /// Implementations must respect MAX_TOKENS / MAX_CHARS.
    async fn synthesize(
        &self,
        query: &str,
        matches: &[SkillMatch],
    ) -> Result<Synthesis, SynthError>;

    /// Name of the model (for the `model` field on `Synthesis`).
    fn model_name(&self) -> &str;
}

/// Reweight matches by graph edge count (V19).
///
/// For each match, look up the source's `applies_to_stack` edge count in
/// the graph. Higher count → higher weight. The new score is
/// `cosine * (1 + 0.1 * edges)`, capped at 1.0, then re-sorted desc.
///
/// Matches not in the graph contribute 1.0× (no boost, no penalty).
pub async fn reweight(
    matches: Vec<SkillMatch>,
    db: &GliaDb,
) -> Result<Vec<SkillMatch>, SynthError> {
    let mut out = Vec::with_capacity(matches.len());
    for m in matches {
        let edges = count_applies_to_stack(db, &m.source).await?;
        let boost = 1.0 + 0.1 * edges as f32;
        let new_score = (m.score * boost).min(1.0);
        out.push(SkillMatch {
            score: new_score,
            ..m
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Count `applies_to_stack` edges for a given skill source.
async fn count_applies_to_stack(db: &GliaDb, source: &str) -> Result<usize, SynthError> {
    // The source of a chunk is the chunk id (e.g. `a.md::0`), which is
    // the primary key of the `skill` record created by glia-chunk.
    db.count_applies_to_stack_for(source)
        .await
        .map_err(Into::into)
}

/// Stub synthesizer. No LLM. Concatenates matches, hard-trims to
/// MAX_CHARS. Used in tests and as an offline fallback.
pub struct StubSynthesizer {
    /// Model label to report in `Synthesis::model`.
    pub model: String,
}

impl Default for StubSynthesizer {
    fn default() -> Self {
        Self {
            model: "stub".into(),
        }
    }
}

#[async_trait]
impl Synthesizer for StubSynthesizer {
    async fn synthesize(
        &self,
        _query: &str,
        matches: &[SkillMatch],
    ) -> Result<Synthesis, SynthError> {
        let mut text = String::new();
        let mut citations = Vec::new();
        for m in matches {
            if text.len() + m.content.len() + 2 > MAX_CHARS {
                break;
            }
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&m.content);
            citations.push(Citation {
                source: m.source.clone(),
                score: m.score,
            });
        }
        if text.len() > MAX_CHARS {
            text.truncate(MAX_CHARS);
        }
        if text.is_empty() {
            text.push_str("(no relevant skills)");
        }
        Ok(Synthesis {
            text,
            citations,
            model: self.model.clone(),
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// OpenAI-compatible HTTP synthesizer.
pub struct HttpSynthesizer {
    /// Base URL of the endpoint (e.g., `https://api.openai.com/v1`).
    pub base_url: String,
    /// API key.
    pub api_key: String,
    /// Model name to request.
    pub model: String,
    /// HTTP client.
    client: reqwest::Client,
}

impl HttpSynthesizer {
    /// Build a new HTTP synthesizer.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl Synthesizer for HttpSynthesizer {
    async fn synthesize(
        &self,
        query: &str,
        matches: &[SkillMatch],
    ) -> Result<Synthesis, SynthError> {
        let system = "You are Glia. Extract and cite from the provided context only. Do not add external knowledge. Keep the answer under 150 tokens. Cite sources with [source] markers.";
        let mut user = String::from("Question: ");
        user.push_str(query);
        user.push_str("\n\nContext:\n");
        for m in matches {
            user.push_str(&format!("[{}] {}\n", m.source, m.content));
        }

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "temperature": 0.0,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| SynthError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| SynthError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(SynthError::Http(format!("{}: {}", status, text)));
        }
        let parsed: ChatResponse =
            serde_json::from_str(&text).map_err(|e| SynthError::Parse(e.to_string()))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| SynthError::Parse("no choices in response".into()))?;

        let citations: Vec<Citation> = matches
            .iter()
            .map(|m| Citation {
                source: m.source.clone(),
                score: m.score,
            })
            .collect();

        Ok(Synthesis {
            text: content,
            citations,
            model: self.model.clone(),
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Wire format for OpenAI-compatible `/chat/completions` responses.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

/// Orchestrator. Wires the synthesizer + reweight step. Used by glia-action
/// to inject synthesis into `ActionResult`.
pub struct SynthOrchestrator {
    synth: Arc<dyn Synthesizer>,
}

impl SynthOrchestrator {
    /// Build a new orchestrator with the given backend.
    pub fn new(synth: Arc<dyn Synthesizer>) -> Self {
        Self { synth }
    }

    /// Reweight matches by graph edge count, then synthesize.
    pub async fn run(
        &self,
        query: &str,
        matches: Vec<SkillMatch>,
        db: &GliaDb,
    ) -> Result<Synthesis, SynthError> {
        let reweighted = reweight(matches, db).await?;
        self.synth.synthesize(query, &reweighted).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glia_db::{Connection, Skill, Stack};

    async fn empty_db() -> Arc<GliaDb> {
        let db = Arc::new(GliaDb::connect(Connection::Memory).await.unwrap());
        db.init_schema().await.unwrap();
        db
    }

    fn m(source: &str, content: &str, score: f32) -> SkillMatch {
        SkillMatch {
            source: source.into(),
            content: content.into(),
            score,
        }
    }

    #[tokio::test]
    async fn stub_returns_cited_synthesis() {
        let s = StubSynthesizer::default();
        let matches = vec![
            m("a.md::0", "Use OAuth for Linear.", 0.9),
            m("b.md::0", "Never use service_role.", 0.7),
        ];
        let out = s.synthesize("linear auth", &matches).await.unwrap();
        assert_eq!(out.citations.len(), 2);
        assert!(out.text.contains("OAuth"));
    }

    #[tokio::test]
    async fn stub_caps_at_max_chars() {
        let s = StubSynthesizer::default();
        let big: Vec<SkillMatch> = (0..100)
            .map(|i| m(&format!("f.md::{i}"), &"x".repeat(100), 1.0))
            .collect();
        let out = s.synthesize("q", &big).await.unwrap();
        assert!(out.text.len() <= MAX_CHARS);
    }

    #[tokio::test]
    async fn stub_handles_empty_input() {
        let s = StubSynthesizer::default();
        let out = s.synthesize("q", &[]).await.unwrap();
        assert_eq!(out.text, "(no relevant skills)");
        assert!(out.citations.is_empty());
    }

    #[tokio::test]
    async fn reweight_boosts_high_edge_count() {
        let db = empty_db().await;
        // Skill "a.md::0" applies to 3 stacks, "b.md::0" applies to 0.
        // a.md::0 should be boosted (0.5 * (1.0 + 0.1*3) = 0.65).
        // b.md::0 stays at 0.5.
        let now = "2026-01-01T00:00:00Z";
        db.upsert_skill(
            "a.md::0",
            Skill {
                content: "x".into(),
                source: "a.md".into(),
                embedding: vec![],
                updated_at: now.into(),
            },
        )
        .await
        .unwrap();
        db.upsert_skill(
            "b.md::0",
            Skill {
                content: "y".into(),
                source: "b.md".into(),
                embedding: vec![],
                updated_at: now.into(),
            },
        )
        .await
        .unwrap();
        for s in ["nextjs", "supabase", "vercel"] {
            db.upsert_stack(s, Stack { name: s.into() }).await.unwrap();
        }
        for s in ["nextjs", "supabase", "vercel"] {
            db.relate_skill_applies_to_stack("a.md::0", s)
                .await
                .unwrap();
        }

        let matches = vec![m("a.md::0", "x", 0.5), m("b.md::0", "y", 0.5)];
        let out = reweight(matches, &db).await.unwrap();
        assert_eq!(out.len(), 2);
        // a.md::0 boosted, comes first.
        assert_eq!(out[0].source, "a.md::0");
        assert!((out[0].score - 0.65).abs() < 1e-3, "got {}", out[0].score);
        assert!((out[1].score - 0.5).abs() < 1e-3);
    }

    #[tokio::test]
    async fn reweight_preserves_order_when_no_edges() {
        let db = empty_db().await;
        let matches = vec![m("a.md::0", "x", 0.8), m("b.md::0", "y", 0.5)];
        let out = reweight(matches, &db).await.unwrap();
        assert!((out[0].score - 0.8).abs() < 1e-3);
        assert!((out[1].score - 0.5).abs() < 1e-3);
    }

    #[tokio::test]
    async fn orchestrator_runs_end_to_end() {
        let db = empty_db().await;
        let s = Arc::new(StubSynthesizer::default());
        let orch = SynthOrchestrator::new(s);
        let matches = vec![m("a.md::0", "Use zustand", 0.8)];
        let out = orch.run("state", matches, &db).await.unwrap();
        assert!(out.text.contains("zustand"));
    }

    #[test]
    fn max_chars_matches_token_budget() {
        assert_eq!(MAX_CHARS, 150 * 4);
    }

    #[test]
    fn synthesis_serializes_round_trip() {
        let s = Synthesis {
            text: "x".into(),
            citations: vec![Citation {
                source: "a.md".into(),
                score: 0.5,
            }],
            model: "stub".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: Synthesis = serde_json::from_str(&j).unwrap();
        assert_eq!(back.model, "stub");
    }
}
