//! Minimal cut set algorithms.

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
