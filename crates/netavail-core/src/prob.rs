//! Node availability probabilities (architecture.md §3.1; D22).

use crate::types::Lit;

/// Probability of each node being up (p) or down (1-p).
///
/// Stored interleaved as `[p0, 1-p0, p1, 1-p1, ...]`, indexed by the `Lit` value,
/// so a lookup is a single array access with no branch.
#[derive(Clone, PartialEq, Debug)]
pub struct ProbabilityMap {
    values: Vec<f64>,
}

impl ProbabilityMap {
    /// Every node has the same availability `p`.
    pub fn uniform(node_count: usize, p: f64) -> ProbabilityMap {
        debug_assert!((0.0..=1.0).contains(&p), "probability out of range: {p}");
        ProbabilityMap {
            values: [p, 1.0 - p].repeat(node_count),
        }
    }

    /// `node_probs[i]` is the availability of node `i`.
    pub fn from_nodes(node_probs: &[f64]) -> ProbabilityMap {
        let mut values = Vec::with_capacity(node_probs.len() * 2);
        for &p in node_probs {
            debug_assert!((0.0..=1.0).contains(&p), "probability out of range: {p}");
            values.extend([p, 1.0 - p]);
        }
        ProbabilityMap { values }
    }

    /// Number of nodes this map has probabilities for.
    pub fn node_count(&self) -> usize {
        self.values.len() / 2
    }

    /// p if `lit` is up, 1-p if down. Panics if the node is out of range.
    pub fn get(&self, lit: Lit) -> f64 {
        self.values[lit.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1-p is computed once at construction, so it matches C++ bit for bit.
    #[test]
    fn uniform_gives_every_node_p_and_one_minus_p() {
        let probs = ProbabilityMap::uniform(3, 0.9);
        for node in 0..3 {
            assert_eq!(probs.get(Lit::up(node)), 0.9);
            // 1.0 - 0.9 in IEEE 754 double
            assert_eq!(probs.get(Lit::down(node)), 0.09999999999999998);
        }
    }

    #[test]
    fn from_nodes_keeps_each_node_separate() {
        let probs = ProbabilityMap::from_nodes(&[0.5, 0.9, 0.99]);
        let expected = [
            (0, 0.5, 0.5),
            (1, 0.9, 0.09999999999999998),
            (2, 0.99, 0.010000000000000009),
        ];
        for (node, up, down) in expected {
            assert_eq!(probs.get(Lit::up(node)), up, "up({node})");
            assert_eq!(probs.get(Lit::down(node)), down, "down({node})");
        }
    }
}
