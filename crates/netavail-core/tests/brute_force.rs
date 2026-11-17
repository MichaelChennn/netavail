//! Brute force on random small graphs (D34 layer 3): enumerates all 2^n node
//! states, so it does not depend on the old code.

use netavail_core::types::{NodeId, NodeSet, SetFamily, SetKind};
use netavail_core::{Graph, NoControl, ProbabilityMap, registry};
use proptest::prelude::*;

/// Node subsets are bit masks: bit v set means node v is in the set (or up).
type Mask = u32;

fn has(mask: Mask, v: NodeId) -> bool {
    mask >> v & 1 == 1
}

fn to_set(n: u32, mask: Mask) -> NodeSet {
    (0..n).filter(|&v| has(mask, v)).collect()
}

/// Whether `dst` is reachable from `src` through nodes in `up` only.
fn connected(g: &Graph, up: Mask, src: NodeId, dst: NodeId) -> bool {
    if !has(up, src) || !has(up, dst) {
        return false;
    }
    let mut seen: Mask = 1 << src;
    let mut stack = vec![src];
    while let Some(u) = stack.pop() {
        for &v in g.neighbors(u) {
            if has(up, v) && !has(seen, v) {
                seen |= 1 << v;
                stack.push(v);
            }
        }
    }
    has(seen, dst)
}

/// Sets that satisfy `ok` but lose it when any single node is removed.
/// `ok` must be monotone (supersets of a satisfying set also satisfy it).
fn minimal(n: u32, ok: impl Fn(Mask) -> bool) -> Vec<NodeSet> {
    (0..1 << n)
        .filter(|&s| ok(s) && (0..n).all(|v| !has(s, v) || !ok(s & !(1 << v))))
        .map(|s| to_set(n, s))
        .collect()
}

fn minimal_paths(g: &Graph, src: NodeId, dst: NodeId) -> Vec<NodeSet> {
    minimal(g.node_count(), |s| connected(g, s, src, dst))
}

/// Includes the single-node cuts {src} and {dst}, like the reference CSVs.
fn minimal_cuts(g: &Graph, src: NodeId, dst: NodeId) -> Vec<NodeSet> {
    let n = g.node_count();
    let all: Mask = (1 << n) - 1;
    let ends: Mask = 1 << src | 1 << dst;
    let mut cuts = minimal(n, |s| s & ends == 0 && !connected(g, all & !s, src, dst));
    cuts.extend([vec![src], vec![dst]]);
    cuts
}

/// Sum of the probabilities of all node states in which src reaches dst.
fn exact_availability(g: &Graph, src: NodeId, dst: NodeId, probs: &[f64]) -> f64 {
    let n = g.node_count();
    (0..1 << n)
        .filter(|&up| connected(g, up, src, dst))
        .map(|up| {
            (0..n)
                .map(|v| {
                    let p = probs[v as usize];
                    if has(up, v) { p } else { 1.0 - p }
                })
                .product::<f64>()
        })
        .sum()
}

fn family(kind: SetKind, src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
    SetFamily {
        kind,
        src,
        dst,
        sets,
    }
}

/// Checks the oracle itself on a graph small enough to count by hand.
#[test]
fn brute_force_matches_hand_count_on_chorded_square() {
    let g = Graph::from_edges(4, &[(0, 3), (3, 2), (2, 1), (1, 0), (1, 3)]).unwrap();

    let paths = family(SetKind::Paths, 0, 2, minimal_paths(&g, 0, 2));
    let cuts = family(SetKind::Cuts, 0, 2, minimal_cuts(&g, 0, 2));
    let avail = exact_availability(&g, 0, 2, &[0.9, 0.8, 0.9, 0.7]);

    assert_eq!(paths.normalize().sets, vec![vec![0, 1, 2], vec![0, 2, 3]]);
    assert_eq!(cuts.normalize().sets, vec![vec![0], vec![2], vec![1, 3]]);
    // p0 * p2 * (1 - q1 * q3)
    assert!(
        (avail - 0.9 * 0.9 * (1.0 - 0.2 * 0.3)).abs() < 1e-15,
        "{avail}"
    );
}

#[derive(Debug)]
struct Case {
    n: u32,
    edges: Vec<(NodeId, NodeId)>,
    src: NodeId,
    dst: NodeId,
    probs: Vec<f64>,
}

/// A connected graph (random spanning tree plus random extra edges, shuffled),
/// one random node pair and a random availability per node.
fn case() -> impl Strategy<Value = Case> {
    (2u32..=12, 0.0..0.6f64).prop_flat_map(|(n, density)| {
        let parents: Vec<_> = (1..n).map(|v| 0..v).collect();
        let pairs = (n * (n - 1) / 2) as usize;
        (
            parents,
            prop::collection::vec(prop::bool::weighted(density), pairs),
            0..n,
            0..n - 1,
            prop::collection::vec(0.5..1.0f64, n as usize),
        )
            .prop_flat_map(move |(parents, extra, src, dst, probs)| {
                let mut edges: Vec<_> = (1..n).map(|v| (parents[v as usize - 1], v)).collect();
                let all_pairs = (0..n).flat_map(|u| (u + 1..n).map(move |v| (u, v)));
                for ((u, v), add) in all_pairs.zip(extra) {
                    if add && parents[v as usize - 1] != u {
                        edges.push((u, v));
                    }
                }
                let dst = if dst >= src { dst + 1 } else { dst };
                Just(edges).prop_shuffle().prop_map(move |edges| Case {
                    n,
                    edges,
                    src,
                    dst,
                    probs: probs.clone(),
                })
            })
    })
}

proptest! {
    #[test]
    fn path_algorithms_find_exactly_the_minimal_path_sets(c in case()) {
        let g = Graph::from_edges(c.n, &c.edges).unwrap();
        let expected = family(SetKind::Paths, c.src, c.dst, minimal_paths(&g, c.src, c.dst));

        for algo in registry::path_algorithms() {
            let actual = algo.find(&g, c.src, c.dst, &NoControl).unwrap();
            prop_assert_eq!(&actual.normalize(), &expected.normalize(), "{}", algo.info().id);
        }
    }

    #[test]
    fn availability_algorithms_match_exact_availability(c in case()) {
        let g = Graph::from_edges(c.n, &c.edges).unwrap();
        let exact = exact_availability(&g, c.src, c.dst, &c.probs);
        let probs = ProbabilityMap::from_nodes(&c.probs);

        for algo in registry::availability_algorithms() {
            let sets = match algo.input() {
                SetKind::Paths => minimal_paths(&g, c.src, c.dst),
                SetKind::Cuts => minimal_cuts(&g, c.src, c.dst),
            };
            let input = family(algo.input(), c.src, c.dst, sets);
            let avail = algo.prepare(&input, &NoControl).unwrap().evaluate(&probs);
            prop_assert!((avail - exact).abs() < 1e-12, "{}: {} vs {}", algo.info().id, avail, exact);
        }
    }
}
