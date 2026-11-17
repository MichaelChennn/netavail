//! netavail-core: network availability algorithms.
//!
//! Works on in-memory data only: no file IO, no timing, no threads,
//! so the same code builds for native, wasm and Python (D15, D21).

pub mod error;
pub mod graph;
pub mod prob;
pub mod types;

pub use error::Error;
pub use graph::Graph;
