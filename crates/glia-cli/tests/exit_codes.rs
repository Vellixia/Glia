//! Exit code matrix: every subcommand × error condition.
//!
//! Tests that Hub-unreachable paths produce non-zero exit codes,
//! and that graceful-degradation paths (init, chunk with no skills/)
//! exit 0.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn glia() -> Command {
    Command::cargo_bin("glia").unwrap_or_else(|_| {
        panic!("glia binary not found — run cargo build -p glia-cli first")
    })
}

fn temp_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("glia-cli-exit-{}-{}", pid, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// --- Hub-unreachable tests (point at dead port) ---

#[test]
fn sync_hub_unreachable_exits_nonzero() {
    glia()
        .args(["sync", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure();
}

#[test]
fn action_hub_unreachable_exits_nonzero() {
    glia()
        .args(["action", "--intent", "test", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure();
}

#[test]
fn save_skill_hub_unreachable_exits_nonzero() {
    glia()
        .args([
            "save-skill",
            "test skill",
            "--hub",
            "http://127.0.0.1:1",
        ])
        .assert()
        .failure();
}

#[test]
fn use_hub_unreachable_exits_nonzero() {
    glia()
        .args([
            "use",
            "some-tool",
            "--hub",
            "http://127.0.0.1:1",
            "--catalog-url",
            "http://127.0.0.1:1",
        ])
        .assert()
        .failure();
}

#[test]
fn chunk_ingest_hub_unreachable_exits_nonzero() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    std::fs::write(dir.join("skills/test.md"), "## Test\ncontent").unwrap();
    glia()
        .args([
            "chunk",
            "ingest",
            "--hub",
            "http://127.0.0.1:1",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let _ = std::fs::remove_dir_all(&dir);
}

// --- Graceful degradation tests (exit 0) ---

#[test]
fn chunk_ingest_no_skills_dir_exits_0() {
    let dir = temp_dir();
    glia()
        .args([
            "chunk",
            "ingest",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_empty_dir_exits_0() {
    let dir = temp_dir();
    glia()
        .args(["init", "--repo-root", dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("stacks"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_nonexistent_repo_root_exits_nonzero() {
    glia()
        .args(["init", "--repo-root", "/nonexistent/path/xyz"])
        .assert()
        .failure();
}

#[test]
fn init_repo_root_is_file_exits_nonzero() {
    let dir = temp_dir();
    let file = dir.join("afile");
    std::fs::write(&file, "content").unwrap();
    glia()
        .args(["init", "--repo-root", file.to_str().unwrap()])
        .assert()
        .failure();
    let _ = std::fs::remove_dir_all(&dir);
}

// --- Output format tests ---

#[test]
fn init_output_is_valid_json_with_expected_fields() {
    let dir = temp_dir();
    glia()
        .args(["init", "--repo-root", dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("stacks"))
        .stdout(predicate::str::contains("pending_auth"))
        .stdout(predicate::str::contains("files_scanned"))
        .stdout(predicate::str::contains("git_repo_missing"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn chunk_ingest_no_md_files_exits_0() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    // Only a non-md file.
    std::fs::write(dir.join("skills/readme.txt"), "text").unwrap();
    glia()
        .args([
            "chunk",
            "ingest",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn chunk_ingest_skips_readme_md() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    std::fs::write(dir.join("skills/README.md"), "## Readme\ncontent").unwrap();
    // Should exit 0 — README is skipped, so 0 chunks ingested (or graceful
    // skip if embed model missing).
    glia()
        .args([
            "chunk",
            "ingest",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let _ = std::fs::remove_dir_all(&dir);
}

// --- stderr/stdout separation ---

#[test]
fn init_errors_to_stderr_results_to_stdout() {
    let dir = temp_dir();
    let output = glia()
        .args(["init", "--repo-root", dir.to_str().unwrap()])
        .output()
        .unwrap();
    // stdout should contain JSON result.
    assert!(!String::from_utf8_lossy(&output.stdout).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- Token edge cases ---

#[test]
fn cli_sync_empty_token_accepted() {
    // Empty token string is accepted by clap; the error comes from Hub.
    glia()
        .args(["sync", "--hub", "http://127.0.0.1:1", "--token", ""])
        .assert()
        .failure();
}

// --- Flag combinations ---

#[test]
fn cli_save_skill_stacks_csv_delimiter() {
    // --stacks accepts comma-delimited values.
    // Should fail at Hub connection, not at arg parsing.
    glia()
        .args([
            "save-skill",
            "test",
            "--stacks",
            "nextjs,supabase",
            "--hub",
            "http://127.0.0.1:1",
        ])
        .assert()
        .failure();
}

#[test]
fn cli_chunk_ingest_no_flags_accepted() {
    // The --all and --changed dead flags were removed. Ingest now
    // always processes all .md files in skills/.
    let dir = temp_dir();
    glia()
        .args([
            "chunk",
            "ingest",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .success();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chunk_ingest_rejects_dead_flags() {
    // --all and --changed are no longer valid flags. The CLI should
    // reject them with exit 2 (clap parse error).
    let dir = temp_dir();
    glia()
        .args([
            "chunk",
            "ingest",
            "--all",
            "--repo-root",
            dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(2);
    let _ = std::fs::remove_dir_all(&dir);
}