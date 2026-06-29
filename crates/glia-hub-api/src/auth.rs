use argon2::PasswordVerifier;
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};

use crate::schema::LoginPayload;

/// JWT claims payload.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Claims {
    /// Subject (always "admin" for single-user Hub).
    pub sub: String,
    /// Expiration (UTC epoch seconds).
    pub exp: u64,
    /// Issued-at (UTC epoch seconds).
    pub iat: u64,
}

/// Axum extractor — injects the authenticated user's subject string.
///
/// Usage in handlers: `AuthUser(user_id): AuthUser`
#[derive(Debug, Clone)]
pub struct AuthUser(pub String);

/// Rejection type for failed auth extraction.
#[derive(Debug)]
pub struct AuthError(pub StatusCode);

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        (self.0, "unauthorized").into_response()
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or(AuthError(StatusCode::UNAUTHORIZED))?;
        let secret = parts
            .extensions
            .get::<std::sync::Arc<String>>()
            .ok_or(AuthError(StatusCode::INTERNAL_SERVER_ERROR))?;

        decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map(|d| AuthUser(d.claims.sub))
        .map_err(|_| AuthError(StatusCode::UNAUTHORIZED))
    }
}

/// SSE-compatible auth extractor.
///
/// The browser's `EventSource` API cannot set custom request headers, so
/// this extractor accepts the JWT from EITHER:
///   1. `Authorization: Bearer <jwt>` header (proxy scenario, curl, etc.)
///   2. `?token=<jwt>` query parameter (raw `EventSource` clients)
///
/// The token is still fully validated against the JWT secret — this is
/// defense-in-depth, not a trust-the-client shortcut.
#[derive(Debug, Clone)]
pub struct AuthUserSse(pub String);

impl<S> FromRequestParts<S> for AuthUserSse
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)
            .map(String::from)
            .or_else(|| query_token(parts))
            .ok_or(AuthError(StatusCode::UNAUTHORIZED))?;

        let secret = parts
            .extensions
            .get::<std::sync::Arc<String>>()
            .ok_or(AuthError(StatusCode::INTERNAL_SERVER_ERROR))?;

        decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map(|d| AuthUserSse(d.claims.sub))
        .map_err(|_| AuthError(StatusCode::UNAUTHORIZED))
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

fn query_token(parts: &Parts) -> Option<String> {
    let query = parts.uri.query()?;
    for pair in query.split('&') {
        // The nested if is fine here — flattening with `&&` mixes the
        // `Option` destructure with a scalar comparison.
        #[allow(clippy::collapsible_if)]
        if let Some((k, v)) = pair.split_once('=') {
            if k == "token" {
                return Some(url_decode(v));
            }
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Same reason: the percent-decode branch has an inner Option
        // destructure that combines poorly with the outer guard.
        #[allow(clippy::collapsible_if)]
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(((h << 4) | l) as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Create a signed JWT for the admin user.
pub fn create_token(secret: &str) -> anyhow::Result<LoginPayload> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let expires = now + 3600; // 1 hour

    let claims = Claims {
        sub: "admin".into(),
        exp: expires,
        iat: now,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    let expires_at = chrono::DateTime::from_timestamp(expires as i64, 0)
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

    Ok(LoginPayload { token, expires_at })
}

/// Verify the admin password against the stored hash and return a JWT.
pub fn verify_and_create_token(password: &str) -> anyhow::Result<LoginPayload> {
    let secret = std::env::var("GLIA_JWT_SECRET")?;
    let admin_hash = std::env::var("GLIA_ADMIN_HASH")?;

    let parsed_hash = argon2::password_hash::PasswordHash::new(&admin_hash)
        .map_err(|e| anyhow::anyhow!("invalid admin hash format: {e}"))?;

    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| anyhow::anyhow!("invalid password"))?;

    create_token(&secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_verify_token_roundtrip() {
        let secret = "test-secret-key-for-unit-tests";
        let payload = create_token(secret).expect("create_token should succeed");

        let decoded = decode::<Claims>(
            &payload.token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .expect("decode should succeed");

        assert_eq!(decoded.claims.sub, "admin");
    }
}
