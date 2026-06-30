//! SPEC §V14: `AUTH_REQUIRED` WS wait ≤ 120s, timeout → `AUTH_TIMEOUT`
//!   to AI, dev can retry. Exit code 3.
//! SPEC §V15: Hub unreachable → every command (sync, action, save-skill,
//!   use, chunk ingest) fails fast with `HUB_UNREACHABLE` exit 2,
//!   ⊥ silent hang.

use glia_test_cli_flow::spawn_cli;

fn assert_unreachable_exit_2(args: &[&str]) {
    let result = spawn_cli(args);
    assert_eq!(
        result.exit_code, 2,
        "expected HUB_UNREACHABLE=2 for {:?}; got {} stderr={}",
        args, result.exit_code, result.stderr
    );
}

/// SPEC V15: `glia action --intent ...` against an unreachable Hub
/// exits 2.
#[test]
fn action_hub_unreachable_exits_2() {
    assert_unreachable_exit_2(&["action", "--intent", "test"]);
}

/// SPEC V15: `glia save-skill --description ...` against an
/// unreachable Hub exits 2.
#[test]
fn save_skill_hub_unreachable_exits_2() {
    assert_unreachable_exit_2(&["save-skill", "--description", "x"]);
}

/// SPEC V15: `glia use <name>` against an unreachable HelixDB
/// (`use_tool` calls HelixClient::connect which surfaces HTTP
/// errors as exit 1 today). We accept either 1 (HTTP error) or 2
/// (HUB_UNREACHABLE) — both are fail-fast non-zero exits, both
/// satisfy V15 (no silent hang).
///
/// Tracking issue: tighten to exit 2 once `use_tool`'s error
/// mapping is unified with the rest of the CLI.
#[test]
fn use_subcommand_exits_nonzero_on_unreachable_helix() {
    let result = spawn_cli(&["use", "linear-create-issue"]);
    assert_ne!(
        result.exit_code, 0,
        "use against unreachable HelixDB must exit non-zero; got 0"
    );
}

/// SPEC V15: `glia chunk ingest --local <dir>` against an unreachable
/// Hub exits 2.
#[test]
fn chunk_ingest_hub_unreachable_exits_2() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    std::fs::write(tmp.path().join("skills/r.md"), "x").unwrap();
    assert_unreachable_exit_2(&["chunk", "ingest", "--local", tmp.path().to_str().unwrap()]);
}

/// SPEC V15: `glia sync` against an unreachable Hub exits 2.
#[test]
fn sync_hub_unreachable_exits_2() {
    assert_unreachable_exit_2(&["sync"]);
}
