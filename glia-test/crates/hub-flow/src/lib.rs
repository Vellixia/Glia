//! Shared helpers for the hub-flow SPEC-driven test suite.
//!
//! Every test that touches OpenBao uses [`StubOpenBao`] (in-memory,
//! deterministic). The Hub code paths (graph routes, broadcast
//! channels, response-wrapping) are exercised end-to-end — only the
//! I/O adapters are stubbed.

/// Re-export frequently used test dependencies so test files can `use` them
/// from one place without pulling them in individually.
pub mod prelude {
    pub use std::collections::HashMap;
    pub use std::sync::Arc;
    pub use std::time::Duration;

    pub use glia_bao::{BaoError, DynamicCreds, OpenBao, Secret, StubOpenBao, TokenCache};
    pub use glia_cache::InMemoryCache;
}

/// Constant admin password for the dev-only stub login flow tests.
/// In `cli-flow`, this is read from `.env` via the test harness.
pub const DEV_ADMIN_PASSWORD: &str = "glia";
