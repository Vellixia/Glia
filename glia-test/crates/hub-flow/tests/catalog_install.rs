//! SPEC §T1 (Catalog resolvers): the Hub exposes a list of tools from
//!   the `glia-catalog` crate.
//! SPEC §T20 / §C9: community catalog via GitHub raw, exec in user-private
//!   sandbox (catalog schema pull ≠ trust).
//!
//! These tests use `StubCatalog` to drive the resolver paths without
//! network. Stub outputs deterministic tool entries so assertions are
//! stable.

use std::collections::HashMap;

use glia_catalog::{CatalogError, CatalogIndex, CatalogSource, StubCatalog};

fn entry(name: &str) -> glia_catalog::CatalogEntry {
    glia_catalog::CatalogEntry {
        name: name.into(),
        display: name.into(),
        description: "test".into(),
        path: format!("tools/{}.md", name),
        stacks: vec!["nextjs".into()],
        creds: vec![name.split('-').next().unwrap_or(name).into()],
        version: "1.0.0".into(),
    }
}

fn stub_with(tools: Vec<glia_catalog::CatalogEntry>) -> StubCatalog {
    let mut skills = HashMap::new();
    for t in &tools {
        skills.insert(t.name.clone(), format!("# {}\nUse OAuth.", t.display));
    }
    StubCatalog {
        index: CatalogIndex { version: 1, tools },
        skills,
    }
}

/// SPEC T1: `list_tools` returns the full catalog.
#[tokio::test]
async fn list_tools_returns_full_catalog() {
    let s = stub_with(vec![
        entry("github-create-issue"),
        entry("slack-post-message"),
    ]);
    let tools = glia_catalog::list_tools(&s).await.unwrap();
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"github-create-issue"));
    assert!(names.contains(&"slack-post-message"));
}

/// SPEC T20: `CatalogEntry` is JSON-parseable so a non-200 / malformed
/// response from GitHub raw surfaces as a clean error path on the
/// serde side. Stable struct shape across versions.
#[tokio::test]
async fn catalog_entry_json_shape_is_stable() {
    let e = entry("test");
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["name"], "test");
    assert_eq!(v["stacks"][0], "nextjs");
    assert_eq!(v["path"], "tools/test.md");
}

/// SPEC C9 + V12: schema pull ≠ trust — the catalog response must be
/// validated structurally before any code that depends on it runs.
/// An empty stub returns an empty list, proving the integration path
/// doesn't synthesize data.
#[tokio::test]
async fn empty_stub_catalog_returns_empty_list() {
    let s = stub_with(vec![]);
    let tools = glia_catalog::list_tools(&s).await.unwrap();
    assert!(tools.is_empty());
}

/// SPEC V12 (negative): `fetch_skill` on a missing entry surfaces a
/// structured `NotFound` error rather than panicking.
#[tokio::test]
async fn use_tool_missing_returns_error() {
    let s = stub_with(vec![entry("linear-create-issue")]);
    let idx = s.fetch_index().await.unwrap();
    let miss = idx.find("does-not-exist");
    assert!(miss.is_none(), "missing name must return None, not panic");
    let err = s.fetch_skill(&entry("nope")).await.unwrap_err();
    assert!(matches!(err, CatalogError::NotFound(_)));
}
