//! Engine logic without wasm-bindgen, so it can be tested natively.

use std::collections::HashMap;

use netavail_core::control::Cancelled;
use netavail_core::registry::{self, AlgoInfo};
use netavail_core::types::{NodeId, SetKind};
use netavail_core::{Cache, Control, Graph, PairResult, Plan, ProbabilityMap, run_pair_cached};
use netavail_io::results::{StageTimer, hex};
use netavail_io::{Prob, Topology};

use crate::wire::{
    AlgoKind, AlgorithmInfo, AlgorithmList, NodeView, PairReport, PairReports, PlanSpec,
    SetKind as WireSetKind, Sets, StageTimes, TopologyView,
};

/// Browser defaults; a pair that needs more fails instead of exhausting memory.
pub const MAX_SETS: usize = 100_000;
pub const MAX_TERMS: usize = 2_000_000;
/// Path and cut sets one graph's cache keeps before evicting the least recently used pairs.
pub const CACHE_SETS: usize = 500_000;

/// Limits only: a computation is cancelled by terminating its Worker (architecture.md §3.5).
struct BrowserLimits;

impl Control for BrowserLimits {
    fn check(&self) -> Result<(), Cancelled> {
        Ok(())
    }

    fn max_sets(&self) -> Option<usize> {
        Some(MAX_SETS)
    }

    fn max_terms(&self) -> Option<usize> {
        Some(MAX_TERMS)
    }
}

type Pair = (NodeId, NodeId);

#[derive(Clone, Copy)]
struct Use {
    tick: u64,
    sets: usize,
}

/// One graph the algorithms run on, with its cache.
struct Layer {
    graph: Graph,
    cache: Cache,
    /// Pairs in the cache, in the order first run; `reevaluate` goes through these.
    pairs: Vec<Pair>,
    /// Last use and cached sets of each pair in `pairs`.
    used: HashMap<Pair, Use>,
    sets: usize,
    tick: u64,
}

impl Layer {
    fn new(graph: Graph) -> Layer {
        Layer {
            cache: Cache::new(&graph),
            graph,
            pairs: Vec::new(),
            used: HashMap::new(),
            sets: 0,
            tick: 0,
        }
    }

    fn touch(&mut self, pair: Pair) {
        self.tick += 1;
        if let Some(u) = self.used.get_mut(&pair) {
            u.tick = self.tick;
        }
    }

    /// Records a run of `pair`, then evicts the least recently used other pairs
    /// while the cache holds more than `limit` sets.
    fn record(&mut self, pair: Pair, limit: usize) {
        self.tick += 1;
        let now = Use {
            tick: self.tick,
            sets: self.cache.pair_sets(pair.0, pair.1),
        };
        match self.used.insert(pair, now) {
            Some(old) => self.sets -= old.sets,
            None => self.pairs.push(pair),
        }
        self.sets += now.sets;
        while self.sets > limit {
            let oldest = self.used.iter().filter(|(p, _)| **p != pair);
            let Some((old, u)) = oldest.min_by_key(|(_, u)| u.tick).map(|(p, u)| (*p, *u)) else {
                break;
            };
            self.cache.remove_pair(old.0, old.1);
            self.used.remove(&old);
            self.pairs.retain(|p| *p != old);
            self.sets -= u.sets;
        }
    }

    /// Keeps the cache if `graph` is the same graph.
    fn replace(&mut self, graph: Graph) {
        if graph.fingerprint() != *self.cache.graph() {
            *self = Layer::new(graph);
        } else {
            self.graph = graph;
        }
    }
}

/// `link_prob` set means links can fail: the algorithms run on the link graph,
/// where edge `i` is node `n + i` (D18). Each graph keeps its own cache;
/// the link graph is built the first time a run lets links fail.
pub struct Session {
    topo: Topology,
    plain: Layer,
    links: Option<Layer>,
    clock: fn() -> f64,
    cache_limit: usize,
}

impl Session {
    /// `clock` returns milliseconds; the browser passes `performance.now()`.
    pub fn new(topology_json: &str, clock: fn() -> f64) -> Result<Session, String> {
        let topo = Topology::from_json(topology_json).map_err(|e| e.to_string())?;
        Ok(Session {
            plain: Layer::new(topo.graph().clone()),
            links: None,
            topo,
            clock,
            cache_limit: CACHE_SETS,
        })
    }

    /// Switches topology; computed sets are kept if the graph is unchanged.
    pub fn load(&mut self, topology_json: &str) -> Result<(), String> {
        let topo = Topology::from_json(topology_json).map_err(|e| e.to_string())?;
        let same = topo.graph().fingerprint() == *self.plain.cache.graph();
        self.plain.replace(topo.graph().clone());
        match &mut self.links {
            // Same nodes and edges give the same link graph, so its cache stays valid
            Some(links) if same => links.graph = topo.link_graph(),
            _ => self.links = None,
        }
        self.topo = topo;
        Ok(())
    }

    pub fn topology(&self) -> TopologyView {
        TopologyView {
            name: self.topo.name().to_string(),
            graph: hex(self.plain.cache.graph()),
            nodes: self
                .topo
                .nodes()
                .iter()
                .map(|n| NodeView {
                    id: n.id.clone(),
                    label: n.label.clone(),
                    x: n.x,
                    y: n.y,
                })
                .collect(),
            edges: self
                .topo
                .edges()
                .iter()
                .map(|e| (e.source, e.target))
                .collect(),
        }
    }

    /// Runs one pair, one scenario per entry of `probs`; with `link_prob`, links can fail too.
    pub fn run_pair(
        &mut self,
        plan: &PlanSpec,
        src: NodeId,
        dst: NodeId,
        probs: &[Prob],
        link_prob: Option<Prob>,
    ) -> Result<PairReport, String> {
        let plan = build_plan(plan)?;
        let n = self.topo.graph().node_count();
        if src >= n || dst >= n || src == dst {
            return Err(format!(
                "pair ({src}, {dst}) is not two different nodes of 0..{n}"
            ));
        }
        let scenarios = self.scenarios(probs, link_prob)?;
        let links = link_prob.is_some();
        let report = self.run(&plan, src, dst, &scenarios, links);
        let limit = self.cache_limit;
        self.layer_mut(links).record((src, dst), limit);
        Ok(report)
    }

    /// Runs `plan` again on every earlier pair whose sets it already has in the cache,
    /// so only evaluate does work. Other pairs are left out.
    pub fn reevaluate(
        &mut self,
        plan: &PlanSpec,
        probs: &[Prob],
        link_prob: Option<Prob>,
    ) -> Result<PairReports, String> {
        let plan = build_plan(plan)?;
        let scenarios = self.scenarios(probs, link_prob)?;
        let links = link_prob.is_some();
        let Some(layer) = self.layer(links) else {
            return Ok(PairReports { reports: vec![] });
        };
        let pairs: Vec<_> = layer
            .pairs
            .iter()
            .copied()
            .filter(|&(src, dst)| is_cached(layer, &plan, src, dst))
            .collect();
        let reports = pairs
            .into_iter()
            .map(|(src, dst)| self.run(&plan, src, dst, &scenarios, links))
            .collect();
        Ok(PairReports { reports })
    }

    /// Sets of one pair from the cache; `links` picks the link graph's cache,
    /// whose sets can hold link nodes `n + i`. Counts as a use of the pair.
    pub fn sets(
        &mut self,
        plan: &PlanSpec,
        src: NodeId,
        dst: NodeId,
        kind: WireSetKind,
        links: bool,
    ) -> Result<Sets, String> {
        let plan = build_plan(plan)?;
        let layer = if links {
            self.links.as_mut()
        } else {
            Some(&mut self.plain)
        };
        let Some(layer) = layer else {
            return Ok(Sets { sets: None });
        };
        layer.touch((src, dst));
        let cache = &layer.cache;
        let keys = plan.keys(cache.graph(), src, dst);
        let family = match kind {
            WireSetKind::Paths => cache.paths(&keys.paths),
            WireSetKind::Cuts => keys.cuts.and_then(|k| cache.cuts(&k)),
        };
        Ok(Sets {
            sets: family.map(|f| f.sets.clone()),
        })
    }

    /// `None` if the link graph has not been built yet.
    fn layer(&self, links: bool) -> Option<&Layer> {
        if links {
            self.links.as_ref()
        } else {
            Some(&self.plain)
        }
    }

    /// Builds the link graph if needed.
    fn layer_mut(&mut self, links: bool) -> &mut Layer {
        if links {
            self.links
                .get_or_insert_with(|| Layer::new(self.topo.link_graph()))
        } else {
            &mut self.plain
        }
    }

    /// Node probabilities first, then one per edge if links can fail.
    fn scenarios(
        &self,
        probs: &[Prob],
        link_prob: Option<Prob>,
    ) -> Result<Vec<ProbabilityMap>, String> {
        let link_prob = link_prob.map(Prob::checked).transpose();
        let link_prob = link_prob.map_err(|e| e.to_string())?;
        probs
            .iter()
            .map(|&p| self.topo.scenario(p, link_prob).map_err(|e| e.to_string()))
            .collect()
    }

    fn run(
        &mut self,
        plan: &Plan,
        src: NodeId,
        dst: NodeId,
        scenarios: &[ProbabilityMap],
        links: bool,
    ) -> PairReport {
        let clock = self.clock;
        let timer = StageTimer::new(&clock, &BrowserLimits);
        let layer = self.layer_mut(links);
        let result = run_pair_cached(
            plan,
            &layer.graph,
            src,
            dst,
            scenarios,
            &timer,
            &mut layer.cache,
        );
        report(result, timer.times())
    }
}

/// Whether the last set stage of `plan` (prepare, else cuts, else paths) is cached.
fn is_cached(layer: &Layer, plan: &Plan, src: NodeId, dst: NodeId) -> bool {
    let cache = &layer.cache;
    let keys = plan.keys(cache.graph(), src, dst);
    match (keys.prepare, keys.cuts) {
        (Some(k), _) => cache.prepared(&k).is_some(),
        (None, Some(k)) => cache.cuts(&k).is_some(),
        (None, None) => cache.paths(&keys.paths).is_some(),
    }
}

fn build_plan(spec: &PlanSpec) -> Result<Plan, String> {
    let unknown = |kind: &str, id: &str| format!("unknown {kind} algorithm {id:?}");
    let paths = registry::find_path(&spec.paths).ok_or_else(|| unknown("path", &spec.paths))?;
    let cuts = match &spec.cuts {
        Some(id) => Some(registry::find_cut(id).ok_or_else(|| unknown("cut", id))?),
        None => None,
    };
    let availability = match &spec.availability {
        Some(id) => {
            Some(registry::find_availability(id).ok_or_else(|| unknown("availability", id))?)
        }
        None => None,
    };
    Plan::new(paths, cuts, availability).map_err(|e| e.to_string())
}

fn report(r: PairResult, t: netavail_io::results::StageTimes) -> PairReport {
    PairReport {
        src: r.src,
        dst: r.dst,
        paths: r.path_count,
        cuts: r.cut_count,
        availability: r.availability,
        error: r.error.map(|e| e.to_string()),
        times: StageTimes {
            paths_ms: t.paths_ms,
            cuts_ms: t.cuts_ms,
            prepare_ms: t.prepare_ms,
            evaluate_ms: t.evaluate_ms,
        },
    }
}

pub fn algorithms() -> AlgorithmList {
    let info = |kind, a: &AlgoInfo, input| AlgorithmInfo {
        kind,
        id: a.id.into(),
        name: a.name.into(),
        summary: a.summary.into(),
        version: a.version,
        aliases: a.aliases.iter().map(|s| s.to_string()).collect(),
        input,
    };
    let mut algorithms = Vec::new();
    for a in registry::path_algorithms() {
        algorithms.push(info(AlgoKind::Paths, a.info(), None));
    }
    for a in registry::cut_algorithms() {
        algorithms.push(info(AlgoKind::Cuts, a.info(), None));
    }
    for a in registry::availability_algorithms() {
        let input = match a.input() {
            SetKind::Paths => WireSetKind::Paths,
            SetKind::Cuts => WireSetKind::Cuts,
        };
        algorithms.push(info(AlgoKind::Availability, a.info(), Some(input)));
    }
    AlgorithmList { algorithms }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netavail_io::Prob::{File, Uniform};

    /// Cycle a-b-d-c-a: two minimal paths from a to d.
    const SQUARE: &str = r#"{"name": "sq", "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}, {"id": "d"}],
        "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "d"},
                  {"source": "a", "target": "c"}, {"source": "c", "target": "d"}]}"#;
    const PATH: &str = r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
        "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "c"}]}"#;

    fn clock() -> f64 {
        0.0
    }

    fn dfs() -> PlanSpec {
        PlanSpec {
            paths: "dfs".into(),
            cuts: None,
            availability: None,
        }
    }

    #[test]
    fn second_run_of_a_pair_uses_the_cache() {
        let mut s = Session::new(SQUARE, clock).unwrap();

        let first = s.run_pair(&dfs(), 0, 3, &[], None).unwrap();
        let second = s.run_pair(&dfs(), 0, 3, &[], None).unwrap();

        assert_eq!(first.paths, Some(2));
        assert_eq!(first.times.paths_ms, Some(0.0));
        assert_eq!(second.paths, Some(2));
        assert_eq!(second.times.paths_ms, None);
    }

    #[test]
    fn sets_come_from_the_cache() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        let before = s.sets(&dfs(), 0, 3, WireSetKind::Paths, false).unwrap();

        s.run_pair(&dfs(), 0, 3, &[], None).unwrap();
        let mut after = s
            .sets(&dfs(), 0, 3, WireSetKind::Paths, false)
            .unwrap()
            .sets
            .unwrap();
        after.sort();

        assert_eq!(before.sets, None);
        assert_eq!(after, vec![vec![0, 1, 3], vec![0, 2, 3]]);
        assert_eq!(
            s.sets(&dfs(), 0, 3, WireSetKind::Cuts, false).unwrap().sets,
            None
        );
    }

    #[test]
    fn same_graph_keeps_the_cache_and_another_graph_clears_it() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        s.run_pair(&dfs(), 0, 2, &[], None).unwrap();

        // Same graph, other names: still cached
        s.load(&SQUARE.replace("\"sq\"", "\"renamed\"")).unwrap();
        let kept = s.reevaluate(&dfs(), &[], None).unwrap();
        assert_eq!(s.topology().name, "renamed");
        assert_eq!(kept.reports.len(), 1);

        s.load(PATH).unwrap();
        assert!(s.reevaluate(&dfs(), &[], None).unwrap().reports.is_empty());
        assert_eq!(s.run_pair(&dfs(), 0, 2, &[], None).unwrap().paths, Some(1));
    }

    #[test]
    fn reevaluate_reruns_only_cached_pairs_in_order() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        s.run_pair(&dfs(), 1, 2, &[], None).unwrap();
        s.run_pair(&dfs(), 0, 1, &[], None).unwrap();
        s.run_pair(&dfs(), 1, 2, &[], None).unwrap();

        let again = s.reevaluate(&dfs(), &[], None).unwrap().reports;

        let pairs: Vec<_> = again.iter().map(|r| (r.src, r.dst)).collect();
        assert_eq!(pairs, [(1, 2), (0, 1)]);
        assert!(again.iter().all(|r| r.times.paths_ms.is_none()));
    }

    #[test]
    fn bad_requests_are_errors() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        let mut plan = dfs();
        plan.paths = "nope".into();

        assert!(
            s.run_pair(&plan, 0, 1, &[], None)
                .unwrap_err()
                .contains("\"nope\"")
        );
        assert!(s.run_pair(&dfs(), 0, 4, &[], None).is_err());
        assert!(s.run_pair(&dfs(), 1, 1, &[], None).is_err());
        assert!(s.run_pair(&dfs(), 0, 1, &[Uniform(1.5)], None).is_err());
        assert!(
            s.run_pair(&dfs(), 0, 1, &[Uniform(0.9)], Some(Uniform(-1.0)))
                .is_err()
        );
        // A bad link probability is an error even without node scenarios
        assert!(s.run_pair(&dfs(), 0, 1, &[], Some(Uniform(-1.0))).is_err());
        assert!(s.reevaluate(&dfs(), &[], Some(Uniform(7.0))).is_err());
        assert!(s.sets(&plan, 0, 1, WireSetKind::Paths, true).is_err());
        assert!(s.links.is_none());
        // Link nodes are not valid endpoints
        assert!(s.run_pair(&dfs(), 0, 4, &[], Some(Uniform(0.9))).is_err());
        assert!(Session::new("{}", clock).is_err());
    }

    #[test]
    fn availability_from_cuts_needs_a_cut_algorithm() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        let plan = PlanSpec {
            availability: Some("cut_disjoint".into()),
            ..dfs()
        };

        let err = s.run_pair(&plan, 0, 1, &[Uniform(0.9)], None).unwrap_err();

        assert!(err.contains("cut"), "{err}");
    }

    #[test]
    fn algorithms_list_every_registered_algorithm() {
        let list = algorithms().algorithms;

        let dfs = list.iter().find(|a| a.id == "dfs").unwrap();
        let cd = list.iter().find(|a| a.id == "cut_disjoint").unwrap();
        assert_eq!(dfs.kind, AlgoKind::Paths);
        assert_eq!(cd.input, Some(WireSetKind::Cuts));
    }

    #[test]
    fn topology_view_has_nodes_edges_and_fingerprint() {
        let s = Session::new(SQUARE, clock).unwrap();

        let view = s.topology();

        assert_eq!(view.nodes.len(), 4);
        assert_eq!(view.nodes[1].label, "b");
        assert_eq!(view.edges, [(0, 1), (1, 3), (0, 2), (2, 3)]);
        assert_eq!(view.graph.len(), 64);
    }

    /// Square pairs hold 1 (adjacent) or 2 (opposite) path sets.
    fn cached(s: &Session) -> Vec<(NodeId, NodeId)> {
        let mut pairs = s.plain.pairs.clone();
        pairs.retain(|&(a, b)| s.plain.cache.pair_sets(a, b) > 0);
        pairs
    }

    #[test]
    fn over_the_limit_the_least_recently_used_pairs_are_evicted() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        s.cache_limit = 3;
        s.run_pair(&dfs(), 0, 1, &[], None).unwrap();
        s.run_pair(&dfs(), 0, 2, &[], None).unwrap();
        s.run_pair(&dfs(), 1, 3, &[], None).unwrap();
        assert_eq!(cached(&s), [(0, 1), (0, 2), (1, 3)]);

        s.run_pair(&dfs(), 0, 3, &[], None).unwrap();

        assert_eq!(cached(&s), [(1, 3), (0, 3)]);
        assert_eq!(s.reevaluate(&dfs(), &[], None).unwrap().reports.len(), 2);
    }

    #[test]
    fn reading_sets_counts_as_a_use() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        s.cache_limit = 2;
        s.run_pair(&dfs(), 0, 1, &[], None).unwrap();
        s.run_pair(&dfs(), 0, 2, &[], None).unwrap();

        s.sets(&dfs(), 0, 1, WireSetKind::Paths, false).unwrap();
        s.run_pair(&dfs(), 1, 3, &[], None).unwrap();

        assert_eq!(cached(&s), [(0, 1), (1, 3)]);
    }

    #[test]
    fn a_pair_over_the_limit_alone_is_kept_until_the_next_run() {
        let mut s = Session::new(SQUARE, clock).unwrap();
        s.cache_limit = 1;
        s.run_pair(&dfs(), 0, 1, &[], None).unwrap();

        s.run_pair(&dfs(), 0, 3, &[], None).unwrap();

        assert_eq!(cached(&s), [(0, 3)]);
        let sets = s.sets(&dfs(), 0, 3, WireSetKind::Paths, false).unwrap();
        assert_eq!(sets.sets.map(|s| s.len()), Some(2));
    }

    /// Path a-b-c: edges 0 and 1 are link nodes 3 and 4.
    #[test]
    fn link_prob_runs_on_the_link_graph_with_its_own_cache() {
        let mut s = Session::new(PATH, clock).unwrap();
        let plan = PlanSpec {
            availability: Some("path_sdp".into()),
            ..dfs()
        };

        let r = s
            .run_pair(&plan, 0, 2, &[Uniform(0.9)], Some(Uniform(0.8)))
            .unwrap();

        assert!((r.availability[0] - 0.9f64.powi(3) * 0.8f64.powi(2)).abs() < 1e-12);
        let sets = s.sets(&plan, 0, 2, WireSetKind::Paths, true).unwrap();
        assert_eq!(sets.sets.unwrap(), vec![vec![0, 3, 1, 4, 2]]);
        assert_eq!(
            s.sets(&plan, 0, 2, WireSetKind::Paths, false).unwrap().sets,
            None
        );
        assert!(
            s.reevaluate(&plan, &[Uniform(0.9)], None)
                .unwrap()
                .reports
                .is_empty()
        );
        let again = s
            .reevaluate(&plan, &[Uniform(0.5)], Some(Uniform(1.0)))
            .unwrap()
            .reports;
        assert_eq!(again[0].availability, [0.125]);
    }

    /// The link graph is built only when a run first lets links fail.
    #[test]
    fn link_graph_is_built_on_first_use() {
        let mut s = Session::new(PATH, clock).unwrap();
        s.load(PATH).unwrap();

        assert!(s.links.is_none());
        let none = s.sets(&sdp(), 0, 2, WireSetKind::Paths, true).unwrap();
        assert_eq!(none.sets, None);
        let empty = s.reevaluate(&sdp(), &[Uniform(0.9)], Some(Uniform(0.8)));
        assert!(empty.unwrap().reports.is_empty());
        assert!(s.links.is_none());

        s.run_pair(&sdp(), 0, 2, &[Uniform(0.9)], Some(Uniform(0.8)))
            .unwrap();
        s.load(PATH).unwrap();

        let again = s.reevaluate(&sdp(), &[Uniform(0.5)], Some(Uniform(1.0)));
        assert_eq!(again.unwrap().reports[0].availability, [0.125]);

        // Another graph drops the link graph until links fail again
        s.load(SQUARE).unwrap();
        assert!(s.links.is_none());
    }

    /// Path a-b-c with probabilities in the file; links 3 and 4 have their own too.
    const PATH_PROBS: &str = r#"{"nodes": [{"id": "a", "prob": 0.5}, {"id": "b"}, {"id": "c", "prob": 0.25}],
        "edges": [{"source": "a", "target": "b", "prob": 0.8},
                  {"source": "b", "target": "c", "prob": 0.6}]}"#;

    fn sdp() -> PlanSpec {
        PlanSpec {
            availability: Some("path_sdp".into()),
            ..dfs()
        }
    }

    #[test]
    fn file_scenario_uses_each_node_and_link_probability() {
        let mut s = Session::new(PATH_PROBS, clock).unwrap();

        let nodes = s
            .run_pair(&sdp(), 0, 2, &[File, Uniform(0.9)], None)
            .unwrap();
        let links = s.run_pair(&sdp(), 0, 2, &[File], Some(File)).unwrap();

        assert_eq!(nodes.availability, [0.125, 0.9f64.powi(3)]);
        assert!((links.availability[0] - 0.125 * 0.8 * 0.6).abs() < 1e-12);
    }

    /// Editing probabilities leaves the graph alone, so the sets stay cached.
    #[test]
    fn loading_new_probabilities_keeps_the_cache() {
        let mut s = Session::new(PATH_PROBS, clock).unwrap();
        s.run_pair(&sdp(), 0, 2, &[File], None).unwrap();

        s.load(&PATH_PROBS.replace("0.25", "0.75")).unwrap();
        let again = s.reevaluate(&sdp(), &[File], None).unwrap().reports;

        assert_eq!(again[0].availability, [0.375]);
        assert_eq!(again[0].times.paths_ms, None);
    }
}
