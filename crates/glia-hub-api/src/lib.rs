//! glia-hub-api — GraphQL API layer for Glia Hub.
//!
//! Exposes a GraphQL schema (queries, mutations) and plain SSE endpoints
//! for log streaming and real-time dashboard events. Mounts onto Axum via
//! [`routes`].

/// JWT issuance, verification, and Axum extractors.
pub mod auth;
/// SSE broadcast of dashboard state-change events.
pub mod events;
/// async-graphql schema (types, queries, mutations).
pub mod schema;
/// SSE broadcast of log entries.
pub mod sse;

use axum::{
    Extension, Router,
    http::{HeaderMap, Method},
    response::{Html, IntoResponse},
    routing::get,
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// Build the Axum router with all GraphQL + SSE routes.
///
/// `jwt_secret` is shared across handlers via [`Extension`].
pub fn routes(
    jwt_secret: Arc<String>,
    bao: std::sync::Arc<dyn glia_bao::OpenBao>,
    catalog_source: std::sync::Arc<dyn glia_catalog::CatalogSource>,
) -> Router {
    let schema = schema::build_schema(&jwt_secret, bao, catalog_source);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
        ]);

    Router::new()
        .route("/graphql", get(graphql_playground).post(graphql_handler))
        .route("/api/logs", get(sse::log_stream_handler))
        .route("/api/events", get(events::events_stream_handler))
        .layer(cors)
        .layer(Extension(schema))
        .layer(Extension(jwt_secret))
}

/// `GET /graphql` — GraphiQL playground (development only).
async fn graphql_playground() -> impl IntoResponse {
    Html(
        async_graphql::http::GraphiQLSource::build()
            .endpoint("/graphql")
            .finish(),
    )
}

/// `POST /graphql` — Execute GraphQL queries and mutations.
async fn graphql_handler(
    Extension(schema): Extension<schema::Schema>,
    Extension(jwt_secret): Extension<std::sync::Arc<String>>,
    headers: HeaderMap,
    req: async_graphql_axum::GraphQLRequest,
) -> async_graphql_axum::GraphQLResponse {
    let mut request = req.into_inner();

    // Nested if chain — flattening obscures the dependency between Option
    // destructures and the `Bearer ` prefix check.
    #[allow(clippy::collapsible_if)]
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if let Ok(data) = decode::<auth::Claims>(
                    token,
                    &DecodingKey::from_secret(jwt_secret.as_bytes()),
                    &Validation::default(),
                ) {
                    request = request.data(data.claims);
                }
            }
        }
    }

    schema.execute(request).await.into()
}
