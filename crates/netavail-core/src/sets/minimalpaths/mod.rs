//! Minimal path set algorithms.

mod dfs;

pub use dfs::Dfs;

use crate::control::Control;
use crate::error::Error;
use crate::graph::Graph;
use crate::registry::AlgoInfo;
use crate::types::{NodeId, SetFamily};

/// Finds all minimal path sets between `src` and `dst`.
pub trait PathAlgorithm: Sync {
    fn info(&self) -> &'static AlgoInfo;
    fn find(
        &self,
        g: &Graph,
        src: NodeId,
        dst: NodeId,
        ctl: &dyn Control,
    ) -> Result<SetFamily, Error>;
}
