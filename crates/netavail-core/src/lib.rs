//! netavail-core: network availability algorithms.
//!
//! Works on in-memory data only: no file IO, no timing, no threads,
//! so the same code builds for native, wasm and Python (D15, D21).

pub mod availability;
pub mod cache;
pub mod control;
pub mod error;
pub mod graph;
pub mod pipeline;
pub mod prob;
pub mod registry;
pub mod sets;
pub mod transform;
pub mod types;

pub use availability::{AvailabilityAlgorithm, Prepared};
pub use cache::Cache;
pub use control::{Control, NoControl};
pub use error::Error;
pub use graph::Graph;
pub use pipeline::{PairResult, Plan, all_pairs, run_all, run_pair, run_pair_cached};
pub use prob::ProbabilityMap;
pub use registry::AlgoInfo;
pub use sets::minimalcuts::CutAlgorithm;
pub use sets::minimalpaths::PathAlgorithm;
