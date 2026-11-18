//! Shannon expansion, ported from PyRBD-Suite `sets/cutsets_shannon.cpp`.
//!
//! Each minimal path (without src and dst) is an OR-clause; the cut sets are the minimal
//! transversals of their AND. The clauses are split once on the most common element x:
//! a cut either avoids x (every clause loses x) or contains x (only the clauses without x
//! remain, plus {x}). Both halves are multiplied out and absorbed together.

use crate::control::Control;
use crate::error::Error;
use crate::registry::{AlgoInfo, Citation};
use crate::sets::absorb::AbsorbList;
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalcuts::common::{
    CutInput, check_size, most_common_element, multiply_pathsets, prepare_paths, with_endpoints,
};
use crate::types::{NodeSet, SetFamily};

pub struct Shannon;

static INFO: AlgoInfo = AlgoInfo {
    id: "shannon",
    name: "Shannon expansion",
    summary: "Split the path CNF once on the most common node and multiply out both halves",
    version: 1,
    aliases: &[],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, \"Leveraging Minimal Path \
               Sets for Reliability Block Diagram Evaluation\", IEEE FNWF, 2025",
        url: Some("https://ieeexplore.ieee.org/document/11317672"),
    }),
};

impl CutAlgorithm for Shannon {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
        let pathsets = match prepare_paths(paths)? {
            CutInput::Done(cuts) => return Ok(cuts),
            CutInput::Clauses(pathsets) => pathsets,
        };
        let result_sets = shannon_expansion(&pathsets, ctl)?;
        let cuts = with_endpoints(paths, result_sets);
        check_size(cuts.sets.len(), ctl)?;
        Ok(cuts)
    }
}

fn shannon_expansion(pathsets: &[NodeSet], ctl: &dyn Control) -> Result<Vec<NodeSet>, Error> {
    let (pivot_elem, counts) = most_common_element(pathsets);

    if counts < 2 {
        return multiply_pathsets(pathsets, ctl);
    }

    let mut left_subset = Vec::with_capacity(pathsets.len());
    let mut right_subset = Vec::new();
    for subset in pathsets {
        // Left: remove pivot_elem from all sets
        let left_s: NodeSet = subset
            .iter()
            .copied()
            .filter(|&e| e != pivot_elem)
            .collect();
        left_subset.push(left_s);

        // Right: only sets not containing pivot_elem
        if !subset.contains(&pivot_elem) {
            right_subset.push(subset.clone());
        }
    }
    right_subset.push(vec![pivot_elem]);

    // Multiply both subsets
    let left_result = multiply_pathsets(&left_subset, ctl)?;
    let right_result = multiply_pathsets(&right_subset, ctl)?;

    // Combine with absorption
    let mut absorbed = AbsorbList::default();
    absorbed.add_many(left_result);
    absorbed.add_many(right_result);
    Ok(absorbed.into_sets())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::types::{NodeId, SetKind};

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    /// Two parallel chains 0-1-3-5 and 0-2-4-5: no shared node, so no split.
    #[test]
    fn two_parallel_chains() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let cuts = Shannon.from_paths(&input, &NoControl).unwrap();

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

    /// Paths 0-1-2-4 and 0-1-3-4 share node 1, so the clauses split on it.
    #[test]
    fn shared_node_is_a_single_cut() {
        let input = paths(0, 4, vec![vec![0, 1, 2, 4], vec![0, 1, 3, 4]]);

        let cuts = Shannon.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![4], vec![1], vec![2, 3]]);
    }

    /// Path 0-1-3 has only the pivot 1 inside, so the left half has an empty clause
    /// and every cut must contain 1.
    #[test]
    fn clause_of_only_the_pivot_forces_it_into_every_cut() {
        let input = paths(0, 3, vec![vec![0, 1, 3], vec![0, 1, 2, 3]]);

        let cuts = Shannon.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![3], vec![1]]);
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

        let err = Shannon.from_paths(&input, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }

    /// The two-chain pair has 6 cuts, {src} and {dst} included.
    #[test]
    fn fails_once_sets_exceed_max_sets() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = Shannon.from_paths(&input, &MaxSets(5)).err();
        let ok = Shannon.from_paths(&input, &MaxSets(6));

        assert_eq!(err, Some(Error::TooManySets { limit: 5 }));
        assert!(ok.is_ok());
    }
}
