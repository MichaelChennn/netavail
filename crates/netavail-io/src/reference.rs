//! Old reference answers (Pathset / Mincutset CSV) and comparison against them (D34).
//!
//! Shared by `cargo test` and, later, `netavail verify`.

use std::collections::HashMap;

use netavail_core::types::{NodeId, NodeSet, SetFamily, SetKind};

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

/// Sets present in only one of two families, compared after normalizing (D3).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SetDiff {
    pub missing: Vec<NodeSet>,
    pub extra: Vec<NodeSet>,
}

impl SetDiff {
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty()
    }
}

/// Compares as multisets, so a set produced twice shows up in `extra`.
pub fn diff_sets(expected: &SetFamily, actual: &SetFamily) -> SetDiff {
    let (expected, actual) = (expected.normalize().sets, actual.normalize().sets);
    SetDiff {
        missing: minus(&expected, &actual),
        extra: minus(&actual, &expected),
    }
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
                missing: vec![vec![0, 1, 3]],
                extra: vec![vec![0, 3], vec![0, 3, 4]],
            }
        );
        assert!(diff_sets(&expected, &expected.normalize()).is_empty());
    }
}
