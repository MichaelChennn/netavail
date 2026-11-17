//! Error type shared by all of core.

use crate::control::Cancelled;
use crate::types::{NodeId, SetKind};

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
    #[error("invalid node pair ({src}, {dst}): nodes must differ and exist in the graph")]
    InvalidPair { src: NodeId, dst: NodeId },
    #[error("more than {limit} sets; raise the limit or pick a smaller topology")]
    TooManySets { limit: usize },
    #[error("expected {expected:?} as input, got {found:?}")]
    WrongInputKind { expected: SetKind, found: SetKind },
    #[error("cancelled")]
    Cancelled,
}

impl From<Cancelled> for Error {
    fn from(_: Cancelled) -> Error {
        Error::Cancelled
    }
}
