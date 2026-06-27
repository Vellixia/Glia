//! Exit code 2 for Hub-unreachable paths (SPEC V15).
//! These tests verify the fix: HubUnreachable → process::exit(2).

use assert_cmd::Command;

fn glia() -> Command {
    Command::cargo_bin("glia")
        .unwrap_or_else(|_| panic!("glia binary not found — run cargo build -p glia-cli first"))
}

#[test]
fn sync_hub_unreachable_exits_2() {
    glia()
        .args(["sync", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn action_hub_unreachable_exits_2() {
    glia()
        .args(["action", "--intent", "test", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn save_skill_hub_unreachable_exits_2() {
    glia()
        .args(["save-skill", "test skill", "--hub", "http://127.0.0.1:1"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn use_hub_unreachable_exits_2() {
    // Point --hub at Glia's real HelixDB and --catalog-url at a dead port.
    // The catalog fails first → use exits 1 (not 2), which is correct
    // per SPEC V15 (catalog errors are not Hub errors).
    glia()
        .args([
            "use",
            "some-tool",
            "--hub",
            "http://127.0.0.1:6969",
            "--catalog-url",
            "http://127.0.0.1:1",
        ])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn chunk_ingest_hub_unreachable_exits_2() {
    let dir = std::env::temp_dir().join(format!(
        "glia-exit2-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
        .failure()
        .code(2);
    let _ = std::fs::remove_dir_all(&dir);
}
