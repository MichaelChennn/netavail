//! Cut disjoint availability (old name `mcs`).
//!
//! Converts minimal cut sets to probability sets: removes {src} and {dst},
//! inverts signs (every node of a cut is down), then iteratively applies
//! [`make_disjoint`]. The resulting sets are mutually exclusive, so
//! availability = P(src) * P(dst) * (1 - sum(product_of_terms_per_set)).

use crate::availability::{AvailabilityAlgorithm, Prepared};
use crate::control::Control;
use crate::error::Error;
use crate::prob::ProbabilityMap;
use crate::registry::AlgoInfo;
use crate::types::{Lit, NodeId, SetFamily, SetKind, Term};

pub struct CutDisjoint;

static INFO: AlgoInfo = AlgoInfo {
    id: "cut_disjoint",
    name: "Cut disjoint",
    summary: "Make minimal cut sets disjoint, sum unavailability",
    version: 1,
    aliases: &["mcs"],
    citation: None,
};

impl AvailabilityAlgorithm for CutDisjoint {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn input(&self) -> SetKind {
        SetKind::Cuts
    }

    fn prepare(&self, cuts: &SetFamily, ctl: &dyn Control) -> Result<Box<dyn Prepared>, Error> {
        if cuts.kind != SetKind::Cuts {
            return Err(Error::WrongInputKind {
                expected: SetKind::Cuts,
                found: cuts.kind,
            });
        }
        // Remove {src} and {dst}, then invert the minimal cut sets
        let mut pending: Vec<Term> = cuts
            .sets
            .iter()
            .filter(|set| **set != [cuts.src] && **set != [cuts.dst])
            .map(|set| set.iter().map(|&node| Lit::down(node)).collect())
            .collect();

        // Keep the first set, make every remaining set disjoint from it, repeat
        let mut terms = Vec::with_capacity(pending.len() * 3);
        while !pending.is_empty() {
            ctl.check()?;
            let mut rest = std::mem::take(&mut pending).into_iter();
            let selected = rest.next().unwrap();
            pending = rest.flat_map(|set| make_disjoint(&selected, set)).collect();
            terms.push(selected);
            // Terms can grow exponentially; the browser sets a limit so memory stays bounded
            if let Some(limit) = ctl.max_terms()
                && terms.len() + pending.len() > limit
            {
                return Err(Error::TooManyTerms { limit });
            }
        }
        Ok(Box::new(ProbaSets {
            src: cuts.src,
            dst: cuts.dst,
            terms,
        }))
    }
}

/// Creates sets disjoint from `set1` that together cover `set2 \ set1`:
/// 1. RC = set1 \ set2
/// 2. Progressively append -RC[i] to set2 producing disjoint products.
fn make_disjoint(set1: &[Lit], mut set2: Term) -> Vec<Term> {
    // RC set: elements in set1 but not in set2
    let mut rc = Vec::new();
    for &lit in set1 {
        // Already disjoint: x in set1 and -x in set2
        if set2.contains(&lit.negate()) {
            return vec![set2];
        }
        if !set2.contains(&lit) {
            rc.push(lit);
        }
    }

    // Empty RC means set1 is a subset of set2: set2 is absorbed
    let mut result = Vec::with_capacity(rc.len());
    for lit in rc {
        set2.push(lit.negate());
        result.push(set2.clone());
        *set2.last_mut().unwrap() = lit;
    }
    result
}

/// Mutually exclusive terms whose probabilities sum to the unavailability
/// between src and dst, given both are up.
struct ProbaSets {
    src: NodeId,
    dst: NodeId,
    terms: Vec<Term>,
}

impl Prepared for ProbaSets {
    fn evaluate(&self, p: &ProbabilityMap) -> f64 {
        let unavail: f64 = self
            .terms
            .iter()
            .map(|term| term.iter().map(|&lit| p.get(lit)).product::<f64>())
            .sum();
        p.get(Lit::up(self.src)) * p.get(Lit::up(self.dst)) * (1.0 - unavail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::types::NodeSet;

    fn cuts(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Cuts,
            src,
            dst,
            sets,
        }
    }

    /// Two parallel chains 0-1-3-5 and 0-2-4-5: the cuts overlap, so terms must be split.
    #[test]
    fn matches_closed_form_on_two_parallel_chains() {
        let family = cuts(
            0,
            5,
            vec![
                vec![0],
                vec![5],
                vec![1, 2],
                vec![1, 4],
                vec![3, 2],
                vec![3, 4],
            ],
        );
        let p = [0.95, 0.9, 0.8, 0.7, 0.6, 0.85];

        let avail = CutDisjoint
            .prepare(&family, &NoControl)
            .unwrap()
            .evaluate(&ProbabilityMap::from_nodes(&p));

        let expected = p[0] * p[5] * (1.0 - (1.0 - p[1] * p[3]) * (1.0 - p[2] * p[4]));
        assert!((avail - expected).abs() < 1e-12, "{avail} vs {expected}");
    }

    #[test]
    fn adjacent_pair_needs_only_both_endpoints() {
        let family = cuts(0, 1, vec![vec![0], vec![1]]);

        let avail = CutDisjoint
            .prepare(&family, &NoControl)
            .unwrap()
            .evaluate(&ProbabilityMap::from_nodes(&[0.9, 0.8]));

        assert!((avail - 0.72).abs() < 1e-12, "{avail}");
    }

    #[test]
    fn rejects_path_sets() {
        let mut family = cuts(0, 2, vec![vec![0, 1, 2]]);
        family.kind = SetKind::Paths;

        let err = CutDisjoint.prepare(&family, &NoControl).err().unwrap();

        assert_eq!(
            err,
            Error::WrongInputKind {
                expected: SetKind::Cuts,
                found: SetKind::Paths
            }
        );
    }

    struct CancelNow;
    impl Control for CancelNow {
        fn check(&self) -> Result<(), Cancelled> {
            Err(Cancelled)
        }
    }

    struct MaxTerms(usize);
    impl Control for MaxTerms {
        fn max_terms(&self) -> Option<usize> {
            Some(self.0)
        }
    }

    /// The two-chain cuts hold at most 5 terms at once (1 kept + 4 pending after the first round).
    #[test]
    fn fails_once_terms_exceed_max_terms() {
        let family = cuts(0, 5, vec![vec![1, 2], vec![1, 4], vec![3, 2], vec![3, 4]]);

        let err = CutDisjoint.prepare(&family, &MaxTerms(4)).err();
        let ok = CutDisjoint.prepare(&family, &MaxTerms(5));

        assert_eq!(err, Some(Error::TooManyTerms { limit: 4 }));
        assert!(ok.is_ok());
    }

    #[test]
    fn stops_when_cancelled() {
        let family = cuts(0, 3, vec![vec![0], vec![3], vec![1, 2]]);

        let err = CutDisjoint.prepare(&family, &CancelNow).err().unwrap();

        assert_eq!(err, Error::Cancelled);
    }
}
