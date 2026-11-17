//! Error type shared by all of core.

use crate::control::Cancelled;
use crate::types::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("edge {edge}: node {node} is out of range (graph has {node_count} nodes)")]
    NodeOutOfRange {
        edge: usize,
        node: NodeId,
        node_count: u32,
    },
    #[error("edge {edge}: self-loop on node {node}")]
    SelfLoop { edge: usize, node: NodeId },
    #[error("edge {edge}: duplicate of edge {first}")]
    DuplicateEdge { edge: usize, first: usize },
    #[error("cancelled")]
    Cancelled,
}

impl From<Cancelled> for Error {
    fn from(_: Cancelled) -> Error {
        Error::Cancelled
    }
}
