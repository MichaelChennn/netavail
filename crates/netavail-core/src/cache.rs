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

    pub(crate) fn for_fingerprint(graph: Fingerprint) -> Cache {
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
}
