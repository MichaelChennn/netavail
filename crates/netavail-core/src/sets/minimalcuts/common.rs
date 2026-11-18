//! Shared helpers used by multiple cutset algorithms, ported from PyRBD-Suite `sets/cutsets_common.cpp`.

use crate::control::Control;
use crate::error::Error;
use crate::sets::absorb::AbsorbList;
use crate::types::{NodeId, NodeSet, SetFamily, SetKind};

/// The node that appears in the most sets, with its count; ties go to the lowest node.
/// Returns `(0, 0)` when there are no nodes, like the C++.
pub(crate) fn most_common_element(sets: &[NodeSet]) -> (NodeId, usize) {
    let max_node = sets.iter().flatten().copied().max().unwrap_or(0);
    let mut counts = vec![0usize; max_node as usize + 1];
    for &elem in sets.iter().flatten() {
        counts[elem as usize] += 1;
    }
    let mut best = 0;
    for i in 1..counts.len() {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    (best as NodeId, counts[best])
}

/// Multiplies each partial set with the OR-clause `factor_set`: a partial set that already
/// meets the clause is kept, any other is extended by each clause element in turn.
pub(crate) fn multiply_two_pathsets(
    partial_set_list: &[NodeSet],
    factor_set: &[NodeId],
) -> Vec<NodeSet> {
    let mut absorbed = AbsorbList::default();
    for partial in partial_set_list {
        // Check disjointness
        let disjoint = !partial.iter().any(|e| factor_set.contains(e));
        if disjoint {
            for &f in factor_set {
                let mut new_set = partial.clone();
                new_set.push(f);
                absorbed.add(new_set);
            }
        } else {
            absorbed.add(partial.clone());
        }
    }
    absorbed.into_sets()
}

/// Fails with `TooManySets` once `len` exceeds the limit set by the caller.
pub(crate) fn check_size(len: usize, ctl: &dyn Control) -> Result<(), Error> {
    match ctl.max_sets() {
        Some(limit) if len > limit => Err(Error::TooManySets { limit }),
        _ => Ok(()),
    }
}

/// What a cut algorithm works on after the cases every algorithm handles the same way.
pub(crate) enum CutInput {
    /// Already solved; return as is.
    Done(SetFamily),
    /// Inner nodes of each path (src and dst removed), shortest path first, each sorted.
    Clauses(Vec<NodeSet>),
}

/// Checks the input kind and handles the trivial cases:
/// no path (src and dst disconnected) gives the empty cut; a direct link gives {src}, {dst}.
pub(crate) fn prepare_paths(paths: &SetFamily) -> Result<CutInput, Error> {
    if paths.kind != SetKind::Paths {
        return Err(Error::WrongInputKind {
            expected: SetKind::Paths,
            found: paths.kind,
        });
    }
    // Nothing has to fail to separate them (the C++ returned {src}, {dst} here)
    if paths.sets.is_empty() {
        return Ok(CutInput::Done(cut_family(paths, vec![vec![]])));
    }

    // Sort by length
    let mut mps: Vec<&NodeSet> = paths.sets.iter().collect();
    mps.sort_by_key(|p| p.len());

    // Remove src and dst from each path, convert to sets
    let mut pathsets = Vec::with_capacity(mps.len());
    for mp in mps {
        let mut ps: NodeSet = mp
            .iter()
            .copied()
            .filter(|&n| n != paths.src && n != paths.dst)
            .collect();
        // Direct connection
        if ps.is_empty() {
            return Ok(CutInput::Done(with_endpoints(paths, vec![])));
        }
        ps.sort_unstable();
        pathsets.push(ps);
    }
    Ok(CutInput::Clauses(pathsets))
}

/// Sorts the cuts by size, then lexicographically, and puts {src} and {dst} first.
pub(crate) fn with_endpoints(paths: &SetFamily, mut cuts: Vec<NodeSet>) -> SetFamily {
    for s in &mut cuts {
        s.sort_unstable();
    }
    cuts.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    let mut sets = vec![vec![paths.src], vec![paths.dst]];
    sets.append(&mut cuts);
    cut_family(paths, sets)
}

fn cut_family(paths: &SetFamily, sets: Vec<NodeSet>) -> SetFamily {
    SetFamily {
        kind: SetKind::Cuts,
        src: paths.src,
        dst: paths.dst,
        sets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    #[test]
    fn most_common_element_breaks_ties_by_lowest_node() {
        assert_eq!(
            most_common_element(&[vec![3, 1], vec![1, 3], vec![2]]),
            (1, 2)
        );
        assert_eq!(most_common_element(&[vec![4], vec![2, 4]]), (4, 2));
        assert_eq!(most_common_element(&[]), (0, 0));
    }

    /// (a + b) * (b + c) = b + a·c
    #[test]
    fn multiply_keeps_sets_that_meet_the_clause_and_extends_the_rest() {
        let partial = vec![vec![1], vec![2]];

        let mut product = multiply_two_pathsets(&partial, &[2, 3]);
        product.sort();

        assert_eq!(product, vec![vec![1, 3], vec![2]]);
    }

    #[test]
    fn clauses_drop_endpoints_and_come_shortest_first() {
        let input = paths(0, 5, vec![vec![0, 4, 2, 3, 5], vec![0, 1, 5]]);

        let Ok(CutInput::Clauses(clauses)) = prepare_paths(&input) else {
            panic!("expected clauses");
        };

        assert_eq!(clauses, vec![vec![1], vec![2, 3, 4]]);
    }

    #[test]
    fn direct_link_gives_only_the_endpoints() {
        let input = paths(2, 0, vec![vec![2, 0]]);

        let Ok(CutInput::Done(cuts)) = prepare_paths(&input) else {
            panic!("expected a finished family");
        };

        assert_eq!(cuts.sets, vec![vec![2], vec![0]]);
    }

    #[test]
    fn disconnected_pair_gives_the_empty_cut() {
        let Ok(CutInput::Done(cuts)) = prepare_paths(&paths(0, 1, vec![])) else {
            panic!("expected a finished family");
        };

        assert_eq!(cuts.kind, SetKind::Cuts);
        assert_eq!(cuts.sets, vec![Vec::<NodeId>::new()]);
    }

    #[test]
    fn rejects_cut_sets() {
        let mut input = paths(0, 1, vec![vec![0], vec![1]]);
        input.kind = SetKind::Cuts;

        assert!(matches!(
            prepare_paths(&input),
            Err(Error::WrongInputKind { .. })
        ));
    }
}
