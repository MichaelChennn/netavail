//! netavail-io: reading topology files (and later result files and reference answers).
//!
//! Takes text, not paths, so it also works in the browser (architecture.md §4).

pub mod reference;
pub mod topology;

pub use topology::Topology;
