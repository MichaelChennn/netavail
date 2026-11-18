//! Combination, ported from PyRBD-Suite `sets/cutsets_combination.cpp`.
//!
//! Uses incidence matrix approach to find combinations of nodes that disconnect all paths:
//! combinations of growing size that hit every path and contain no smaller cut are cuts.
//!
//! Differences from the C++ (same cuts wherever the C++ finds them all):
//! - Candidates are the nodes on some path; a node on no path is in no minimal cut.
//! - No `order` limit (C++ default ceil(n/2) misses larger cuts, e.g. src and dst joined
//!   through 4 separate nodes: the only inner cut has 4 of 6 nodes). Instead the search
//!   stops once no combination of the current size can grow into a minimal cut.

use crate::control::Control;
use crate::error::Error;
use crate::registry::{AlgoInfo, Citation};
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalcuts::common::{
    CutInput, check_size, next_combination, prepare_paths, with_endpoints,
};
use crate::types::{NodeId, NodeSet, SetFamily};

pub struct Combination;

static INFO: AlgoInfo = AlgoInfo {
    id: "combination",
    name: "Combination",
    summary: "Try node combinations of growing size and keep those that cut every path",
    version: 1,
    aliases: &[],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, C. Mas-Machuca, \"PyRBD++: An Open-Source Fast \
               Reliability Block Diagram Evaluation Tool\", RNDM, 2025",
        url: None,
    }),
};

impl CutAlgorithm for Combination {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
        let clauses = match prepare_paths(paths)? {
            CutInput::Done(cuts) => return Ok(cuts),
            CutInput::Clauses(clauses) => clauses,
        };
        let minimal = combination_cuts(&clauses, ctl)?;
        let cuts = with_endpoints(paths, minimal);
        check_size(cuts.sets.len(), ctl)?;
        Ok(cuts)
    }
}

/// Inner cuts of the paths `clauses` (src and dst removed, each sorted).
fn combination_cuts(clauses: &[NodeSet], ctl: &dyn Control) -> Result<Vec<NodeSet>, Error> {
    // Build valid nodes (all nodes on some path, except src and dst)
    let mut valid_nodes: Vec<NodeId> = clauses.iter().flatten().copied().collect();
    valid_nodes.sort_unstable();
    valid_nodes.dedup();

    // Build incidence matrix: paths × valid_nodes
    // incidence[p][n] = true if node n is in path p
    let incidence: Vec<Vec<bool>> = clauses
        .iter()
        .map(|path| {
            let on_path = |n: &NodeId| path.binary_search(n).is_ok();
            valid_nodes.iter().map(on_path).collect()
        })
        .collect();

    let mut minimal: Vec<NodeSet> = Vec::new();

    // Find order-1 cut sets (single nodes that are in ALL paths)
    let mut firstpairs: Vec<usize> = Vec::new();
    for n in 0..valid_nodes.len() {
        if incidence.iter().all(|row| row[n]) {
            minimal.push(vec![valid_nodes[n]]);
        } else {
            firstpairs.push(n);
        }
    }

    // Iteratively find higher-order cut sets; a minimal cut needs one own path per node
    let max_order = clauses.len().min(firstpairs.len());
    let mut hits = vec![0usize; clauses.len()];
    for combo_size in 2..=max_order {
        let mut newpairs = Vec::new();
        let mut can_grow = false;

        // Generate all combinations of size combo_size from firstpairs
        let mut indices: Vec<usize> = (0..combo_size).collect();
        loop {
            ctl.check()?;
            let cols: Vec<usize> = indices.iter().map(|&i| firstpairs[i]).collect();
            let combo: NodeSet = cols.iter().map(|&c| valid_nodes[c]).collect();

            // Check if combo is a superset of any existing minimal cut set
            let is_superset = minimal
                .iter()
                .any(|m| m.iter().all(|e| combo.binary_search(e).is_ok()));

            if !is_superset && every_node_has_own_path(&incidence, &cols, &mut hits) {
                can_grow = true;
                // Covers all paths: no path is missed by the combo
                if hits.iter().all(|&h| h > 0) {
                    newpairs.push(combo);
                }
            }

            // Next combination
            if !next_combination(&mut indices, firstpairs.len()) {
                break;
            }
        }

        minimal.append(&mut newpairs);
        check_size(minimal.len(), ctl)?;
        // Every larger combination contains one of this size, so none can be minimal
        if !can_grow {
            break;
        }
    }
    Ok(minimal)
}

/// Fills `hits` with how many of `cols` each path contains, and checks that every
/// column is the only one on some path (otherwise no superset is a minimal cut).
fn every_node_has_own_path(incidence: &[Vec<bool>], cols: &[usize], hits: &mut [usize]) -> bool {
    for (h, row) in hits.iter_mut().zip(incidence) {
        *h = cols.iter().filter(|&&c| row[c]).count();
    }
    cols.iter().all(|&c| {
        incidence
            .iter()
            .zip(hits.iter())
            .any(|(row, &h)| h == 1 && row[c])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::types::SetKind;

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    /// Two parallel chains 0-1-3-5 and 0-2-4-5: one node from each chain.
    #[test]
    fn two_parallel_chains() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let cuts = Combination.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.kind, SetKind::Cuts);
        assert_eq!(
            cuts.sets,
            vec![
                vec![0],
                vec![5],
                vec![1, 2],
                vec![1, 4],
                vec![2, 3],
                vec![3, 4]
            ]
        );
    }

    #[test]
    fn node_on_every_path_is_a_single_cut() {
        let input = paths(0, 4, vec![vec![0, 1, 2, 4], vec![0, 1, 3, 4]]);

        let cuts = Combination.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![4], vec![1], vec![2, 3]]);
    }

    /// 0 and 5 joined through 1, 2, 3 and 4: the C++ default order 3 missed this cut.
    #[test]
    fn finds_cuts_larger_than_half_the_nodes() {
        let input = paths(
            0,
            5,
            vec![vec![0, 1, 5], vec![0, 2, 5], vec![0, 3, 5], vec![0, 4, 5]],
        );

        let cuts = Combination.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![5], vec![1, 2, 3, 4]]);
    }

    struct CancelNow;
    impl Control for CancelNow {
        fn check(&self) -> Result<(), Cancelled> {
            Err(Cancelled)
        }
    }

    struct MaxSets(usize);
    impl Control for MaxSets {
        fn max_sets(&self) -> Option<usize> {
            Some(self.0)
        }
    }

    #[test]
    fn stops_when_cancelled() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = Combination.from_paths(&input, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }

    /// The two-chain pair has 6 cuts, {src} and {dst} included.
    #[test]
    fn fails_once_sets_exceed_max_sets() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = Combination.from_paths(&input, &MaxSets(5)).err();
        let ok = Combination.from_paths(&input, &MaxSets(6));

        assert_eq!(err, Some(Error::TooManySets { limit: 5 }));
        assert!(ok.is_ok());
    }
}
