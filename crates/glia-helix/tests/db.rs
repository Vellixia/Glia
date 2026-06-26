//! Integration tests for glia-helix. Hit a live HelixDB instance on
//! `localhost:6969` if available; skip otherwise (CI cold cache, asset-less
//! runs).

use chrono::Utc;
use glia_helix::{Auth, HelixClient, Skill, Stack, Tool};

fn now() -> String {
    Utc::now().to_rfc3339()
}

async fn try_setup() -> Option<HelixClient> {
    let client = HelixClient::connect(None, None).ok()?;
    match client.ping().await {
        Ok(()) => Some(client),
        Err(_) => None,
    }
}

#[tokio::test]
async fn embedded_style_crud() {
    let Some(db) = try_setup().await else {
        eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
        return;
    };

    let tool_id = format!(
        "test-tool-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let tool = Tool {
        name: "Create Linear Issue".to_string(),
        category: "issue-tracker".to_string(),
        local: false,
        params_schema: serde_json::json!({"title": "string"}),
        updated_at: now(),
        runtime: None,
        min_version: None,
    };
    db.upsert_tool(&tool_id, tool.clone()).await.unwrap();

    let got = db.get_tool(&tool_id).await.unwrap();
    assert!(got.is_some(), "tool {tool_id} should exist");
    assert_eq!(got.unwrap().name, "Create Linear Issue");
}

#[tokio::test]
async fn graph_edge_requires() {
    let Some(db) = try_setup().await else {
        eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
        return;
    };

    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let tool_id = format!("test-rel-tool-{ts}");
    let auth_id = format!("test-rel-auth-{ts}");

    db.upsert_tool(
        &tool_id,
        Tool {
            name: "Create Issue".to_string(),
            category: "issue-tracker".to_string(),
            local: false,
            params_schema: serde_json::json!({}),
            updated_at: now(),
            runtime: None,
            min_version: None,
        },
    )
    .await
    .unwrap();

    db.upsert_auth(
        &auth_id,
        Auth {
            auth_type: "oauth".to_string(),
            provider: "linear".to_string(),
            updated_at: now(),
        },
    )
    .await
    .unwrap();

    db.relate_tool_requires_auth(&tool_id, &auth_id)
        .await
        .unwrap();

    let tools = db.tools_requiring_auth(&auth_id).await.unwrap();
    assert!(
        tools.iter().any(|t| t.name == "Create Issue"),
        "should find tool requiring this auth"
    );
}

#[tokio::test]
async fn graph_edge_applies_to() {
    let Some(db) = try_setup().await else {
        eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
        return;
    };

    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let skill_id = format!("test-skill-{ts}");
    let stack_id = format!("test-stack-{ts}");

    db.upsert_skill(
        &skill_id,
        Skill {
            content: "Never use service_role in route handlers.".to_string(),
            source: "supabase-auth.md".to_string(),
            embedding: vec![0.1, 0.2, 0.3],
            updated_at: now(),

            usage_count: 0,
        },
    )
    .await
    .unwrap();

    db.upsert_stack(
        &stack_id,
        Stack {
            name: "Next.js".to_string(),
        },
    )
    .await
    .unwrap();

    db.relate_skill_applies_to_stack(&skill_id, &stack_id)
        .await
        .unwrap();

    let skills = db.skills_for_stack(&stack_id).await.unwrap();
    assert!(
        skills.iter().any(|s| s.content.contains("service_role")),
        "should find skill for stack"
    );
}

#[tokio::test]
async fn local_namespace_detection() {
    assert!(HelixClient::is_local_skill("local::use-zustand"));
    assert!(!HelixClient::is_local_skill("supabase-auth-rules"));
}

#[tokio::test]
async fn skill_upsert_with_local_namespace() {
    let Some(db) = try_setup().await else {
        eprintln!("SKIP: no helixdb at http://127.0.0.1:6969");
        return;
    };

    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let skill_id = format!("local::test-skill-{ts}");

    db.upsert_skill(
        &skill_id,
        Skill {
            content: "Use Zustand for global state".to_string(),
            source: "dev-correction".to_string(),
            embedding: vec![0.5, 0.5],
            updated_at: now(),

            usage_count: 0,
        },
    )
    .await
    .unwrap();

    let got = db.get_skill(&skill_id).await.unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().content, "Use Zustand for global state");
}
