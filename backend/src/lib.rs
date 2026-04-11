//! fynance core library. Exposed as a crate so integration tests and
//! future binaries (serve, export) can reuse the same modules.

pub mod cli;
pub mod commands;
pub mod importers;
pub mod model;
pub mod storage;
pub mod util;
