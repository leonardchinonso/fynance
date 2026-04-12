//! `GET /api/docs` — hand-crafted OpenAPI 3.1 spec.
//!
//! This endpoint is the self-describing contract external AI agents
//! use to discover the API without any out-of-band documentation. The
//! spec intentionally embeds the full category taxonomy from
//! `backend/config/categories.yaml` so an agent can categorize new
//! transactions with zero extra fetches. Phases 3+ will extend this
//! with concrete request/response schemas as routes land; the shape
//! defined here is forward-compatible.

use std::sync::OnceLock;

use axum::Json;
use serde_json::{Value, json};

use crate::server::error::AppError;

/// Parsed at first request, cached forever: reading YAML on every hit
/// would be wasteful given the file is baked into the binary.
static CATEGORIES_JSON: OnceLock<Value> = OnceLock::new();

const CATEGORIES_YAML: &str = include_str!("../../../config/categories.yaml");

fn categories_json() -> &'static Value {
    CATEGORIES_JSON.get_or_init(|| {
        // If parsing fails we still want the docs endpoint to respond,
        // so fall back to an empty object instead of panicking.
        serde_yaml::from_str::<Value>(CATEGORIES_YAML).unwrap_or_else(|err| {
            tracing::warn!(?err, "failed to parse categories.yaml for /api/docs");
            json!({})
        })
    })
}

pub async fn openapi_spec() -> Result<Json<Value>, AppError> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "fynance API",
            "version": "0.1.0",
            "description": concat!(
                "Local-first personal finance tracker. All routes live under `/api`. ",
                "Browser requests from `127.0.0.1` need no auth. Programmatic clients ",
                "(scripts, agents) must supply `Authorization: Bearer fyn_...`. ",
                "Tokens are generated via `fynance token create`.",
            ),
        },
        "servers": [
            { "url": "http://localhost:7433", "description": "default local instance" }
        ],
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "fyn_<hex>",
                    "description": "API token created via `fynance token create --name <name>`."
                }
            },
            "schemas": {
                "Transaction": {
                    "type": "object",
                    "required": ["id", "date", "description", "amount", "currency", "account_id"],
                    "properties": {
                        "id": { "type": "string" },
                        "date": { "type": "string", "format": "date" },
                        "description": { "type": "string" },
                        "normalized": { "type": "string" },
                        "amount": {
                            "type": "string",
                            "description": "Decimal as string. Negative = money out, positive = money in."
                        },
                        "currency": { "type": "string", "example": "GBP" },
                        "account_id": { "type": "string" },
                        "category": { "type": ["string", "null"] },
                        "category_source": {
                            "type": ["string", "null"],
                            "enum": ["rule", "agent", "manual", null],
                            "description": concat!(
                                "Where the category was assigned. ",
                                "`rule`: matched a config rule during CSV import. ",
                                "`agent`: set by an external AI agent via the API. ",
                                "`manual`: user-edited in the UI or CLI."
                            )
                        },
                        "confidence": { "type": ["number", "null"] },
                        "notes": { "type": ["string", "null"] },
                        "is_recurring": { "type": "boolean" }
                    }
                },
                "ImportTransaction": {
                    "type": "object",
                    "required": ["date", "description", "amount"],
                    "properties": {
                        "date": { "type": "string", "format": "date" },
                        "description": { "type": "string" },
                        "amount": { "type": "string", "description": "Decimal string, signed." },
                        "currency": { "type": "string", "default": "GBP" },
                        "category": { "type": ["string", "null"] },
                        "category_source": {
                            "type": "string",
                            "enum": ["rule", "agent", "manual"],
                            "default": "agent"
                        }
                    }
                },
                "ImportResult": {
                    "type": "object",
                    "properties": {
                        "rows_total": { "type": "integer" },
                        "rows_inserted": { "type": "integer" },
                        "rows_duplicate": { "type": "integer" },
                        "filename": { "type": "string" },
                        "account_id": { "type": "string" }
                    }
                },
                "Error": {
                    "type": "object",
                    "properties": {
                        "error": { "type": "string" },
                        "code": { "type": "string" }
                    }
                }
            }
        },
        "paths": {
            "/api/health": {
                "get": {
                    "summary": "Readiness probe",
                    "responses": { "200": { "description": "Server is up" } }
                }
            },
            "/api/docs": {
                "get": {
                    "summary": "This OpenAPI spec",
                    "responses": { "200": { "description": "OpenAPI 3.1 document" } }
                }
            },
            "/api/transactions": {
                "get": {
                    "summary": "List transactions (Phase 3)",
                    "parameters": [
                        { "name": "month", "in": "query", "schema": { "type": "string", "example": "2026-04" } },
                        { "name": "category", "in": "query", "schema": { "type": "string" } },
                        { "name": "account_id", "in": "query", "schema": { "type": "string" } },
                        { "name": "page", "in": "query", "schema": { "type": "integer", "default": 1 } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50 } }
                    ],
                    "responses": { "200": { "description": "Paginated transaction list" } }
                }
            },
            "/api/import": {
                "post": {
                    "summary": "Programmatic bulk import (Phase 3)",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["account_id", "transactions"],
                                    "properties": {
                                        "account_id": { "type": "string" },
                                        "transactions": {
                                            "type": "array",
                                            "items": { "$ref": "#/components/schemas/ImportTransaction" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Import summary",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ImportResult" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "x-fynance": {
            "categories": categories_json(),
            "category_sources": {
                "rule": "Assigned by a regex rule in config/rules.yaml during CSV import.",
                "agent": "Assigned by an external AI agent pushing via /api/import.",
                "manual": "Set by the end user through the UI or CLI."
            }
        }
    });
    Ok(Json(spec))
}
