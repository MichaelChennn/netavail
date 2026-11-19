//! netavail-io: topology files, result files and reference answers.
//!
//! Takes text, not paths, so it also works in the browser (architecture.md §4).

pub mod reference;
pub mod results;
pub mod topology;

pub use topology::{Prob, Topology};
