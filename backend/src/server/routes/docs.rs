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
                },
                "S104PoolState": {
                    "type": "object",
                    "required": ["symbol", "current_shares", "total_allowable_expenditure", "average_cost_per_share"],
                    "properties": {
                        "symbol": { "type": "string" },
                        "current_shares": { "type": "string", "description": "Decimal as string." },
                        "total_allowable_expenditure": { "type": "string", "description": "Decimal as string, in the user's preferred (base) currency." },
                        "average_cost_per_share": { "type": "string", "description": "Decimal as string, in the user's preferred (base) currency." }
                    }
                },
                "CgtSummary": {
                    "type": "object",
                    "required": ["total_proceeds", "total_allowable_costs", "total_gains", "total_losses", "net_gain_loss", "base_currency"],
                    "properties": {
                        "total_proceeds": { "type": "string", "description": "Decimal as string, in base_currency." },
                        "total_allowable_costs": { "type": "string" },
                        "total_gains": { "type": "string" },
                        "total_losses": { "type": "string", "description": "Positive number (absolute losses)." },
                        "net_gain_loss": { "type": "string" },
                        "base_currency": { "type": "string", "example": "GBP", "description": "User's preferred currency." }
                    }
                },
                "SymbolSummary": {
                    "type": "object",
                    "required": ["symbol", "total_proceeds", "total_allowable_costs", "total_gains", "total_losses", "net_gain_loss", "original_currency"],
                    "properties": {
                        "symbol": { "type": "string" },
                        "total_proceeds": { "type": "string", "description": "Decimal in preferred currency." },
                        "total_allowable_costs": { "type": "string" },
                        "total_gains": { "type": "string" },
                        "total_losses": { "type": "string" },
                        "net_gain_loss": { "type": "string" },
                        "original_currency": { "type": "string", "description": "The symbol's trading currency." }
                    }
                },
                "CgtMatchDetail": {
                    "type": "object",
                    "required": ["quantity", "price"],
                    "properties": {
                        "acquisition_id": { "type": ["string", "null"] },
                        "acquisition_date": {
                            "type": ["string", "null"],
                            "description": "ISO 8601 datetime, the literal string 'S104 Pool' for pool matches, or null for unmatched remainders."
                        },
                        "quantity": { "type": "string" },
                        "price": { "type": "string", "description": "Per-share acquisition price. Native currency for Same-Day and 30-Day matches; the S104 average pool cost (in the preferred base currency) for S104 Pool matches." }
                    }
                },
                "CgtRealizedEvent": {
                    "type": "object",
                    "required": ["symbol", "disposal_id", "disposal_date", "quantity", "disposal_price", "proceeds", "cost_basis", "gain_loss", "rule_applied", "original_currency", "matches"],
                    "properties": {
                        "symbol": { "type": "string" },
                        "disposal_id": { "type": "string" },
                        "disposal_date": { "type": "string", "format": "date-time" },
                        "quantity": { "type": "string", "description": "Matched quantity. A single disposal split across rules produces one event per rule." },
                        "disposal_price": { "type": "string", "description": "Per share, in the trade's native currency (see original_currency)." },
                        "proceeds": { "type": "string", "description": "Matched quantity * disposal price, net of proportional fee, converted to the user's preferred (base) currency." },
                        "cost_basis": { "type": "string", "description": "Matched quantity * acquisition price, converted to the user's preferred (base) currency. Zero for Unmatched rule." },
                        "gain_loss": { "type": "string", "description": "proceeds - cost_basis, in the user's preferred (base) currency." },
                        "rule_applied": {
                            "type": "string",
                            "enum": ["Same-Day", "30-Day Rule", "S104 Pool", "Unmatched"]
                        },
                        "original_currency": { "type": "string" },
                        "matches": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/CgtMatchDetail" }
                        }
                    }
                },
                "CapitalGainsResponse": {
                    "type": "object",
                    "required": ["summary", "symbol_summaries", "realized_events", "pools"],
                    "properties": {
                        "summary": { "$ref": "#/components/schemas/CgtSummary" },
                        "symbol_summaries": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/SymbolSummary" }
                        },
                        "realized_events": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/CgtRealizedEvent" }
                        },
                        "pools": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/S104PoolState" }
                        }
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
            },
            "/api/documents": {
                "get": {
                    "summary": "List stored source documents with reference count and orphan flag",
                    "responses": { "200": { "description": "Array of DocumentSummary" } }
                },
                "post": {
                    "summary": "Upload one or more standalone documents (origin=manual, deduped by content hash)",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": { "multipart/form-data": { "schema": { "type": "object" } } }
                    },
                    "responses": { "200": { "description": "Array of created DocumentSummary" } }
                }
            },
            "/api/documents/{id}": {
                "get": {
                    "summary": "Document metadata (with reference count and orphan flag)",
                    "responses": { "200": { "description": "DocumentSummary" }, "404": { "description": "Not found" } }
                },
                "delete": {
                    "summary": "Delete a document; 409 unless ?force=true unlinks referencing rows first",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "force", "in": "query", "schema": { "type": "boolean", "default": false },
                          "description": "Strip the id from every referencing row, then delete." }
                    ],
                    "responses": {
                        "200": { "description": "DocumentDeleteResult" },
                        "404": { "description": "Not found" },
                        "409": { "description": "Referenced and force not set; body includes a references breakdown" }
                    }
                }
            },
            "/api/documents/{id}/download": {
                "get": {
                    "summary": "Stream the raw stored file bytes back as an attachment",
                    "responses": { "200": { "description": "File bytes" }, "404": { "description": "Not found or missing on disk" } }
                }
            },
            "/api/investments/pools": {
                "get": {
                    "summary": "S104 average-cost pool snapshot per symbol",
                    "description": concat!(
                        "Returns the current S104 pool state for every symbol with a non-empty history. ",
                        "ISA and Pension accounts are excluded. ",
                        "All monetary values are in the user's preferred (base) currency.",
                    ),
                    "parameters": [
                        {
                            "name": "as_at",
                            "in": "query",
                            "schema": { "type": "string", "format": "date", "example": "2026-04-05" },
                            "description": "Replay only events up to and including this date. Omit for the current state."
                        },
                        {
                            "name": "profile_ids",
                            "in": "query",
                            "schema": { "type": "string", "example": "personal,joint" },
                            "description": "Comma-separated profile IDs. Engine includes events from accounts whose profile_ids intersect this set. Omit for all profiles."
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Array of S104 pool states, one per symbol.",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/S104PoolState" }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/investments/capital-gains": {
                "get": {
                    "summary": "UK HMRC-compliant Capital Gains Tax report",
                    "description": concat!(
                        "Replays all investment events through HMRC's matching rules in strict order: ",
                        "same-day FIFO, 30-day Bed & Breakfast, S104 pool, then any unmatched remainder. ",
                        "ISA and Pension accounts are excluded. ",
                        "All monetary fields (per-event `proceeds` / `cost_basis` / `gain_loss`, the ",
                        "`pools`, `summary`, and `symbol_summaries`) are converted into the user's ",
                        "preferred currency via the `currencies` table; only `disposal_price` and ",
                        "`original_currency` reflect the trade's native currency. ",
                        "If `tax_year` is provided it overrides `start_date` and `end_date`."
                    ),
                    "parameters": [
                        {
                            "name": "tax_year",
                            "in": "query",
                            "schema": { "type": "string", "example": "2024-25" },
                            "description": "UK tax year in `YYYY-YY` or `YYYY-YYYY` form. Resolves to 6 Apr YYYY1 to 5 Apr YYYY2."
                        },
                        {
                            "name": "start_date",
                            "in": "query",
                            "schema": { "type": "string", "format": "date" },
                            "description": "Custom range start (used when tax_year is omitted)."
                        },
                        {
                            "name": "end_date",
                            "in": "query",
                            "schema": { "type": "string", "format": "date" },
                            "description": "Custom range end (used when tax_year is omitted)."
                        },
                        {
                            "name": "as_at",
                            "in": "query",
                            "schema": { "type": "string", "format": "date" },
                            "description": "Truncate the event replay at this date (point-in-time view)."
                        },
                        {
                            "name": "account_id",
                            "in": "query",
                            "schema": { "type": "string" },
                            "description": "Restrict fetched events to one account. Pool math remains symbol-global across all fetched accounts."
                        },
                        {
                            "name": "symbol",
                            "in": "query",
                            "schema": { "type": "string" },
                            "description": "Restrict fetched events to one symbol."
                        },
                        {
                            "name": "profile_ids",
                            "in": "query",
                            "schema": { "type": "string", "example": "personal,joint" },
                            "description": "Comma-separated profile IDs. Engine includes events from accounts whose profile_ids intersect this set. Omit for all profiles."
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Full CGT report: summary, per-symbol breakdown, realized disposals, and final pool states.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/CapitalGainsResponse" }
                                }
                            }
                        },
                        "400": {
                            "description": "Invalid tax_year format or invalid date range.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Error" }
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
