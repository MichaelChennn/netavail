//! Layered cache keys and an in-memory cache (architecture.md §3.6; D28, D30).
//!
//! Core computes the keys; the adapter owns the [`Cache`] and decides how long it lives.

use std::collections::HashMap;

use crate::availability::Prepared;
use crate::graph::{Fingerprint, Graph};
use crate::registry::AlgoInfo;
use crate::types::{NodeId, SetFamily};

/// An algorithm as it appears in a key: bumping `version` invalidates old entries.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AlgoKey {
    pub id: &'static str,
    pub version: u32,
}

impl From<&AlgoInfo> for AlgoKey {
    fn from(info: &AlgoInfo) -> AlgoKey {
        AlgoKey {
            id: info.id,
            version: info.version,
        }
    }
}

/// (graph fingerprint, src, dst, path algorithm)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PathKey {
    pub graph: Fingerprint,
    pub src: NodeId,
    pub dst: NodeId,
    pub algo: AlgoKey,
}

/// (path key, cut algorithm)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CutKey {
    pub paths: PathKey,
    pub algo: AlgoKey,
}

/// The sets an availability algorithm was prepared from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SetKey {
    Paths(PathKey),
    Cuts(CutKey),
}

/// (set key, availability algorithm)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PrepareKey {
    pub sets: SetKey,
    pub algo: AlgoKey,
}

/// Keys of every stage a [`crate::Plan`] runs for one pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairKeys {
    pub paths: PathKey,
    pub cuts: Option<CutKey>,
    pub prepare: Option<PrepareKey>,
}

/// Path sets, cut sets and prepared availability of one graph, keyed by stage.
pub struct Cache {
    pub(crate) graph: Fingerprint,
    pub(crate) paths: HashMap<PathKey, SetFamily>,
    pub(crate) cuts: HashMap<CutKey, SetFamily>,
    pub(crate) prepared: HashMap<PrepareKey, Box<dyn Prepared>>,
}

impl Cache {
    /// An empty cache for `graph`; build a new one when the graph's fingerprint changes.
    pub fn new(graph: &Graph) -> Cache {
        Cache::for_fingerprint(graph.fingerprint())
    }

    /// Like [`Cache::new`], for a fingerprint computed once and reused.
    pub fn for_fingerprint(graph: Fingerprint) -> Cache {
        Cache {
            graph,
            paths: HashMap::new(),
            cuts: HashMap::new(),
            prepared: HashMap::new(),
        }
    }

    /// Fingerprint of the graph this cache belongs to.
    pub fn graph(&self) -> &Fingerprint {
        &self.graph
    }

    pub fn paths(&self, key: &PathKey) -> Option<&SetFamily> {
        self.paths.get(key)
    }

    pub fn cuts(&self, key: &CutKey) -> Option<&SetFamily> {
        self.cuts.get(key)
    }

    pub fn prepared(&self, key: &PrepareKey) -> Option<&dyn Prepared> {
        self.prepared.get(key).map(|p| &**p)
    }

    /// Path and cut sets stored for the pair, over every algorithm.
    pub fn pair_sets(&self, src: NodeId, dst: NodeId) -> usize {
        let of = |k: &PathKey| k.src == src && k.dst == dst;
        let paths = self.paths.iter().filter(|(k, _)| of(k)).map(|(_, f)| f);
        let cuts = self
            .cuts
            .iter()
            .filter(|(k, _)| of(&k.paths))
            .map(|(_, f)| f);
        paths.chain(cuts).map(|f| f.sets.len()).sum()
    }

    /// Drops every stage stored for the pair.
    pub fn remove_pair(&mut self, src: NodeId, dst: NodeId) {
        let of = |k: &PathKey| k.src == src && k.dst == dst;
        self.paths.retain(|k, _| !of(k));
        self.cuts.retain(|k, _| !of(&k.paths));
        self.prepared.retain(|k, _| match &k.sets {
            SetKey::Paths(p) => !of(p),
            SetKey::Cuts(c) => !of(&c.paths),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::availability::CutDisjoint;
    use crate::control::NoControl;
    use crate::pipeline::{Plan, run_pair_cached};
    use crate::prob::ProbabilityMap;
    use crate::sets::minimalcuts::DecisionTree;
    use crate::sets::minimalpaths::Dfs;

    /// Cycle 0-1-3-2-0.
    fn square() -> Graph {
        Graph::from_edges(4, &[(0, 1), (1, 3), (0, 2), (2, 3)]).unwrap()
    }

    fn fill(cache: &mut Cache, g: &Graph, pairs: &[(NodeId, NodeId)]) -> Plan {
        let plan = Plan::new(&Dfs, Some(&DecisionTree), Some(&CutDisjoint)).unwrap();
        let p = [ProbabilityMap::from_nodes(&[0.9; 4])];
        for &(s, d) in pairs {
            run_pair_cached(&plan, g, s, d, &p, &NoControl, cache);
        }
        plan
    }

    #[test]
    fn pair_sets_counts_paths_and_cuts_of_that_pair_only() {
        let g = square();
        let mut cache = Cache::new(&g);
        let plan = fill(&mut cache, &g, &[(0, 3), (0, 1)]);

        let keys = plan.keys(&g.fingerprint(), 0, 3);
        let paths = cache.paths(&keys.paths).unwrap().sets.len();
        let cuts = cache.cuts(&keys.cuts.unwrap()).unwrap().sets.len();
        assert_eq!(paths, 2);
        assert_eq!(cache.pair_sets(0, 3), paths + cuts);
        assert_eq!(cache.pair_sets(3, 0), 0);
    }

    #[test]
    fn remove_pair_drops_every_stage_of_that_pair_only() {
        let g = square();
        let mut cache = Cache::new(&g);
        let plan = fill(&mut cache, &g, &[(0, 3), (0, 1)]);

        cache.remove_pair(0, 3);

        let gone = plan.keys(&g.fingerprint(), 0, 3);
        assert!(cache.paths(&gone.paths).is_none());
        assert!(cache.cuts(&gone.cuts.unwrap()).is_none());
        assert!(cache.prepared(&gone.prepare.unwrap()).is_none());
        let kept = plan.keys(&g.fingerprint(), 0, 1);
        assert!(cache.prepared(&kept.prepare.unwrap()).is_some());
    }
}
