//! `GET /api/docs` — hand-crafted OpenAPI 3.1 spec.
//!
//! This endpoint is the self-describing contract external AI agents
//! use to discover the API without any out-of-band documentation. It
//! is a complete inventory of the API surface: every route under
//! `/api` is listed here with its method, params, auth, and response.
//! The spec intentionally embeds the full category taxonomy from
//! `backend/config/categories.yaml` so an agent can categorize new
//! transactions with zero extra fetches. The rich human-readable
//! contract, with full field-level schemas for every request and
//! response, lives in `docs/api.html`.

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
            "version": env!("CARGO_PKG_VERSION"),
            "description": concat!(
                "Local-first personal finance tracker. All routes live under `/api`. ",
                "Browser requests from `127.0.0.1` need no auth. Programmatic clients ",
                "(scripts, agents) must supply `Authorization: Bearer fyn_...`. ",
                "Tokens are generated via `fynance token create`. ",
                "This spec is a complete inventory of the API surface; the rich ",
                "human-readable contract with full field-level schemas lives in ",
                "`docs/api.html`.",
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
                    "required": ["id", "date", "description", "amount", "currency", "account_id", "fingerprint", "is_recurring", "exclude_from_summary"],
                    "properties": {
                        "id": { "type": "string" },
                        "date": { "type": "string", "format": "date-time", "description": "ISO 8601 datetime (`YYYY-MM-DDTHH:MM:SS`)." },
                        "description": { "type": "string" },
                        "normalized": { "type": "string", "description": "Normalized merchant/description used for matching." },
                        "amount": {
                            "type": "string",
                            "description": "Decimal as string. Negative = money out, positive = money in."
                        },
                        "currency": { "type": "string", "example": "GBP" },
                        "account_id": { "type": "string" },
                        "category_id": {
                            "type": ["string", "null"],
                            "description": "FK to categories.id (a category UUID); only leaf nodes are valid. Resolve the display name from the categories list."
                        },
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
                        "is_recurring": { "type": "boolean" },
                        "exclude_from_summary": {
                            "type": "boolean",
                            "description": "When true, the transaction is omitted from spending/budget summaries (e.g. transfers)."
                        },
                        "fingerprint": {
                            "type": "string",
                            "description": "Stable dedup hash: sha256(datetime, amount, account_id)."
                        },
                        "fitid": {
                            "type": ["string", "null"],
                            "description": "Financial institution transaction id from the source statement, when present."
                        },
                        "source_document_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "IDs of the source documents (`documents.id`) this transaction was extracted from. Empty for manual / CSV / API imports with no document."
                        }
                    }
                },
                "ImportTransaction": {
                    "type": "object",
                    "required": ["date", "description", "amount"],
                    "properties": {
                        "date": { "type": "string", "format": "date-time", "description": "ISO 8601 datetime; date-only values are accepted and stored at `T00:00:00`." },
                        "description": { "type": "string" },
                        "amount": { "type": "string", "description": "Decimal string, signed. Negative = money out, positive = money in." },
                        "currency": { "type": ["string", "null"], "default": "GBP" },
                        "category_id": {
                            "type": ["string", "null"],
                            "description": "FK to categories.id (a leaf category UUID)."
                        },
                        "category_source": {
                            "type": ["string", "null"],
                            "enum": ["rule", "agent", "manual", null],
                            "default": "agent"
                        },
                        "notes": { "type": ["string", "null"] },
                        "is_recurring": { "type": ["boolean", "null"] },
                        "exclude_from_summary": { "type": ["boolean", "null"] },
                        "source_document_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "IDs of the source documents (`documents.id`) this row was extracted from. Empty for manual rows."
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
                        "original_currency": { "type": "string", "description": "Source metadata: the currency the trade was denominated in. NOT a formatting label — proceeds, cost_basis and gain_loss are all in the user's preferred (base) currency. Only disposal_price is native." },
                        "matches": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/CgtMatchDetail" }
                        }
                    }
                },
                "CgtDisposalGroup": {
                    "type": "object",
                    "description": concat!(
                        "`realized_events` rolled up by (symbol, disposal_date) into one row per actual ",
                        "sale — the honest answer to SA108 box 23 'number of disposals'. `realized_events` ",
                        "emits one row per matched bucket (same-day / 30-day / S104), so a single sale ",
                        "matching more than one rule becomes multiple rows there; this collapses those back ",
                        "into one, with the constituent rows carried through in `events`. Deliberately NOT ",
                        "grouped by rule_applied (realized_events already gives you that) or by rate band ",
                        "(a tax-computation concern, out of scope here).",
                    ),
                    "required": ["symbol", "disposal_date", "quantity", "proceeds", "cost_basis", "gain_loss", "original_currency", "events"],
                    "properties": {
                        "symbol": { "type": "string" },
                        "disposal_date": { "type": "string", "format": "date", "description": "The calendar day of the sale (YYYY-MM-DD). Date-only on purpose: UK capital gains are reckoned by day, so every sale of one holding on one date is a single disposal. The per-event disposal_date on CgtRealizedEvent keeps its full timestamp." },
                        "quantity": { "type": "string", "description": "Summed across every matched bucket for this disposal." },
                        "proceeds": { "type": "string", "description": "Summed proceeds, in the user's preferred (base) currency." },
                        "cost_basis": { "type": "string", "description": "Summed cost basis, in the user's preferred (base) currency." },
                        "gain_loss": { "type": "string", "description": "proceeds - cost_basis, in the user's preferred (base) currency." },
                        "original_currency": { "type": "string", "description": "Source metadata: the currency the constituent trades were denominated in. NOT a formatting label — every money field on this group is in the user's preferred (base) currency." },
                        "events": {
                            "type": "array",
                            "description": "The constituent realized_events rows this group rolls up.",
                            "items": { "$ref": "#/components/schemas/CgtRealizedEvent" }
                        }
                    }
                },
                "CapitalGainsResponse": {
                    "type": "object",
                    "required": ["summary", "symbol_summaries", "realized_events", "disposal_groups", "pools"],
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
                        "disposal_groups": {
                            "type": "array",
                            "description": "realized_events rolled up by (symbol, disposal_date) — one row per actual sale. Additive: use realized_events for the matching-rule breakdown, disposal_groups for the SA108-style disposal count.",
                            "items": { "$ref": "#/components/schemas/CgtDisposalGroup" }
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
            "/api/accounts": {
                "get": {
                    "summary": "List accounts",
                    "parameters": [
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter accounts by profile." }
                    ],
                    "responses": { "200": { "description": "Array of accounts" } }
                },
                "post": {
                    "summary": "Create an account",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Created account" } }
                }
            },
            "/api/accounts/{id}": {
                "patch": {
                    "summary": "Update an account",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Updated account" } }
                },
                "delete": {
                    "summary": "Delete an account (soft-delete by default; ?hard=true removes the row)",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "hard", "in": "query", "schema": { "type": "boolean", "default": false },
                          "description": "Hard-delete the row instead of deactivating. Refuses if the account still has transactions, holdings, or investment events; its ingestion-checklist rows are removed with it." }
                    ],
                    "responses": {
                        "200": { "description": "Account deleted or deactivated" },
                        "409": { "description": "Account still has transactions, holdings, or investment events" }
                    }
                }
            },
            "/api/accounts/{id}/balance": {
                "patch": {
                    "summary": "Set an account's balance as of a date",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Balance updated" } }
                }
            },
            "/api/profiles": {
                "get": {
                    "summary": "List profiles",
                    "responses": { "200": { "description": "Array of profiles" } }
                },
                "post": {
                    "summary": "Create a profile",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Created profile" } }
                }
            },
            "/api/profiles/{id}": {
                "patch": {
                    "summary": "Update a profile",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Updated profile" } }
                },
                "delete": {
                    "summary": "Delete a profile",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Profile deleted" } }
                }
            },
            "/api/categories": {
                "get": {
                    "summary": "List categories (the full hierarchical taxonomy)",
                    "responses": { "200": { "description": "Array of categories" } }
                },
                "post": {
                    "summary": "Create a category",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Created category" } }
                }
            },
            "/api/categories/resolve": {
                "get": {
                    "summary": "Resolve a category id by name",
                    "parameters": [
                        { "name": "name", "in": "query", "required": true, "schema": { "type": "string" },
                          "description": "Category name to resolve to its id." }
                    ],
                    "responses": { "200": { "description": "Resolved category" }, "404": { "description": "No matching category" } }
                }
            },
            "/api/categories/{id}": {
                "get": {
                    "summary": "Get a category by id",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Category" }, "404": { "description": "Not found" } }
                },
                "patch": {
                    "summary": "Update a category",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Updated category" } }
                },
                "delete": {
                    "summary": "Delete a category",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "hard", "in": "query", "schema": { "type": "boolean", "default": false },
                          "description": "When true, permanently delete the row instead of soft-deleting." }
                    ],
                    "responses": { "200": { "description": "Category deleted" } }
                }
            },
            "/api/transactions": {
                "get": {
                    "summary": "List transactions (paginated)",
                    "parameters": [
                        { "name": "start", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "accounts", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated account ids to include." },
                        { "name": "categories", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category ids to include." },
                        { "name": "category_types", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category_type values to include." },
                        { "name": "search", "in": "query", "schema": { "type": "string" }, "description": "Free-text search over description / merchant." },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." },
                        { "name": "category_source", "in": "query", "schema": { "type": "string", "enum": ["rule", "agent", "manual"] }, "description": "Filter by how the category was assigned." },
                        { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["date", "amount", "category"] }, "description": "Sort column (default: date)." },
                        { "name": "sort_dir", "in": "query", "schema": { "type": "string", "enum": ["asc", "desc"] }, "description": "Sort direction (default: desc)." },
                        { "name": "page", "in": "query", "schema": { "type": "integer", "default": 1 } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 25 } }
                    ],
                    "responses": {
                        "200": {
                            "description": "Paginated transaction list",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/Transaction" }
                                    }
                                }
                            }
                        }
                    }
                },
                "patch": {
                    "summary": "Bulk update transactions (re-categorize)",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "description": "Assign one leaf `category_id` to all transactions in `ids`.",
                                    "required": ["ids", "category_id"],
                                    "properties": {
                                        "ids": { "type": "array", "items": { "type": "string" } },
                                        "category_id": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": { "200": { "description": "Count of updated transactions" } }
                },
                "delete": {
                    "summary": "Bulk hard-delete transactions",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "description": "Either `{ ids: [...] }` to delete specific transactions, or `{ account_id }` to clear an account.",
                                    "properties": {
                                        "ids": { "type": "array", "items": { "type": "string" } },
                                        "account_id": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": { "200": { "description": "Count of deleted transactions" } }
                }
            },
            "/api/transactions/by-category": {
                "get": {
                    "summary": "Transactions grouped/summarised by category",
                    "parameters": [
                        { "name": "start", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "accounts", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated account ids." },
                        { "name": "categories", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category ids." },
                        { "name": "category_types", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category_type values." },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." },
                        { "name": "direction", "in": "query", "schema": { "type": "string", "enum": ["outflow", "income"] }, "description": "outflow or income. Omit for signed net sums." }
                    ],
                    "responses": { "200": { "description": "Per-category transaction breakdown" } }
                }
            },
            "/api/transactions/categories": {
                "get": {
                    "summary": "Distinct categories present in the transaction set",
                    "responses": { "200": { "description": "Array of category ids/names" } }
                }
            },
            "/api/transactions/accounts": {
                "get": {
                    "summary": "Distinct accounts present in the transaction set",
                    "responses": { "200": { "description": "Array of accounts" } }
                }
            },
            "/api/transactions/import": {
                "post": {
                    "summary": "Programmatic typed JSON import of transactions",
                    "description": "Current route for structured (non-CSV) imports by agents and scripts.",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "dry_run", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Preview without committing (returns TransactionImportPreview)." }
                    ],
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
            "/api/transactions/{id}": {
                "patch": {
                    "summary": "Edit a transaction (category, notes, flags)",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": {
                            "description": "Updated transaction",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Transaction" }
                                }
                            }
                        }
                    }
                },
                "delete": {
                    "summary": "Hard-delete one transaction",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Transaction deleted" } }
                }
            },
            "/api/import": {
                "post": {
                    "summary": "Deprecated: programmatic typed JSON import",
                    "deprecated": true,
                    "description": "Deprecated. Use `POST /api/transactions/import` instead. This legacy route returns a `Deprecation` / `Link` header pointing at the successor.",
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
            "/api/import/csv": {
                "post": {
                    "summary": "Upload a single CSV statement (auto-detects bank format)",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "account", "in": "query", "required": true, "schema": { "type": "string" }, "description": "Target account ID." },
                        { "name": "dry_run", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Preview without committing." }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": { "multipart/form-data": { "schema": { "type": "object" } } }
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
            "/api/import/bulk": {
                "post": {
                    "summary": "Upload multiple CSV statements in one request",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": { "multipart/form-data": { "schema": { "type": "object" } } }
                    },
                    "responses": { "200": { "description": "Array of per-file ImportResult" } }
                }
            },
            "/api/parse": {
                "post": {
                    "summary": "Stage 1 parse: extract transactions from uploaded documents (PDF/CSV)",
                    "description": "Accepts a multipart upload (50 MB total, 10 MB per file). Kicks off LLM extraction; track progress via `GET /api/parse/progress/{parse_id}`.",
                    "security": [{ "bearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": { "multipart/form-data": { "schema": { "type": "object" } } }
                    },
                    "responses": { "200": { "description": "Parse result / parse_id for progress polling" } }
                }
            },
            "/api/parse/progress/{parse_id}": {
                "get": {
                    "summary": "Poll progress of an in-flight parse",
                    "parameters": [
                        { "name": "parse_id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Parse progress snapshot" } }
                }
            },
            "/api/documents": {
                "get": {
                    "summary": "List stored source documents with orphan flag (reference_count is null unless include=refs; also available per-doc via GET /api/documents/{id})",
                    "parameters": [
                        { "name": "include", "in": "query", "schema": { "type": "string", "enum": ["refs"] },
                          "description": "Set to refs to populate reference_count for every row in one batched query." }
                    ],
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
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "DocumentSummary" }, "404": { "description": "Not found" } }
                },
                "delete": {
                    "summary": "Delete a document; 409 unless ?force=true unlinks referencing rows first",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
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
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "File bytes" }, "404": { "description": "Not found or missing on disk" } }
                }
            },
            "/api/budget/spending-grid": {
                "get": {
                    "summary": "Multi-month spending vs budget grid",
                    "parameters": [
                        { "name": "start", "in": "query", "schema": { "type": "string", "example": "2026-01" } },
                        { "name": "end", "in": "query", "schema": { "type": "string", "example": "2026-06" } },
                        { "name": "granularity", "in": "query", "schema": { "type": "string", "example": "month" } },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" } },
                        { "name": "accounts", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated account ids." },
                        { "name": "categories", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category ids." },
                        { "name": "category_types", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category_type values." },
                        { "name": "group_by", "in": "query", "schema": { "type": "string", "enum": ["leaf_category", "parent_category", "category_type", "account"] }, "description": "Grouping dimension (default leaf_category)." }
                    ],
                    "responses": { "200": { "description": "{ preferred_currency, rows: SpendingGridRow[] }" } }
                }
            },
            "/api/budget/cash-summary": {
                "get": {
                    "summary": "Category-type cash summary (income, spending, savings growth, new cash invested, investment metrics)",
                    "parameters": [
                        { "name": "start", "in": "query", "schema": { "type": "string", "example": "2026-01-01" } },
                        { "name": "end", "in": "query", "schema": { "type": "string", "example": "2026-06-30" } },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "CashSummaryResponse" } }
                }
            },
            "/api/budget/{month}": {
                "get": {
                    "summary": "Per-month budget view (effective budget + actual spend per category)",
                    "parameters": [
                        { "name": "month", "in": "path", "required": true, "schema": { "type": "string", "example": "2026-04" } }
                    ],
                    "responses": { "200": { "description": "Budget vs actuals for the month" } }
                }
            },
            "/api/budget": {
                "post": {
                    "summary": "Set a standing monthly budget for a category",
                    "description": "Body `{ category_id, amount }`. Applies every month unless overridden.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Standing budget set" } }
                }
            },
            "/api/budget/override": {
                "post": {
                    "summary": "Set a per-month budget override for a category",
                    "description": "Body `{ month (YYYY-MM), category_id, amount }`.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Override set" } }
                }
            },
            "/api/holdings": {
                "get": {
                    "summary": "List holdings for an account",
                    "parameters": [
                        { "name": "account_id", "in": "query", "schema": { "type": "string" }, "description": "Single account ID." },
                        { "name": "account_ids", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated account IDs." },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile (investment accounts only)." },
                        { "name": "include_closed", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Include closed positions." }
                    ],
                    "responses": { "200": { "description": "Array of holdings" } }
                }
            },
            "/api/holdings/summary": {
                "get": {
                    "summary": "Portfolio summary: net worth, by_asset_class, by_type, by_institution",
                    "parameters": [
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." },
                        { "name": "as_of", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "Date (YYYY-MM-DD). Default: today." }
                    ],
                    "responses": { "200": { "description": "HoldingsSummaryResponse" } }
                }
            },
            "/api/holdings/history": {
                "get": {
                    "summary": "Net worth history over time (available/unavailable/total)",
                    "parameters": [
                        { "name": "start", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "granularity", "in": "query", "required": true, "schema": { "type": "string", "enum": ["monthly", "quarterly", "yearly"] }, "description": "monthly, quarterly, or yearly." },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." }
                    ],
                    "responses": { "200": { "description": "Net worth history series" } }
                }
            },
            "/api/holdings/account-history": {
                "get": {
                    "summary": "Per-account, per-holding value series over time",
                    "parameters": [
                        { "name": "account_id", "in": "query", "schema": { "type": "string" } },
                        { "name": "start", "in": "query", "schema": { "type": "string", "format": "date" } },
                        { "name": "end", "in": "query", "schema": { "type": "string", "format": "date" } },
                        { "name": "granularity", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Per-holding value series" } }
                }
            },
            "/api/holdings/balances": {
                "get": {
                    "summary": "Per-account balances derived from holdings SUM",
                    "parameters": [
                        { "name": "start", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "summary", "in": "query", "schema": { "type": "string" }, "description": "\"true\" for BalanceDelta[] instead of AccountSnapshot[]." }
                    ],
                    "responses": { "200": { "description": "Per-account balances" } }
                }
            },
            "/api/holdings/cash-flow": {
                "get": {
                    "summary": "Income/spending cash flow",
                    "parameters": [
                        { "name": "start", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "required": true, "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "granularity", "in": "query", "required": true, "schema": { "type": "string", "enum": ["monthly", "quarterly", "yearly"] }, "description": "monthly, quarterly, or yearly." },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." },
                        { "name": "exclude_category_ids", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated category IDs to exclude." }
                    ],
                    "responses": { "200": { "description": "Cash flow series" } }
                }
            },
            "/api/holdings/import": {
                "post": {
                    "summary": "Bulk import holdings",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "dry_run", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Preview without committing." }
                    ],
                    "responses": { "200": { "description": "Holdings import summary" } }
                }
            },
            "/api/holdings/{account_id}": {
                "post": {
                    "summary": "Upsert holdings for an account",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "account_id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Holdings upserted" } }
                }
            },
            "/api/holdings/{account_id}/{symbol}": {
                "get": {
                    "summary": "Value history for one holding (account + symbol)",
                    "parameters": [
                        { "name": "account_id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "symbol", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Holding value history" } }
                },
                "patch": {
                    "summary": "Update a single holding snapshot",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "account_id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "symbol", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Updated holding" } }
                },
                "delete": {
                    "summary": "Delete a holding",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "account_id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "symbol", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "as_of", "in": "query", "schema": { "type": "string", "format": "date" },
                          "description": "Delete the snapshot effective on this date." },
                        { "name": "sub_account", "in": "query", "schema": { "type": "string" },
                          "description": "Sub-account label (further scoping)." }
                    ],
                    "responses": { "200": { "description": "Holding deleted" } }
                }
            },
            "/api/ingestion/checklist/{month}": {
                "get": {
                    "summary": "Ingestion checklist for a month (per-account import status)",
                    "parameters": [
                        { "name": "month", "in": "path", "required": true, "schema": { "type": "string", "example": "2026-04" } }
                    ],
                    "responses": { "200": { "description": "Checklist items" } }
                }
            },
            "/api/ingestion/checklist/{month}/{account_id}": {
                "post": {
                    "summary": "Mark an account's import complete for a month",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "month", "in": "path", "required": true, "schema": { "type": "string", "example": "2026-04" } },
                        { "name": "account_id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Checklist item updated" } }
                }
            },
            "/api/currencies": {
                "get": {
                    "summary": "List currencies and their FX rates to the preferred currency",
                    "responses": { "200": { "description": "Array of currencies" } }
                },
                "post": {
                    "summary": "Add a currency",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Created currency" } }
                }
            },
            "/api/currencies/{code}": {
                "patch": {
                    "summary": "Update a currency's rate/metadata",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "code", "in": "path", "required": true, "schema": { "type": "string", "example": "USD" } }
                    ],
                    "responses": { "200": { "description": "Updated currency" } }
                },
                "delete": {
                    "summary": "Delete a currency",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "code", "in": "path", "required": true, "schema": { "type": "string", "example": "USD" } }
                    ],
                    "responses": { "200": { "description": "Currency deleted" } }
                }
            },
            "/api/exchange-rates": {
                "get": {
                    "summary": "List date-keyed exchange rates",
                    "description": "User-owned historical FX rates. `rate` is quote units per ONE base unit, so amount_in_quote = amount_in_base * rate.",
                    "parameters": [
                        { "name": "base", "in": "query", "schema": { "type": "string", "example": "USD" } },
                        { "name": "quote", "in": "query", "schema": { "type": "string", "example": "GBP" } },
                        { "name": "start_date", "in": "query", "schema": { "type": "string", "example": "2024-04-06" },
                          "description": "Inclusive lower bound, YYYY-MM-DD." },
                        { "name": "end_date", "in": "query", "schema": { "type": "string", "example": "2025-04-05" },
                          "description": "Inclusive upper bound, YYYY-MM-DD." }
                    ],
                    "responses": { "200": { "description": "Array of exchange rates" } }
                },
                "post": {
                    "summary": "Bulk create or update exchange rates",
                    "description": "A CGT report for one tax year can need ~49 rates, so a batch is the normal case. Existing (base, quote, date) rows are overwritten. The whole batch is validated before anything is written.",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "201": { "description": "The stored rates" } }
                }
            },
            "/api/exchange-rates/{base}/{quote}/{date}": {
                "delete": {
                    "summary": "Delete one stored exchange rate",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "base", "in": "path", "required": true, "schema": { "type": "string", "example": "USD" } },
                        { "name": "quote", "in": "path", "required": true, "schema": { "type": "string", "example": "GBP" } },
                        { "name": "date", "in": "path", "required": true, "schema": { "type": "string", "example": "2024-06-03" } }
                    ],
                    "responses": { "204": { "description": "Exchange rate deleted" } }
                }
            },
            "/api/investments": {
                "get": {
                    "summary": "List investment events",
                    "parameters": [
                        { "name": "account_id", "in": "query", "schema": { "type": "string" } },
                        { "name": "symbol", "in": "query", "schema": { "type": "string" } },
                        { "name": "event_type", "in": "query", "schema": { "type": "string" },
                          "description": "Filter by event type (e.g. buy, sell, vest)." }
                    ],
                    "responses": { "200": { "description": "Array of investment events" } }
                },
                "post": {
                    "summary": "Create one investment event",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Created investment event" } }
                }
            },
            "/api/investments/import": {
                "post": {
                    "summary": "Bulk import investment events",
                    "security": [{ "bearerAuth": [] }],
                    "responses": { "200": { "description": "Investment import summary" } }
                }
            },
            "/api/investments/history": {
                "get": {
                    "summary": "Cumulative net invested vs market value over time",
                    "parameters": [
                        { "name": "start", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "Start date (YYYY-MM-DD)." },
                        { "name": "end", "in": "query", "schema": { "type": "string", "format": "date" }, "description": "End date (YYYY-MM-DD)." },
                        { "name": "granularity", "in": "query", "schema": { "type": "string", "enum": ["monthly", "quarterly", "yearly"] } },
                        { "name": "profile_id", "in": "query", "schema": { "type": "string" }, "description": "Filter by profile." },
                        { "name": "accounts", "in": "query", "schema": { "type": "string" }, "description": "Comma-separated account ids. Scopes both series. Omitted = all investment + ISA accounts." }
                    ],
                    "responses": { "200": { "description": "Per-period { net_invested, market_value }, null where no data. Net invested is capital contributed: a disposal removes the shares' average book cost, not the sale proceeds." } }
                }
            },
            "/api/investments/{id}": {
                "patch": {
                    "summary": "Update an investment event",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Updated investment event" } }
                },
                "delete": {
                    "summary": "Delete an investment event",
                    "security": [{ "bearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Investment event deleted" } }
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
                            "name": "end_date",
                            "in": "query",
                            "schema": { "type": "string", "format": "date", "example": "2026-04-05" },
                            "description": "Replay only events up to and including this date (truncates the event ledger itself — a genuine point-in-time snapshot). Omit for the current state."
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
                        "`start_date` and `end_date` only filter which disposals are *emitted* — unlike ",
                        "`/api/investments/pools`, the event ledger itself is never truncated here, so the ",
                        "S104 pool (and therefore the 30-day rule's ability to reach forward to a later ",
                        "acquisition) always sees the full history regardless of the window requested. ",
                        "Omitting `start_date` means \"from time zero\", which is the report equivalent of ",
                        "a point-in-time \"as at this date\" view: pass only `end_date`.",
                    ),
                    "parameters": [
                        {
                            "name": "start_date",
                            "in": "query",
                            "schema": { "type": "string", "format": "date" },
                            "description": "Only disposals on or after this date are emitted. Omit for no lower bound (\"from time zero\")."
                        },
                        {
                            "name": "end_date",
                            "in": "query",
                            "schema": { "type": "string", "format": "date" },
                            "description": "Only disposals on or before this date are emitted. Omit for no upper bound."
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
                        },
                        {
                            "name": "tax_year",
                            "in": "query",
                            "schema": { "type": "string", "example": "2024-25" },
                            "description": concat!(
                                "UK tax year as `YYYY-YY`. When present, the response carries a `tax` object ",
                                "computed from the statutory rates in `tax_config` and the taxpayer's figures in ",
                                "`tax_inputs`. Gains are bucketed by disposal date against the rate bands in force, ",
                                "so a 2024-25 report splits across the pre- and post-30-October rates; losses and ",
                                "the annual exempt amount are deducted from the highest-rate band first. Deliberately ",
                                "separate from `start_date`/`end_date`, which bound what is *reported*: a caller may ",
                                "report a window that is not a tax year, and tax is only defined for a whole one. ",
                                "Omit it and `tax` is absent entirely, which means \"not asked for\", never \"no tax due\"."
                            )
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Full CGT report: summary, per-symbol breakdown, realized disposals (both granular and grouped by actual sale), and final pool states. Includes a `tax` computation when `tax_year` was supplied.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/CapitalGainsResponse" }
                                }
                            }
                        },
                        "400": {
                            "description": "Invalid date format or start_date after end_date.",
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
        "/api/tax-config": {
            "get": {
                "summary": "Statutory tax configuration (annual exempt amounts and CGT rate bands)",
                "description": concat!(
                    "The law: identical for every UK user and changed only by a Budget. Served separately ",
                    "from `/api/tax-inputs` (the taxpayer's own situation) for that reason — reseeding ",
                    "statutory values must never touch a user's figures. Seeded with HMRC-verified values ",
                    "on startup, and an entry edited through PUT is never overwritten by a later restart. ",
                    "A tax year normally carries one `rate` entry per `rate_kind`; 2024-25 carries two of ",
                    "each, because the Autumn Budget 2024 raised CGT on shares from 10%/20% to 18%/24% for ",
                    "disposals on or after 30 October 2024. That is modelled as ordinary rows with adjacent ",
                    "inclusive date ranges, not a special case, so a future Budget needs a row rather than ",
                    "a code change. `rate` is a decimal fraction: 24% is 0.24."
                ),
                "parameters": [
                    {
                        "name": "tax_year",
                        "in": "query",
                        "schema": { "type": "string", "example": "2024-25" },
                        "description": "Restrict to one tax year, as `YYYY-YY`. Omit for every year held."
                    }
                ],
                "responses": {
                    "200": {
                        "description": "The statutory entries, as `{ \"entries\": [...] }`.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/TaxConfigEntry" }
                            }
                        }
                    },
                    "400": {
                        "description": "tax_year is not in YYYY-YY form.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Error" }
                            }
                        }
                    }
                }
            },
            "put": {
                "summary": "Replace the statutory configuration for one tax year",
                "description": concat!(
                    "Replaces the complete entry set for the named year rather than merging into it. The ",
                    "entries for a year are a set that must tile it: a partial update could leave a gap ",
                    "between two rate bands, and a disposal falling in that gap would have no rate to be ",
                    "charged at. Rates are validated as fractions between 0 and 1, so sending 24 instead of ",
                    "0.24 is rejected rather than computing a bill 100x too large."
                ),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/PutTaxConfigPayload" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Written, as `{ \"tax_year\": ..., \"written\": n }`."
                    },
                    "400": {
                        "description": "Invalid tax year, kind, rate_kind, rate range, date or validity range.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Error" }
                            }
                        }
                    }
                }
            }
        },
        "/api/tax-inputs/{profile_id}/{tax_year}": {
            "get": {
                "summary": "One taxpayer's own figures for a tax year",
                "description": concat!(
                    "Nothing here is statutory and nothing here is derivable from the ledger — it is what ",
                    "the taxpayer brings to the computation. A profile-year that has never been given inputs ",
                    "returns the documented defaults rather than 404 (no brought-forward losses, no ",
                    "basic-rate headroom, AEA claimed); each is a real position rather than a placeholder, ",
                    "so the computation has no unconfigured branch. `allowable_income_remaining` is unused ",
                    "headroom in the basic-rate *income* band: gains within it are charged at the basic CGT ",
                    "rate and the excess at the higher rate. It cannot be derived (this app does not see ",
                    "PAYE income), so it defaults to 0, meaning every gain is charged at the higher rate — ",
                    "the safe direction, since over-estimating does not produce a surprise bill."
                ),
                "parameters": [
                    {
                        "name": "profile_id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "Profile slug."
                    },
                    {
                        "name": "tax_year",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string", "example": "2024-25" },
                        "description": "UK tax year as `YYYY-YY`."
                    }
                ],
                "responses": {
                    "200": {
                        "description": "The taxpayer's figures for that year.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/TaxInputs" }
                            }
                        }
                    },
                    "400": {
                        "description": "Invalid profile id or tax year.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Error" }
                            }
                        }
                    }
                }
            },
            "put": {
                "summary": "Set one taxpayer's figures for a tax year",
                "description": concat!(
                    "Every field is optional and an absent key leaves the stored value alone, so a request ",
                    "that only toggles the AEA will not silently zero the brought-forward losses and change ",
                    "the tax due without saying so. `brought_forward_losses` is user-entered, never computed: ",
                    "a derived figure may be offered as a prefill, but it can only ever overstate, because a ",
                    "UK capital loss carries forward only if it was claimed within four years of the end of ",
                    "the year it arose and only the excess after that year's own gains carries at all — ",
                    "neither fact being visible in the ledger."
                ),
                "parameters": [
                    {
                        "name": "profile_id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "Profile slug."
                    },
                    {
                        "name": "tax_year",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string", "example": "2024-25" },
                        "description": "UK tax year as `YYYY-YY`."
                    }
                ],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/PutTaxInputsPayload" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "The stored figures after the update.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/TaxInputs" }
                            }
                        }
                    },
                    "400": {
                        "description": "Invalid profile id or tax year, unknown profile, or a negative amount.",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Error" }
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
