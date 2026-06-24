//! CLI argument parsing + exit code tests.
//!
//! Uses assert_cmd to invoke the `glia` binary and assert exit codes + stderr.
//! Tests: no args, unknown subcommand, missing required positionals, help/version,
//! flag edge cases.

use assert_cmd::Command;
use predicates::prelude::*;

fn glia() -> Command {
    Command::cargo_bin("glia").unwrap_or_else(|_| {
        panic!("glia binary not found — run cargo build -p glia-cli first")
    })
}

#[test]
fn cli_no_subcommand_errors_exit_2() {
    glia()
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn cli_unknown_subcommand_errors_exit_2() {
    glia()
        .arg("foobar")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_action_missing_intent_errors_exit_2() {
    glia()
        .arg("action")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_action_intent_flag_without_value_errors_exit_2() {
    glia()
        .args(["action", "--intent"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_save_skill_missing_description_errors_exit_2() {
    glia()
        .arg("save-skill")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_use_missing_tool_positional_errors_exit_2() {
    glia()
        .arg("use")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_chunk_no_op_errors_exit_2() {
    glia()
        .arg("chunk")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_chunk_unknown_op_errors_exit_2() {
    glia()
        .args(["chunk", "foobar"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn cli_help_top_level_exits_0() {
    glia()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn cli_help_bridge_exits_0() {
    glia()
        .args(["bridge", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("bridge"));
}

#[test]
fn cli_help_sync_exits_0() {
    glia()
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"));
}

#[test]
fn cli_help_init_exits_0() {
    glia()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("init"));
}

#[test]
fn cli_help_action_exits_0() {
    glia()
        .args(["action", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("action"));
}

#[test]
fn cli_help_save_skill_exits_0() {
    glia()
        .args(["save-skill", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("save-skill"));
}

#[test]
fn cli_help_use_exits_0() {
    glia()
        .args(["use", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("use"));
}

#[test]
fn cli_help_chunk_exits_0() {
    glia()
        .args(["chunk", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("chunk"));
}

#[test]
fn cli_help_chunk_ingest_exits_0() {
    glia()
        .args(["chunk", "ingest", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_version_flag_exits_0() {
    glia()
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn cli_help_context_exits_0() {
    glia()
        .args(["context", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_context_no_file_exits_0() {
    // Without --file, context emits a warning and exits 0 (graceful).
    glia()
        .args(["context"])
        .assert()
        .success();
}

#[test]
fn cli_action_with_ws_scheme_hub_errors() {
    // WS scheme on HTTP endpoint should error (not crash).
    glia()
        .args(["action", "--intent", "test", "--hub", "ws://127.0.0.1:1"])
        .assert()
        .failure();
}

#[test]
fn cli_action_non_routable_hub_errors() {
    // Port 1 is privileged and almost certainly not listening.
    glia()
        .args(["action", "--intent", "test", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure();
}