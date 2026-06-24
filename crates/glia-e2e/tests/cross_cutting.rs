//! Cross-cutting unicode + concurrency tests for crates available in glia-e2e.
//! Tests for other crates live in their own integration test files.

use std::sync::Arc;
use std::time::Duration;

#[test]
fn unicode_cache_key_roundtrip() {
    use glia_cache::{Cache, InMemoryCache};
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let c = InMemoryCache::new();
        let keys = [
            "oauth::creds::héllo::access",
            "synth::result::日本語-🐍",
            "auth::user::ünïcödë::creds",
        ];
        for key in keys {
            c.put_bytes(key, b"unicode-val", Duration::from_secs(60))
                .await
                .unwrap();
            let got = c.get_bytes(key).await.unwrap();
            assert!(got.is_some(), "key {key} not found");
            assert_eq!(got.unwrap(), b"unicode-val");
        }
    });
}

#[test]
fn unicode_helix_skill_id() {
    use glia_helix::HelixClient;
    // local:: prefix is matched via starts_with — unicode after the
    // prefix should still be detected as local.
    assert!(HelixClient::is_local_skill("local::héllo"));
    assert!(HelixClient::is_local_skill("local::日本語"));
}

#[test]
fn concurrent_cache_set_get() {
    use glia_cache::{Cache, InMemoryCache};
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let c = Arc::new(InMemoryCache::new());
        let mut handles = Vec::new();
        for i in 0..50 {
            let cc = c.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("conc-key-{i}");
                cc.put_bytes(&key, format!("v{i}").as_bytes(), Duration::from_secs(60))
                    .await
                    .unwrap();
                let got = cc.get_bytes(&key).await.unwrap();
                assert!(got.is_some());
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    });
}

#[test]
fn concurrent_embed_same_text() {
    use glia_embed::Embedder;
    let emb = match Embedder::try_new() {
        Some(e) => Arc::new(e),
        None => return,
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut handles = Vec::new();
        for _ in 0..10 {
            let e = emb.clone();
            handles.push(tokio::spawn(async move {
                e.embed("concurrent embed test").unwrap()
            }));
        }
        let first = handles.remove(0).await.unwrap();
        for h in handles {
            let v = h.await.unwrap();
            assert_eq!(v.len(), first.len());
            for (a, b) in v.iter().zip(first.iter()) {
                assert!((a - b).abs() < 1e-6, "embeddings differ");
            }
        }
    });
}

#[test]
fn concurrent_auth_waiters_independent() {
    use glia_auth::AuthWaiter;
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Set up waiters + wait tasks FIRST, then send callbacks.
        let mut wait_tasks = Vec::new();
        let mut urls = Vec::new();
        for i in 0..3 {
            let w = AuthWaiter::new(0).await.unwrap();
            let port = w.addr().port();
            let url = format!("http://127.0.0.1:{port}/callback?code=c{i}&state=s");
            let w_clone = std::sync::Arc::new(w);
            let w_for_wait = w_clone.clone();
            // Start waiting FIRST so the tx is set up.
            let wait_task = tokio::spawn(async move {
                w_for_wait.wait_for_callback(Duration::from_secs(10)).await
            });
            wait_tasks.push(wait_task);
            urls.push(url);
            // Hold the Arc to prevent shutdown.
            std::mem::forget(w_clone);
        }

        // Give waiters time to set up the tx channel.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Now send all callbacks concurrently.
        let mut send_tasks = Vec::new();
        for url in urls {
            send_tasks.push(tokio::spawn(async move {
                let resp = reqwest::get(&url).await.unwrap();
                assert_eq!(resp.status(), 200);
            }));
        }
        for t in send_tasks {
            t.await.unwrap();
        }
        for t in wait_tasks {
            let auth = t.await.unwrap().unwrap();
            assert!(auth.code.starts_with("c"));
        }
    });
}