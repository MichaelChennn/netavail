//! Old reference answers (Pathset / Mincutset / Availability CSV) and comparison against them (D34).
//!
//! Shared by `cargo test` and `netavail verify`.

use std::collections::{HashMap, VecDeque};

use netavail_core::types::{NodeId, NodeSet, SetFamily, SetKind};
use netavail_core::{
    AvailabilityAlgorithm, CutAlgorithm, Graph, NoControl, PathAlgorithm, ProbabilityMap,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("line {line}: {reason}")]
    BadRow { line: usize, reason: String },
    #[error("line {line}: length column says {expected} sets, found {found}")]
    LengthMismatch {
        line: usize,
        expected: usize,
        found: usize,
    },
    #[error("pair ({src}, {dst}): {reason}")]
    InvalidSet {
        src: NodeId,
        dst: NodeId,
        reason: String,
    },
}

/// Reads a `Pathset_<T>.csv` or `Mincutset_<T>.csv`, one family per row.
///
/// Rows look like `0,3,"[[0, 1, 3], [0, 2, 3]]",2`; the header line is skipped.
pub fn read_sets(text: &str, kind: SetKind) -> Result<Vec<SetFamily>, Error> {
    let mut families = Vec::new();
    for (i, row) in text.lines().enumerate().skip(1) {
        if row.trim().is_empty() {
            continue;
        }
        let line = i + 1;
        let (family, expected) =
            read_row(row, kind).map_err(|reason| Error::BadRow { line, reason })?;
        if family.sets.len() != expected {
            return Err(Error::LengthMismatch {
                line,
                expected,
                found: family.sets.len(),
            });
        }
        families.push(family);
    }
    Ok(families)
}

/// The set column contains commas, so split two fields off the front and one off the back.
/// Returns the family and the value of its length column.
fn read_row(row: &str, kind: SetKind) -> Result<(SetFamily, usize), String> {
    let mut front = row.splitn(3, ',');
    let src = front.next().unwrap_or_default();
    let dst = front.next().ok_or("missing target column")?;
    let rest = front.next().ok_or("missing sets column")?;
    let (sets, len) = rest.rsplit_once(',').ok_or("missing length column")?;

    let node = |s: &str| {
        s.trim()
            .parse::<NodeId>()
            .map_err(|e| format!("{s:?}: {e}"))
    };
    let len = len
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("{len:?}: {e}"))?;
    let sets = serde_json::from_str(sets.trim().trim_matches('"')).map_err(|e| e.to_string())?;
    let family = SetFamily {
        kind,
        src: node(src)?,
        dst: node(dst)?,
        sets,
    };
    Ok((family, len))
}

/// Contents of an `Availability_<T>.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct AvailabilityTable {
    /// The uniform node availability of each column, read from the header.
    pub probs: Vec<f64>,
    pub rows: Vec<AvailabilityRow>,
}

/// One node pair; `values[i]` belongs to `probs[i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct AvailabilityRow {
    pub src: NodeId,
    pub dst: NodeId,
    pub values: Vec<f64>,
}

/// Reads an `Availability_<T>.csv`.
///
/// The header looks like `source,target,0.9 Availability,0.99 Availability`;
/// every node has the column's p and links never fail.
pub fn read_availability(text: &str) -> Result<AvailabilityTable, Error> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let probs = header
        .split(',')
        .skip(2)
        .map(|col| {
            let p = col
                .trim()
                .strip_suffix(" Availability")
                .ok_or("column name")?;
            p.parse::<f64>().map_err(|_| "probability")
        })
        .collect::<Result<Vec<f64>, _>>()
        .map_err(|what| Error::BadRow {
            line: 1,
            reason: format!("bad {what} in header {header:?}"),
        })?;

    let mut rows = Vec::new();
    for (i, row) in lines.enumerate() {
        if row.trim().is_empty() {
            continue;
        }
        let line = i + 2;
        let row = read_availability_row(row, probs.len())
            .map_err(|reason| Error::BadRow { line, reason })?;
        rows.push(row);
    }
    Ok(AvailabilityTable { probs, rows })
}

fn read_availability_row(row: &str, columns: usize) -> Result<AvailabilityRow, String> {
    let fields: Vec<&str> = row.split(',').map(str::trim).collect();
    if fields.len() != columns + 2 {
        return Err(format!(
            "expected {} columns, found {}",
            columns + 2,
            fields.len()
        ));
    }
    let parse_err = |s: &str, e: &dyn std::fmt::Display| format!("{s:?}: {e}");
    let node = |s: &str| s.parse::<NodeId>().map_err(|e| parse_err(s, &e));
    let values = fields[2..]
        .iter()
        .map(|s| s.parse::<f64>().map_err(|e| parse_err(s, &e)))
        .collect::<Result<_, _>>()?;
    Ok(AvailabilityRow {
        src: node(fields[0])?,
        dst: node(fields[1])?,
        values,
    })
}

/// What identifies a family: (kind, src, dst).
pub type FamilyHeader = (SetKind, NodeId, NodeId);

/// How two families differ: a different (kind, src, dst), or sets present in only one
/// of them after normalizing (D3).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SetDiff {
    /// (expected, actual) when kind, src or dst differ.
    pub header: Option<(FamilyHeader, FamilyHeader)>,
    pub missing: Vec<NodeSet>,
    pub extra: Vec<NodeSet>,
}

impl SetDiff {
    pub fn is_empty(&self) -> bool {
        self.header.is_none() && self.missing.is_empty() && self.extra.is_empty()
    }
}

/// Compares as multisets, so a set produced twice shows up in `extra`.
pub fn diff_sets(expected: &SetFamily, actual: &SetFamily) -> SetDiff {
    let head = |f: &SetFamily| -> FamilyHeader { (f.kind, f.src, f.dst) };
    let header = (head(expected) != head(actual)).then(|| (head(expected), head(actual)));
    let (expected, actual) = (expected.normalize().sets, actual.normalize().sets);
    SetDiff {
        header,
        missing: minus(&expected, &actual),
        extra: minus(&actual, &expected),
    }
}

/// Checks reference sets against the graph, since old CSVs carry no fingerprint (D28):
/// every path walks along edges from src to dst; every cut separates src from dst.
pub fn validate_sets(g: &Graph, families: &[SetFamily]) -> Result<(), Error> {
    for f in families {
        let fail = |reason: String| Error::InvalidSet {
            src: f.src,
            dst: f.dst,
            reason,
        };
        let n = g.node_count();
        if f.src >= n || f.dst >= n || f.src == f.dst {
            return Err(fail(format!("not a valid pair in a graph of {n} nodes")));
        }
        for set in &f.sets {
            if let Some(&v) = set.iter().find(|&&v| v >= n) {
                return Err(fail(format!("{set:?}: node {v} is out of range")));
            }
            let ok = match f.kind {
                SetKind::Paths => is_path(g, f.src, f.dst, set),
                SetKind::Cuts => separates(g, f.src, f.dst, set),
            };
            if !ok {
                let what = match f.kind {
                    SetKind::Paths => "is not a path",
                    SetKind::Cuts => "does not separate the pair",
                };
                return Err(fail(format!("{set:?} {what}")));
            }
        }
    }
    Ok(())
}

fn is_path(g: &Graph, src: NodeId, dst: NodeId, set: &[NodeId]) -> bool {
    set.first() == Some(&src)
        && set.last() == Some(&dst)
        && set.windows(2).all(|w| g.neighbors(w[0]).contains(&w[1]))
}

/// Breadth-first search from src that may not enter a cut node.
fn separates(g: &Graph, src: NodeId, dst: NodeId, cut: &[NodeId]) -> bool {
    let mut seen = vec![false; g.node_count() as usize];
    for &v in cut {
        seen[v as usize] = true;
    }
    if seen[src as usize] || seen[dst as usize] {
        return true;
    }
    seen[src as usize] = true;
    let mut queue = VecDeque::from([src]);
    while let Some(u) = queue.pop_front() {
        for &v in g.neighbors(u) {
            if v == dst {
                return false;
            }
            if !seen[v as usize] {
                seen[v as usize] = true;
                queue.push_back(v);
            }
        }
    }
    true
}

/// One scenario per column of the table: every node at the column's p (D9).
pub fn scenarios(table: &AvailabilityTable, node_count: u32) -> Vec<ProbabilityMap> {
    table
        .probs
        .iter()
        .map(|&p| ProbabilityMap::uniform(node_count as usize, p))
        .collect()
}

/// Runs a path algorithm on every pair of `expected`; returns one line per mismatching pair.
pub fn check_paths(algo: &dyn PathAlgorithm, g: &Graph, expected: &[SetFamily]) -> Vec<String> {
    let id = algo.info().id;
    expected
        .iter()
        .filter_map(|exp| {
            let line = match algo.find(g, exp.src, exp.dst, &NoControl) {
                Ok(actual) => {
                    let diff = diff_sets(exp, &actual);
                    (!diff.is_empty()).then(|| format!("{diff:?}"))
                }
                Err(e) => Some(e.to_string()),
            };
            line.map(|l| format!("{id} {}->{}: {l}", exp.src, exp.dst))
        })
        .collect()
}

/// Runs a cut algorithm on the reference paths of each pair and compares with the reference cuts.
pub fn check_cuts(
    algo: &dyn CutAlgorithm,
    paths: &[SetFamily],
    expected: &[SetFamily],
) -> Vec<String> {
    let id = algo.info().id;
    paths
        .iter()
        .zip(expected)
        .filter_map(|(paths, exp)| {
            let line = match algo.from_paths(paths, &NoControl) {
                Ok(actual) => {
                    let diff = diff_sets(exp, &actual);
                    (!diff.is_empty()).then(|| format!("{diff:?}"))
                }
                Err(e) => Some(e.to_string()),
            };
            line.map(|l| format!("{id} {}->{}: {l}", exp.src, exp.dst))
        })
        .collect()
}

/// Prepares each reference family (of the algorithm's input kind) and compares every
/// scenario with the table, within 1e-9 (D3).
pub fn check_availability(
    algo: &dyn AvailabilityAlgorithm,
    families: &[SetFamily],
    table: &AvailabilityTable,
    node_count: u32,
) -> Vec<String> {
    let id = algo.info().id;
    let scenarios = scenarios(table, node_count);
    let mut failures = Vec::new();
    if families.len() != table.rows.len() {
        failures.push(format!(
            "{id}: {} set families but {} availability rows",
            families.len(),
            table.rows.len()
        ));
    }
    for (family, row) in families.iter().zip(&table.rows) {
        let pair = format!("{id} {}->{}", row.src, row.dst);
        if (family.src, family.dst) != (row.src, row.dst) {
            failures.push(format!(
                "{pair}: sets are for {}->{}",
                family.src, family.dst
            ));
            continue;
        }
        let prepared = match algo.prepare(family, &NoControl) {
            Ok(prepared) => prepared,
            Err(e) => {
                failures.push(format!("{pair}: {e}"));
                continue;
            }
        };
        for ((p, scenario), &expected) in table.probs.iter().zip(&scenarios).zip(&row.values) {
            let actual = prepared.evaluate(scenario);
            if (actual - expected).abs() > 1e-9 {
                failures.push(format!("{pair} p={p}: {actual} vs {expected}"));
            }
        }
    }
    failures
}

/// Multiset difference `a - b`, keeping the order of `a`.
fn minus(a: &[NodeSet], b: &[NodeSet]) -> Vec<NodeSet> {
    let mut unmatched: HashMap<&NodeSet, usize> = HashMap::new();
    for set in b {
        *unmatched.entry(set).or_default() += 1;
    }
    a.iter()
        .filter(|set| match unmatched.get_mut(set) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "source,target,pathsets,length\n\
                       0,1,\"[[0, 1]]\",1\n\
                       0,3,\"[[0, 1, 3], [0, 2, 3]]\",2\n";

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    #[test]
    fn reads_one_family_per_row() {
        let families = read_sets(CSV, SetKind::Paths).unwrap();

        assert_eq!(
            families,
            vec![
                paths(0, 1, vec![vec![0, 1]]),
                paths(0, 3, vec![vec![0, 1, 3], vec![0, 2, 3]]),
            ]
        );
    }

    #[test]
    fn rejects_row_whose_length_column_disagrees() {
        let text = "source,target,pathsets,length\n0,3,\"[[0, 1, 3], [0, 2, 3]]\",3\n";

        let err = read_sets(text, SetKind::Paths).unwrap_err();

        assert!(matches!(
            err,
            Error::LengthMismatch {
                line: 2,
                expected: 3,
                found: 2
            }
        ));
    }

    #[test]
    fn rejects_malformed_row_with_line_number() {
        for row in [
            "0,3",
            "0,x,\"[[0, 3]]\",1",
            "0,3,\"[[0, 3]\",1",
            "0,3,\"[[0, 3]]\",one",
        ] {
            let text = format!("source,target,pathsets,length\n0,1,\"[[0, 1]]\",1\n{row}\n");

            let err = read_sets(&text, SetKind::Paths).unwrap_err();

            assert!(matches!(err, Error::BadRow { line: 3, .. }), "{row}: {err}");
        }
    }

    #[test]
    fn reads_probabilities_from_header_and_one_row_per_pair() {
        let text = "source,target,0.9 Availability,0.99 Availability\n\
                    0,1,0.81,0.9801\n\
                    0,2,0.8019,0.979901\n";

        let table = read_availability(text).unwrap();

        assert_eq!(table.probs, vec![0.9, 0.99]);
        assert_eq!(
            table.rows,
            vec![
                AvailabilityRow {
                    src: 0,
                    dst: 1,
                    values: vec![0.81, 0.9801]
                },
                AvailabilityRow {
                    src: 0,
                    dst: 2,
                    values: vec![0.8019, 0.979901]
                },
            ]
        );
    }

    #[test]
    fn rejects_bad_availability_header_or_row_with_line_number() {
        let header = "source,target,0.9 Availability,0.99 Availability";
        for (text, line) in [
            ("source,target,0.9 Avail\n".to_string(), 1),
            ("source,target,p Availability\n".to_string(), 1),
            (format!("{header}\n0,1,0.81,0.98\n0,2,0.81\n"), 3),
            (format!("{header}\n0,1,0.81,0.98\n0,2,0.81,x\n"), 3),
        ] {
            let err = read_availability(&text).unwrap_err();

            assert!(
                matches!(err, Error::BadRow { line: l, .. } if l == line),
                "{text:?}: {err}"
            );
        }
    }

    #[test]
    fn diff_ignores_order_and_counts_duplicates() {
        let expected = paths(0, 3, vec![vec![0, 1, 3], vec![0, 2, 3], vec![0, 3]]);
        let actual = paths(
            0,
            3,
            vec![vec![3, 0], vec![3, 2, 0], vec![0, 4, 3], vec![3, 0]],
        );

        let diff = diff_sets(&expected, &actual);

        assert_eq!(
            diff,
            SetDiff {
                header: None,
                missing: vec![vec![0, 1, 3]],
                extra: vec![vec![0, 3], vec![0, 3, 4]],
            }
        );
        assert!(diff_sets(&expected, &expected.normalize()).is_empty());
    }

    #[test]
    fn diff_reports_other_pair_or_kind() {
        let expected = paths(0, 3, vec![vec![0, 3]]);
        let mut other_pair = expected.clone();
        other_pair.dst = 4;
        let mut other_kind = expected.clone();
        other_kind.kind = SetKind::Cuts;

        let pair_diff = diff_sets(&expected, &other_pair);
        let kind_diff = diff_sets(&expected, &other_kind);

        assert_eq!(
            pair_diff.header,
            Some(((SetKind::Paths, 0, 3), (SetKind::Paths, 0, 4)))
        );
        assert!(!kind_diff.is_empty());
    }

    /// 0 - 1 - 3 and 0 - 2 - 3
    fn diamond() -> Graph {
        Graph::from_edges(4, &[(0, 1), (1, 3), (0, 2), (2, 3)]).unwrap()
    }

    fn cuts(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Cuts,
            ..paths(src, dst, sets)
        }
    }

    #[test]
    fn valid_paths_and_cuts_pass() {
        let ok = [
            paths(0, 3, vec![vec![0, 1, 3], vec![0, 2, 3]]),
            cuts(0, 3, vec![vec![0], vec![3], vec![1, 2]]),
        ];

        assert!(validate_sets(&diamond(), &ok).is_ok());
    }

    #[test]
    fn rejects_broken_paths_and_leaky_cuts() {
        for bad in [
            paths(0, 3, vec![vec![0, 3]]),
            paths(0, 3, vec![vec![1, 3]]),
            paths(0, 3, vec![vec![0, 1, 9]]),
            cuts(0, 3, vec![vec![1]]),
            cuts(0, 0, vec![vec![1]]),
        ] {
            let err = validate_sets(&diamond(), std::slice::from_ref(&bad));

            assert!(
                matches!(err, Err(Error::InvalidSet { .. })),
                "{bad:?} passed"
            );
        }
    }
}
