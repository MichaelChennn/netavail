//! Result files: per-pair CSV with stage times, set export and `meta.json` (architecture.md §4, §6).

use std::fmt::Write;
use std::sync::Mutex;

use netavail_core::cache::AlgoKey;
use netavail_core::control::{Cancelled, Stage, StageEvent};
use netavail_core::graph::Fingerprint;
use netavail_core::types::{NodeId, SetFamily, SetKind};
use netavail_core::{Control, PairResult};
use serde::Serialize;

use crate::topology::Prob;

/// Milliseconds spent in each stage; `None` if the stage did not run (not chosen or cached).
#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize)]
pub struct StageTimes {
    pub paths_ms: Option<f64>,
    pub cuts_ms: Option<f64>,
    pub prepare_ms: Option<f64>,
    pub evaluate_ms: Option<f64>,
}

impl StageTimes {
    fn slot(&mut self, stage: Stage) -> &mut Option<f64> {
        match stage {
            Stage::Paths => &mut self.paths_ms,
            Stage::Cuts => &mut self.cuts_ms,
            Stage::Prepare => &mut self.prepare_ms,
            Stage::Evaluate => &mut self.evaluate_ms,
        }
    }
}

/// A [`PairResult`] with the time of each stage.
#[derive(Clone, PartialEq, Debug)]
pub struct PairReport {
    pub result: PairResult,
    pub times: StageTimes,
}

/// Times the stages of one pair with `clock` (milliseconds); other calls go to `inner`.
///
/// Core cannot time itself (D15), so the CLI passes an `Instant`-based clock
/// and the browser passes `performance.now()`.
pub struct StageTimer<'a> {
    clock: &'a (dyn Fn() -> f64 + Sync),
    inner: &'a dyn Control,
    state: Mutex<(StageTimes, f64)>,
}

impl<'a> StageTimer<'a> {
    pub fn new(clock: &'a (dyn Fn() -> f64 + Sync), inner: &'a dyn Control) -> StageTimer<'a> {
        StageTimer {
            clock,
            inner,
            state: Mutex::new((StageTimes::default(), 0.0)),
        }
    }

    pub fn times(&self) -> StageTimes {
        self.state.lock().unwrap().0
    }
}

impl Control for StageTimer<'_> {
    fn check(&self) -> Result<(), Cancelled> {
        self.inner.check()
    }

    fn on_stage(&self, src: NodeId, dst: NodeId, stage: Stage, event: StageEvent) {
        let now = (self.clock)();
        let mut state = self.state.lock().unwrap();
        match event {
            StageEvent::Started => state.1 = now,
            StageEvent::Finished => *state.0.slot(stage) = Some(now - state.1),
        }
        drop(state);
        self.inner.on_stage(src, dst, stage, event);
    }

    fn max_sets(&self) -> Option<usize> {
        self.inner.max_sets()
    }

    fn max_terms(&self) -> Option<usize> {
        self.inner.max_terms()
    }
}

/// One row per pair: counts, one availability column per scenario, stage times in ms, error.
///
/// Availability columns are named like the reference answers (`0.9 Availability`);
/// a scenario from the file's probabilities is `file Availability`.
pub fn results_csv(probs: &[Prob], reports: &[PairReport]) -> String {
    let mut out = String::from("source,target,paths,cuts");
    for p in probs {
        write!(out, ",{p} Availability").unwrap();
    }
    out.push_str(",paths_ms,cuts_ms,prepare_ms,evaluate_ms,error\n");

    let opt = |v: Option<usize>| v.map(|v| v.to_string()).unwrap_or_default();
    let ms = |v: Option<f64>| v.map(|v| format!("{v:.3}")).unwrap_or_default();
    for PairReport {
        result: r,
        times: t,
    } in reports
    {
        write!(
            out,
            "{},{},{},{}",
            r.src,
            r.dst,
            opt(r.path_count),
            opt(r.cut_count)
        )
        .unwrap();
        for i in 0..probs.len() {
            let value = r.availability.get(i).map(|a| a.to_string());
            write!(out, ",{}", value.unwrap_or_default()).unwrap();
        }
        let error = r.error.as_ref().map(|e| e.to_string()).unwrap_or_default();
        writeln!(
            out,
            ",{},{},{},{},{}",
            ms(t.paths_ms),
            ms(t.cuts_ms),
            ms(t.prepare_ms),
            ms(t.evaluate_ms),
            csv_field(&error)
        )
        .unwrap();
    }
    out
}

/// Sets in the reference format (`Pathset_<T>.csv` / `Mincutset_<T>.csv`), one family per row.
pub fn sets_csv(families: &[SetFamily]) -> String {
    let column = match families.first().map(|f| f.kind) {
        Some(SetKind::Cuts) => "mincutsets",
        _ => "pathsets",
    };
    let mut out = format!("source,target,{column},length\n");
    for f in families {
        let sets: Vec<String> = f
            .sets
            .iter()
            .map(|set| {
                let nodes: Vec<String> = set.iter().map(|v| v.to_string()).collect();
                format!("[{}]", nodes.join(", "))
            })
            .collect();
        writeln!(
            out,
            "{},{},\"[{}]\",{}",
            f.src,
            f.dst,
            sets.join(", "),
            f.sets.len()
        )
        .unwrap();
    }
    out
}

/// Contents of `meta.json` next to exported sets: which graph and algorithms produced them (D28).
#[derive(Serialize, Debug)]
pub struct Meta {
    /// Graph fingerprint as 64 hex digits.
    pub graph: String,
    pub paths: AlgoMeta,
    pub cuts: Option<AlgoMeta>,
}

#[derive(Serialize, Debug)]
pub struct AlgoMeta {
    pub id: &'static str,
    pub version: u32,
}

impl From<AlgoKey> for AlgoMeta {
    fn from(key: AlgoKey) -> AlgoMeta {
        AlgoMeta {
            id: key.id,
            version: key.version,
        }
    }
}

impl Meta {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Meta always serializes")
    }
}

/// Lower-case hex of a fingerprint.
pub fn hex(fp: &Fingerprint) -> String {
    fp.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Quotes a field that contains a comma, quote or newline.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::read_sets;
    use netavail_core::sets::minimalpaths::Dfs;
    use netavail_core::{Error, Graph, NoControl, Plan, run_pair};
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn timer_records_each_stage_between_its_events() {
        let ticks = AtomicU32::new(0);
        // 0, 10, 30, 60, …: each call advances 10 ms more than the last
        let clock = || {
            let n = ticks.fetch_add(1, Ordering::Relaxed) as f64;
            5.0 * n * (n + 1.0)
        };
        let timer = StageTimer::new(&clock, &NoControl);
        let plan = Plan::new(&Dfs, None, None).unwrap();
        let g = Graph::from_edges(3, &[(0, 1), (1, 2)]).unwrap();

        run_pair(&plan, &g, 0, 2, &[], &timer);

        assert_eq!(
            timer.times(),
            StageTimes {
                paths_ms: Some(10.0),
                ..StageTimes::default()
            }
        );
    }

    #[test]
    fn results_csv_has_one_row_per_pair_and_blank_cells_for_missing_values() {
        let ok = PairReport {
            result: PairResult {
                src: 0,
                dst: 2,
                path_count: Some(1),
                cut_count: Some(3),
                availability: vec![0.125, 0.5],
                error: None,
            },
            times: StageTimes {
                paths_ms: Some(1.5),
                cuts_ms: Some(0.25),
                prepare_ms: None,
                evaluate_ms: Some(0.0005),
            },
        };
        let failed = PairReport {
            result: PairResult {
                src: 1,
                dst: 2,
                path_count: Some(4),
                cut_count: None,
                availability: vec![],
                error: Some(Error::ScenarioSize {
                    scenario: 1,
                    nodes: 2,
                    node_count: 3,
                }),
            },
            times: StageTimes::default(),
        };

        let csv = results_csv(&[Prob::Uniform(0.9), Prob::File], &[ok, failed]);

        assert_eq!(
            csv,
            "source,target,paths,cuts,0.9 Availability,file Availability,\
             paths_ms,cuts_ms,prepare_ms,evaluate_ms,error\n\
             0,2,1,3,0.125,0.5,1.500,0.250,,0.001,\n\
             1,2,4,,,,,,,,\"scenario 1 has probabilities for 2 nodes, the graph has 3\"\n"
        );
    }

    #[test]
    fn exported_sets_read_back_unchanged() {
        let families = vec![
            SetFamily {
                kind: SetKind::Cuts,
                src: 0,
                dst: 1,
                sets: vec![vec![0], vec![1], vec![2, 13, 14]],
            },
            SetFamily {
                kind: SetKind::Cuts,
                src: 0,
                dst: 2,
                sets: vec![vec![0], vec![2]],
            },
        ];

        let csv = sets_csv(&families);

        assert!(
            csv.starts_with("source,target,mincutsets,length\n0,1,\"[[0], [1], [2, 13, 14]]\",3\n")
        );
        assert_eq!(read_sets(&csv, SetKind::Cuts).unwrap(), families);
    }

    #[test]
    fn meta_names_graph_and_algorithms() {
        let g = Graph::from_edges(2, &[(0, 1)]).unwrap();
        let meta = Meta {
            graph: hex(&g.fingerprint()),
            paths: AlgoMeta {
                id: "dfs",
                version: 1,
            },
            cuts: None,
        };

        let json: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();

        assert_eq!(json["graph"].as_str().unwrap().len(), 64);
        assert_eq!(json["paths"]["id"], "dfs");
        assert!(json["cuts"].is_null());
    }
}
