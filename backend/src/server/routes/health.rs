//! `GET /api/health` — simple readiness probe.

use axum::Json;
use serde_json::{Value, json};

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "fynance" }))
}
