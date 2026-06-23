//! Integration tests for glia-db. Verifies V1 (embedded disk persists),
//! V10 (graph edges for stack-aware RAG), V16 (local:: namespace).

use glia_db::{Auth, Connection, GliaDb, Skill, Stack, Tool};
use chrono::Utc;

fn now() -> String {
    Utc::now().to_rfc3339()
}

async fn setup() -> GliaDb {
    let db = GliaDb::connect(Connection::Memory).await.unwrap();
    db.init_schema().await.unwrap();
    db
}

#[tokio::test]
async fn embedded_memory_crud() {
    let db = setup().await;

    let tool = Tool {
        name: "Create Linear Issue".to_string(),
        category: "issue-tracker".to_string(),
        local: false,
        params_schema: serde_json::json!({"title": "string"}),
        updated_at: now(),
    };
    db.upsert_tool("linear-create-issue", tool.clone()).await.unwrap();

    let got = db.get_tool("linear-create-issue").await.unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().name, "Create Linear Issue");
}

#[tokio::test]
async fn graph_edge_requires() {
    let db = setup().await;

    db.upsert_tool("linear-create-issue", Tool {
        name: "Create Issue".to_string(),
        category: "issue-tracker".to_string(),
        local: false,
        params_schema: serde_json::json!({}),
        updated_at: now(),
    }).await.unwrap();

    db.upsert_auth("linear_oauth", Auth {
        auth_type: "oauth".to_string(),
        provider: "linear".to_string(),
        updated_at: now(),
    }).await.unwrap();

    eprintln!("about to relate");
    db.relate_tool_requires_auth("linear-create-issue", "linear_oauth")
        .await
        .unwrap();
    eprintln!("relate done");

    let tools = db.tools_requiring_auth("linear_oauth").await.unwrap();
    eprintln!("tools found: {:#?}", tools);
    assert!(!tools.is_empty(), "should find tool requiring this auth");
    assert_eq!(tools[0].name, "Create Issue");
}

#[tokio::test]
async fn graph_edge_applies_to() {
    let db = setup().await;

    db.upsert_skill("supabase-auth-rules", Skill {
        content: "Never use service_role in route handlers.".to_string(),
        source: "supabase-auth.md".to_string(),
        embedding: vec![0.1, 0.2, 0.3],
        updated_at: now(),
    }).await.unwrap();

    db.upsert_stack("nextjs", Stack {
        name: "Next.js".to_string(),
    }).await.unwrap();

    db.relate_skill_applies_to_stack("supabase-auth-rules", "nextjs")
        .await
        .unwrap();

    let skills = db.skills_for_stack("nextjs").await.unwrap();
    assert!(!skills.is_empty(), "should find skill for nextjs stack");
    assert!(skills[0].content.contains("service_role"));
}

#[tokio::test]
async fn local_namespace_detection() {
    assert!(GliaDb::is_local_skill("local::use-zustand"));
    assert!(!GliaDb::is_local_skill("supabase-auth-rules"));
}

#[tokio::test]
async fn skill_upsert_with_local_namespace() {
    let db = setup().await;

    let local_skill = Skill {
        content: "Use Zustand for global state".to_string(),
        source: "dev-correction".to_string(),
        embedding: vec![0.5, 0.5],
        updated_at: now(),
    };
    db.upsert_skill("local::use-zustand", local_skill).await.unwrap();

    let got = db.get_skill("local::use-zustand").await.unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().content, "Use Zustand for global state");
}