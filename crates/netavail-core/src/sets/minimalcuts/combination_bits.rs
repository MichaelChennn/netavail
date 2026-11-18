//! Combination (bits), ported from PyRBD-Suite `sets/cutsets_combination_matrix.cpp`
//! (old name `combination_matrix`).
//!
//! Like `combination`, but each round first lists the upper sets (supersets) of the known
//! minimal cut sets and drops them from the combinations, and tests a combination by the
//! logical OR of its incidence columns. Each column is a bitset over the paths.
//!
//! Differences from the C++ (same cuts wherever the C++ finds them all), as in `combination`:
//! candidates are the nodes on some path, and there is no `order` limit; the search stops
//! once no combination of the current size can grow into a minimal cut.

use std::collections::HashSet;

use crate::control::Control;
use crate::error::Error;
use crate::registry::{AlgoInfo, Citation};
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalcuts::common::{
    CutInput, check_size, next_combination, prepare_paths, with_endpoints,
};
use crate::types::{NodeId, NodeSet, SetFamily};

pub struct CombinationBits;

static INFO: AlgoInfo = AlgoInfo {
    id: "combination_bits",
    name: "Combination (bits)",
    summary: "Combination search that drops known supersets up front and ORs path bitsets",
    version: 1,
    aliases: &["combination_matrix"],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, C. Mas-Machuca, \"PyRBD++: An Open-Source Fast \
               Reliability Block Diagram Evaluation Tool\", RNDM, 2025",
        url: None,
    }),
};

impl CutAlgorithm for CombinationBits {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
        let clauses = match prepare_paths(paths)? {
            CutInput::Done(cuts) => return Ok(cuts),
            CutInput::Clauses(clauses) => clauses,
        };
        let minimal = combination_matrix_cuts(&clauses, ctl)?;
        let cuts = with_endpoints(paths, minimal);
        check_size(cuts.sets.len(), ctl)?;
        Ok(cuts)
    }
}

/// A set of paths, one bit per path.
type Bits = Vec<u64>;

/// Inner cuts of the paths `clauses` (src and dst removed, each sorted).
fn combination_matrix_cuts(clauses: &[NodeSet], ctl: &dyn Control) -> Result<Vec<NodeSet>, Error> {
    // get all nodes on some path except source and destination
    let mut valid_nodes: Vec<NodeId> = clauses.iter().flatten().copied().collect();
    valid_nodes.sort_unstable();
    valid_nodes.dedup();

    // create origin incidence matrix: one column per node, bit p set if the node is on path p
    let words = clauses.len().div_ceil(64);
    let mut columns: Vec<Bits> = vec![vec![0; words]; valid_nodes.len()];
    for (p, path) in clauses.iter().enumerate() {
        for node in path {
            let col = valid_nodes.binary_search(node).expect("node is valid");
            columns[col][p / 64] |= 1 << (p % 64);
        }
    }
    let mut all_paths: Bits = vec![u64::MAX; words];
    if !clauses.len().is_multiple_of(64) {
        all_paths[words - 1] = (1 << (clauses.len() % 64)) - 1;
    }

    // save the minimal cut sets, as column indices
    let mut minimal: Vec<Vec<usize>> = Vec::new();

    // find all true columns; the first pairs are the columns not all true
    let mut firstpairs: Vec<usize> = Vec::new();
    for (col, bits) in columns.iter().enumerate() {
        if *bits == all_paths {
            minimal.push(vec![col]);
        } else {
            firstpairs.push(col);
        }
    }

    // loop for finding minimal cut sets; a minimal cut needs one own path per node
    let max_order = clauses.len().min(firstpairs.len());
    let mut ones: Bits = vec![0; words];
    let mut twos: Bits = vec![0; words];
    for combo_size in 2..=max_order {
        ctl.check()?;
        let upperset = upper_sets(&minimal, &firstpairs, combo_size, ctl)?;

        // find the new pairs by removing the upper sets from all combinations
        let mut newpairs = Vec::new();
        let mut can_grow = false;
        let mut indices: Vec<usize> = (0..combo_size).collect();
        loop {
            ctl.check()?;
            let combo: Vec<usize> = indices.iter().map(|&i| firstpairs[i]).collect();
            if !upperset.contains(&combo) {
                // do the logical or operation of the columns
                or_columns(&columns, &combo, &mut ones, &mut twos);
                let own_path = |&c: &usize| columns[c].iter().zip(&twos).any(|(b, t)| b & !t != 0);
                if combo.iter().all(own_path) {
                    can_grow = true;
                    if ones == all_paths {
                        newpairs.push(combo);
                    }
                }
            }
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

    // convert the column indices back to nodes
    let to_nodes = |cols: Vec<usize>| cols.into_iter().map(|c| valid_nodes[c]).collect();
    Ok(minimal.into_iter().map(to_nodes).collect())
}

/// All combinations of `combo_size` first pairs that contain a known minimal cut set.
/// Can be large, so it counts against the set limit.
fn upper_sets(
    minimal: &[Vec<usize>],
    firstpairs: &[usize],
    combo_size: usize,
    ctl: &dyn Control,
) -> Result<HashSet<Vec<usize>>, Error> {
    let mut upperset = HashSet::new();
    // finding all the upper sets of the current minimal cut sets
    for tup in minimal {
        // Order-1 cuts are not first pairs, so no combination contains them
        if tup.len() == 1 && firstpairs.binary_search(&tup[0]).is_err() {
            continue;
        }
        // find the remainder of the first pairs and the current minimal cut set
        let remainder: Vec<usize> = firstpairs
            .iter()
            .copied()
            .filter(|c| !tup.contains(c))
            .collect();
        // find the number of nodes needed to add to the current minimal cut set
        let need = combo_size - tup.len();
        if need == 0 || remainder.len() < need {
            continue;
        }
        // add the current minimal cut set and each combination of remainder nodes
        let mut indices: Vec<usize> = (0..need).collect();
        loop {
            ctl.check()?;
            let mut combo: Vec<usize> = indices.iter().map(|&i| remainder[i]).collect();
            combo.extend_from_slice(tup);
            combo.sort_unstable();
            upperset.insert(combo);
            check_size(upperset.len(), ctl)?;
            if !next_combination(&mut indices, remainder.len()) {
                break;
            }
        }
    }
    Ok(upperset)
}

/// `ones` gets the paths hit by any column of `combo`, `twos` those hit by at least two.
fn or_columns(columns: &[Bits], combo: &[usize], ones: &mut Bits, twos: &mut Bits) {
    ones.fill(0);
    twos.fill(0);
    for &c in combo {
        for ((o, t), b) in ones.iter_mut().zip(twos.iter_mut()).zip(&columns[c]) {
            *t |= *o & b;
            *o |= b;
        }
    }
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

        let cuts = CombinationBits.from_paths(&input, &NoControl).unwrap();

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

        let cuts = CombinationBits.from_paths(&input, &NoControl).unwrap();

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

        let cuts = CombinationBits.from_paths(&input, &NoControl).unwrap();

        assert_eq!(cuts.sets, vec![vec![0], vec![5], vec![1, 2, 3, 4]]);
    }

    /// Paths 0-a-b-18 for a in 1..=9, b in 10..=17: 72 paths, so the bitsets span two words.
    #[test]
    fn handles_more_than_64_paths() {
        let all = (1..=9).flat_map(|a| (10..=17).map(move |b| vec![0, a, b, 18]));
        let input = paths(0, 18, all.collect());

        let cuts = CombinationBits.from_paths(&input, &NoControl).unwrap();

        let expected = vec![vec![0], vec![18], (10..=17).collect(), (1..=9).collect()];
        assert_eq!(cuts.sets, expected);
    }

    /// The size-3 upper sets of the cut {1, 2} among the first pairs 1..=4.
    #[test]
    fn skips_upper_sets_of_known_cuts() {
        let upper = upper_sets(&[vec![1, 2]], &[1, 2, 3, 4], 3, &NoControl).unwrap();

        assert_eq!(upper, HashSet::from([vec![1, 2, 3], vec![1, 2, 4]]));
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

        let err = CombinationBits.from_paths(&input, &CancelNow).err();

        assert_eq!(err, Some(Error::Cancelled));
    }

    /// The two-chain pair has 6 cuts, {src} and {dst} included.
    #[test]
    fn fails_once_sets_exceed_max_sets() {
        let input = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = CombinationBits.from_paths(&input, &MaxSets(5)).err();
        let ok = CombinationBits.from_paths(&input, &MaxSets(6));

        assert_eq!(err, Some(Error::TooManySets { limit: 5 }));
        assert!(ok.is_ok());
    }
}
