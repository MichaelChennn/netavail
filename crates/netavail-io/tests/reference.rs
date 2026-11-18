//! Every registered algorithm against the old reference answers (D34).

mod common;

use std::collections::HashMap;

use netavail_core::registry::AlgoInfo;
use netavail_core::types::{NodeId, SetFamily, SetKind};
use netavail_core::{Control, CutAlgorithm, Error, NoControl, Plan, all_pairs, registry, run_all};
use netavail_io::Topology;
use netavail_io::reference::{
    check_availability, check_cuts, check_paths, read_availability, read_sets, scenarios,
    validate_sets,
};

#[test]
fn reference_sets_are_valid_for_their_topology() {
    for (name, _, _) in common::TOPOLOGIES {
        let topo = Topology::from_json(&common::read(name, "topology.json")).unwrap();
        for (prefix, kind) in [("Pathset", SetKind::Paths), ("Mincutset", SetKind::Cuts)] {
            let csv = common::read(name, &format!("{prefix}_{name}.csv"));
            let families = read_sets(&csv, kind).unwrap();

            validate_sets(topo.graph(), &families).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}

#[test]
fn path_algorithms_match_reference_pathsets() {
    let mut failures = Vec::new();
    for algo in registry::path_algorithms() {
        for (name, nodes, _) in common::TOPOLOGIES {
            let topo = Topology::from_json(&common::read(name, "topology.json")).unwrap();
            let csv = common::read(name, &format!("Pathset_{name}.csv"));
            let expected = read_sets(&csv, SetKind::Paths).unwrap();
            assert_eq!(
                expected.len() as u32,
                nodes * (nodes - 1) / 2,
                "{name}: one row per pair"
            );

            let lines = check_paths(*algo, topo.graph(), &expected);
            failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Each cut algorithm gets the reference path sets as input (D34).
#[test]
fn cut_algorithms_match_reference_cutsets() {
    assert!(!registry::cut_algorithms().is_empty());
    let mut failures = Vec::new();
    for algo in registry::cut_algorithms() {
        for (name, _, _) in common::TOPOLOGIES {
            let csv = common::read(name, &format!("Pathset_{name}.csv"));
            let paths = read_sets(&csv, SetKind::Paths).unwrap();
            let csv = common::read(name, &format!("Mincutset_{name}.csv"));
            let expected = read_sets(&csv, SetKind::Cuts).unwrap();

            let lines = check_cuts(*algo, &paths, &expected);
            failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Each availability algorithm gets the reference sets of its input kind (D10).
#[test]
fn availability_algorithms_match_reference_availability() {
    let mut failures = Vec::new();
    for algo in registry::availability_algorithms() {
        let prefix = match algo.input() {
            SetKind::Paths => "Pathset",
            SetKind::Cuts => "Mincutset",
        };
        for (name, nodes, _) in common::TOPOLOGIES {
            let csv = common::read(name, &format!("{prefix}_{name}.csv"));
            let families = read_sets(&csv, algo.input()).unwrap();
            let csv = common::read(name, &format!("Availability_{name}.csv"));
            let table = read_availability(&csv).unwrap();
            assert_eq!(table.probs, [0.9, 0.99, 0.999, 0.9999], "{name}");

            let lines = check_availability(*algo, &families, &table, nodes);
            failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

static REFERENCE_CUTS: AlgoInfo = AlgoInfo {
    id: "reference_cuts",
    name: "Reference cuts",
    summary: "Test double: returns the cut sets from Mincutset_<T>.csv",
    version: 1,
    aliases: &[],
    citation: None,
};

/// Stands in for a cut algorithm until stage 4 ports one.
struct ReferenceCuts(HashMap<(NodeId, NodeId), SetFamily>);

impl CutAlgorithm for ReferenceCuts {
    fn info(&self) -> &'static AlgoInfo {
        &REFERENCE_CUTS
    }
    fn from_paths(&self, paths: &SetFamily, _: &dyn Control) -> Result<SetFamily, Error> {
        Ok(self.0[&(paths.src, paths.dst)].clone())
    }
}

/// The whole pipeline (paths -> reference cuts -> availability) over all pairs.
#[test]
fn pipeline_matches_reference_on_all_pairs() {
    let mut failures = Vec::new();
    for (name, nodes, _) in common::TOPOLOGIES {
        let topo = Topology::from_json(&common::read(name, "topology.json")).unwrap();
        let paths = read_sets(
            &common::read(name, &format!("Pathset_{name}.csv")),
            SetKind::Paths,
        )
        .unwrap();
        let csv = common::read(name, &format!("Mincutset_{name}.csv"));
        let cuts = read_sets(&csv, SetKind::Cuts).unwrap();
        let table =
            read_availability(&common::read(name, &format!("Availability_{name}.csv"))).unwrap();
        let scenarios = scenarios(&table, nodes);
        // Plan takes &'static; leaking one small map per topology is fine in a test
        let cut_algo: &'static ReferenceCuts = Box::leak(Box::new(ReferenceCuts(
            cuts.iter().map(|f| ((f.src, f.dst), f.clone())).collect(),
        )));

        for &path_algo in registry::path_algorithms() {
            for &avail_algo in registry::availability_algorithms() {
                let ids = format!("{}+{}", path_algo.info().id, avail_algo.info().id);
                let plan = Plan::new(path_algo, Some(cut_algo), Some(avail_algo)).unwrap();
                let results = run_all(
                    &plan,
                    topo.graph(),
                    &all_pairs(nodes),
                    &scenarios,
                    &NoControl,
                );
                assert_eq!(
                    results.len(),
                    table.rows.len(),
                    "{ids} {name}: result count"
                );

                for (i, result) in results.iter().enumerate() {
                    let row = &table.rows[i];
                    let pair = (result.src, result.dst);
                    assert_eq!(pair, (row.src, row.dst), "{ids} {name}: pair order");
                    let counts = (result.path_count, result.cut_count, &result.error);
                    let expected = (Some(paths[i].sets.len()), Some(cuts[i].sets.len()), &None);
                    if counts != expected {
                        failures.push(format!("{ids} {name} {pair:?}: {counts:?} vs {expected:?}"));
                        continue;
                    }
                    for ((p, &actual), &expected) in table
                        .probs
                        .iter()
                        .zip(&result.availability)
                        .zip(&row.values)
                    {
                        if (actual - expected).abs() > 1e-9 {
                            failures.push(format!(
                                "{ids} {name} {pair:?} p={p}: {actual} vs {expected}"
                            ));
                        }
                    }
                    if result.availability.len() != row.values.len() {
                        failures.push(format!("{ids} {name} {pair:?}: scenario count"));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
