//! Topology JSON in the PyRBD-Suite frontend format (architecture.md §4; D24).

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use netavail_core::transform::to_link_graph;
use netavail_core::types::NodeId;
use netavail_core::{Graph, ProbabilityMap};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid topology JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported topology version {0} (expected 1)")]
    UnsupportedVersion(u64),
    #[error("duplicate node id {id:?}")]
    DuplicateNodeId { id: String },
    #[error("edge {edge}: unknown node id {id:?}")]
    UnknownNodeId { edge: usize, id: String },
    #[error("node {node:?}: probability {prob} is not in [0, 1]")]
    InvalidProbability { node: String, prob: f64 },
    #[error("edge {edge}: probability {prob} is not in [0, 1]")]
    InvalidLinkProbability { edge: usize, prob: f64 },
    #[error("probability {0} is not in [0, 1]")]
    ProbabilityOutOfRange(f64),
    #[error(transparent)]
    Graph(#[from] netavail_core::Error),
}

/// Where a scenario's availabilities come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prob {
    /// Every node (or every link) has availability p.
    Uniform(f64),
    /// Each node (or link) has the `prob` given in the topology file.
    File,
}

/// `file`, or the number itself; also the CSV column name.
impl fmt::Display for Prob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Prob::Uniform(p) => write!(f, "{p}"),
            Prob::File => f.write_str("file"),
        }
    }
}

/// Reads `file` or a number; the range is checked by [`Topology::scenario`].
impl FromStr for Prob {
    type Err = String;

    fn from_str(s: &str) -> Result<Prob, String> {
        if s == "file" {
            return Ok(Prob::File);
        }
        s.parse()
            .map(Prob::Uniform)
            .map_err(|_| format!("{s:?} is neither a probability nor \"file\""))
    }
}

/// A node as given in the file. `prob` is its availability.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub prob: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// An edge with endpoints already mapped to node indices. `prob` is the link availability.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub prob: f64,
}

#[derive(Deserialize)]
struct RawTopology {
    version: Option<u64>,
    #[serde(default)]
    name: String,
    nodes: Vec<RawNode>,
    edges: Vec<RawEdge>,
}

#[derive(Deserialize)]
struct RawNode {
    id: String,
    label: Option<String>,
    #[serde(default = "one")]
    prob: f64,
    x: Option<f64>,
    y: Option<f64>,
}

#[derive(Deserialize)]
struct RawEdge {
    source: String,
    target: String,
    #[serde(default = "one")]
    prob: f64,
}

fn one() -> f64 {
    1.0
}

/// A validated topology: holding one means the graph and probabilities are well formed.
#[derive(Clone, Debug)]
pub struct Topology {
    name: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    graph: Graph,
}

impl Topology {
    /// Parses and validates. Node ids map to 0..n-1 in the order they appear in `nodes`.
    pub fn from_json(text: &str) -> Result<Topology, Error> {
        let raw: RawTopology = serde_json::from_str(text)?;
        if let Some(v) = raw.version.filter(|&v| v != 1) {
            return Err(Error::UnsupportedVersion(v));
        }

        let mut index: HashMap<&str, NodeId> = HashMap::new();
        for (i, n) in raw.nodes.iter().enumerate() {
            if index.insert(&n.id, i as NodeId).is_some() {
                return Err(Error::DuplicateNodeId { id: n.id.clone() });
            }
            if !(0.0..=1.0).contains(&n.prob) {
                return Err(Error::InvalidProbability {
                    node: n.id.clone(),
                    prob: n.prob,
                });
            }
        }
        let lookup = |edge: usize, id: &str| {
            index.get(id).copied().ok_or_else(|| Error::UnknownNodeId {
                edge,
                id: id.to_string(),
            })
        };
        let edges = raw
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if !(0.0..=1.0).contains(&e.prob) {
                    return Err(Error::InvalidLinkProbability {
                        edge: i,
                        prob: e.prob,
                    });
                }
                Ok(Edge {
                    source: lookup(i, &e.source)?,
                    target: lookup(i, &e.target)?,
                    prob: e.prob,
                })
            })
            .collect::<Result<Vec<Edge>, Error>>()?;
        let pairs: Vec<(NodeId, NodeId)> = edges.iter().map(|e| (e.source, e.target)).collect();
        let graph = Graph::from_edges(raw.nodes.len() as u32, &pairs)?;

        let nodes = raw
            .nodes
            .into_iter()
            .map(|n| Node {
                label: n.label.unwrap_or_else(|| n.id.clone()),
                id: n.id,
                prob: n.prob,
                x: n.x,
                y: n.y,
            })
            .collect();
        Ok(Topology {
            name: raw.name,
            nodes,
            edges,
            graph,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// One scenario: a probability per node, then, if `links` is given, one per edge,
    /// in the order of [`Topology::link_graph`].
    pub fn scenario(&self, nodes: Prob, links: Option<Prob>) -> Result<ProbabilityMap, Error> {
        let expand = |prob: Prob, file: Vec<f64>| match prob {
            Prob::Uniform(p) if !(0.0..=1.0).contains(&p) => Err(Error::ProbabilityOutOfRange(p)),
            Prob::Uniform(p) => Ok(vec![p; file.len()]),
            Prob::File => Ok(file),
        };
        let mut probs = expand(nodes, self.nodes.iter().map(|n| n.prob).collect())?;
        if let Some(links) = links {
            probs.extend(expand(links, self.edges.iter().map(|e| e.prob).collect())?);
        }
        Ok(ProbabilityMap::from_nodes(&probs))
    }

    /// The graph with every edge turned into a node, so links can fail (D18).
    /// Edge `i` becomes node `n + i`; see [`netavail_core::transform::to_link_graph`].
    pub fn link_graph(&self) -> Graph {
        let pairs: Vec<(NodeId, NodeId)> =
            self.edges.iter().map(|e| (e.source, e.target)).collect();
        to_link_graph(self.nodes.len() as u32, &pairs)
            .expect("edges were checked when the topology was loaded")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netavail_core::types::Lit;

    #[test]
    fn maps_string_ids_in_order_of_appearance() {
        let t = Topology::from_json(
            r#"{
                "name": "tiny",
                "nodes": [
                    {"id": "c", "label": "Cork", "prob": 0.9, "x": 1.5, "y": -2.0},
                    {"id": "a"},
                    {"id": "b"}
                ],
                "edges": [
                    {"source": "a", "target": "c", "prob": 0.8},
                    {"source": "b", "target": "a"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(t.name(), "tiny");
        assert_eq!(
            t.nodes(),
            &[
                Node {
                    id: "c".into(),
                    label: "Cork".into(),
                    prob: 0.9,
                    x: Some(1.5),
                    y: Some(-2.0)
                },
                Node {
                    id: "a".into(),
                    label: "a".into(),
                    prob: 1.0,
                    x: None,
                    y: None
                },
                Node {
                    id: "b".into(),
                    label: "b".into(),
                    prob: 1.0,
                    x: None,
                    y: None
                },
            ]
        );
        assert_eq!(
            t.edges(),
            &[
                Edge {
                    source: 1,
                    target: 0,
                    prob: 0.8
                },
                Edge {
                    source: 2,
                    target: 1,
                    prob: 1.0
                },
            ]
        );
        assert_eq!(t.graph().node_count(), 3);
        assert_eq!(t.graph().neighbors(1), &[0, 2]);
    }

    fn load_err(json: &str) -> Error {
        Topology::from_json(json).unwrap_err()
    }

    #[test]
    fn rejects_unknown_node_id_in_edge() {
        let err = load_err(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}],
                "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "z"}]}"#,
        );

        assert!(
            matches!(&err, Error::UnknownNodeId { edge: 1, id } if id == "z"),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_duplicate_node_id() {
        let err = load_err(r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "a"}], "edges": []}"#);

        assert!(
            matches!(&err, Error::DuplicateNodeId { id } if id == "a"),
            "{err:?}"
        );
    }

    #[test]
    fn accepts_version_1_only() {
        assert!(Topology::from_json(r#"{"version": 1, "nodes": [], "edges": []}"#).is_ok());

        let err = load_err(r#"{"version": 2, "nodes": [], "edges": []}"#);

        assert!(matches!(err, Error::UnsupportedVersion(2)), "{err:?}");
    }

    #[test]
    fn rejects_node_probability_outside_unit_interval() {
        let err = load_err(r#"{"nodes": [{"id": "a"}, {"id": "b", "prob": 1.5}], "edges": []}"#);

        assert!(
            matches!(&err, Error::InvalidProbability { node, prob } if node == "b" && *prob == 1.5),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_link_probability_outside_unit_interval() {
        let err = load_err(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "c", "prob": -0.1}]}"#,
        );

        assert!(
            matches!(err, Error::InvalidLinkProbability { edge: 1, prob } if prob == -0.1),
            "{err:?}"
        );
    }

    /// Path a-b-c: edges 0 and 1 become nodes 3 and 4.
    #[test]
    fn link_graph_puts_a_node_on_each_edge() {
        let t = Topology::from_json(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "c"}]}"#,
        )
        .unwrap();

        let g = t.link_graph();

        assert_eq!(g.node_count(), 5);
        assert_eq!(g.neighbors(1), &[3, 4]);
        assert_eq!(g.neighbors(3), &[0, 1]);
        assert_eq!(g.neighbors(4), &[1, 2]);
    }

    #[test]
    fn rejects_malformed_json_and_missing_required_fields() {
        for json in [
            r#"{"nodes": [], "edges": ["#,
            r#"{"nodes": [{"label": "no id"}], "edges": []}"#,
            r#"{"nodes": [{"id": "a"}], "edges": [{"source": "a"}]}"#,
        ] {
            let err = load_err(json);
            assert!(matches!(err, Error::Json(_)), "{json}: {err:?}");
        }
    }

    #[test]
    fn passes_graph_errors_through() {
        let err = load_err(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}],
                "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "a"}]}"#,
        );

        assert!(
            matches!(
                err,
                Error::Graph(netavail_core::Error::DuplicateEdge { edge: 1, first: 0 })
            ),
            "{err:?}"
        );
    }

    /// Path x-y-z with its own probabilities on every node and edge.
    fn with_probs() -> Topology {
        Topology::from_json(
            r#"{"nodes": [{"id": "x", "prob": 0.7}, {"id": "y"}, {"id": "z", "prob": 0.25}],
                "edges": [{"source": "x", "target": "y", "prob": 0.5},
                          {"source": "y", "target": "z"}]}"#,
        )
        .unwrap()
    }

    fn ups(p: &ProbabilityMap) -> Vec<f64> {
        (0..p.node_count() as u32)
            .map(|i| p.get(Lit::up(i)))
            .collect()
    }

    #[test]
    fn file_scenario_follows_node_order() {
        let p = with_probs().scenario(Prob::File, None).unwrap();

        assert_eq!(ups(&p), [0.7, 1.0, 0.25]);
        assert_eq!(p.get(Lit::down(2)), 0.75);
    }

    #[test]
    fn uniform_scenario_gives_every_node_p() {
        let p = with_probs().scenario(Prob::Uniform(0.9), None).unwrap();

        assert_eq!(p, ProbabilityMap::uniform(3, 0.9));
    }

    /// Edge i is node n + i of the link graph, so its probability comes after the nodes.
    #[test]
    fn link_probabilities_follow_the_nodes() {
        let t = with_probs();

        let uniform = t.scenario(Prob::File, Some(Prob::Uniform(0.99))).unwrap();
        let file = t.scenario(Prob::Uniform(0.9), Some(Prob::File)).unwrap();

        assert_eq!(ups(&uniform), [0.7, 1.0, 0.25, 0.99, 0.99]);
        assert_eq!(ups(&file), [0.9, 0.9, 0.9, 0.5, 1.0]);
    }

    /// The same p written into the file gives bit-identical probabilities.
    #[test]
    fn file_with_one_p_everywhere_equals_uniform() {
        let t = Topology::from_json(
            r#"{"nodes": [{"id": "a", "prob": 0.9}, {"id": "b", "prob": 0.9}],
                "edges": [{"source": "a", "target": "b", "prob": 0.9}]}"#,
        )
        .unwrap();

        assert_eq!(
            t.scenario(Prob::File, Some(Prob::File)).unwrap(),
            t.scenario(Prob::Uniform(0.9), Some(Prob::Uniform(0.9)))
                .unwrap()
        );
    }

    #[test]
    fn rejects_uniform_probability_outside_unit_interval() {
        let t = with_probs();

        for (nodes, links) in [
            (Prob::Uniform(1.5), None),
            (Prob::File, Some(Prob::Uniform(-0.1))),
            (Prob::Uniform(f64::NAN), None),
        ] {
            let err = t.scenario(nodes, links).unwrap_err();
            assert!(matches!(err, Error::ProbabilityOutOfRange(_)), "{err:?}");
        }
    }

    #[test]
    fn prob_reads_and_writes_a_number_or_file() {
        assert_eq!("file".parse::<Prob>(), Ok(Prob::File));
        assert_eq!("0.99".parse::<Prob>(), Ok(Prob::Uniform(0.99)));
        assert!("high".parse::<Prob>().is_err());

        assert_eq!(Prob::File.to_string(), "file");
        assert_eq!(Prob::Uniform(0.9).to_string(), "0.9");
        assert_eq!(Prob::Uniform(1.0).to_string(), "1");
    }
}
