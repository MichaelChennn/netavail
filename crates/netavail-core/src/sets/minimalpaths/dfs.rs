//! DFS-based minimal paths, ported from PyRBD-Suite `sets/minimalpaths.cpp` (old name `minimalpaths`).
//!
//! Implements the "dontgo" pruning strategy: for each node in visited (except the last),
//! all its neighbours are forbidden for the current extension. This prunes paths that
//! revisit neighbours of already-traversed intermediate nodes, so only chordless paths
//! (= minimal path sets, D9) are produced.

use crate::control::Control;
use crate::error::Error;
use crate::graph::Graph;
use crate::registry::AlgoInfo;
use crate::sets::minimalpaths::PathAlgorithm;
use crate::types::{NodeId, SetFamily, SetKind};

pub struct Dfs;

static INFO: AlgoInfo = AlgoInfo {
    id: "dfs",
    name: "DFS",
    summary: "Enumerate chordless paths by depth-first search",
    version: 1,
    aliases: &["minimalpaths"],
    citation: None,
};

impl PathAlgorithm for Dfs {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn find(
        &self,
        g: &Graph,
        src: NodeId,
        dst: NodeId,
        ctl: &dyn Control,
    ) -> Result<SetFamily, Error> {
        // Also checked by the pipeline; kept for direct callers
        g.check_pair(src, dst)?;
        let mut sets = Vec::new();

        // Stack-based iterative DFS
        let mut visited = vec![src];
        let mut on_path = vec![false; g.node_count() as usize];
        on_path[src as usize] = true;

        // Stack of iterators (index into adjacency list); advanced as soon as a child is taken
        let mut iters = vec![0];

        while let Some(&i) = iters.last() {
            let current = *visited.last().unwrap();
            let Some(&child) = g.neighbors(current).get(i) else {
                // Backtrack
                iters.pop();
                visited.pop();
                on_path[current as usize] = false;
                continue;
            };
            *iters.last_mut().unwrap() += 1;

            if in_dontgo(g, &visited, child) {
                continue;
            }
            if child == dst {
                // Found a path
                let mut path = visited.clone();
                path.push(dst);
                sets.push(path);
                if let Some(limit) = ctl.max_sets()
                    && sets.len() > limit
                {
                    return Err(Error::TooManySets { limit });
                }
                ctl.check()?;
            } else if !on_path[child as usize] {
                // Extend path
                ctl.check()?;
                visited.push(child);
                on_path[child as usize] = true;
                iters.push(0);
            }
        }

        Ok(SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        })
    }
}

/// Whether `child` neighbours any visited node except the last one.
fn in_dontgo(g: &Graph, visited: &[NodeId], child: NodeId) -> bool {
    visited[..visited.len() - 1]
        .iter()
        .any(|&v| g.neighbors(v).contains(&child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};

    fn chorded_square() -> Graph {
        Graph::from_edges(4, &[(0, 3), (3, 2), (2, 1), (1, 0), (1, 3)]).unwrap()
    }

    struct Limit(usize);
    impl Control for Limit {
        fn max_sets(&self) -> Option<usize> {
            Some(self.0)
        }
    }

    struct CancelNow;
    impl Control for CancelNow {
        fn check(&self) -> Result<(), Cancelled> {
            Err(Cancelled)
        }
    }

    /// 4-cycle 0-3-2-1-0 plus chord 1-3. Plain DFS would also find 0-1-3-2 and 0-3-1-2.
    #[test]
    fn finds_only_chordless_paths_in_neighbor_order() {
        let paths = Dfs.find(&chorded_square(), 0, 2, &NoControl).unwrap();

        assert_eq!(paths.kind, SetKind::Paths);
        assert_eq!((paths.src, paths.dst), (0, 2));
        assert_eq!(paths.sets, vec![vec![0, 3, 2], vec![0, 1, 2]]);
    }

    #[test]
    fn rejects_same_or_missing_node() {
        let g = chorded_square();

        for (src, dst) in [(1, 1), (0, 4), (4, 0)] {
            let err = Dfs.find(&g, src, dst, &NoControl).unwrap_err();
            assert_eq!(err, Error::InvalidPair { src, dst });
        }
    }

    #[test]
    fn fails_once_paths_exceed_max_sets() {
        let g = chorded_square();

        assert_eq!(
            Dfs.find(&g, 0, 2, &Limit(1)).unwrap_err(),
            Error::TooManySets { limit: 1 }
        );
        assert_eq!(Dfs.find(&g, 0, 2, &Limit(2)).unwrap().sets.len(), 2);
    }

    #[test]
    fn stops_when_cancelled() {
        let err = Dfs.find(&chorded_square(), 0, 2, &CancelNow).unwrap_err();

        assert_eq!(err, Error::Cancelled);
    }
}
