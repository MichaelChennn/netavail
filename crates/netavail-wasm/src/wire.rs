//! Types crossing the JS boundary; tsify turns them into TS types (D31).
//! They pass through [`tsify::Ts`], so a malformed value is an ordinary error, not a leak.

use serde::{Deserialize, Serialize};
use tsify::Tsify;

/// Algorithm ids or old names, looked up in the registry.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
pub struct PlanSpec {
    pub paths: String,
    #[serde(default)]
    #[tsify(optional)]
    pub cuts: Option<String>,
    #[serde(default)]
    #[tsify(optional)]
    pub availability: Option<String>,
}

/// A p for every node (or link), or `"file"` for each one's own prob in the topology.
#[derive(Tsify, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(untagged)]
#[tsify(type = "number | \"file\"")]
pub enum Prob {
    Uniform(f64),
    File(FileTag),
}

/// The string `"file"`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FileTag {
    File,
}

impl From<Prob> for netavail_io::Prob {
    fn from(p: Prob) -> netavail_io::Prob {
        match p {
            Prob::Uniform(p) => netavail_io::Prob::Uniform(p),
            Prob::File(FileTag::File) => netavail_io::Prob::File,
        }
    }
}

#[derive(Tsify, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AlgoKind {
    Paths,
    Cuts,
    Availability,
}

#[derive(Tsify, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SetKind {
    Paths,
    Cuts,
}

#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
pub struct AlgorithmInfo {
    pub kind: AlgoKind,
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: u32,
    pub aliases: Vec<String>,
    /// Sets an availability algorithm takes; missing for set algorithms.
    pub input: Option<SetKind>,
}

#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
pub struct AlgorithmList {
    pub algorithms: Vec<AlgorithmInfo>,
}

#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
pub struct NodeView {
    pub id: String,
    pub label: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// The loaded topology, for drawing and labelling.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
pub struct TopologyView {
    pub name: String,
    /// Graph fingerprint, 64 hex digits.
    pub graph: String,
    pub nodes: Vec<NodeView>,
    /// Node index pairs.
    pub edges: Vec<(u32, u32)>,
}

/// Milliseconds per stage; missing if the stage did not run (not chosen or cached).
#[derive(Tsify, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Debug)]
pub struct StageTimes {
    pub paths_ms: Option<f64>,
    pub cuts_ms: Option<f64>,
    pub prepare_ms: Option<f64>,
    pub evaluate_ms: Option<f64>,
}

#[derive(Tsify, Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PairReport {
    pub src: u32,
    pub dst: u32,
    pub paths: Option<usize>,
    pub cuts: Option<usize>,
    /// One value per entry of `probs`; empty without an availability algorithm.
    pub availability: Vec<f64>,
    pub error: Option<String>,
    pub times: StageTimes,
}

#[derive(Tsify, Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PairReports {
    pub reports: Vec<PairReport>,
}

/// Sets of one pair, nodes as indices; missing if not computed yet.
#[derive(Tsify, Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Sets {
    pub sets: Option<Vec<Vec<u32>>>,
}
