//! glia-hooks — Hook generation engine: `.cursor/rules` + Claude `PreToolUse` (T16, V7/V10).
//!
//! Generates IDE/agent hook files that inject Glia skill context before
//! tool calls. Two targets:
//!
//! - **Cursor**: `.cursor/rules/<name>.mdc` files with YAML frontmatter
//!   (`description`, `globs`, `alwaysApply`) + markdown body.
//! - **Claude Code**: `.claude/settings.json` with `PreToolUse` hook
//!   entries that call `glia context` before each tool invocation.
//!
//! Inputs: skill records from `glia-db` (filtered by stack). The engine
//! formats each skill into a rule/hook entry.

use std::path::Path;

use serde::{Deserialize, Serialize};
/// Errors from hook generation.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    /// I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON / YAML serialization failed.
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    /// YAML serialization failed.
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// DB operation failed.
    #[error("db: {0}")]
    Db(String),
}

/// Cursor rule frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CursorRuleFrontmatter {
    /// Human description of when this rule applies.
    pub description: String,
    /// Glob patterns this rule matches (e.g., `["**/*.tsx"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globs: Vec<String>,
    /// If true, always inject this rule regardless of context.
    #[serde(default, skip_serializing_if = "is_false")]
    pub always_apply: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// A complete Cursor rule file.
#[derive(Debug, Clone)]
pub struct CursorRule {
    /// File name (without extension).
    pub name: String,
    /// Frontmatter.
    pub frontmatter: CursorRuleFrontmatter,
    /// Markdown body.
    pub body: String,
}

impl CursorRule {
    /// Render to `.mdc` file content.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&serde_yaml::to_string(&self.frontmatter).expect("frontmatter serializes"));
        out.push_str("---\n");
        out.push_str(&self.body);
        if !self.body.ends_with('\n') {
            out.push('\n');
        }
        out
    }
}

/// Claude Code `PreToolUse` hook entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookEntry {
    /// Tool matcher (e.g., `"Edit"` or `"*"`).
    pub matcher: String,
    /// Command to run.
    pub command: String,
}

/// Claude Code settings.json (PreToolUse hooks section).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeSettings {
    /// Hooks under `PreToolUse`.
    #[serde(rename = "PreToolUse")]
    pub pre_tool_use: Vec<ClaudeHookEntry>,
}

impl ClaudeSettings {
    /// Render to JSON string.
    pub fn render(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serializes")
    }
}

/// Build a Cursor rule from a skill.
pub fn skill_to_cursor_rule(
    skill: &glia_helix::Skill,
    name: &str,
    globs: Vec<String>,
) -> CursorRule {
    let description = format!("Glia skill: {}", name);
    let body = format!("# {}\n\n{}", name, skill.content);
    CursorRule {
        name: name.into(),
        frontmatter: CursorRuleFrontmatter {
            description,
            globs,
            always_apply: false,
        },
        body,
    }
}

/// Build a Claude `PreToolUse` hook entry that calls `glia context`.
pub fn build_claude_hook(matcher: &str, stacks: &[String]) -> ClaudeHookEntry {
    let stacks_arg = if stacks.is_empty() {
        String::new()
    } else {
        format!("--stacks {}", stacks.join(","))
    };
    ClaudeHookEntry {
        matcher: matcher.into(),
        command: format!("glia context {} 2>/dev/null || true", stacks_arg),
    }
}

/// Generate Cursor rule files for a set of skills, writing them to
/// `<repo>/.cursor/rules/`.
pub async fn generate_cursor_rules(
    repo_root: &Path,
    skills: &[glia_helix::Skill],
    stacks: &[String],
) -> Result<Vec<String>, HookError> {
    let rules_dir = repo_root.join(".cursor").join("rules");
    tokio::fs::create_dir_all(&rules_dir).await?;
    let mut written = Vec::new();
    for skill in skills {
        let name = derive_rule_name(&skill.source);
        let globs = stack_globs(stacks);
        let rule = skill_to_cursor_rule(skill, &name, globs);
        let path = rules_dir.join(format!("{}.mdc", name));
        tokio::fs::write(&path, rule.render()).await?;
        written.push(path.to_string_lossy().into_owned());
    }
    if written.is_empty() {
        // No skills yet — write a placeholder so `glia init` always
        // produces a recognizable `.cursor/rules/` directory.
        let readme = rules_dir.join("README.md");
        tokio::fs::write(
            &readme,
            "# Glia Cursor Rules\n\nNo local skills yet. Run `glia save-skill` or `glia chunk ingest` to populate.\n",
        )
        .await?;
        written.push(readme.to_string_lossy().into_owned());
    }
    Ok(written)
}

/// Generate Claude Code settings with PreToolUse hooks.
pub async fn generate_claude_hooks(
    repo_root: &Path,
    stacks: &[String],
) -> Result<String, HookError> {
    let settings = ClaudeSettings {
        pre_tool_use: vec![
            build_claude_hook("Edit", stacks),
            build_claude_hook("Write", stacks),
        ],
    };
    let json = settings.render();
    let claude_dir = repo_root.join(".claude");
    tokio::fs::create_dir_all(&claude_dir).await?;
    let path = claude_dir.join("settings.json");
    tokio::fs::write(&path, &json).await?;
    Ok(path.to_string_lossy().into_owned())
}

/// Derive a rule name from a skill source path.
/// `supabase-auth.md::0` → `supabase-auth-0`.
/// `local::use-zustand::1` → `local-use-zustand-1`.
fn derive_rule_name(source: &str) -> String {
    source
        .replace(".md", "")
        .replace("::", "-")
        .replace('/', "-")
}

/// Map stack ids to glob patterns (heuristic).
fn stack_globs(stacks: &[String]) -> Vec<String> {
    let mut globs = Vec::new();
    for s in stacks {
        match s.as_str() {
            "nextjs" | "react" => {
                globs.push("**/*.{tsx,jsx,ts,js}".into());
            }
            "supabase" => {
                globs.push("**/supabase/**".into());
                globs.push("**/*.sql".into());
            }
            "python" => {
                globs.push("**/*.py".into());
            }
            "rust" => {
                globs.push("**/*.rs".into());
            }
            _ => {}
        }
    }
    globs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(source: &str, content: &str) -> glia_helix::Skill {
        glia_helix::Skill {
            source: source.into(),
            content: content.into(),
            embedding: vec![],
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn cursor_rule_render_includes_frontmatter() {
        let s = skill("supabase-auth.md::0", "Use OAuth.");
        let rule = skill_to_cursor_rule(&s, "supabase-auth", vec!["**/*.sql".into()]);
        let md = rule.render();
        assert!(md.starts_with("---\n"));
        // YAML may quote the description because of the colon.
        assert!(md.contains("Glia skill") && md.contains("supabase-auth"));
        assert!(md.contains("globs:"));
        assert!(md.contains("Use OAuth."));
    }

    #[test]
    fn claude_hook_command_includes_stacks() {
        let hook = build_claude_hook("Edit", &["nextjs".into()]);
        assert!(hook.command.contains("--stacks nextjs"));
        assert_eq!(hook.matcher, "Edit");
    }

    #[test]
    fn claude_hook_no_stacks() {
        let hook = build_claude_hook("*", &[]);
        assert!(!hook.command.contains("--stacks"));
    }

    #[test]
    fn claude_settings_render_is_valid_json() {
        let settings = ClaudeSettings {
            pre_tool_use: vec![build_claude_hook("Edit", &[])],
        };
        let json = settings.render();
        let back: ClaudeSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pre_tool_use.len(), 1);
    }

    #[test]
    fn derive_rule_name_strips_double_colon() {
        assert_eq!(derive_rule_name("supabase-auth.md::0"), "supabase-auth-0");
        assert_eq!(
            derive_rule_name("local::use-zustand::1"),
            "local-use-zustand-1"
        );
        assert_eq!(derive_rule_name("supabase-auth.md"), "supabase-auth");
    }

    #[test]
    fn stack_globs_nextjs() {
        let globs = stack_globs(&["nextjs".into()]);
        assert!(globs.contains(&"**/*.{tsx,jsx,ts,js}".to_string()));
    }

    #[test]
    fn stack_globs_unknown_stack_empty() {
        let globs = stack_globs(&["unknown".into()]);
        assert!(globs.is_empty());
    }

    #[tokio::test]
    async fn generate_cursor_rules_writes_files() {
        let tmp = std::env::temp_dir().join(format!("glia-hooks-test-{}", uuid_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let skills = vec![
            skill("supabase-auth.md::0", "Use OAuth for Supabase."),
            skill("zustand.md::0", "Use zustand for state."),
        ];
        let written = generate_cursor_rules(&tmp, &skills, &["nextjs".into()])
            .await
            .unwrap();
        assert_eq!(written.len(), 2);
        for path in &written {
            let content = tokio::fs::read_to_string(path).await.unwrap();
            assert!(content.starts_with("---\n"));
        }
        // Cleanup
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn generate_claude_hooks_writes_settings() {
        let tmp = std::env::temp_dir().join(format!("glia-hooks-claude-{}", uuid_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let path = generate_claude_hooks(&tmp, &["nextjs".into()])
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("PreToolUse"));
        assert!(content.contains("glia context"));
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    fn uuid_v4() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", nanos)
    }

    #[test]
    fn frontmatter_round_trip() {
        let fm = CursorRuleFrontmatter {
            description: "test".into(),
            globs: vec!["**/*.rs".into()],
            always_apply: false,
        };
        let y = serde_yaml::to_string(&fm).unwrap();
        let back: CursorRuleFrontmatter = serde_yaml::from_str(&y).unwrap();
        assert_eq!(back, fm);
    }
}
