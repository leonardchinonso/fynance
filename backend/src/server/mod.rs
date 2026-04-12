//! Axum HTTP server plumbing for `fynance serve`.

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_files;
pub mod validation;

use std::sync::{Arc, Mutex};

use axum::{Router, middleware, routing::{get, patch, post, put}};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::storage::Db;

pub use error::AppError;
pub use state::AppState;

/// Build the Axum router for `fynance serve`.
pub fn build_router(db: Arc<Mutex<Db>>, loopback_only: bool) -> Router {
    let state = AppState { db, loopback_only };

    let api_routes = Router::new()
        // ── Always-public ──────────────────────────────────────────────────
        .route("/docs", get(routes::docs::openapi_spec))
        .route("/health", get(routes::health::health))
        // ── Accounts ───────────────────────────────────────────────────────
        .route("/accounts", get(routes::accounts::list_accounts))
        .route("/accounts", post(routes::accounts::create_account))
        .route(
            "/accounts/:id/balance",
            patch(routes::accounts::set_account_balance),
        )
        // ── Profiles ───────────────────────────────────────────────────────
        .route("/profiles", get(routes::profiles::list_profiles))
        .route("/profiles", post(routes::profiles::create_profile))
        // ── Section mappings ───────────────────────────────────────────────
        .route("/sections", get(routes::sections::list_sections))
        .route("/sections", put(routes::sections::replace_sections))
        // ── Transactions ───────────────────────────────────────────────────
        .route("/transactions", get(routes::transactions::list_transactions))
        .route(
            "/transactions/by-category",
            get(routes::transactions::transactions_by_category),
        )
        .route(
            "/transactions/categories",
            get(routes::transactions::list_categories),
        )
        .route(
            "/transactions/accounts",
            get(routes::transactions::list_transaction_accounts),
        )
        .route(
            "/transactions/:id",
            patch(routes::transactions::patch_transaction),
        )
        // ── Import ─────────────────────────────────────────────────────────
        .route("/import", post(routes::import_api::import_json))
        .route("/import/csv", post(routes::import_api::import_csv))
        .route("/import/bulk", post(routes::import_api::import_bulk))
        // ── Budget ─────────────────────────────────────────────────────────
        .route(
            "/budget/spending-grid",
            get(routes::budget::get_spending_grid),
        )
        .route(
            "/budget/:month",
            get(routes::budget::get_budget_for_month),
        )
        .route("/budget", post(routes::budget::set_standing_budget))
        .route(
            "/budget/override",
            post(routes::budget::set_budget_override),
        )
        // ── Portfolio ──────────────────────────────────────────────────────
        .route("/portfolio", get(routes::portfolio::get_portfolio))
        .route(
            "/portfolio/history",
            get(routes::portfolio::get_portfolio_history),
        )
        .route(
            "/portfolio/balances",
            get(routes::portfolio::get_portfolio_balances),
        )
        // ── Cash flow ──────────────────────────────────────────────────────
        .route("/cash-flow", get(routes::portfolio::get_cash_flow))
        // ── Holdings ───────────────────────────────────────────────────────
        .route("/holdings", get(routes::holdings::list_holdings))
        .route(
            "/holdings/:account_id",
            post(routes::holdings::post_holdings),
        )
        // ── Ingestion checklist ────────────────────────────────────────────
        .route(
            "/ingestion/checklist/:month",
            get(routes::ingestion::get_checklist),
        )
        .route(
            "/ingestion/checklist/:month/:account_id",
            post(routes::ingestion::mark_complete),
        )
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
