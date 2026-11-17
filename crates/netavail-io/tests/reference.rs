//! Every registered algorithm against the old reference answers (D34).

mod common;

use netavail_core::ProbabilityMap;
use netavail_core::types::SetKind;
use netavail_core::{NoControl, registry};
use netavail_io::Topology;
use netavail_io::reference::{diff_sets, read_availability, read_sets};

#[test]
fn path_algorithms_match_reference_pathsets() {
    let mut failures = Vec::new();
    for algo in registry::path_algorithms() {
        let id = algo.info().id;
        for (name, nodes, _) in common::TOPOLOGIES {
            let topo = Topology::from_json(&common::read(name, "topology.json")).unwrap();
            let csv = common::read(name, &format!("Pathset_{name}.csv"));
            let expected = read_sets(&csv, SetKind::Paths).unwrap();
            assert_eq!(
                expected.len() as u32,
                nodes * (nodes - 1) / 2,
                "{name}: one row per pair"
            );

            for exp in &expected {
                let actual = algo
                    .find(topo.graph(), exp.src, exp.dst, &NoControl)
                    .unwrap();
                let diff = diff_sets(exp, &actual);
                if !diff.is_empty() {
                    failures.push(format!("{id} {name} {}->{}: {diff:?}", exp.src, exp.dst));
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

/// Each availability algorithm gets the reference sets of its input kind (D10).
#[test]
fn availability_algorithms_match_reference_availability() {
    let mut failures = Vec::new();
    for algo in registry::availability_algorithms() {
        let id = algo.info().id;
        let (prefix, kind) = match algo.input() {
            SetKind::Paths => ("Pathset", SetKind::Paths),
            SetKind::Cuts => ("Mincutset", SetKind::Cuts),
        };
        for (name, nodes, _) in common::TOPOLOGIES {
            let csv = common::read(name, &format!("{prefix}_{name}.csv"));
            let families = read_sets(&csv, kind).unwrap();
            let csv = common::read(name, &format!("Availability_{name}.csv"));
            let table = read_availability(&csv).unwrap();
            assert_eq!(table.probs, [0.9, 0.99, 0.999, 0.9999], "{name}");
            assert_eq!(table.rows.len(), families.len(), "{name}: row count");
            let scenarios: Vec<ProbabilityMap> = table
                .probs
                .iter()
                .map(|&p| ProbabilityMap::uniform(nodes as usize, p))
                .collect();

            for (family, row) in families.iter().zip(&table.rows) {
                assert_eq!((family.src, family.dst), (row.src, row.dst), "{name}");
                let prepared = algo.prepare(family, &NoControl).unwrap();
                for ((p, scenario), &expected) in
                    table.probs.iter().zip(&scenarios).zip(&row.values)
                {
                    let actual = prepared.evaluate(scenario);
                    if (actual - expected).abs() > 1e-9 {
                        failures.push(format!(
                            "{id} {name} {}->{} p={p}: {actual} vs {expected}",
                            row.src, row.dst
                        ));
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
