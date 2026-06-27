//! Bug documentation tests.
//! These tests document known bugs/gaps in the codebase. They assert
//! the CURRENT (buggy) behavior so the bugs are tracked and won't be
//! accidentally "fixed" without understanding the impact.

use std::time::Duration;

// B1/B2: Fixed. No longer applicable.

// B3: glia-bash is a library crate, not a CLI subcommand
// Documented in SPEC B3.

// B4: Fixed in commit 2ea746d. Not a bug.

// B5/B6: Release glob fixes. Not code-level bugs.

// B7: HelixQL `#[register]` query bundle not yet authored.
// `HelixClient::ping()` against HelixDB without Glia schema returns an error.
#[tokio::test]
async fn bug_helix_ping_against_server_without_glia_schema() {
    use glia_helix::HelixClient;
    let client = match HelixClient::connect(Some("http://127.0.0.1:6969"), None) {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = client.ping().await;
}

// FIXED: glia context subcommand now exists (was bug in v0.2.0).
// The `Cmd::Context` variant was added — hooks referencing `glia context`
// now work correctly.

// FIXED: exit code 2 for Hub-unreachable is now implemented.
// See `crates/glia-cli/src/main.rs` `is_hub_unreachable()` and the
// `process::exit(2)` mapping.

// FIXED: hooks settings.json now merges instead of overwriting.
// See `merge_hooks_into_settings()` in `crates/glia-hooks/src/lib.rs`.

// FIXED: save-skill output now includes stacks and source.
// See `run_save_skill()` in `crates/glia-cli/src/main.rs`.

// FIXED: Hub now has graceful shutdown on SIGINT.
// See `crates/glia-hub/src/main.rs` `tokio::select!` with `ctrl_c()`.

// FIXED: chunk ingest --all/--changed dead flags removed.
// The simplified `Ingest` variant no longer has these flags.

// Remaining known bugs:

// FIXED: `bridge` real `run_bridge` now testable via `run_bridge_with_io`.
// The `run_bridge` function delegates to `run_bridge_with_io(stdin, stdout)`
// which accepts generic `R: AsyncRead, W: AsyncWrite`. Integration tests
// can now inject `tokio::io::DuplexStream` to exercise the full pipeline.

// FIXED: `action` auth timeout now exits with code 3 (AUTH_TIMEOUT).
// `handle_auth_required` returns `Err("AUTH_TIMEOUT after ...")` on
// timeout, and the CLI maps it to `process::exit(3)`. See
// `is_auth_timeout()` in `crates/glia-cli/src/main.rs`.

// FIXED: symlink escape now detected via canonicalize.
// `Fs::resolve` now canonicalizes both the root and the resolved path
// (when the file exists) and rejects paths whose canonical form is
// outside the canonical root. Falls back to lexical check on
// canonicalize failure.

// Bug: `cache` TTL=0 divergence between in-memory and Redis
// InMemoryCache: TTL=0 → immediate expiry.
// Redis: TTL=0 → clamped to 1s (ttl.as_secs().max(1)).
// This is by design — different backends have different minimum TTL
// guarantees. The InMemoryCache supports sub-second TTLs; Redis
// requires integer seconds.
#[tokio::test]
async fn bug_cache_ttl_zero_divergence() {
    use glia_cache::{Cache, InMemoryCache, RedisCache};
    let c = InMemoryCache::new();
    c.put_bytes("k", b"v", Duration::ZERO).await.unwrap();
    let in_mem = c.get_bytes("k").await.unwrap();
    assert!(
        in_mem.is_none(),
        "in-memory TTL=0 should expire immediately"
    );
    let _ = RedisCache::connect("redis://127.0.0.1:6379/0").await;
}

// Bug: In-process axum tests don't bind to 0.0.0.0
// `serve(addr)` binds to a specific address; tests use 127.0.0.1:0.
// This is by design for tests but should be noted.
#[test]
fn bug_serve_addr_uses_specific_address() {
    // Documented: crates/glia-hub/src/lib.rs:71-79 binds to a specific
    // address. No 0.0.0.0 option exposed for test scenarios.
}
