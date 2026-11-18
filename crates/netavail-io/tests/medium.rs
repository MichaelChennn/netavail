//! Medium topologies against the old reference answers (D34). Not in the repository:
//! run with `NETAVAIL_REFERENCE_DIR=<.../topologies> cargo test --release -- --ignored`.
//!
//! Topologies come from `web/public/topologies/<T>.json`, whose node order matches the CSVs.
//! cut_disjoint takes minutes per topology here, so availability is only checked with path_sdp.

use std::path::PathBuf;

use netavail_core::registry;
use netavail_core::types::{SetFamily, SetKind};
use netavail_io::Topology;
use netavail_io::reference::{
    check_availability, check_cuts, check_paths, read_availability, read_sets, validate_sets,
};

/// (name, nodes, edges), counted with Python's json module.
const TOPOLOGIES: [(&str, u32, usize); 4] = [
    ("Norway", 27, 51),
    ("USA_26", 26, 42),
    ("Nobel_EU", 28, 41),
    ("janos-us-ca", 39, 61),
];

/// Cut algorithms only get this many pairs per topology, to keep the run short.
const CUT_PAIRS: usize = 10;

/// combination grows exponentially: 10 pairs of Norway take about a minute,
/// janos-us-ca much longer. The other cut algorithms take seconds.
fn too_slow(algo: &str, topology: &str) -> bool {
    algo.starts_with("combination") && matches!(topology, "Norway" | "janos-us-ca")
}

fn reference(topology: &str, file: &str) -> String {
    let dir = std::env::var("NETAVAIL_REFERENCE_DIR")
        .expect("set NETAVAIL_REFERENCE_DIR to the reference topologies directory");
    let path = PathBuf::from(dir).join(topology).join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn topology(name: &str) -> Topology {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/public/topologies")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Topology::from_json(&text).unwrap()
}

fn sets(name: &str, prefix: &str, kind: SetKind) -> Vec<SetFamily> {
    read_sets(&reference(name, &format!("{prefix}_{name}.csv")), kind).unwrap()
}

fn assert_none(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
}

#[test]
#[ignore = "needs NETAVAIL_REFERENCE_DIR"]
fn medium_topologies_match_their_reference_sets() {
    for (name, nodes, edges) in TOPOLOGIES {
        let topo = topology(name);
        assert_eq!(
            (topo.graph().node_count(), topo.edges().len()),
            (nodes, edges),
            "{name}"
        );
        for (prefix, kind) in [("Pathset", SetKind::Paths), ("Mincutset", SetKind::Cuts)] {
            let families = sets(name, prefix, kind);
            assert_eq!(families.len() as u32, nodes * (nodes - 1) / 2, "{name}");
            validate_sets(topo.graph(), &families).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}

#[test]
#[ignore = "needs NETAVAIL_REFERENCE_DIR"]
fn medium_path_algorithms_match_reference() {
    let mut failures = Vec::new();
    for (name, _, _) in TOPOLOGIES {
        let topo = topology(name);
        let expected = sets(name, "Pathset", SetKind::Paths);
        for algo in registry::path_algorithms() {
            let lines = check_paths(*algo, topo.graph(), &expected);
            failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
        }
    }
    assert_none(failures);
}

#[test]
#[ignore = "needs NETAVAIL_REFERENCE_DIR"]
fn medium_cut_algorithms_match_reference_on_some_pairs() {
    let mut failures = Vec::new();
    for (name, _, _) in TOPOLOGIES {
        let paths = sets(name, "Pathset", SetKind::Paths);
        let cuts = sets(name, "Mincutset", SetKind::Cuts);
        // Every few pairs, so all sources are covered
        let step = paths.len().div_ceil(CUT_PAIRS);
        let pick =
            |f: &[SetFamily]| -> Vec<SetFamily> { f.iter().step_by(step).cloned().collect() };
        let (paths, cuts) = (pick(&paths), pick(&cuts));
        for algo in registry::cut_algorithms() {
            if too_slow(algo.info().id, name) {
                continue;
            }
            let lines = check_cuts(*algo, &paths, &cuts);
            failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
        }
    }
    assert_none(failures);
}

#[test]
#[ignore = "needs NETAVAIL_REFERENCE_DIR"]
fn medium_path_sdp_matches_reference_availability() {
    let algo = registry::find_availability("path_sdp").unwrap();
    let mut failures = Vec::new();
    for (name, nodes, _) in TOPOLOGIES {
        let paths = sets(name, "Pathset", SetKind::Paths);
        let table =
            read_availability(&reference(name, &format!("Availability_{name}.csv"))).unwrap();

        let lines = check_availability(algo, &paths, &table, nodes);
        failures.extend(lines.into_iter().map(|l| format!("{name} {l}")));
    }
    assert_none(failures);
}
