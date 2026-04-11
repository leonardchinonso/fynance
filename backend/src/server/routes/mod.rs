//! Route handler modules, one per resource group.
//!
//! Phase 2 only wires up `docs` (agent-readable OpenAPI spec) and
//! `health` (readiness probe). Phases 3+ add `transactions`, `budget`,
//! `portfolio`, `reports`, and `export` modules here.

pub mod docs;
pub mod health;
