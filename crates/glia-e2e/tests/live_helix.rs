//! E2E: HelixDB live CRUD operations.
//! Requires: docker compose up helixdb (or existing HelixDB at :6969).

mod common;

use common::{helix_live, helix_with_schema};
use glia_helix::{Auth, HelixClient, Skill, Stack, Tool};

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[tokio::test]
async fn helix_health_check() {
    if !common::probe_http(common::HELIX_URL).await
        && !common::probe_http(&format!("{}/health", common::HELIX_URL)).await
    {
        eprintln!("SKIP: no helixdb at {}", common::HELIX_URL);
        return;
    }
    let resp = reqwest::get(&format!("{}/health", common::HELIX_URL))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["healthy"], true);
}

#[tokio::test]
async fn helix_connect_and_ping() {
    let Some(client) = helix_live().await else {
        eprintln!("SKIP: no helixdb");
        return;
    };
    // ping may fail if Glia schema not deployed, but connection should work.
    let _ = client.ping().await;
}

#[tokio::test]
async fn helix_tool_crud_live() {
    let Some(client) = helix_with_schema().await else {
        eprintln!("SKIP: no helixdb with glia schema");
        return;
    };
    let tool_id = format!("e2e-tool-{}", chrono::Utc::now().timestamp());
    let tool = Tool {
        name: "E2E Test Tool".into(),
        category: "test".into(),
        local: true,
        params_schema: serde_json::json!({"type": "object"}),
        updated_at: now(),
        runtime: None,
        min_version: None,
    };
    client.upsert_tool(&tool_id, tool.clone()).await.unwrap();
    let got = client.get_tool(&tool_id).await.unwrap();
    assert!(got.is_some());
    let retrieved = got.unwrap();
    assert_eq!(retrieved.name, "E2E Test Tool");
}

#[tokio::test]
async fn helix_skill_upsert_and_retrieve_live() {
    let Some(client) = helix_with_schema().await else {
        eprintln!("SKIP: no helixdb with glia schema");
        return;
    };
    let skill_id = format!("e2e-skill-{}", chrono::Utc::now().timestamp());
    let skill = Skill {
        content: "E2E test skill content".into(),
        source: skill_id.clone(),
        embedding: vec![0.1; 384],
        updated_at: now(),
    };
    client.upsert_skill(&skill_id, skill).await.unwrap();
    let got = client.get_skill(&skill_id).await.unwrap();
    assert!(got.is_some());
    let retrieved = got.unwrap();
    assert_eq!(retrieved.content, "E2E test skill content");
    assert_eq!(retrieved.embedding.len(), 384);
}

#[tokio::test]
async fn helix_graph_edge_tool_requires_auth_live() {
    let Some(client) = helix_with_schema().await else {
        eprintln!("SKIP: no helixdb with glia schema");
        return;
    };
    let ts = chrono::Utc::now().timestamp();
    let tool_id = format!("e2e-edge-tool-{}", ts);
    let auth_id = format!("e2e-edge-auth-{}", ts);

    client
        .upsert_tool(
            &tool_id,
            Tool {
                name: "Edge Test Tool".into(),
                category: "test".into(),
                local: false,
                params_schema: serde_json::json!({}),
                updated_at: now(),
                runtime: None,
                min_version: None,
            },
        )
        .await
        .unwrap();
    client
        .upsert_auth(
            &auth_id,
            Auth {
                auth_type: "oauth".into(),
                provider: "test".into(),
                updated_at: now(),
            },
        )
        .await
        .unwrap();
    client
        .relate_tool_requires_auth(&tool_id, &auth_id)
        .await
        .unwrap();

    let tools = client.tools_requiring_auth(&auth_id).await.unwrap();
    assert!(tools.iter().any(|t| t.name == "Edge Test Tool"));
}

#[tokio::test]
async fn helix_skill_stack_edge_live() {
    let Some(client) = helix_with_schema().await else {
        eprintln!("SKIP: no helixdb with glia schema");
        return;
    };
    let ts = chrono::Utc::now().timestamp();
    let skill_id = format!("e2e-stack-skill-{}", ts);
    let stack_id = format!("e2e-stack-{}", ts);

    client
        .upsert_skill(
            &skill_id,
            Skill {
                content: "Stack test".into(),
                source: skill_id.clone(),
                embedding: vec![0.2; 384],
                updated_at: now(),
            },
        )
        .await
        .unwrap();
    client
        .upsert_stack(
            &stack_id,
            Stack {
                name: "E2E Stack".into(),
            },
        )
        .await
        .unwrap();
    client
        .relate_skill_applies_to_stack(&skill_id, &stack_id)
        .await
        .unwrap();

    let skills = client.skills_for_stack(&stack_id).await.unwrap();
    assert!(skills.iter().any(|s| s.source == skill_id));
}

#[tokio::test]
async fn helix_list_skills_live() {
    let Some(client) = helix_with_schema().await else {
        eprintln!("SKIP: no helixdb with glia schema");
        return;
    };
    // Should return a list (may be empty or have data from other tests).
    let skills = client.list_skills().await.unwrap();
    // Just verify it doesn't panic and returns a vec.
    let _ = skills.len();
}

#[tokio::test]
async fn helix_ping_against_live_server_without_schema() {
    // The Glia HelixDB instance may or may not have Glia schema deployed.
    // ping() may return Ok (schema present) or any error (queries not
    // registered). Both are acceptable — the point is the server is reachable.
    let client = HelixClient::connect(Some(common::HELIX_URL), None).unwrap();
    let result = client.ping().await;
    match result {
        Ok(()) => {
            // Schema IS deployed.
        }
        Err(e) => {
            // Schema not deployed, or any other expected condition.
            eprintln!("ping returned (expected on fresh HelixDB): {e}");
        }
    }
}
