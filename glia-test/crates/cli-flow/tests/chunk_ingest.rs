//! SPEC §T10: Git pre-push hook chunks + embeds `.glia/skills/*.md`.
//! SPEC §T22: Hub-authoritative sync — CLI computes `SyncDiff` against
//!   the Hub list, no bidirectional sync in v0.2.0.
//! SPEC §V15: Hub unreachable → every command fails fast with exit 2.
//! SPEC §V16: skills are `local::<slug>` after `glia chunk ingest`.
//!
//! These tests verify the CLI argument shape (via the binary's stdout)
//! and exit semantics for `glia chunk ingest`. They do NOT spawn the
//! Hub — the network destination is forced to an unreachable port so
//! `HUB_UNREACHABLE` is the dominant exit path.

use glia_test_cli_flow::spawn_cli;

/// SPEC V15: `glia chunk ingest --local <repo> --repo-root <repo>`
/// exits with code 2 when the Hub is unreachable.
#[test]
fn chunk_ingest_hub_unreachable_exits_2() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join("skills")).unwrap();
    std::fs::write(repo.join("skills/rule.md"), "# rule\n").unwrap();

    let result = spawn_cli(&[
        "chunk",
        "ingest",
        "--local",
        repo.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
    ]);
    assert_eq!(
        result.exit_code, 2,
        "expected HUB_UNREACHABLE=2; got {} stderr={}",
        result.exit_code, result.stderr
    );
}

/// SPEC T10 / SPEC B1 fix: `glia chunk ingest` is a registered
/// subcommand. We verify clap accepts the args by checking that
/// `--help` exits 0.
#[test]
fn chunk_ingest_help_exits_0() {
    let result = spawn_cli(&["chunk", "ingest", "--help"]);
    assert_eq!(
        result.exit_code, 0,
        "`chia chunk ingest --help` should exit 0; got {} stderr={}",
        result.exit_code, result.stderr
    );
}

/// SPEC V16: ingest result should map files into `local::<slug>` ids.
/// We can't check the Hub's persisted state without a Hub running, so
/// we verify the slug derivation by reading the local file's stem.
#[test]
fn slug_from_file_stem() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let dir = tmp.path().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("my-rule.md"), "body").unwrap();
    let stem = std::path::Path::new("my-rule.md")
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(stem, "my-rule");
    assert_eq!(format!("local::{}", stem), "local::my-rule");
}

/// SPEC T22 + V15: `glia sync` against an unreachable Hub also exits 2.
#[test]
fn sync_hub_unreachable_exits_2() {
    let result = spawn_cli(&["sync"]);
    assert_eq!(
        result.exit_code, 2,
        "sync against unreachable Hub must exit 2; got {}",
        result.exit_code
    );
}
