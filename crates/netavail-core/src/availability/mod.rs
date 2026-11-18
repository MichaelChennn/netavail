//! Availability algorithms.

mod cut_disjoint;
mod disjoint;
mod path_disjoint;

pub use cut_disjoint::CutDisjoint;
pub use path_disjoint::PathDisjoint;

use crate::control::Control;
use crate::error::Error;
use crate::prob::ProbabilityMap;
use crate::registry::AlgoInfo;
use crate::types::{SetFamily, SetKind};

/// Computes availability from path sets or cut sets in two steps:
/// a slow `prepare` that ignores probabilities, then a fast `evaluate` per scenario.
pub trait AvailabilityAlgorithm: Sync {
    fn info(&self) -> &'static AlgoInfo;
    /// The kind of sets `prepare` accepts.
    fn input(&self) -> SetKind;
    fn prepare(&self, sets: &SetFamily, ctl: &dyn Control) -> Result<Box<dyn Prepared>, Error>;
}

/// Result of [`AvailabilityAlgorithm::prepare`]; can be evaluated many times.
pub trait Prepared: Send + Sync {
    fn evaluate(&self, p: &ProbabilityMap) -> f64;
}
