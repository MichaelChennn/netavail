//! Every small reference topology loads with the expected size.

mod common;

use netavail_io::Topology;

#[test]
fn all_reference_topologies_load() {
    for (name, nodes, edges) in common::TOPOLOGIES {
        let text = common::read(name, "topology.json");

        let t = Topology::from_json(&text).unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(t.name(), name);
        assert_eq!(t.graph().node_count(), nodes, "{name}");
        assert_eq!(t.edges().len(), edges, "{name}");
    }
}
