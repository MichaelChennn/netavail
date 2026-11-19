//! Every small reference topology loads with the expected size and survives a write.

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

#[test]
fn all_reference_topologies_keep_their_fingerprint_when_written() {
    for (name, _, _) in common::TOPOLOGIES {
        let t = Topology::from_json(&common::read(name, "topology.json")).unwrap();

        let back = Topology::from_json(&t.to_json()).unwrap();

        assert_eq!(
            back.graph().fingerprint(),
            t.graph().fingerprint(),
            "{name}"
        );
        assert_eq!(back.nodes(), t.nodes(), "{name}");
        assert_eq!(back.edges(), t.edges(), "{name}");
    }
}
