//! Bearer-token auth middleware for the Axum server.
//!
//! Security model (see `docs/design/05_security_isolation.md`):
//!
//! - When the server is bound to a loopback address (the default),
//!   the OS network stack guarantees every request originates on the
//!   same machine as the user. The browser UI and any local script
//!   can hit `/api/*` without a token.
//! - When the server is bound to a non-loopback address (Docker,
//!   `FYNANCE_HOST=0.0.0.0`), every `/api/*` request must present
//!   `Authorization: Bearer fyn_...`. Tokens are hashed and looked up
//!   in `api_tokens`.
//! - `/api/docs` and `/api/health` are always public so an agent can
//!   bootstrap and a dumb probe can confirm the server is up.
//! - Non-`/api` paths are the embedded React bundle and are always
//!   served (no credentials in the UI assets to leak).

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use super::state::AppState;

/// Request extension attached by the middleware so downstream handlers
/// can tell an anonymous caller from a token-authenticated one. Useful
/// for future per-token audit logging and rate limits.
#[derive(Debug, Clone)]
pub enum AuthContext {
    LoopbackAnonymous,
    Token { name: String },
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(rest.trim().to_string())
}

fn is_public_api(path: &str) -> bool {
    matches!(path, "/api/docs" | "/api/health")
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let is_api = path.starts_with("/api/");

    // Static assets fall through unconditionally.
    if !is_api {
        req.extensions_mut().insert(AuthContext::LoopbackAnonymous);
        return Ok(next.run(req).await);
    }

    // Public API routes accept any caller.
    if is_public_api(&path) {
        req.extensions_mut().insert(AuthContext::LoopbackAnonymous);
        return Ok(next.run(req).await);
    }

    // Loopback bind: the OS already proved the caller is local. Still
    // record a token name if the caller bothered to send one so audit
    // logs can attribute the request.
    if state.loopback_only {
        if let Some(token) = bearer_token(req.headers()) {
            let validated = {
                let db = state.db.lock().expect("db mutex poisoned");
                db.validate_token(&token)
            };
            if let Ok(Some(name)) = validated {
                req.extensions_mut().insert(AuthContext::Token { name });
                return Ok(next.run(req).await);
            }
        }
        req.extensions_mut().insert(AuthContext::LoopbackAnonymous);
        return Ok(next.run(req).await);
    }

    // Non-loopback bind: bearer token required for everything else.
    let Some(token) = bearer_token(req.headers()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let validated = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.validate_token(&token)
    };
    match validated {
        Ok(Some(name)) => {
            req.extensions_mut().insert(AuthContext::Token { name });
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
