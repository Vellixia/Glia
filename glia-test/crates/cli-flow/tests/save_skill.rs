//! SPEC §T13: `impl glia_save_skill` AI self-authoring.
//! SPEC §V11: `glia save-skill` → embed + team-shared (not per-dev).
//! SPEC §V15: Hub unreachable → fail fast with `HUB_UNREACHABLE` exit 2.
//!
//! These tests verify the `glia save-skill` argument surface and
//! exit semantics. They do not exercise the embedder or the Hub
//! directly — that's covered in `glia-e2e/tests/live_auth.rs`.

use glia_test_cli_flow::spawn_cli;

/// SPEC V11: save-skill writes a markdown file under `<repo>/skills/`
/// with the slug derived from the description. `--help` exits 0.
#[test]
fn save_skill_help_exits_0() {
    let result = spawn_cli(&["save-skill", "--help"]);
    assert_eq!(
        result.exit_code, 0,
        "`glia save-skill --help` should exit 0; got {} stderr={}",
        result.exit_code, result.stderr
    );
}

/// SPEC V15: save-skill against an unreachable Hub exits 2.
#[test]
fn save_skill_hub_unreachable_exits_2() {
    let result = spawn_cli(&["save-skill", "--description", "Some rule", "--local", "."]);
    assert_eq!(result.exit_code, 2);
}

/// SPEC V8 / T13: when the description is missing, the CLI must
/// reject the input with a clear clap error (exit 2).
#[test]
fn save_skill_missing_description_errors() {
    let result = spawn_cli(&["save-skill"]);
    assert_eq!(
        result.exit_code, 2,
        "missing --description must exit 2; got: {}\nstderr: {}",
        result.exit_code, result.stderr
    );
}

/// SPEC V15: `glia use` against an unreachable HelixDB also fails
/// fast — see `exit_codes::use_subcommand_exits_nonzero_on_unreachable_helix`.
/// We don't re-test here.
#[test]
#[ignore = "covered by exit_codes::use_subcommand_exits_nonzero_on_unreachable_helix"]
fn use_hub_unreachable_exits_2() {
    let result = spawn_cli(&["use", "linear-create-issue"]);
    let _ = result;
}
