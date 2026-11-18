//! Graph transforms applied before the algorithms, which never see them (D18).

use crate::error::Error;
use crate::graph::Graph;
use crate::types::NodeId;

/// Convert a graph to a link counted graph representation, ported from PyRBD-Suite
/// `graph.to_link_graph`: for each edge (u, v), a new node is inserted between u and v,
/// so a link can fail like a node. Edge `i` becomes node `n + i`; nodes `0..n` keep their ids.
///
/// The probabilities follow the same order: the `n` node availabilities, then one per edge.
pub fn to_link_graph(n: u32, edges: &[(NodeId, NodeId)]) -> Result<Graph, Error> {
    // Check the original graph first, so errors name the caller's edges
    Graph::from_edges(n, edges)?;
    let link_edges: Vec<(NodeId, NodeId)> = edges
        .iter()
        .zip(n..)
        .flat_map(|(&(u, v), new_node)| [(u, new_node), (new_node, v)])
        .collect();
    Graph::from_edges(n + edges.len() as u32, &link_edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Triangle 0-1-2: edges 0, 1, 2 become nodes 3, 4, 5.
    #[test]
    fn inserts_one_node_per_edge() {
        let g = to_link_graph(3, &[(0, 1), (1, 2), (2, 0)]).unwrap();

        assert_eq!(g.node_count(), 6);
        assert_eq!(g.neighbors(0), [3, 5]);
        assert_eq!(g.neighbors(1), [3, 4]);
        assert_eq!(g.neighbors(2), [4, 5]);
        assert_eq!(g.neighbors(3), [0, 1]);
        assert_eq!(g.neighbors(4), [1, 2]);
        assert_eq!(g.neighbors(5), [2, 0]);
    }

    #[test]
    fn rejects_the_same_edges_as_the_original_graph() {
        let err = to_link_graph(3, &[(0, 1), (1, 0)]).err();

        assert_eq!(err, Some(Error::DuplicateEdge { edge: 1, first: 0 }));
    }
}
