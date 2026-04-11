//! Axum HTTP server plumbing for `fynance serve`.
//!
//! Phase 2 wires up the router, shared state, middleware, static-file
//! fallback, and the agent-readable OpenAPI doc endpoint. Real data
//! endpoints (transactions, budget, portfolio) arrive in Phase 3+ and
//! plug into the router built here.

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_files;

use std::sync::{Arc, Mutex};

use axum::{Router, middleware, routing::get};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::storage::Db;

pub use error::AppError;
pub use state::AppState;

/// Build the Axum router for `fynance serve`.
///
/// The router layers a permissive CORS policy (we only ever bind to
/// loopback, so CORS is not a real security boundary here) and a
/// request-scoped auth middleware that lets browser traffic through
/// without a token but requires a bearer token for any non-loopback
/// hit. Anything that does not match an `/api/*` route falls through to
/// the embedded frontend bundle.
pub fn build_router(db: Arc<Mutex<Db>>, loopback_only: bool) -> Router {
    let state = AppState { db, loopback_only };

    let api_routes = Router::new()
        .route("/docs", get(routes::docs::openapi_spec))
        .route("/health", get(routes::health::health))
        .with_state(state.clone());

    Router::new()
        .nest("/api", api_routes)
        .fallback(static_files::serve_static)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
