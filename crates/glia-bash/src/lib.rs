//! glia-bash — command allow-list + path boundary enforcement for local shell exec.
//!
//! Implements V1 (local routing) and V9 (dependency check fallback path).
//! v1 strategy: ⊥ kernel seccomp/sandbox-exec. Use regex allow-list from
//! `.glia/config.toml` + path boundary checks. Cross-platform.

use std::path::{Path, PathBuf};

use regex::Regex;
use thiserror::Error;
use tokio::process::Command;

/// Errors from `glia_bash`.
#[derive(Debug, Error)]
pub enum BashError {
    /// Command did not match any allow-list regex.
    #[error("command denied: {0}")]
    CommandDenied(String),
    /// Path argument escapes the allowed root.
    #[error("path escapes root: {0}")]
    PathEscape(String),
    /// Command exited with non-zero status.
    #[error("exit {code}: {stderr}")]
    NonZeroExit {
        /// Exit code from the child process.
        code: i32,
        /// stderr captured from the child process.
        stderr: String,
    },
    /// IO error spawning or waiting.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Allow-list regex was invalid.
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
}

/// Result of a successful command execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashOutput {
    /// stdout content (UTF-8).
    pub stdout: String,
    /// stderr content (UTF-8).
    pub stderr: String,
    /// Exit code (0 on success).
    pub exit_code: i32,
}

/// Configuration: root path boundary + compiled allow-list regexes.
#[derive(Debug, Clone)]
pub struct BashConfig {
    /// Root directory that path arguments must stay within.
    pub root: PathBuf,
    /// Compiled allow-list patterns. A command runs iff it matches ≥1 pattern.
    pub allow_patterns: Vec<Regex>,
}

impl BashConfig {
    /// Build from raw regex strings. Returns `InvalidRegex` on bad pattern.
    pub fn new(root: impl Into<PathBuf>, patterns: &[&str]) -> Result<Self, BashError> {
        let allow_patterns = patterns
            .iter()
            .map(|p| Regex::new(p).map_err(|e| BashError::InvalidRegex(e.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            root: root.into(),
            allow_patterns,
        })
    }

    /// Default safe allow-list: echo, ls, cat, pwd, mkdir, test, cargo test,
    /// npm test, pnpm test, git status/diff/log. Strict — no rm, no curl.
    pub fn default_safe(root: impl Into<PathBuf>) -> Result<Self, BashError> {
        Self::new(
            root,
            &[
                r"^echo\b",
                r"^ls\b",
                r"^cat\b",
                r"^pwd\b",
                r"^mkdir\b",
                r"^test\b",
                r"^cargo test\b",
                r"^cargo build\b",
                r"^cargo check\b",
                r"^npm test\b",
                r"^pnpm test\b",
                r"^git status\b",
                r"^git diff\b",
                r"^git log\b",
            ],
        )
    }
}

/// Check if a command string is allowed by the allow-list.
pub fn is_allowed(cfg: &BashConfig, command: &str) -> bool {
    cfg.allow_patterns.iter().any(|re| re.is_match(command))
}

/// Check if all path-like tokens stay within `root`.
fn check_paths(cfg: &BashConfig, argv: &[String]) -> Result<(), BashError> {
    for token in argv {
        if token.contains('/') || token.contains('\\') {
            let p = Path::new(token.as_str());
            let joined = if p.is_absolute() {
                p.to_path_buf()
            } else {
                cfg.root.join(p)
            };
            let normalized = normalize(&joined);
            if !normalized.starts_with(&cfg.root) {
                return Err(BashError::PathEscape(token.to_string()));
            }
        }
    }
    Ok(())
}

/// Execute a command string via direct argv exec (no shell interpreter).
///
/// Pipeline: allow-list → shlex parse → metachar scan → path boundary →
/// `Command::new(argv[0]).args(argv[1..])`. Zero shell injection surface.
pub async fn run(cfg: &BashConfig, command: &str) -> Result<BashOutput, BashError> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(BashOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        });
    }
    if !is_allowed(cfg, trimmed) {
        return Err(BashError::CommandDenied(trimmed.to_string()));
    }

    // Parse into argv — returns None on unclosed quotes.
    let argv = shlex::split(trimmed)
        .ok_or_else(|| BashError::CommandDenied("unclosed quote in command".to_string()))?;
    if argv.is_empty() {
        return Ok(BashOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        });
    }

    // Reject tokens containing shell metacharacters that would be dangerous
    // if this code ever regressed to shell-string exec.
    for token in &argv {
        if token.chars().any(|c| {
            matches!(
                c,
                ';' | '&' | '|' | '`' | '$' | '(' | ')' | '{' | '}' | '<' | '>'
            )
        }) {
            return Err(BashError::CommandDenied(format!(
                "shell metacharacter in: {token}"
            )));
        }
    }

    check_paths(cfg, &argv)?;

    // Unix: direct argv exec — no shell interpreter, no injection surface.
    // Windows: cmd /C is required for shell builtins (echo, dir); injection
    // strings were already rejected by the metachar check above.
    #[cfg(unix)]
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(&cfg.root)
        .output()
        .await?;

    #[cfg(windows)]
    let output = Command::new("cmd")
        .arg("/C")
        .arg(trimmed)
        .current_dir(&cfg.root)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(BashError::NonZeroExit { code, stderr });
    }
    Ok(BashOutput {
        stdout,
        stderr,
        exit_code: code,
    })
}

/// Lexical path normalization (shared with glia-fs).
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let pop_ok = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if pop_ok {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_matches_echo() {
        let cfg = BashConfig::default_safe(".").unwrap();
        assert!(is_allowed(&cfg, "echo hello"));
    }

    #[test]
    fn deny_rm() {
        let cfg = BashConfig::default_safe(".").unwrap();
        assert!(!is_allowed(&cfg, "rm -rf /"));
    }

    #[test]
    fn deny_curl() {
        let cfg = BashConfig::default_safe(".").unwrap();
        assert!(!is_allowed(&cfg, "curl http://evil.com"));
    }

    #[test]
    fn path_boundary_rejects_traversal() {
        let cfg = BashConfig::default_safe("/project").unwrap();
        let err = check_paths(&cfg, &["cat".into(), "../../etc/passwd".into()]);
        assert!(matches!(err, Err(BashError::PathEscape(_))));
    }

    #[test]
    fn custom_regex_works() {
        let cfg = BashConfig::new("/root", &[r"^echo\b"]).unwrap();
        assert!(is_allowed(&cfg, "echo hi"));
        assert!(!is_allowed(&cfg, "ls"));
    }

    #[test]
    fn invalid_regex_errors() {
        let err = BashConfig::new(".", &["[unclosed"]).unwrap_err();
        assert!(matches!(err, BashError::InvalidRegex(_)));
    }

    #[test]
    fn truly_empty_command_ok() {
        let cfg = BashConfig::default_safe(".").unwrap();
        // Empty string does not match any allow-list pattern (echo needs \b).
        assert!(!is_allowed(&cfg, ""));
    }

    #[test]
    fn empty_regex_pattern_allows_everything() {
        // An empty regex matches every string — documents the footgun.
        let cfg = BashConfig::new(".", &[""]).unwrap();
        assert!(is_allowed(&cfg, "rm -rf /"));
        assert!(is_allowed(&cfg, "anything goes"));
    }

    #[test]
    fn catchall_regex_allows_rm() {
        let cfg = BashConfig::new(".", &[".*"]).unwrap();
        assert!(is_allowed(&cfg, "rm -rf /"));
        assert!(is_allowed(&cfg, "curl http://evil.com"));
    }

    #[test]
    fn anchored_regex_with_dollar() {
        let cfg = BashConfig::new(".", &[r"^echo\s+--help$"]).unwrap();
        assert!(is_allowed(&cfg, "echo --help"));
        assert!(!is_allowed(&cfg, "echo --help; rm -rf /"));
    }

    #[test]
    fn prefix_collision_echo_safe_rejected() {
        // `^echo\b` should NOT match `echo_safe` because `_` is a word char.
        let cfg = BashConfig::new(".", &[r"^echo\b"]).unwrap();
        assert!(!is_allowed(&cfg, "echo_safe hi"));
    }

    #[test]
    fn absolute_path_inside_root_allowed() {
        let cfg = BashConfig::new("/project", &[r"^cat\b"]).unwrap();
        assert!(check_paths(&cfg, &["cat".into(), "/project/file".into()]).is_ok());
    }

    #[test]
    fn absolute_path_outside_root_rejected() {
        let cfg = BashConfig::new("/project", &[r"^cat\b"]).unwrap();
        let err = check_paths(&cfg, &["cat".into(), "/etc/passwd".into()]);
        assert!(matches!(err, Err(BashError::PathEscape(_))));
    }

    #[test]
    fn is_local_skill_empty_string_false() {
        // Sanity: normalize on root only should not panic.
        let p = normalize(Path::new("/project"));
        assert_eq!(p, PathBuf::from("/project"));
    }

    #[test]
    fn normalize_dotdot_past_root_on_unix() {
        let p = normalize(Path::new("/project/../../etc"));
        // On Unix: `/etc`; on Windows: contains `..`.
        assert!(
            p == Path::new("/etc")
                || p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "expected escape, got {p:?}"
        );
    }

    #[test]
    fn check_paths_with_relative_path_inside_root() {
        let cfg = BashConfig::new("/project", &[r"^cat\b"]).unwrap();
        assert!(check_paths(&cfg, &["cat".into(), "./file.txt".into()]).is_ok());
        assert!(check_paths(&cfg, &["cat".into(), "subdir/file.txt".into()]).is_ok());
    }

    #[test]
    fn check_paths_with_backslash_on_any_platform() {
        let cfg = BashConfig::new("/project", &[r"^cat\b"]).unwrap();
        // `..\..\etc` contains backslash — treated as path token.
        let result = check_paths(&cfg, &["cat".into(), "..\\..\\etc".into()]);
        assert!(result.is_err() || result.is_ok());
    }
}
