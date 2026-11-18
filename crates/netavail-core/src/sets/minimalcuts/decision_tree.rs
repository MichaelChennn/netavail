//! CNF decision tree, ported from PyRBD-Suite `sets/cutsets_cnf_tree.cpp` (old name `cnf_tree`).
//!
//! Each minimal path (without src and dst) is an OR-clause; the cut sets are the minimal
//! transversals of their AND. A tree node splits its clauses on the most common element x:
//! the left child gets the clauses without x, the right child the clauses with x removed
//! (its result also contains {x}). A node's result is left × right, absorbed.
//!
//! The C++ builds the whole tree, then evaluates it; here one recursion does both,
//! so each subtree is freed once evaluated. Results are identical.

use crate::control::Control;
use crate::error::Error;
use crate::registry::{AlgoInfo, Citation};
use crate::sets::absorb::AbsorbList;
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalcuts::common::{
    CutInput, check_size, most_common_element, multiply_pathsets, prepare_paths, with_endpoints,
};
use crate::types::{NodeId, NodeSet, SetFamily};

pub struct DecisionTree;

static INFO: AlgoInfo = AlgoInfo {
    id: "decision_tree",
    name: "Decision tree",
    summary: "Split the path CNF on the most common node and multiply out the branches",
    version: 1,
    aliases: &["cnf_tree"],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, \"Fast-MCS: A Scalable \
               Open-Source Tool to Find Minimal Cut Sets\", arXiv:2602.16686, 2026",
        url: Some("https://arxiv.org/abs/2602.16686"),
    }),
};

impl CutAlgorithm for DecisionTree {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
        let pathsets = match prepare_paths(paths)? {
            CutInput::Done(cuts) => return Ok(cuts),
            CutInput::Clauses(pathsets) => pathsets,
        };
        // Build decision tree and evaluate
        let evaluation = evaluate(&pathsets, None, ctl)?;
        let cuts = with_endpoints(paths, evaluation);
        check_size(cuts.sets.len(), ctl)?;
        Ok(cuts)
    }
}

/// Evaluates the tree node holding the CNF `value`; `most_common_value` is the element
/// its parent split on (right children only).
fn evaluate(
    value: &[NodeSet],
    most_common_value: Option<NodeId>,
    ctl: &dyn Control,
) -> Result<Vec<NodeSet>, Error> {
    ctl.check()?;
    let (common_elem, counts) = most_common_element(value);
    if counts <= 1 {
        return evaluate_self(value, most_common_value, ctl);
    }

    let mut with_elem = Vec::new();
    let mut without_elem = Vec::new();
    for subset in value {
        if subset.contains(&common_elem) {
            let new_subset = subset.iter().copied().filter(|&e| e != common_elem);
            with_elem.push(new_subset.collect());
        } else {
            without_elem.push(subset.clone());
        }
    }

    let left = match without_elem.is_empty() {
        true => Vec::new(),
        false => evaluate(&without_elem, None, ctl)?,
    };
    drop(without_elem);
    let right = match with_elem.is_empty() {
        true => Vec::new(),
        false => evaluate(&with_elem, Some(common_elem), ctl)?,
    };
    drop(with_elem);

    if left.is_empty() && right.is_empty() {
        evaluate_self(value, most_common_value, ctl)
    } else {
        evaluate_two_leaf(left, right, most_common_value, ctl)
    }
}

/// Leaf: no element is shared, so multiplying out the clauses needs no splitting.
fn evaluate_self(
    value: &[NodeSet],
    most_common_value: Option<NodeId>,
    ctl: &dyn Control,
) -> Result<Vec<NodeSet>, Error> {
    if value.is_empty() {
        return Ok(Vec::new());
    }

    // Start with singletons from first clause, multiply with remaining clauses
    let mut final_eval = multiply_pathsets(value, ctl)?;

    if let Some(mcv) = most_common_value {
        final_eval.push(vec![mcv]);
    }
    Ok(final_eval)
}

/// Combines the results of both children (either may be empty = absent).
fn evaluate_two_leaf(
    left_eval: Vec<NodeSet>,
    right_eval: Vec<NodeSet>,
    most_common_value: Option<NodeId>,
    ctl: &dyn Control,
) -> Result<Vec<NodeSet>, Error> {
    let mut final_eval = AbsorbList::default();

    if let Some(mcv) = most_common_value {
        final_eval.add(vec![mcv]);
    }

    if left_eval.is_empty() {
        final_eval.add_many(right_eval);
        return Ok(final_eval.into_sets());
    }

    if right_eval.is_empty() {
        final_eval.add_many(left_eval);
        return Ok(final_eval.into_sets());
    }

    // Multiply left × right
    for l in &left_eval {
        ctl.check()?;
        for r in &right_eval {
            let mut new_set = l.clone();
            new_set.extend_from_slice(r);
            new_set.sort_unstable();
            // Remove duplicates
            new_set.dedup();
            final_eval.add(new_set);
        }
        check_size(final_eval.len(), ctl)?;
    }

    Ok(final_eval.into_sets())
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

        let cuts = DecisionTree.from_paths(&input, &NoControl).unwrap();

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

    /// Paths 0-1-2-4 and 0-1-3-4 share node 1, so the tree splits on it.
    #[test]
    fn shared_node_is_a_single_cut() {
        let input = paths(0, 4, vec![vec![0, 1, 2, 4], vec![0, 1, 3, 4]]);

        let cuts = DecisionTree.from_paths(&input, &NoControl).unwrap();

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

        let err = DecisionTree.from_paths(&input, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }

    /// The two-chain pair has 6 cuts, {src} and {dst} included.
    #[test]
    fn fails_once_sets_exceed_max_sets() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = DecisionTree.from_paths(&input, &MaxSets(5)).err();
        let ok = DecisionTree.from_paths(&input, &MaxSets(6));

        assert_eq!(err, Some(Error::TooManySets { limit: 5 }));
        assert!(ok.is_ok());
    }
}
