//! UK Capital Gains Tax calculation engine.
//!
//! Pure tax mathematics, deliberately free of HTTP concerns. The Axum handlers
//! that drive this live in `server::routes::capital_gains` and are a thin
//! adapter over it: parse query params, fetch rows, call the engine, serialise.
//!
//! Layout:
//! - [`guards`] -- pre-flight refusals that run before the engine.
//! - [`models`] -- response types (TS-exported) and internal calculation types.
//! - [`engine`] -- the HMRC matching rules themselves.

pub mod engine;
pub mod guards;
pub mod models;
