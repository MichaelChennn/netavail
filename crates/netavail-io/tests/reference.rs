//! Every registered algorithm against the old reference answers (D34).

mod common;

use netavail_core::types::SetKind;
use netavail_core::{NoControl, registry};
use netavail_io::Topology;
use netavail_io::reference::{diff_sets, read_sets};

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
