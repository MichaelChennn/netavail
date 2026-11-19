//! SDP availability, ported from PyRBD-Suite `availability/sdp.cpp` (old name `sdp`).
//!
//! Sum of disjoint products over the minimal paths: path i contributes
//! P_i AND NOT(P_1 \ P_i) AND ... AND NOT(P_{i-1} \ P_i). Each NOT-group ("complementary")
//! means "not all of these nodes are up". Groups that share nodes are decomposed until all
//! factors of a product are independent, so availability = sum of products of factors.

use std::collections::VecDeque;

use crate::availability::{AvailabilityAlgorithm, Prepared};
use crate::control::Control;
use crate::error::Error;
use crate::prob::ProbabilityMap;
use crate::registry::{AlgoInfo, Citation};
use crate::types::{Lit, NodeId, NodeSet, SetFamily, SetKind};

pub struct PathSdp;

const CITATION: Citation = Citation {
    text: "S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, \"Leveraging Minimal Path \
           Sets for Reliability Block Diagram Evaluation\", IEEE FNWF, 2025",
    url: Some("https://ieeexplore.ieee.org/document/11317672"),
};

static INFO: AlgoInfo = AlgoInfo {
    id: "path_sdp",
    name: "SDP",
    summary: "Sum of disjoint products over sorted minimal path sets, with decomposition",
    version: 1,
    aliases: &["sdp"],
    citation: Some(CITATION),
};

impl AvailabilityAlgorithm for PathSdp {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn input(&self) -> SetKind {
        SetKind::Paths
    }

    fn prepare(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<Box<dyn Prepared>, Error> {
        if paths.kind != SetKind::Paths {
            return Err(Error::WrongInputKind {
                expected: SetKind::Paths,
                found: paths.kind,
            });
        }
        // No path (src and dst disconnected) gives no product, so availability 0
        let products = to_sdp_set(paths.sets.clone(), ctl)?;
        Ok(Box::new(SdpProducts { products }))
    }
}

/// [`PathSdp`] with the paths split across rayon's threads, ported from `toSDPSetParallel`.
/// Same products in the same order, so the same availability bit for bit.
#[cfg(feature = "parallel")]
pub struct PathSdpParallel;

#[cfg(feature = "parallel")]
static PARALLEL_INFO: AlgoInfo = AlgoInfo {
    id: "path_sdp_parallel",
    name: "SDP (parallel)",
    summary: "SDP with the products of each path computed in parallel; from 200 paths up",
    version: 1,
    aliases: &[],
    citation: Some(CITATION),
};

#[cfg(feature = "parallel")]
impl AvailabilityAlgorithm for PathSdpParallel {
    fn info(&self) -> &'static AlgoInfo {
        &PARALLEL_INFO
    }

    fn input(&self) -> SetKind {
        SetKind::Paths
    }

    fn prepare(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<Box<dyn Prepared>, Error> {
        if paths.kind != SetKind::Paths {
            return Err(Error::WrongInputKind {
                expected: SetKind::Paths,
                found: paths.kind,
            });
        }
        let products = to_sdp_set_parallel(paths.sets.clone(), ctl)?;
        Ok(Box::new(SdpProducts { products }))
    }
}

/// One factor of a product: all nodes of `set` are up, or (complementary) not all are.
/// `set` is always sorted.
#[derive(Clone, Debug, PartialEq)]
struct Sdp {
    complementary: bool,
    set: NodeSet,
}

impl Sdp {
    fn normal(set: NodeSet) -> Sdp {
        Sdp {
            complementary: false,
            set,
        }
    }

    fn complementary(set: NodeSet) -> Sdp {
        Sdp {
            complementary: true,
            set,
        }
    }
}

/// A product of independent factors.
type SdpSets = Vec<Sdp>;

/// Elements of sorted `a` not in sorted `b`.
fn difference(a: &[NodeId], b: &[NodeId]) -> NodeSet {
    a.iter()
        .copied()
        .filter(|e| b.binary_search(e).is_err())
        .collect()
}

/// Elements of sorted `a` also in sorted `b`.
fn intersection(a: &[NodeId], b: &[NodeId]) -> NodeSet {
    a.iter()
        .copied()
        .filter(|e| b.binary_search(e).is_ok())
        .collect()
}

/// Both factors of the same kind and every element of `sdp1` in `sdp2`.
fn is_sub_set(sdp1: &Sdp, sdp2: &Sdp) -> bool {
    sdp1.complementary == sdp2.complementary
        && sdp1.set.iter().all(|e| sdp2.set.binary_search(e).is_ok())
}

/// The first pair of complementary factors that share nodes, with the shared nodes.
fn common_pair(sdps: &[Sdp]) -> Option<(usize, usize, NodeSet)> {
    let comps: Vec<usize> = (0..sdps.len()).filter(|&i| sdps[i].complementary).collect();
    for (k, &i) in comps.iter().enumerate() {
        for &j in &comps[k + 1..] {
            let common = intersection(&sdps[i].set, &sdps[j].set);
            if !common.is_empty() {
                return Some((i, j, common));
            }
        }
    }
    None
}

/// Eliminate redundant elements from complementary SDP sets: nodes of a normal factor
/// are up, so they drop out of every complementary factor.
fn eliminate_sdp_set(mut sdp_sets: SdpSets) -> SdpSets {
    sdp_sets.sort_by_key(|sdp| sdp.complementary);

    let mut eliminated_elements: NodeSet = Vec::new();
    let mut eliminated_set = Vec::with_capacity(sdp_sets.len());
    for sdp in sdp_sets {
        if !sdp.complementary {
            eliminated_elements.extend_from_slice(&sdp.set);
            eliminated_elements.sort_unstable();
            eliminated_set.push(sdp);
        } else {
            // An empty factor is "not all of nothing up", so it keeps the product at 0
            let new_set = difference(&sdp.set, &eliminated_elements);
            eliminated_set.push(Sdp::complementary(new_set));
        }
    }
    eliminated_set
}

/// Absorb supersets within an SDP family.
fn absorb_sdp_set(sdp_sets: SdpSets) -> SdpSets {
    let mut absorbed = vec![false; sdp_sets.len()];
    let mut absorbed_sdps = Vec::with_capacity(sdp_sets.len());

    for i in 0..sdp_sets.len() {
        if absorbed[i] {
            continue;
        }
        let current = &sdp_sets[i];
        for j in i + 1..sdp_sets.len() {
            if absorbed[j] {
                continue;
            }
            let other = &sdp_sets[j];
            // Equal sets count as a subset
            if is_sub_set(current, other) {
                absorbed[j] = true;
            } else if is_sub_set(other, current) {
                absorbed[i] = true;
                break;
            }
        }
        if !absorbed[i] {
            absorbed_sdps.push(sdp_sets[i].clone());
        }
    }
    absorbed_sdps
}

/// Recursively decompose SDP sets with common complementary elements C:
/// NOT(C + A) AND NOT(C + B) = NOT(C)  OR  C AND NOT(A) AND NOT(B).
fn decompose_sdp_set(
    sdp_sets: SdpSets,
    done: usize,
    ctl: &dyn Control,
) -> Result<Vec<SdpSets>, Error> {
    let mut results = Vec::new();
    let mut queue = VecDeque::from([sdp_sets]);

    while let Some(mut current) = queue.pop_front() {
        ctl.check()?;
        let Some((i, j, common_elements)) = common_pair(&current) else {
            results.push(current);
            continue;
        };

        // Take out the pair, the rest stays as is
        let mut second = current.remove(j);
        let mut first = current.remove(i);
        first
            .set
            .retain(|e| common_elements.binary_search(e).is_err());
        second
            .set
            .retain(|e| common_elements.binary_search(e).is_err());

        let mut decomposed1 = current.clone();
        decomposed1.push(Sdp::complementary(common_elements.clone()));

        let mut decomposed2 = current;
        decomposed2.push(Sdp::normal(common_elements));
        decomposed2.push(first);
        decomposed2.push(second);

        queue.push_back(absorb_sdp_set(eliminate_sdp_set(decomposed1)));
        queue.push_back(absorb_sdp_set(eliminate_sdp_set(decomposed2)));
        check_terms(done + results.len() + queue.len(), ctl)?;
    }
    Ok(results)
}

/// Sort path sets for optimal SDP processing: by length, and within a length the paths
/// sharing the fewest nodes with any shorter path first.
fn sort_path_set(mut path_sets: Vec<NodeSet>) -> Vec<NodeSet> {
    for set in &mut path_sets {
        set.sort_unstable();
    }
    path_sets.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

    let mut sorted_path_set: Vec<NodeSet> = Vec::with_capacity(path_sets.len());
    for (k, group) in path_sets.chunk_by(|a, b| a.len() == b.len()).enumerate() {
        let mut group = group.to_vec();
        if k > 0 {
            let max_common = |set: &NodeSet| {
                let common = |preced: &NodeSet| set.iter().filter(|e| preced.contains(e)).count();
                sorted_path_set.iter().map(common).max().unwrap_or(0)
            };
            // Stable, so ties keep lexicographic order (C++ std::sort leaves it unspecified)
            group.sort_by_cached_key(max_common);
        }
        sorted_path_set.append(&mut group);
    }
    sorted_path_set
}

/// Convert path sets to SDP sets.
fn to_sdp_set(path_sets: Vec<NodeSet>, ctl: &dyn Control) -> Result<Vec<SdpSets>, Error> {
    let sorted_path_set = sort_path_set(path_sets);
    let Some(first) = sorted_path_set.first() else {
        return Ok(Vec::new());
    };

    let mut final_sdps = vec![vec![Sdp::normal(first.clone())]];
    for i in 1..sorted_path_set.len() {
        let mut products = path_products(&sorted_path_set, i, final_sdps.len(), ctl)?;
        final_sdps.append(&mut products);
        check_terms(final_sdps.len(), ctl)?;
    }
    Ok(final_sdps)
}

/// Path i AND NOT(each preceding path), split into independent products;
/// `done` products exist already, for the term limit.
fn path_products(
    sorted_path_set: &[NodeSet],
    i: usize,
    done: usize,
    ctl: &dyn Control,
) -> Result<Vec<SdpSets>, Error> {
    ctl.check()?;
    let current_set = &sorted_path_set[i];
    let mut result_sdps = vec![Sdp::normal(current_set.clone())];
    for preceding_set in &sorted_path_set[..i] {
        result_sdps.push(Sdp::complementary(difference(preceding_set, current_set)));
    }
    // A preceding path inside this one: the product is impossible (non-minimal input)
    if result_sdps
        .iter()
        .any(|sdp| sdp.complementary && sdp.set.is_empty())
    {
        return Ok(Vec::new());
    }

    let result_sdps = absorb_sdp_set(result_sdps);
    if common_pair(&result_sdps).is_some() {
        decompose_sdp_set(result_sdps, done, ctl)
    } else {
        Ok(vec![result_sdps])
    }
}

/// Below this many paths threads cost more than they save, so [`to_sdp_set`] runs (as in C++).
#[cfg(feature = "parallel")]
const PARALLEL_MIN_PATHS: usize = 200;

/// [`to_sdp_set`] with each path's products computed in parallel, then joined in path order.
/// The term limit applies per path while running and to the total at the end.
#[cfg(feature = "parallel")]
fn to_sdp_set_parallel(path_sets: Vec<NodeSet>, ctl: &dyn Control) -> Result<Vec<SdpSets>, Error> {
    use rayon::prelude::*;

    if path_sets.len() < PARALLEL_MIN_PATHS {
        return to_sdp_set(path_sets, ctl);
    }
    let sorted_path_set = sort_path_set(path_sets);
    let per_path = (1..sorted_path_set.len())
        .into_par_iter()
        .map(|i| path_products(&sorted_path_set, i, 0, ctl))
        .collect::<Result<Vec<_>, Error>>()?;

    let mut final_sdps = vec![vec![Sdp::normal(sorted_path_set[0].clone())]];
    final_sdps.extend(per_path.into_iter().flatten());
    check_terms(final_sdps.len(), ctl)?;
    Ok(final_sdps)
}

/// Products can grow exponentially; the browser sets a limit so memory stays bounded.
fn check_terms(len: usize, ctl: &dyn Control) -> Result<(), Error> {
    match ctl.max_terms() {
        Some(limit) if len > limit => Err(Error::TooManyTerms { limit }),
        _ => Ok(()),
    }
}

/// Mutually exclusive products whose probabilities sum to the availability.
struct SdpProducts {
    products: Vec<SdpSets>,
}

impl Prepared for SdpProducts {
    fn evaluate(&self, p: &ProbabilityMap) -> f64 {
        let all_up = |set: &NodeSet| set.iter().map(|&n| p.get(Lit::up(n))).product::<f64>();
        let factor = |sdp: &Sdp| {
            if sdp.complementary {
                1.0 - all_up(&sdp.set)
            } else {
                all_up(&sdp.set)
            }
        };
        self.products
            .iter()
            .map(|product| product.iter().map(factor).product::<f64>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    fn avail(family: &SetFamily, p: &[f64]) -> f64 {
        PathSdp
            .prepare(family, &NoControl)
            .unwrap()
            .evaluate(&ProbabilityMap::from_nodes(p))
    }

    /// Two parallel chains 0-1-3-5 and 0-2-4-5.
    #[test]
    fn matches_closed_form_on_two_parallel_chains() {
        let family = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);
        let p = [0.95, 0.9, 0.8, 0.7, 0.6, 0.85];

        let got = avail(&family, &p);

        let expected = p[0] * p[5] * (1.0 - (1.0 - p[1] * p[3]) * (1.0 - p[2] * p[4]));
        assert!((got - expected).abs() < 1e-12, "{got} vs {expected}");
    }

    /// A duplicate and a non-minimal path add nothing.
    #[test]
    fn ignores_duplicate_and_non_minimal_paths() {
        let family = paths(0, 2, vec![vec![0, 2], vec![0, 1, 2], vec![0, 2]]);
        let p = [0.9, 0.8, 0.7];

        let got = avail(&family, &p);

        assert!((got - p[0] * p[2]).abs() < 1e-12, "{got}");
    }

    /// Paths 0-1-4-6, 0-2-4-6, 0-3-5-6: the last path's complements {1, 4} and {2, 4}
    /// share node 4, so the decomposition runs.
    #[test]
    fn matches_closed_form_when_complements_overlap() {
        let family = paths(
            0,
            6,
            vec![vec![0, 1, 4, 6], vec![0, 2, 4, 6], vec![0, 3, 5, 6]],
        );
        let p = [0.95, 0.9, 0.8, 0.7, 0.6, 0.85, 0.75];

        let got = avail(&family, &p);

        let via_4 = p[4] * (1.0 - (1.0 - p[1]) * (1.0 - p[2]));
        let expected = p[0] * p[6] * (1.0 - (1.0 - via_4) * (1.0 - p[3] * p[5]));
        assert!((got - expected).abs() < 1e-12, "{got} vs {expected}");
    }

    /// NOT{1,2} AND NOT{2,3} share 2: NOT{2}, or 2 up with NOT{1} AND NOT{3}.
    #[test]
    fn decomposes_complements_that_share_nodes() {
        let input = vec![
            Sdp::normal(vec![4]),
            Sdp::complementary(vec![1, 2]),
            Sdp::complementary(vec![2, 3]),
        ];

        let products = decompose_sdp_set(input, 0, &NoControl).unwrap();

        let expected = vec![
            vec![Sdp::normal(vec![4]), Sdp::complementary(vec![2])],
            vec![
                Sdp::normal(vec![4]),
                Sdp::normal(vec![2]),
                Sdp::complementary(vec![1]),
                Sdp::complementary(vec![3]),
            ],
        ];
        assert_eq!(products, expected);
    }

    /// Length 4: {0,1,2,5} shares 3 nodes with {0,1,5}, so it goes after {0,3,4,5} and
    /// {0,5,6,7}, which share 2 each and keep lexicographic order.
    #[test]
    fn sorts_by_length_then_fewest_shared_nodes() {
        let sorted = sort_path_set(vec![
            vec![5, 1, 2, 0],
            vec![0, 6, 7, 8, 5],
            vec![0, 3, 4, 5],
            vec![0, 1, 5],
            vec![0, 2, 5],
            vec![7, 6, 5, 0],
        ]);

        let expected = vec![
            vec![0, 1, 5],
            vec![0, 2, 5],
            vec![0, 3, 4, 5],
            vec![0, 5, 6, 7],
            vec![0, 1, 2, 5],
            vec![0, 5, 6, 7, 8],
        ];
        assert_eq!(sorted, expected);
    }

    #[test]
    fn adjacent_pair_needs_only_both_endpoints() {
        let got = avail(&paths(0, 1, vec![vec![0, 1]]), &[0.9, 0.8]);

        assert!((got - 0.72).abs() < 1e-12, "{got}");
    }

    #[test]
    fn disconnected_pair_has_zero_availability() {
        let got = avail(&paths(0, 1, vec![]), &[0.9, 0.8]);

        assert_eq!(got, 0.0);
    }

    #[test]
    fn rejects_cut_sets() {
        let mut family = paths(0, 1, vec![vec![0], vec![1]]);
        family.kind = SetKind::Cuts;

        let err = PathSdp.prepare(&family, &NoControl).err().unwrap();

        assert_eq!(
            err,
            Error::WrongInputKind {
                expected: SetKind::Paths,
                found: SetKind::Cuts
            }
        );
    }

    struct CancelNow;
    impl Control for CancelNow {
        fn check(&self) -> Result<(), Cancelled> {
            Err(Cancelled)
        }
    }

    struct MaxTerms(usize);
    impl Control for MaxTerms {
        fn max_terms(&self) -> Option<usize> {
            Some(self.0)
        }
    }

    /// The two-chain paths give 2 products, one per path.
    #[test]
    fn fails_once_terms_exceed_max_terms() {
        let family = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = PathSdp.prepare(&family, &MaxTerms(1)).err();
        let ok = PathSdp.prepare(&family, &MaxTerms(2));

        assert_eq!(err, Some(Error::TooManyTerms { limit: 1 }));
        assert!(ok.is_ok());
    }

    #[test]
    fn stops_when_cancelled() {
        let family = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = PathSdp.prepare(&family, &CancelNow).err().unwrap();

        assert_eq!(err, Error::Cancelled);
    }
}

#[cfg(all(test, feature = "parallel"))]
mod parallel_tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::graph::Graph;
    use crate::registry;
    use crate::sets::minimalpaths::{Dfs, PathAlgorithm};

    /// Minimal paths between opposite corners of an r x c grid; 3 x 9 gives 403.
    fn grid_paths(r: u32, c: u32) -> SetFamily {
        let id = |i: u32, j: u32| i * c + j;
        let mut edges = Vec::new();
        for i in 0..r {
            for j in 0..c {
                if j + 1 < c {
                    edges.push((id(i, j), id(i, j + 1)));
                }
                if i + 1 < r {
                    edges.push((id(i, j), id(i + 1, j)));
                }
            }
        }
        let g = Graph::from_edges(r * c, &edges).unwrap();
        Dfs.find(&g, 0, r * c - 1, &NoControl).unwrap()
    }

    #[test]
    fn parallel_gives_the_same_products_as_serial() {
        for (r, c) in [(3, 8), (3, 9)] {
            let family = grid_paths(r, c);

            let serial = to_sdp_set(family.sets.clone(), &NoControl).unwrap();
            let parallel = to_sdp_set_parallel(family.sets.clone(), &NoControl).unwrap();

            assert_eq!(parallel, serial, "{r} x {c}");
        }
    }

    #[test]
    fn parallel_availability_is_bit_identical_and_registered() {
        let family = grid_paths(3, 9);
        let p = ProbabilityMap::uniform(27, 0.9);
        let algo = registry::find_availability("path_sdp_parallel").unwrap();

        let got = algo.prepare(&family, &NoControl).unwrap().evaluate(&p);
        let want = PathSdp.prepare(&family, &NoControl).unwrap().evaluate(&p);

        assert_eq!(got.to_bits(), want.to_bits());
        assert_eq!(algo.input(), SetKind::Paths);
    }

    struct CancelNow;
    impl Control for CancelNow {
        fn check(&self) -> Result<(), Cancelled> {
            Err(Cancelled)
        }
    }

    #[test]
    fn parallel_stops_when_cancelled() {
        let err = to_sdp_set_parallel(grid_paths(3, 9).sets, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }
}
