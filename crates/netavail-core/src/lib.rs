//! netavail-core: network availability algorithms.
//!
//! Works on in-memory data only: no file IO, no timing, no threads,
//! so the same code builds for native, wasm and Python (D15, D21).

pub mod prob;
pub mod types;
