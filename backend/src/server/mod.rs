//! Axum HTTP server plumbing for `fynance serve`.

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_files;
pub mod validation;

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, patch, post, put},
};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::storage::Db;

pub use error::AppError;
pub use state::AppState;

/// Build the Axum router for `fynance serve`.
pub fn build_router(db: Arc<Mutex<Db>>, loopback_only: bool) -> Router {
    let state = AppState {
        db,
        loopback_only,
        progress_channels: std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
    };

    let api_routes = Router::new()
        // ── Always-public ──────────────────────────────────────────────────
        .route("/docs", get(routes::docs::openapi_spec))
        .route("/health", get(routes::health::health))
        // ── Accounts ───────────────────────────────────────────────────────
        .route("/accounts", get(routes::accounts::list_accounts))
        .route("/accounts", post(routes::accounts::create_account))
        .route("/accounts/:id", patch(routes::accounts::update_account))
        .route("/accounts/:id", delete(routes::accounts::delete_account))
        .route(
            "/accounts/:id/balance",
            patch(routes::accounts::set_account_balance),
        )
        // ── Profiles ───────────────────────────────────────────────────────
        .route("/profiles", get(routes::profiles::list_profiles))
        .route("/profiles", post(routes::profiles::create_profile))
        .route("/profiles/:id", patch(routes::profiles::update_profile))
        .route("/profiles/:id", delete(routes::profiles::delete_profile))
        // ── Categories ─────────────────────────────────────────────────────
        .route("/categories", post(routes::categories::create_category))
        .route("/categories", get(routes::categories::list_categories))
        .route(
            "/categories/resolve",
            get(routes::categories::resolve_category),
        )
        .route("/categories/:id", get(routes::categories::get_category))
        .route(
            "/categories/:id",
            patch(routes::categories::update_category),
        )
        .route(
            "/categories/:id",
            delete(routes::categories::delete_category),
        )
        // ── Transactions ───────────────────────────────────────────────────
        .route(
            "/transactions",
            get(routes::transactions::list_transactions)
                .patch(routes::transactions::bulk_patch_transactions)
                .delete(routes::transactions::bulk_delete_transactions),
        )
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
            "/transactions/import",
            post(routes::import_api::import_json),
        )
        .route(
            "/transactions/:id",
            patch(routes::transactions::patch_transaction)
                .delete(routes::transactions::delete_transaction),
        )
        // ── Import ────��─────────────────────────────���──────────────────────
        // `/api/import` is deprecated; new callers should use
        // `/api/transactions/import` (registered above). The legacy route
        // adds a `Deprecation` / `Link` header pointing at the successor.
        .route("/import", post(routes::import_api::import_json_legacy))
        .route("/import/csv", post(routes::import_api::import_csv))
        .route("/import/bulk", post(routes::import_api::import_bulk))
        // ── Parse (Stage 1) ───────────────────────────────────────────────
        // The route enforces its own size caps (50 MB total, 10 MB per
        // file) inside the handler. Lift Axum's 2 MB default so multi-PDF
        // uploads aren't rejected at the body layer.
        .route(
            "/parse",
            post(routes::parse::parse_documents).layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route(
            "/parse/progress/:parse_id",
            get(routes::parse::parse_progress),
        )
        // ── Documents (source-file storage & provenance) ───────────────────
        .route(
            "/documents",
            get(routes::documents::list_documents)
                .post(routes::documents::upload_document)
                .layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route(
            "/documents/:id",
            get(routes::documents::get_document).delete(routes::documents::delete_document),
        )
        .route(
            "/documents/:id/download",
            get(routes::documents::download_document),
        )
        // ── Budget ─────────────────────────────────────────────────────────
        .route(
            "/budget/spending-grid",
            get(routes::budget::get_spending_grid),
        )
        .route(
            "/budget/cash-summary",
            get(routes::budget::get_cash_summary),
        )
        .route("/budget/:month", get(routes::budget::get_budget_for_month))
        .route("/budget", post(routes::budget::set_standing_budget))
        .route(
            "/budget/override",
            post(routes::budget::set_budget_override),
        )
        // ── Holdings ───────────────────────────────────────────────────────
        .route("/holdings", get(routes::holdings::list_holdings))
        .route(
            "/holdings/summary",
            get(routes::holdings::get_holdings_summary),
        )
        .route(
            "/holdings/history",
            get(routes::holdings::get_holdings_history),
        )
        .route(
            "/holdings/account-history",
            get(routes::holdings::get_account_holdings_history),
        )
        .route(
            "/holdings/balances",
            get(routes::holdings::get_holdings_balances),
        )
        .route(
            "/holdings/cash-flow",
            get(routes::holdings::get_holdings_cash_flow),
        )
        .route("/holdings/import", post(routes::holdings::import_holdings))
        .route(
            "/holdings/:account_id",
            post(routes::holdings::post_holdings),
        )
        .route(
            "/holdings/:account_id/:symbol",
            get(routes::holdings::get_holding_history)
                .patch(routes::holdings::patch_holding)
                .delete(routes::holdings::delete_holding_handler),
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
        // ── Currencies ────────────────────────────────────────────────────────
        .route("/currencies", get(routes::currencies::list_currencies))
        .route("/currencies", post(routes::currencies::create_currency))
        .route(
            "/currencies/:code",
            patch(routes::currencies::update_currency),
        )
        .route(
            "/currencies/:code",
            delete(routes::currencies::delete_currency),
        )
        // ── Exchange rates (date-keyed, user-owned) ───────────────────────────
        .route(
            "/exchange-rates",
            get(routes::exchange_rates::list_exchange_rates),
        )
        .route(
            "/exchange-rates",
            post(routes::exchange_rates::create_exchange_rates),
        )
        .route(
            "/exchange-rates/:base/:quote/:date",
            delete(routes::exchange_rates::delete_exchange_rate),
        )
        // ── Investments ───────────────────────────────────────────────────────
        .route("/investments", get(routes::investments::list_investments))
        .route("/investments", post(routes::investments::create_investment))
        .route(
            "/investments/import",
            post(routes::investments::import_investments),
        )
        .route(
            "/investments/history",
            get(routes::investments::get_investment_history),
        )
        .route(
            "/investments/pools",
            get(routes::capital_gains::get_s104_pools),
        )
        .route(
            "/investments/capital-gains",
            get(routes::capital_gains::get_capital_gains),
        )
        .route("/tax-config", get(routes::tax::get_tax_config))
        .route("/tax-config", put(routes::tax::put_tax_config))
        .route(
            "/tax-inputs/:profile_id/:tax_year",
            get(routes::tax::get_tax_inputs),
        )
        .route(
            "/tax-inputs/:profile_id/:tax_year",
            put(routes::tax::put_tax_inputs),
        )
        .route(
            "/investments/:id",
            patch(routes::investments::update_investment),
        )
        .route(
            "/investments/:id",
            delete(routes::investments::delete_investment),
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
        // Outermost: a panicking handler becomes a 500 instead of a dropped
        // connection (the db mutex guard recovers from the poisoning).
        .layer(CatchPanicLayer::new())
        .with_state(state)
}
