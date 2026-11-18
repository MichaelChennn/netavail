//! AbsorbList, ported from PyRBD-Suite `sets/absorb_list.cpp`.
//!
//! Maintains a family of sets where no set is a subset or superset
//! of another. Sets are stored sorted for efficient comparison.

use std::collections::BTreeMap;

use crate::types::{NodeId, NodeSet};

#[derive(Default, Debug)]
pub(crate) struct AbsorbList {
    /// Sets grouped by size; within a size, in insertion order.
    buckets: BTreeMap<usize, Vec<NodeSet>>,
    count: usize,
}

/// Check if sorted set a ⊆ sorted set b
fn is_subset_sorted(a: &[NodeId], b: &[NodeId]) -> bool {
    let mut b = b.iter().peekable();
    for x in a {
        // Skip smaller elements of b; stop early once b passes x
        while b.next_if(|y| *y < x).is_some() {}
        if b.next() != Some(x) {
            return false;
        }
    }
    true
}

impl AbsorbList {
    /// Inserts a set; returns true if the family changed.
    pub(crate) fn add(&mut self, mut set: NodeSet) -> bool {
        // Store sets sorted internally
        set.sort_unstable();
        let m = set.len();

        // Case 0: exact duplicate
        if self.buckets.get(&m).is_some_and(|b| b.contains(&set)) {
            return false;
        }

        // Case 1: if any existing set (smaller or equal) is a subset of new → discard new
        for bucket in self.buckets.range(..=m).map(|(_, b)| b) {
            if bucket
                .iter()
                .any(|existing| is_subset_sorted(existing, &set))
            {
                return false;
            }
        }

        // Case 2: remove all existing supersets of the new one
        for bucket in self.buckets.range_mut(m..).map(|(_, b)| b) {
            let before = bucket.len();
            bucket.retain(|existing| !is_subset_sorted(&set, existing));
            self.count -= before - bucket.len();
        }
        self.buckets.retain(|_, b| !b.is_empty());

        self.buckets.entry(m).or_default().push(set);
        self.count += 1;
        true
    }

    /// Inserts many sets; returns how many changed the family.
    pub(crate) fn add_many(&mut self, sets: impl IntoIterator<Item = NodeSet>) -> usize {
        // Sort by size first (insert smaller sets first to prune more)
        let mut sets: Vec<NodeSet> = sets.into_iter().collect();
        sets.sort_by_key(Vec::len);
        sets.into_iter()
            .map(|s| self.add(s))
            .filter(|&changed| changed)
            .count()
    }

    pub(crate) fn len(&self) -> usize {
        self.count
    }

    /// All stored sets, smallest first.
    pub(crate) fn into_sets(self) -> Vec<NodeSet> {
        self.buckets.into_values().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn absorbed(sets: Vec<NodeSet>) -> Vec<NodeSet> {
        let mut list = AbsorbList::default();
        list.add_many(sets);
        let len = list.len();
        let out = list.into_sets();
        assert_eq!(out.len(), len);
        out
    }

    #[test]
    fn rejects_duplicates_and_supersets_of_existing_sets() {
        let mut list = AbsorbList::default();

        assert!(list.add(vec![3, 1]));
        assert!(!list.add(vec![1, 3]));
        assert!(!list.add(vec![1, 2, 3]));
        assert_eq!(list.into_sets(), vec![vec![1, 3]]);
    }

    #[test]
    fn new_subset_removes_existing_supersets() {
        let mut list = AbsorbList::default();
        list.add(vec![1, 2, 3]);
        list.add(vec![2, 4]);
        list.add(vec![5]);

        assert!(list.add(vec![2]));

        assert_eq!(list.len(), 2);
        // Same size: insertion order
        assert_eq!(list.into_sets(), vec![vec![5], vec![2]]);
    }

    #[test]
    fn empty_set_absorbs_everything() {
        assert_eq!(absorbed(vec![vec![1], vec![], vec![2, 3]]), vec![vec![]]);
    }

    /// Brute force: keep the distinct sets that have no proper subset in the input.
    fn minimal_sets(sets: &[NodeSet]) -> Vec<NodeSet> {
        let mut sorted: Vec<NodeSet> = sets
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.sort_unstable();
                s.dedup();
                s
            })
            .collect();
        sorted.sort();
        sorted.dedup();
        sorted
            .iter()
            .filter(|s| !sorted.iter().any(|t| t != *s && is_subset_sorted(t, s)))
            .cloned()
            .collect()
    }

    fn canonical(mut sets: Vec<NodeSet>) -> Vec<NodeSet> {
        sets.sort();
        sets
    }

    /// Random families of sets over nodes 0..6, each set without repeated nodes.
    fn families() -> impl Strategy<Value = Vec<NodeSet>> {
        let set = prop::collection::btree_set(0u32..6, 0..5).prop_map(|s| s.into_iter().collect());
        prop::collection::vec(set, 0..12)
    }

    proptest! {
        #[test]
        fn keeps_exactly_the_minimal_sets(sets in families()) {
            prop_assert_eq!(canonical(absorbed(sets.clone())), minimal_sets(&sets));
        }

        #[test]
        fn result_does_not_depend_on_insertion_order(sets in families()) {
            let mut list = AbsorbList::default();
            for s in sets.iter().rev() {
                list.add(s.clone());
            }
            prop_assert_eq!(canonical(list.into_sets()), minimal_sets(&sets));
        }
    }
}
