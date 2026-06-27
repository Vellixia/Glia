//! E2E: Auth callback live HTTP tests.
//! AuthWaiter spawns a real localhost Axum server and we hit it with reqwest.

mod common;

use glia_auth::{AUTH_TIMEOUT, AuthError, AuthWaiter};
use std::time::Duration;

#[tokio::test]
async fn auth_callback_full_flow_live() {
    let waiter = AuthWaiter::new(0).await.unwrap();
    let port = waiter.addr().port();
    let w = std::sync::Arc::new(waiter);
    let wait_task = {
        let w = w.clone();
        tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
    };

    // Simulate OAuth redirect.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!(
        "http://127.0.0.1:{}/callback?code=e2e-live-code&state=e2e-live-state",
        port
    );
    let resp = reqwest::get(&url).await.unwrap();
    assert_eq!(resp.status(), 200);

    let auth = wait_task.await.unwrap().unwrap();
    assert_eq!(auth.code, "e2e-live-code");
    assert_eq!(auth.state, "e2e-live-state");
    w.shutdown().await;
}

#[tokio::test]
async fn auth_callback_timeout_live() {
    let waiter = AuthWaiter::new(0).await.unwrap();
    let w = std::sync::Arc::new(waiter);
    // Use a short timeout for the test.
    let result = w.wait_for_callback(Duration::from_millis(200)).await;
    assert!(matches!(result, Err(AuthError::Timeout(_))));
    w.shutdown().await;
}

#[tokio::test]
async fn auth_callback_oauth_error_live() {
    let waiter = AuthWaiter::new(0).await.unwrap();
    let port = waiter.addr().port();
    let w = std::sync::Arc::new(waiter);
    let wait_task = {
        let w = w.clone();
        tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
    };

    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!(
        "http://127.0.0.1:{}/callback?error=access_denied&error_description=user+cancelled",
        port
    );
    let resp = reqwest::get(&url).await.unwrap();
    assert_eq!(resp.status(), 400);

    let err = wait_task.await.unwrap().unwrap_err();
    assert!(matches!(err, AuthError::OAuthError(_)));
    w.shutdown().await;
}

#[tokio::test]
async fn auth_callback_missing_code_live() {
    let waiter = AuthWaiter::new(0).await.unwrap();
    let port = waiter.addr().port();
    let w = std::sync::Arc::new(waiter);
    let wait_task = {
        let w = w.clone();
        tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
    };

    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("http://127.0.0.1:{}/callback?state=only-state", port);
    let resp = reqwest::get(&url).await.unwrap();
    assert_eq!(resp.status(), 400);

    let err = wait_task.await.unwrap().unwrap_err();
    assert!(matches!(err, AuthError::MissingParam("code")));
    w.shutdown().await;
}

#[tokio::test]
async fn auth_multiple_waiters_independent_live() {
    let w1 = AuthWaiter::new(0).await.unwrap();
    let w2 = AuthWaiter::new(0).await.unwrap();
    let p1 = w1.addr().port();
    let p2 = w2.addr().port();
    assert_ne!(p1, p2);

    let w1 = std::sync::Arc::new(w1);
    let w2 = std::sync::Arc::new(w2);

    let t1 = {
        let w = w1.clone();
        tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
    };
    let t2 = {
        let w = w2.clone();
        tokio::spawn(async move { w.wait_for_callback(AUTH_TIMEOUT).await })
    };

    tokio::time::sleep(Duration::from_millis(50)).await;
    // Only send callback to w1.
    reqwest::get(&format!(
        "http://127.0.0.1:{}/callback?code=w1-code&state=s",
        p1
    ))
    .await
    .unwrap();

    let auth1 = t1.await.unwrap().unwrap();
    assert_eq!(auth1.code, "w1-code");

    // w2 should still be waiting — shut it down.
    w2.shutdown().await;
    drop(t2);
    w1.shutdown().await;
}
