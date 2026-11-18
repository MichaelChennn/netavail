//! Minimal cut set algorithms.

mod common;
mod decision_tree;
mod path_product;
mod shannon;

pub use decision_tree::DecisionTree;
pub use path_product::PathProduct;
pub use shannon::Shannon;

use crate::control::Control;
use crate::error::Error;
use crate::registry::AlgoInfo;
use crate::types::SetFamily;

/// Derives all minimal cut sets from the minimal path sets of one node pair (D26).
pub trait CutAlgorithm: Sync {
    fn info(&self) -> &'static AlgoInfo;
    #[allow(clippy::wrong_self_convention)] // name from D26: reads as "cuts from paths"
    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error>;
}
