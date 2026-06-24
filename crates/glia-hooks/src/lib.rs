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

/// Claude Code `PreToolUse` hook entry (matches Claude Code's settings.json schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookEntry {
    /// Tool matcher (e.g., `"Edit"` or `"*"`).
    #[serde(rename = "matcher")]
    pub matcher: String,
    /// Hook commands (Claude Code expects an array of `{type, command}`).
    #[serde(rename = "hooks")]
    pub hooks: Vec<ClaudeHookCommand>,
}

/// A single hook command in a Claude Code PreToolUse entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookCommand {
    /// Hook type (always `"command"` for our use case).
    #[serde(rename = "type")]
    pub hook_type: String,
    /// Shell command to run.
    #[serde(rename = "command")]
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
        hooks: vec![ClaudeHookCommand {
            hook_type: "command".into(),
            command: format!("glia context {} 2>/dev/null || true", stacks_arg),
        }],
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
/// If `.claude/settings.json` already exists, merge Glia hooks with the
/// existing `PreToolUse` array (preserving other settings and hooks).
pub async fn generate_claude_hooks(
    repo_root: &Path,
    stacks: &[String],
) -> Result<String, HookError> {
    let glia_hooks = vec![
        build_claude_hook("Edit", stacks),
        build_claude_hook("Write", stacks),
    ];
    let claude_dir = repo_root.join(".claude");
    tokio::fs::create_dir_all(&claude_dir).await?;
    let path = claude_dir.join("settings.json");

    // Read existing settings (if any) and merge.
    let merged_json = if path.exists() {
        let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        merge_hooks_into_settings(&existing, &glia_hooks)
    } else {
        ClaudeSettings { pre_tool_use: glia_hooks }.render()
    };

    tokio::fs::write(&path, &merged_json).await?;
    Ok(path.to_string_lossy().into_owned())
}

/// Merge Glia hooks into an existing Claude settings.json string.
/// Preserves all other top-level keys and existing PreToolUse entries.
/// Glia hook commands are de-duplicated by command string.
fn merge_hooks_into_settings(existing: &str, glia_hooks: &[ClaudeHookEntry]) -> String {
    let mut settings: serde_json::Value = serde_json::from_str(existing)
        .unwrap_or_else(|_| serde_json::json!({}));

    // Get the existing PreToolUse array, or start fresh.
    let mut arr: Vec<serde_json::Value> = match settings.get("PreToolUse") {
        Some(serde_json::Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };

    // Collect existing commands (deduplication).
    let existing_commands: std::collections::HashSet<String> = arr
        .iter()
        .filter_map(|v| v.get("hooks").and_then(|h| h.as_array()))
        .flat_map(|h| h.iter())
        .filter_map(|hook| hook.get("command").and_then(|c| c.as_str()))
        .map(String::from)
        .collect();

    // Add Glia hooks whose command is not already present.
    for glia_hook in glia_hooks {
        let cmd = glia_hook
            .hooks
            .first()
            .map(|h| h.command.clone())
            .unwrap_or_default();
        if !existing_commands.contains(&cmd) {
            let entry = serde_json::json!({
                "matcher": glia_hook.matcher,
                "hooks": glia_hook.hooks,
            });
            arr.push(entry);
        }
    }

    // Write back the merged array.
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("PreToolUse".to_string(), serde_json::json!(arr));
    } else {
        settings = serde_json::json!({"PreToolUse": arr});
    }

    serde_json::to_string_pretty(&settings).unwrap_or_else(|_| "{}".to_string())
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
        let cmd = &hook.hooks[0].command;
        assert!(cmd.contains("--stacks nextjs"));
        assert_eq!(hook.matcher, "Edit");
    }

    #[test]
    fn claude_hook_no_stacks() {
        let hook = build_claude_hook("*", &[]);
        let cmd = &hook.hooks[0].command;
        assert!(!cmd.contains("--stacks"));
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

    #[tokio::test]
    async fn generate_claude_hooks_merges_with_existing() {
        let tmp = std::env::temp_dir().join(format!("glia-hooks-merge-{}", uuid_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        // Pre-existing settings with a custom hook the user wants to keep.
        let existing = serde_json::json!({
            "theme": "dark",
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{
                    "type": "command",
                    "command": "my-custom-pre-tool-hook"
                }]
            }]
        });
        let path = tmp.join(".claude").join("settings.json");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, serde_json::to_string_pretty(&existing).unwrap())
            .await
            .unwrap();

        // Generate Glia hooks — should merge, not overwrite.
        generate_claude_hooks(&tmp, &["nextjs".into()])
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        // Custom hook preserved.
        assert!(content.contains("my-custom-pre-tool-hook"));
        assert!(content.contains("Bash"));
        // Other settings preserved.
        assert!(content.contains("\"theme\""));
        assert!(content.contains("dark"));
        // Glia hooks added.
        assert!(content.contains("glia context"));
        assert!(content.contains("Edit"));
        assert!(content.contains("Write"));
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn generate_claude_hooks_idempotent() {
        let tmp = std::env::temp_dir().join(format!("glia-hooks-idem-{}", uuid_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        // Run twice — second run should not duplicate Glia hooks.
        generate_claude_hooks(&tmp, &["nextjs".into()])
            .await
            .unwrap();
        let first = tokio::fs::read_to_string(
            tmp.join(".claude").join("settings.json"),
        )
        .await
        .unwrap();
        generate_claude_hooks(&tmp, &["nextjs".into()])
            .await
            .unwrap();
        let second = tokio::fs::read_to_string(
            tmp.join(".claude").join("settings.json"),
        )
        .await
        .unwrap();
        // Count "glia context" occurrences — should be 2 (one per matcher).
        let first_count = first.matches("glia context").count();
        let second_count = second.matches("glia context").count();
        assert_eq!(first_count, 2, "first run should have 2 glia hooks");
        assert_eq!(second_count, 2, "second run should not duplicate hooks");
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

    #[test]
    fn derive_rule_name_unicode() {
        // Unicode passes through — only .md, ::, / are replaced.
        let name = derive_rule_name("héllo.md::0");
        assert!(name.contains("héllo"));
    }

    #[test]
    fn derive_rule_name_source_with_special_chars() {
        let name = derive_rule_name("a/b:c.md::0");
        // `::` → `-`, `.md` → removed, `/` → `-`, `:` stays.
        assert!(!name.contains("::"));
        assert!(!name.contains(".md"));
    }

    #[test]
    fn stack_globs_supabase() {
        let globs = stack_globs(&["supabase".into()]);
        assert!(!globs.is_empty(), "supabase should have globs");
    }

    #[test]
    fn stack_globs_python() {
        let globs = stack_globs(&["python".into()]);
        assert!(!globs.is_empty(), "python should have globs");
    }

    #[test]
    fn stack_globs_rust() {
        let globs = stack_globs(&["rust".into()]);
        assert!(!globs.is_empty(), "rust should have globs");
    }

    #[tokio::test]
    async fn generate_cursor_rules_unicode_source() {
        let tmp = std::env::temp_dir().join(format!(
            "glia-hooks-unicode-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let skills = vec![skill("héllo-日本語.md::0", "Unicode content 🎫")];
        let written = generate_cursor_rules(&tmp, &skills, &["nextjs".into()])
            .await
            .unwrap();
        assert!(!written.is_empty());
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn generate_cursor_rules_nonexistent_dir_auto_creates() {
        let tmp = std::env::temp_dir().join(format!("glia-hooks-nested-{}", uuid_v4()));
        let nested = tmp.join("deep").join("path");
        // Don't create nested — generate_cursor_rules should create it.
        let skills = vec![skill("test.md::0", "Test.")];
        let written = generate_cursor_rules(&nested, &skills, &[]).await.unwrap();
        assert_eq!(written.len(), 1);
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[test]
    fn claude_settings_always_has_edit_and_write_matchers() {
        let settings = ClaudeSettings {
            pre_tool_use: vec![
                build_claude_hook("Edit", &[]),
                build_claude_hook("Write", &[]),
            ],
        };
        let json = settings.render();
        assert!(json.contains("Edit"));
        assert!(json.contains("Write"));
    }

    #[test]
    fn hook_error_display() {
        let e = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        let he = HookError::from(e);
        assert!(format!("{}", he).contains("io"));
    }
}
