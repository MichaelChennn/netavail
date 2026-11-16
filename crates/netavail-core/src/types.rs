//! Core types (architecture.md §3.1; D7, D8, D22).

use std::cmp::Ordering;

/// Node index, starting at 0.
pub type NodeId = u32;

/// A node together with its state (up or down).
///
/// Stored as `node << 1 | is_down`, so it doubles as a `ProbabilityMap` index.
/// Sorting does not follow this value; see the `Ord` impl.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Lit(pub(crate) u32);

impl Lit {
    /// The node is up.
    pub fn up(node: NodeId) -> Lit {
        Lit(node << 1)
    }

    /// The node is down.
    pub fn down(node: NodeId) -> Lit {
        Lit(node << 1 | 1)
    }

    pub fn node(self) -> NodeId {
        self.0 >> 1
    }

    pub fn is_down(self) -> bool {
        self.0 & 1 == 1
    }

    /// The signed int the C++ code uses: `node+1` if up, `-(node+1)` if down.
    fn cpp_key(self) -> i64 {
        let n = i64::from(self.node()) + 1;
        if self.is_down() { -n } else { n }
    }
}

/// Same order as the C++ signed ints (D8): down nodes first with the highest
/// node first, then up nodes with the lowest node first. SDP relies on this.
impl Ord for Lit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cpp_key().cmp(&other.cpp_key())
    }
}

impl PartialOrd for Lit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One path or one cut set. A path lists nodes in travel order, src and dst included.
pub type NodeSet = Vec<NodeId>;

/// A product term in availability computation.
pub type Term = Vec<Lit>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SetKind {
    Paths,
    Cuts,
}

/// All path sets or all cut sets of one node pair.
///
/// `sets` keeps the order the algorithm produced; call [`SetFamily::normalize`] before comparing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SetFamily {
    pub kind: SetKind,
    pub src: NodeId,
    pub dst: NodeId,
    pub sets: Vec<NodeSet>,
}

impl SetFamily {
    /// Returns a canonical copy for comparison (D3): each set sorted ascending,
    /// sets ordered by length, then lexicographically.
    pub fn normalize(&self) -> SetFamily {
        let mut sets = self.sets.clone();
        for set in &mut sets {
            set.sort_unstable();
        }
        sets.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        SetFamily {
            kind: self.kind,
            src: self.src,
            dst: self.dst,
            sets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_sorts_like_cpp_signed_ints() {
        let mut lits = vec![Lit::up(2), Lit::down(0), Lit::up(0), Lit::down(2)];
        lits.sort();
        assert_eq!(
            lits,
            vec![Lit::down(2), Lit::down(0), Lit::up(0), Lit::up(2)]
        );
    }

    /// Node 0 going down must be representable (the old +1 offset existed for this).
    #[test]
    fn lit_round_trips_node_and_state() {
        for (lit, node, is_down) in [
            (Lit::up(0), 0, false),
            (Lit::down(0), 0, true),
            (Lit::up(5), 5, false),
            (Lit::down(5), 5, true),
        ] {
            assert_eq!((lit.node(), lit.is_down()), (node, is_down), "{lit:?}");
        }
    }

    #[test]
    fn normalize_sorts_within_and_across_sets() {
        let family = SetFamily {
            kind: SetKind::Paths,
            src: 0,
            dst: 3,
            sets: vec![vec![3, 1, 2], vec![2, 0], vec![0, 1]],
        };
        let original = family.clone();

        let normalized = family.normalize();

        assert_eq!(
            normalized,
            SetFamily {
                kind: SetKind::Paths,
                src: 0,
                dst: 3,
                sets: vec![vec![0, 1], vec![0, 2], vec![1, 2, 3]],
            }
        );
        // The original keeps its algorithm order.
        assert_eq!(family, original);
    }
}
