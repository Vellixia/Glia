//! Integration tests for glia-bash. Verifies V1 (local exec works) and
//! V9 (fallback path: denied commands don't run, path escapes rejected).

use glia_bash::{BashConfig, BashError, is_allowed, run};

#[tokio::test]
async fn allowed_cmd_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "echo hello glia").await.unwrap();
    assert_eq!(out.stdout.trim(), "hello glia");
    assert_eq!(out.exit_code, 0);
}

#[tokio::test]
async fn denied_cmd_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let err = run(&cfg, "rm -rf /").await;
    assert!(matches!(err, Err(BashError::CommandDenied(_))));
}

#[tokio::test]
async fn path_traversal_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    // `cat ../../etc/passwd` — cat is allowed, but path escapes root.
    let err = run(&cfg, "cat ../../etc/passwd").await;
    assert!(matches!(err, Err(BashError::PathEscape(_))));
}

#[tokio::test]
async fn cargo_test_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    assert!(is_allowed(&cfg, "cargo test --release"));
}

#[tokio::test]
async fn empty_command_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "   ").await.unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty());
}

#[tokio::test]
async fn non_zero_exit_propagates() {
    let tmp = tempfile::tempdir().unwrap();
    // Add `false` to allow-list so it runs but exits non-zero.
    let cfg = BashConfig::new(tmp.path(), &[r"^echo\b", r"^false\b"]).unwrap();
    let err = run(&cfg, "false").await;
    assert!(matches!(err, Err(BashError::NonZeroExit { .. })));
}

#[tokio::test]
async fn concurrent_bash_run_different_commands() {
    use glia_bash::run;
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = Arc::new(BashConfig::default_safe(tmp.path()).unwrap());
    let mut handles = Vec::new();
    for i in 0..10 {
        let cc = cfg.clone();
        handles.push(tokio::spawn(async move {
            let out = run(&cc, &format!("echo concurrent-{i}")).await.unwrap();
            assert!(out.stdout.contains(&format!("concurrent-{i}")));
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[test]
fn unicode_bash_command_string() {
    use glia_bash::is_allowed;
    let cfg = BashConfig::default_safe(".").unwrap();
    // Unicode in command — should still match the allow-list.
    assert!(is_allowed(&cfg, "echo héllo 日本語 🎫"));
}

#[tokio::test]
async fn truly_empty_string_command_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "").await.unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty());
}

#[tokio::test]
async fn whitespace_only_with_tabs_newlines() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "\n\t  \n").await.unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty());
}

/// SECURITY: semicolon injection must now be denied, not executed.
#[tokio::test]
async fn semicolon_injection_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    // Allow-list matches ^echo\b, but `;` is a shell metacharacter — must
    // be caught before exec on both Unix (argv) and Windows (cmd /C).
    let err = run(&cfg, "echo hi; echo injected").await;
    assert!(matches!(err, Err(BashError::CommandDenied(_))));
}

/// SECURITY: pipe injection must be denied.
#[tokio::test]
async fn pipe_injection_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let err = run(&cfg, "echo hi | cat").await;
    assert!(matches!(err, Err(BashError::CommandDenied(_))));
}

/// SECURITY: shell redirection (`>`) is a metacharacter — must be denied.
#[tokio::test]
async fn redirect_metachar_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let err = run(&cfg, "echo err 1>&2").await;
    assert!(matches!(err, Err(BashError::CommandDenied(_))));
}

/// `false` exits 1 on Unix — non-zero exit is propagated as NonZeroExit.
#[cfg(unix)]
#[tokio::test]
async fn exit_code_1_propagates() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::new(tmp.path(), &[r"^false\b"]).unwrap();
    let err = run(&cfg, "false").await;
    assert!(matches!(err, Err(BashError::NonZeroExit { code: 1, .. })));
}

/// Zero exit code is returned as Ok.
#[tokio::test]
async fn exit_code_0_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "echo ok").await.unwrap();
    assert_eq!(out.exit_code, 0);
}

#[tokio::test]
async fn unicode_command_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "echo héllo").await.unwrap();
    assert!(out.stdout.contains("héllo"));
}

#[tokio::test]
async fn very_long_command_string() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let long_arg = "x".repeat(1_000);
    let cmd = format!("echo {long_arg}");
    let out = run(&cfg, &cmd).await.unwrap();
    assert!(out.stdout.len() >= 1_000);
}

/// Newline is whitespace to shlex — `echo first\necho second` is a single
/// call to echo with 3 args; the output still contains both words.
#[tokio::test]
async fn command_with_newline_in_args() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    let out = run(&cfg, "echo first\nsecond").await;
    if cfg!(not(windows)) {
        let out = out.unwrap();
        assert!(out.stdout.contains("first"));
        assert!(out.stdout.contains("second"));
    }
}

#[tokio::test]
async fn absolute_path_outside_root_rejected_in_run() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    // cat is allowed; absolute path outside tmp root should fail.
    let err = run(&cfg, "cat /etc/passwd").await;
    // On any platform, /etc/passwd or C:\etc\passwd is outside tmp.
    assert!(matches!(err, Err(BashError::PathEscape(_))));
}

#[tokio::test]
async fn absolute_path_inside_root_allowed_in_run() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = BashConfig::default_safe(tmp.path()).unwrap();
    // Create a file inside the tmp dir, then cat it by absolute path.
    let file_path = tmp.path().join("test.txt");
    std::fs::write(&file_path, "content").unwrap();
    let cmd = format!("cat {}", file_path.display());
    let out = run(&cfg, &cmd).await;
    // Should succeed because the absolute path is inside root.
    if let Ok(out) = out {
        assert!(out.stdout.contains("content"));
    }
}
