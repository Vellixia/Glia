//! Integration tests for glia-bash. Verifies V1 (local exec works) and
//! V9 (fallback path: denied commands don't run, path escapes rejected).

use glia_bash::{is_allowed, run, BashConfig, BashError};

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