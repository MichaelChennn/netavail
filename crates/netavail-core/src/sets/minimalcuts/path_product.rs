//! Path product, ported from PyRBD-Suite `sets/cutsets_multiplication.cpp` (old name `multiplication`).
//!
//! Each minimal path (without src and dst) is an OR-clause; the cut sets are the minimal
//! transversals of their AND, found by multiplying out the clauses one by one with absorption.

use crate::control::Control;
use crate::error::Error;
use crate::registry::{AlgoInfo, Citation};
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalcuts::common::{
    CutInput, check_size, multiply_pathsets, prepare_paths, with_endpoints,
};
use crate::types::SetFamily;

pub struct PathProduct;

static INFO: AlgoInfo = AlgoInfo {
    id: "path_product",
    name: "Path product",
    summary: "Multiply out the path CNF clause by clause, absorbing supersets",
    version: 1,
    aliases: &["multiplication"],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, \"Leveraging Minimal Path \
               Sets for Reliability Block Diagram Evaluation\", IEEE FNWF, 2025",
        url: Some("https://ieeexplore.ieee.org/document/11317672"),
    }),
};

impl CutAlgorithm for PathProduct {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
        // Remove src and dst, sort each path
        let pathsets = match prepare_paths(paths)? {
            CutInput::Done(cuts) => return Ok(cuts),
            CutInput::Clauses(pathsets) => pathsets,
        };
        // Multiply pathsets
        let result_sets = multiply_pathsets(&pathsets, ctl)?;
        let cuts = with_endpoints(paths, result_sets);
        check_size(cuts.sets.len(), ctl)?;
        Ok(cuts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::types::{NodeId, NodeSet, SetKind};

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

        let cuts = PathProduct.from_paths(&input, &NoControl).unwrap();

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

    /// (1 + 2) * (1 + 3) = 1 + 2·3: the shared node absorbs the longer products.
    #[test]
    fn shared_node_is_a_single_cut() {
        let input = paths(0, 4, vec![vec![0, 1, 2, 4], vec![0, 1, 3, 4]]);

        let cuts = PathProduct.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![4], vec![1], vec![2, 3]]);
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

        let err = PathProduct.from_paths(&input, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }

    /// The two-chain pair has 6 cuts, {src} and {dst} included.
    #[test]
    fn fails_once_sets_exceed_max_sets() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = PathProduct.from_paths(&input, &MaxSets(5)).err();
        let ok = PathProduct.from_paths(&input, &MaxSets(6));

        assert_eq!(err, Some(Error::TooManySets { limit: 5 }));
        assert!(ok.is_ok());
    }
}
