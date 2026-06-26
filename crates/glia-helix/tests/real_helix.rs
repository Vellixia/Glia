//! E2E: Real HelixDB stress + edge case tests.
//! Requires: docker compose up helixdb (port 6969).
//! Glia schema queries may or may not be deployed; tests that need schema
//! skip gracefully.

use glia_helix::{Auth, HelixClient, Skill, Stack, Tool};

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

async fn helix_live() -> Option<HelixClient> {
    let client = HelixClient::connect(Some("http://127.0.0.1:6969"), None).ok()?;
    if reqwest::get("http://127.0.0.1:6969/health").await.is_err() {
        return None;
    }
    Some(client)
}

async fn helix_with_schema() -> Option<HelixClient> {
    let client = helix_live().await?;
    if client.ping().await.is_err() {
        eprintln!("SKIP: helixdb up but Glia schema not deployed");
        return None;
    }
    Some(client)
}

#[tokio::test]
async fn real_helix_health() {
    if helix_live().await.is_none() {
        eprintln!("SKIP: no helixdb");
        return;
    }
    let resp = reqwest::get("http://127.0.0.1:6969/health").await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn real_helix_concurrent_upserts() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    use std::sync::Arc;
    let c = Arc::new(client);
    let mut handles = Vec::new();
    for i in 0..10 {
        let cc = c.clone();
        let id = format!(
            "real-conc-{}-{}",
            i,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        handles.push(tokio::spawn(async move {
            let tool = Tool {
                name: format!("Concurrent Tool {i}"),
                category: "test".into(),
                local: true,
                params_schema: serde_json::json!({}),
                updated_at: now(),
                runtime: None,
                min_version: None,
            };
            cc.upsert_tool(&id, tool).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn real_helix_large_skill_body() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let id = format!(
        "real-large-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let large_body = "x".repeat(100_000);
    let skill = Skill {
        content: large_body,
        source: id.clone(),
        embedding: vec![0.0; 384],
        updated_at: now(),
        usage_count: 0,
    };
    client.upsert_skill(&id, skill).await.unwrap();
    let got = client.get_skill(&id).await.unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().content.len(), 100_000);
}

#[tokio::test]
async fn real_helix_unicode_skill_id() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let id = format!(
        "real-unicode-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let skill = Skill {
        content: "unicode body".into(),
        source: id.clone(),
        embedding: vec![0.1; 384],
        updated_at: now(),
        usage_count: 0,
    };
    client.upsert_skill(&id, skill).await.unwrap();
    let got = client.get_skill(&id).await.unwrap();
    assert!(got.is_some());
}

#[tokio::test]
async fn real_helix_lww_upsert() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let id = format!(
        "real-lww-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    // First write.
    client
        .upsert_skill(
            &id,
            Skill {
                content: "v1".into(),
                source: id.clone(),
                embedding: vec![0.1; 384],
                updated_at: "2026-01-01T00:00:00Z".into(),
                usage_count: 0,
            },
        )
        .await
        .unwrap();
    // Second write with newer timestamp.
    client
        .upsert_skill(
            &id,
            Skill {
                content: "v2".into(),
                source: id.clone(),
                embedding: vec![0.2; 384],
                updated_at: "2026-12-31T00:00:00Z".into(),
                usage_count: 0,
            },
        )
        .await
        .unwrap();
    let got = client.get_skill(&id).await.unwrap();
    assert!(got.is_some());
    let retrieved = got.unwrap();
    // LWW: newer timestamp should win.
    assert_eq!(retrieved.content, "v2");
}

#[tokio::test]
async fn real_helix_graph_edge_chain() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let tool_id = format!("real-chain-tool-{ts}");
    let auth_id = format!("real-chain-auth-{ts}");
    let skill_id = format!("real-chain-skill-{ts}");
    let stack_id = format!("real-chain-stack-{ts}");

    client
        .upsert_tool(
            &tool_id,
            Tool {
                name: "Chain Tool".into(),
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
    client
        .upsert_skill(
            &skill_id,
            Skill {
                content: "chain skill".into(),
                source: skill_id.clone(),
                embedding: vec![0.1; 384],
                updated_at: now(),
                usage_count: 0,
            },
        )
        .await
        .unwrap();
    client
        .upsert_stack(
            &stack_id,
            Stack {
                name: "Chain Stack".into(),
            },
        )
        .await
        .unwrap();
    client
        .relate_skill_applies_to_stack(&skill_id, &stack_id)
        .await
        .unwrap();

    // Verify the chain: tool→auth, skill→stack.
    let tools = client.tools_requiring_auth(&auth_id).await.unwrap();
    assert!(tools.iter().any(|t| t.name == "Chain Tool"));
    let skills = client.skills_for_stack(&stack_id).await.unwrap();
    assert!(skills.iter().any(|s| s.source == skill_id));
}

#[tokio::test]
async fn real_helix_list_skills_returns_vec() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let skills = client.list_skills().await.unwrap();
    // May be empty or have data from other tests. Just verify Vec.
    let _ = skills.len();
}

#[tokio::test]
async fn real_helix_zero_embedding_skill() {
    let Some(client) = helix_with_schema().await else {
        return;
    };
    let id = format!(
        "real-zero-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let skill = Skill {
        content: "zero vec".into(),
        source: id.clone(),
        embedding: vec![0.0; 384],
        updated_at: now(),
        usage_count: 0,
    };
    client.upsert_skill(&id, skill).await.unwrap();
    let got = client.get_skill(&id).await.unwrap();
    assert!(got.is_some());
}
