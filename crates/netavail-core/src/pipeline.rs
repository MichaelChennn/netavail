//! Runs the chosen algorithms for node pairs (architecture.md §3.4; D29).

use crate::availability::AvailabilityAlgorithm;
use crate::cache::{AlgoKey, Cache, CutKey, PairKeys, PathKey, PrepareKey, SetKey};
use crate::control::{Control, Stage, StageEvent};
use crate::error::Error;
use crate::graph::{Fingerprint, Graph};
use crate::prob::ProbabilityMap;
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalpaths::PathAlgorithm;
use crate::types::{NodeId, SetFamily, SetKind};
use std::collections::hash_map::Entry;

/// A checked combination of algorithms; build it with [`Plan::new`].
pub struct Plan {
    paths: &'static dyn PathAlgorithm,
    cuts: Option<&'static dyn CutAlgorithm>,
    availability: Option<&'static dyn AvailabilityAlgorithm>,
}

impl Plan {
    pub fn new(
        paths: &'static dyn PathAlgorithm,
        cuts: Option<&'static dyn CutAlgorithm>,
        availability: Option<&'static dyn AvailabilityAlgorithm>,
    ) -> Result<Plan, Error> {
        if cuts.is_none() && availability.is_some_and(|a| a.input() == SetKind::Cuts) {
            return Err(Error::MissingCutAlgorithm);
        }
        Ok(Plan {
            paths,
            cuts,
            availability,
        })
    }

    /// Cache keys of every stage this plan runs for one pair of the graph `graph`.
    pub fn keys(&self, graph: &Fingerprint, src: NodeId, dst: NodeId) -> PairKeys {
        let paths = PathKey {
            graph: *graph,
            src,
            dst,
            algo: AlgoKey::from(self.paths.info()),
        };
        let cuts = self.cuts.map(|algo| CutKey {
            paths,
            algo: AlgoKey::from(algo.info()),
        });
        let prepare = self.availability.map(|algo| PrepareKey {
            sets: match algo.input() {
                SetKind::Paths => SetKey::Paths(paths),
                SetKind::Cuts => SetKey::Cuts(cuts.expect("Plan::new requires a cut algorithm")),
            },
            algo: AlgoKey::from(algo.info()),
        });
        PairKeys {
            paths,
            cuts,
            prepare,
        }
    }
}

/// Outcome for one node pair. Sets are not kept (D29).
#[derive(Clone, PartialEq, Debug)]
pub struct PairResult {
    pub src: NodeId,
    pub dst: NodeId,
    /// `None` if the stage was not reached or failed.
    pub path_count: Option<usize>,
    pub cut_count: Option<usize>,
    /// One value per scenario; empty if no availability was computed.
    pub availability: Vec<f64>,
    pub error: Option<Error>,
}

/// Runs paths, then cuts (if chosen), then prepare and evaluate (if chosen) for one pair.
pub fn run_pair(
    plan: &Plan,
    g: &Graph,
    src: NodeId,
    dst: NodeId,
    scenarios: &[ProbabilityMap],
    ctl: &dyn Control,
) -> PairResult {
    let mut cache = Cache::for_fingerprint(Fingerprint::PLACEHOLDER);
    run_pair_cached(plan, g, src, dst, scenarios, ctl, &mut cache)
}

/// Like [`run_pair`], but reuses and fills `cache`, which must belong to `g`.
/// A stage found in the cache is skipped and reports no stage events; evaluate always runs.
pub fn run_pair_cached(
    plan: &Plan,
    g: &Graph,
    src: NodeId,
    dst: NodeId,
    scenarios: &[ProbabilityMap],
    ctl: &dyn Control,
    cache: &mut Cache,
) -> PairResult {
    debug_assert!(
        cache.graph == Fingerprint::PLACEHOLDER || cache.graph == g.fingerprint(),
        "cache belongs to another graph"
    );
    let mut result = PairResult {
        src,
        dst,
        path_count: None,
        cut_count: None,
        availability: Vec::new(),
        error: None,
    };
    if let Err(err) = run_stages(plan, g, src, dst, scenarios, ctl, cache, &mut result) {
        result.error = Some(err);
    }
    result
}

/// Runs every pair in order; one pair's error does not affect the others.
/// After a cancel no new pair is started, so fewer results than pairs may come back.
/// Serial for now; stage 5 switches to rayon under feature `parallel`.
pub fn run_all(
    plan: &Plan,
    g: &Graph,
    pairs: &[(NodeId, NodeId)],
    scenarios: &[ProbabilityMap],
    ctl: &dyn Control,
) -> Vec<PairResult> {
    pairs
        .iter()
        .map_while(|&(src, dst)| {
            ctl.check().ok()?;
            Some(run_pair(plan, g, src, dst, scenarios, ctl))
        })
        .collect()
}

/// All pairs (i, j) with i < j, in the order (0,1), (0,2), …, (1,2), … of the reference answers.
pub fn all_pairs(node_count: u32) -> Vec<(NodeId, NodeId)> {
    (0..node_count)
        .flat_map(|i| (i + 1..node_count).map(move |j| (i, j)))
        .collect()
}

/// Fills `result` stage by stage, so counts from finished stages survive a later error.
#[allow(clippy::too_many_arguments)] // private; the public entry points stay within the limit
fn run_stages(
    plan: &Plan,
    g: &Graph,
    src: NodeId,
    dst: NodeId,
    scenarios: &[ProbabilityMap],
    ctl: &dyn Control,
    cache: &mut Cache,
    result: &mut PairResult,
) -> Result<(), Error> {
    // Before the cache lookup, and in one place for every algorithm
    g.check_pair(src, dst)?;
    // A short scenario would make evaluate panic; in the browser that kills the Worker
    let node_count = g.node_count();
    if plan.availability.is_some()
        && let Some((scenario, p)) = scenarios
            .iter()
            .enumerate()
            .find(|(_, p)| p.node_count() != node_count as usize)
    {
        return Err(Error::ScenarioSize {
            scenario,
            nodes: p.node_count(),
            node_count,
        });
    }
    let keys = plan.keys(&cache.graph, src, dst);
    // Separate fields, so one can be borrowed while another is filled
    let Cache {
        paths: path_cache,
        cuts: cut_cache,
        prepared: prepared_cache,
        ..
    } = cache;

    let paths: &SetFamily = match path_cache.entry(keys.paths) {
        Entry::Occupied(hit) => hit.into_mut(),
        Entry::Vacant(miss) => miss.insert(with_stage_events(ctl, src, dst, Stage::Paths, || {
            plan.paths.find(g, src, dst, ctl)
        })?),
    };
    result.path_count = Some(paths.sets.len());

    let cuts = match (plan.cuts, keys.cuts) {
        (Some(algo), Some(key)) => {
            let cuts: &SetFamily = match cut_cache.entry(key) {
                Entry::Occupied(hit) => hit.into_mut(),
                Entry::Vacant(miss) => {
                    miss.insert(with_stage_events(ctl, src, dst, Stage::Cuts, || {
                        algo.from_paths(paths, ctl)
                    })?)
                }
            };
            result.cut_count = Some(cuts.sets.len());
            Some(cuts)
        }
        _ => None,
    };

    let (Some(algo), Some(key)) = (plan.availability, keys.prepare) else {
        return Ok(());
    };
    let prepared = match prepared_cache.entry(key) {
        Entry::Occupied(hit) => hit.into_mut(),
        Entry::Vacant(miss) => {
            let input = match algo.input() {
                SetKind::Paths => paths,
                SetKind::Cuts => cuts.expect("Plan::new requires a cut algorithm"),
            };
            miss.insert(with_stage_events(ctl, src, dst, Stage::Prepare, || {
                algo.prepare(input, ctl)
            })?)
        }
    };
    result.availability = with_stage_events(ctl, src, dst, Stage::Evaluate, || {
        Ok(scenarios.iter().map(|p| prepared.evaluate(p)).collect())
    })?;
    Ok(())
}

/// Runs one stage between Started and Finished; Finished is reported even on error.
fn with_stage_events<T>(
    ctl: &dyn Control,
    src: NodeId,
    dst: NodeId,
    stage: Stage,
    run: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    ctl.on_stage(src, dst, stage, StageEvent::Started);
    let out = run();
    ctl.on_stage(src, dst, stage, StageEvent::Finished);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::availability::CutDisjoint;
    use crate::control::{Cancelled, NoControl};
    use crate::registry::AlgoInfo;
    use crate::sets::minimalpaths::Dfs;
    use crate::types::SetFamily;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    static FAKE_INFO: AlgoInfo = AlgoInfo {
        id: "fake",
        name: "Fake",
        summary: "Test double",
        version: 1,
        aliases: &[],
        citation: None,
    };

    /// Valid only on a chain: every node of the single path is a cut.
    struct ChainCuts;

    impl CutAlgorithm for ChainCuts {
        fn info(&self) -> &'static AlgoInfo {
            &FAKE_INFO
        }
        fn from_paths(&self, paths: &SetFamily, _: &dyn Control) -> Result<SetFamily, Error> {
            Ok(SetFamily {
                kind: SetKind::Cuts,
                src: paths.src,
                dst: paths.dst,
                sets: paths.sets[0].iter().map(|&v| vec![v]).collect(),
            })
        }
    }

    /// Always fails, like a cut algorithm hitting its set limit.
    struct FailingCuts;

    impl CutAlgorithm for FailingCuts {
        fn info(&self) -> &'static AlgoInfo {
            &FAKE_INFO
        }
        fn from_paths(&self, _: &SetFamily, _: &dyn Control) -> Result<SetFamily, Error> {
            Err(Error::TooManySets { limit: 0 })
        }
    }

    /// Records every stage event.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<(NodeId, NodeId, Stage, StageEvent)>>);

    impl Control for Recorder {
        fn on_stage(&self, src: NodeId, dst: NodeId, stage: Stage, event: StageEvent) {
            self.0.lock().unwrap().push((src, dst, stage, event));
        }
    }

    impl Recorder {
        fn stages(&self) -> Vec<(Stage, StageEvent)> {
            self.0.lock().unwrap().iter().map(|e| (e.2, e.3)).collect()
        }
    }

    /// Cancels once the first pair has finished evaluating.
    #[derive(Default)]
    struct CancelAfterFirstPair(AtomicBool);

    impl Control for CancelAfterFirstPair {
        fn check(&self) -> Result<(), Cancelled> {
            if self.0.load(Ordering::Relaxed) {
                Err(Cancelled)
            } else {
                Ok(())
            }
        }
        fn on_stage(&self, _: NodeId, _: NodeId, stage: Stage, event: StageEvent) {
            if (stage, event) == (Stage::Evaluate, StageEvent::Finished) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
    }

    fn chain() -> Graph {
        Graph::from_edges(3, &[(0, 1), (1, 2)]).unwrap()
    }

    #[test]
    fn plan_needs_a_cut_algorithm_when_availability_takes_cuts() {
        let err = Plan::new(&Dfs, None, Some(&CutDisjoint)).err();

        assert_eq!(err, Some(Error::MissingCutAlgorithm));
    }

    #[test]
    fn runs_every_stage_in_order_and_counts_sets() {
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let scenarios = [
            ProbabilityMap::uniform(3, 0.5),
            ProbabilityMap::from_nodes(&[0.9, 0.8, 0.7]),
        ];
        let ctl = Recorder::default();

        let result = run_pair(&plan, &chain(), 0, 2, &scenarios, &ctl);

        assert_eq!(result.error, None);
        assert_eq!((result.src, result.dst), (0, 2));
        assert_eq!(result.path_count, Some(1));
        assert_eq!(result.cut_count, Some(3));
        // A chain is up only when all its nodes are up
        assert_eq!(result.availability.len(), 2);
        assert!((result.availability[0] - 0.125).abs() < 1e-15);
        assert!((result.availability[1] - 0.504).abs() < 1e-15);
        use {Stage::*, StageEvent::*};
        assert_eq!(
            ctl.stages(),
            [
                (Paths, Started),
                (Paths, Finished),
                (Cuts, Started),
                (Cuts, Finished),
                (Prepare, Started),
                (Prepare, Finished),
                (Evaluate, Started),
                (Evaluate, Finished),
            ]
        );
        assert!(ctl.0.lock().unwrap().iter().all(|e| (e.0, e.1) == (0, 2)));
    }

    #[test]
    fn failed_stage_keeps_earlier_counts_and_skips_the_rest() {
        let plan = Plan::new(&Dfs, Some(&FailingCuts), Some(&CutDisjoint)).unwrap();
        let ctl = Recorder::default();

        let result = run_pair(
            &plan,
            &chain(),
            0,
            2,
            &[ProbabilityMap::uniform(3, 0.9)],
            &ctl,
        );

        assert_eq!(result.error, Some(Error::TooManySets { limit: 0 }));
        assert_eq!(result.path_count, Some(1));
        assert_eq!(result.cut_count, None);
        assert!(result.availability.is_empty());
        use {Stage::*, StageEvent::*};
        assert_eq!(
            ctl.stages(),
            [
                (Paths, Started),
                (Paths, Finished),
                (Cuts, Started),
                (Cuts, Finished)
            ]
        );
    }

    #[test]
    fn rejects_scenario_that_does_not_cover_the_graph_before_any_stage() {
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let scenarios = [
            ProbabilityMap::uniform(3, 0.9),
            ProbabilityMap::uniform(2, 0.9),
        ];
        let ctl = Recorder::default();

        let result = run_pair(&plan, &chain(), 0, 2, &scenarios, &ctl);

        assert_eq!(
            result.error,
            Some(Error::ScenarioSize {
                scenario: 1,
                nodes: 2,
                node_count: 3
            })
        );
        assert_eq!(result.path_count, None);
        assert!(ctl.stages().is_empty());
    }

    #[test]
    fn all_pairs_follow_reference_order() {
        assert_eq!(
            all_pairs(4),
            [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        );
        assert!(all_pairs(1).is_empty());
    }

    #[test]
    fn bad_pair_fails_alone() {
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let scenarios = [ProbabilityMap::uniform(3, 0.5)];

        let results = run_all(
            &plan,
            &chain(),
            &[(0, 1), (1, 1), (0, 2)],
            &scenarios,
            &NoControl,
        );

        let summary: Vec<_> = results
            .iter()
            .map(|r| (r.src, r.dst, r.path_count, r.error.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                (0, 1, Some(1), None),
                (1, 1, None, Some(Error::InvalidPair { src: 1, dst: 1 })),
                (0, 2, Some(1), None),
            ]
        );
        assert_eq!(results[2].availability, [0.125]);
    }

    #[test]
    fn stops_starting_new_pairs_once_cancelled() {
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let scenarios = [ProbabilityMap::uniform(3, 0.5)];

        let results = run_all(
            &plan,
            &chain(),
            &all_pairs(3),
            &scenarios,
            &CancelAfterFirstPair::default(),
        );

        assert_eq!(results.len(), 1);
        assert_eq!((results[0].src, results[0].dst), (0, 1));
        assert_eq!(results[0].error, None);
    }

    static FAKE_V2_INFO: AlgoInfo = AlgoInfo {
        id: "fake",
        name: "Fake",
        summary: "Test double, bumped version",
        version: 2,
        aliases: &[],
        citation: None,
    };

    /// Same cuts as [`ChainCuts`], but a newer version of the algorithm.
    struct ChainCutsV2;

    impl CutAlgorithm for ChainCutsV2 {
        fn info(&self) -> &'static AlgoInfo {
            &FAKE_V2_INFO
        }
        fn from_paths(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<SetFamily, Error> {
            ChainCuts.from_paths(paths, ctl)
        }
    }

    static OTHER_AVAIL_INFO: AlgoInfo = AlgoInfo {
        id: "other_avail",
        name: "Other",
        summary: "Test double: cut_disjoint under another id",
        version: 1,
        aliases: &[],
        citation: None,
    };

    struct OtherAvail;

    impl AvailabilityAlgorithm for OtherAvail {
        fn info(&self) -> &'static AlgoInfo {
            &OTHER_AVAIL_INFO
        }
        fn input(&self) -> SetKind {
            SetKind::Cuts
        }
        fn prepare(
            &self,
            sets: &SetFamily,
            ctl: &dyn Control,
        ) -> Result<Box<dyn crate::Prepared>, Error> {
            CutDisjoint.prepare(sets, ctl)
        }
    }

    fn run_cached(plan: &Plan, g: &Graph, cache: &mut Cache) -> (PairResult, Recorder) {
        let ctl = Recorder::default();
        let scenarios = [ProbabilityMap::uniform(3, 0.5)];
        let result = run_pair_cached(plan, g, 0, 2, &scenarios, &ctl, cache);
        (result, ctl)
    }

    #[test]
    fn keys_follow_the_plan() {
        let fp = chain().fingerprint();
        let paths = PathKey {
            graph: fp,
            src: 0,
            dst: 2,
            algo: AlgoKey::from(Dfs.info()),
        };
        let cuts = CutKey {
            paths,
            algo: AlgoKey::from(&FAKE_INFO),
        };

        let full = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let bare = Plan::new(&Dfs, None, None).unwrap();

        assert_eq!(
            full.keys(&fp, 0, 2),
            PairKeys {
                paths,
                cuts: Some(cuts),
                prepare: Some(PrepareKey {
                    sets: SetKey::Cuts(cuts),
                    algo: AlgoKey::from(CutDisjoint.info()),
                }),
            }
        );
        assert_eq!(
            bare.keys(&fp, 0, 2),
            PairKeys {
                paths,
                cuts: None,
                prepare: None
            }
        );
    }

    #[test]
    fn second_run_only_evaluates() {
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        let g = chain();
        let mut cache = Cache::new(&g);

        let (first, _) = run_cached(&plan, &g, &mut cache);
        let (second, ctl) = run_cached(&plan, &g, &mut cache);

        assert_eq!(first, second);
        assert_eq!(second.availability, [0.125]);
        use {Stage::*, StageEvent::*};
        assert_eq!(ctl.stages(), [(Evaluate, Started), (Evaluate, Finished)]);
        let keys = plan.keys(&g.fingerprint(), 0, 2);
        assert_eq!(cache.paths(&keys.paths).unwrap().sets.len(), 1);
        assert_eq!(cache.cuts(&keys.cuts.unwrap()).unwrap().sets.len(), 3);
        assert!(cache.prepared(&keys.prepare.unwrap()).is_some());
    }

    #[test]
    fn changed_stage_and_everything_after_it_rerun() {
        let g = chain();
        let mut cache = Cache::new(&g);
        let plan = Plan::new(&Dfs, Some(&ChainCuts), Some(&CutDisjoint)).unwrap();
        run_cached(&plan, &g, &mut cache);
        use {Stage::*, StageEvent::*};

        let other_avail = Plan::new(&Dfs, Some(&ChainCuts), Some(&OtherAvail)).unwrap();
        let (_, ctl) = run_cached(&other_avail, &g, &mut cache);
        assert_eq!(
            ctl.stages(),
            [
                (Prepare, Started),
                (Prepare, Finished),
                (Evaluate, Started),
                (Evaluate, Finished)
            ]
        );

        let new_version = Plan::new(&Dfs, Some(&ChainCutsV2), Some(&CutDisjoint)).unwrap();
        let (_, ctl) = run_cached(&new_version, &g, &mut cache);
        assert_eq!(ctl.stages()[0], (Cuts, Started));
        assert_eq!(ctl.stages().len(), 6);

        // Same structure, different edge order: same fingerprint, so a full hit
        let reordered = Graph::from_edges(3, &[(2, 1), (1, 0)]).unwrap();
        let (_, ctl) = run_cached(&plan, &reordered, &mut cache);
        assert_eq!(ctl.stages().len(), 2);
    }

    #[test]
    fn failed_stage_is_not_cached() {
        let plan = Plan::new(&Dfs, Some(&FailingCuts), Some(&CutDisjoint)).unwrap();
        let g = chain();
        let mut cache = Cache::new(&g);

        run_cached(&plan, &g, &mut cache);
        let (result, ctl) = run_cached(&plan, &g, &mut cache);

        assert_eq!(result.error, Some(Error::TooManySets { limit: 0 }));
        assert_eq!(result.path_count, Some(1));
        use {Stage::*, StageEvent::*};
        assert_eq!(ctl.stages(), [(Cuts, Started), (Cuts, Finished)]);
    }

    #[test]
    fn plan_without_availability_ignores_scenarios() {
        let plan = Plan::new(&Dfs, None, None).unwrap();

        let result = run_pair(
            &plan,
            &chain(),
            0,
            2,
            &[ProbabilityMap::uniform(2, 0.9)],
            &NoControl,
        );

        assert_eq!(result.error, None);
        assert_eq!(result.path_count, Some(1));
        assert!(result.availability.is_empty());
    }
}
