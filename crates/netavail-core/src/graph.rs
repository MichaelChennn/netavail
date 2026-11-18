//! Undirected simple graph (architecture.md §3.2; D23, D28).

use std::collections::HashMap;

use crate::error::Error;
use crate::types::NodeId;

/// Undirected simple graph stored as an adjacency list.
///
/// Neighbors keep the order their edges were given in; DFS output order depends on it.
#[derive(Clone, Debug)]
pub struct Graph {
    adj: Vec<Vec<NodeId>>,
}

impl Graph {
    /// Builds a graph with nodes `0..n`. Fails on a self-loop, a duplicate edge
    /// (in either direction) or a node `>= n`, naming the offending edge index.
    pub fn from_edges(n: u32, edges: &[(NodeId, NodeId)]) -> Result<Graph, Error> {
        let mut adj = vec![Vec::new(); n as usize];
        let mut seen = HashMap::new(); // (min, max) -> first edge index
        for (edge, &(u, v)) in edges.iter().enumerate() {
            if let Some(node) = [u, v].into_iter().find(|&x| x >= n) {
                return Err(Error::NodeOutOfRange {
                    edge,
                    node,
                    node_count: n,
                });
            }
            if u == v {
                return Err(Error::SelfLoop { edge, node: u });
            }
            if let Some(&first) = seen.get(&(u.min(v), u.max(v))) {
                return Err(Error::DuplicateEdge { edge, first });
            }
            seen.insert((u.min(v), u.max(v)), edge);
            adj[u as usize].push(v);
            adj[v as usize].push(u);
        }
        Ok(Graph { adj })
    }

    pub fn node_count(&self) -> u32 {
        self.adj.len() as u32
    }

    /// Fails with `InvalidPair` unless src and dst differ and both exist.
    pub fn check_pair(&self, src: NodeId, dst: NodeId) -> Result<(), Error> {
        if src == dst || src >= self.node_count() || dst >= self.node_count() {
            return Err(Error::InvalidPair { src, dst });
        }
        Ok(())
    }

    /// Neighbors of `u` in edge input order. Panics if `u` is out of range.
    pub fn neighbors(&self, u: NodeId) -> &[NodeId] {
        &self.adj[u as usize]
    }

    /// Structural hash of the graph (D28): depends only on the node count and
    /// the edge set, not on edge order or direction.
    pub fn fingerprint(&self) -> Fingerprint {
        let mut edges: Vec<(NodeId, NodeId)> = (0..self.node_count())
            .flat_map(|u| {
                self.neighbors(u)
                    .iter()
                    .filter(move |&&v| u < v)
                    .map(move |&v| (u, v))
            })
            .collect();
        edges.sort_unstable();

        // "netavail-graph-v1" + n + m + sorted (min, max) edges, all u32 little-endian
        let mut h = blake3::Hasher::new();
        h.update(b"netavail-graph-v1");
        h.update(&self.node_count().to_le_bytes());
        h.update(&(edges.len() as u32).to_le_bytes());
        for (u, v) in edges {
            h.update(&u.to_le_bytes());
            h.update(&v.to_le_bytes());
        }
        Fingerprint(*h.finalize().as_bytes())
    }
}

/// BLAKE3 hash identifying a graph's structure; used in cache keys and exported files.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Stands in for a real fingerprint in a throwaway cache.
    pub(crate) const PLACEHOLDER: Fingerprint = Fingerprint([0; 32]);

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_pair_rejects_equal_or_missing_nodes() {
        let g = Graph::from_edges(3, &[(0, 1), (1, 2)]).unwrap();

        assert_eq!(g.check_pair(0, 2), Ok(()));
        for (src, dst) in [(1, 1), (0, 3), (3, 0)] {
            assert_eq!(g.check_pair(src, dst), Err(Error::InvalidPair { src, dst }));
        }
    }

    #[test]
    fn neighbors_follow_edge_input_order_in_both_directions() {
        let g = Graph::from_edges(4, &[(0, 2), (1, 0), (0, 3), (2, 1)]).unwrap();

        assert_eq!(g.node_count(), 4);
        assert_eq!(g.neighbors(0), &[2, 1, 3]);
        assert_eq!(g.neighbors(1), &[0, 2]);
        assert_eq!(g.neighbors(2), &[0, 1]);
        assert_eq!(g.neighbors(3), &[0]);
    }

    #[test]
    fn rejects_node_out_of_range() {
        let err = Graph::from_edges(3, &[(0, 1), (1, 3)]).unwrap_err();

        assert_eq!(
            err,
            Error::NodeOutOfRange {
                edge: 1,
                node: 3,
                node_count: 3
            }
        );
    }

    #[test]
    fn rejects_self_loop() {
        let err = Graph::from_edges(3, &[(0, 1), (2, 2)]).unwrap_err();

        assert_eq!(err, Error::SelfLoop { edge: 1, node: 2 });
    }

    #[test]
    fn rejects_duplicate_edge_in_either_direction() {
        let err = Graph::from_edges(3, &[(0, 1), (1, 2), (1, 0)]).unwrap_err();

        assert_eq!(err, Error::DuplicateEdge { edge: 2, first: 0 });
    }

    #[test]
    fn fingerprint_ignores_edge_order_and_direction() {
        let a = Graph::from_edges(4, &[(0, 1), (1, 2), (2, 3), (0, 3)]).unwrap();
        let b = Graph::from_edges(4, &[(3, 0), (2, 3), (0, 1), (2, 1)]).unwrap();

        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_changes_with_node_count_or_edge_set() {
        let base = Graph::from_edges(4, &[(0, 1), (1, 2)])
            .unwrap()
            .fingerprint();
        let more_nodes = Graph::from_edges(5, &[(0, 1), (1, 2)])
            .unwrap()
            .fingerprint();
        let other_edge = Graph::from_edges(4, &[(0, 1), (1, 3)])
            .unwrap()
            .fingerprint();
        let extra_edge = Graph::from_edges(4, &[(0, 1), (1, 2), (2, 3)])
            .unwrap()
            .fingerprint();

        assert_ne!(base, more_nodes);
        assert_ne!(base, other_edge);
        assert_ne!(base, extra_edge);
    }

    /// Locks the byte format: fingerprints are stored in browsers and exported files.
    #[test]
    fn fingerprint_format_is_stable() {
        let g = Graph::from_edges(3, &[(2, 0), (1, 2), (1, 0)]).unwrap();

        let mut bytes = b"netavail-graph-v1".to_vec();
        for x in [3u32, 3, 0, 1, 0, 2, 1, 2] {
            bytes.extend_from_slice(&x.to_le_bytes()); // n, m, then sorted edges
        }
        assert_eq!(g.fingerprint().as_bytes(), blake3::hash(&bytes).as_bytes());
    }
}
