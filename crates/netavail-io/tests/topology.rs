//! Every small reference topology loads with the expected size.

use std::path::PathBuf;

use netavail_io::Topology;

/// (name, nodes, edges), counted independently with Python's json module.
const TOPOLOGIES: [(&str, u32, usize); 8] = [
    ("Abilene", 11, 14),
    ("Austria_24", 24, 50),
    ("Germany_17", 17, 26),
    ("HiberniaUk", 13, 13),
    ("Spain", 17, 28),
    ("Sweden", 25, 29),
    ("dfn-bwin", 10, 45),
    ("polska", 12, 18),
];

fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference")
}

#[test]
fn all_reference_topologies_load() {
    for (name, nodes, edges) in TOPOLOGIES {
        let path = reference_dir().join(name).join("topology.json");
        let text = std::fs::read_to_string(&path).unwrap();

        let t = Topology::from_json(&text).unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(t.name(), name);
        assert_eq!(t.graph().node_count(), nodes, "{name}");
        assert_eq!(t.edges().len(), edges, "{name}");
    }
}
