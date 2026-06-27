//! fynance core library. Exposed as a crate so integration tests and
//! future binaries (serve, export) can reuse the same modules.

// The `/api/docs` OpenAPI spec is one large `json!` literal whose macro
// expansion exceeds the default 128 recursion limit.
#![recursion_limit = "512"]

pub mod cli;
pub mod commands;
pub mod importers;
pub mod model;
pub mod server;
pub mod storage;
pub mod util;
