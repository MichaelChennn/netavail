//! Path disjoint availability, ported from PyRBD-Suite `availability/pathset.cpp`
//! (old name `pathset`).
//!
//! Every node of a minimal path (src and dst included) is up; the paths are made
//! mutually exclusive with `make_disjoint`, so availability = sum(product_of_terms_per_set).

use crate::availability::disjoint::disjoint_terms;
use crate::availability::{AvailabilityAlgorithm, Prepared};
use crate::control::Control;
use crate::error::Error;
use crate::prob::ProbabilityMap;
use crate::registry::{AlgoInfo, Citation};
use crate::types::{Lit, SetFamily, SetKind, Term};

pub struct PathDisjoint;

static INFO: AlgoInfo = AlgoInfo {
    id: "path_disjoint",
    name: "Path disjoint",
    summary: "Make minimal path sets disjoint, sum availability",
    version: 1,
    aliases: &["pathset"],
    citation: Some(Citation {
        text: "S. Janardhanan, Y. Chen, C. Mas-Machuca, \"PyRBD++: An Open-Source Fast \
               Reliability Block Diagram Evaluation Tool\", RNDM, 2025",
        url: None,
    }),
};

impl AvailabilityAlgorithm for PathDisjoint {
    fn info(&self) -> &'static AlgoInfo {
        &INFO
    }

    fn input(&self) -> SetKind {
        SetKind::Paths
    }

    fn prepare(&self, paths: &SetFamily, ctl: &dyn Control) -> Result<Box<dyn Prepared>, Error> {
        if paths.kind != SetKind::Paths {
            return Err(Error::WrongInputKind {
                expected: SetKind::Paths,
                found: paths.kind,
            });
        }
        // No path (src and dst disconnected) gives no term, so availability 0
        let pending: Vec<Term> = paths
            .sets
            .iter()
            .map(|set| set.iter().map(|&node| Lit::up(node)).collect())
            .collect();

        let terms = disjoint_terms(pending, ctl)?;
        Ok(Box::new(ProbaSets { terms }))
    }
}

/// Mutually exclusive terms whose probabilities sum to the availability between src and dst.
struct ProbaSets {
    terms: Vec<Term>,
}

impl Prepared for ProbaSets {
    fn evaluate(&self, p: &ProbabilityMap) -> f64 {
        self.terms
            .iter()
            .map(|term| term.iter().map(|&lit| p.get(lit)).product::<f64>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Cancelled, NoControl};
    use crate::types::{NodeId, NodeSet};

    fn paths(src: NodeId, dst: NodeId, sets: Vec<NodeSet>) -> SetFamily {
        SetFamily {
            kind: SetKind::Paths,
            src,
            dst,
            sets,
        }
    }

    fn avail(family: &SetFamily, p: &[f64]) -> f64 {
        PathDisjoint
            .prepare(family, &NoControl)
            .unwrap()
            .evaluate(&ProbabilityMap::from_nodes(p))
    }

    /// Two parallel chains 0-1-3-5 and 0-2-4-5 share src and dst, so terms must be split.
    #[test]
    fn matches_closed_form_on_two_parallel_chains() {
        let family = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);
        let p = [0.95, 0.9, 0.8, 0.7, 0.6, 0.85];

        let got = avail(&family, &p);

        let expected = p[0] * p[5] * (1.0 - (1.0 - p[1] * p[3]) * (1.0 - p[2] * p[4]));
        assert!((got - expected).abs() < 1e-12, "{got} vs {expected}");
    }

    #[test]
    fn adjacent_pair_needs_only_both_endpoints() {
        let got = avail(&paths(0, 1, vec![vec![0, 1]]), &[0.9, 0.8]);

        assert!((got - 0.72).abs() < 1e-12, "{got}");
    }

    #[test]
    fn disconnected_pair_has_zero_availability() {
        let got = avail(&paths(0, 1, vec![]), &[0.9, 0.8]);

        assert_eq!(got, 0.0);
    }

    #[test]
    fn rejects_cut_sets() {
        let mut family = paths(0, 1, vec![vec![0], vec![1]]);
        family.kind = SetKind::Cuts;

        let err = PathDisjoint.prepare(&family, &NoControl).err().unwrap();

        assert_eq!(
            err,
            Error::WrongInputKind {
                expected: SetKind::Paths,
                found: SetKind::Cuts
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

    /// The two-chain paths hold at most 3 terms at once (1 kept + 2 pending after the first round).
    #[test]
    fn fails_once_terms_exceed_max_terms() {
        let family = paths(0, 5, vec![vec![0, 1, 3, 5], vec![0, 2, 4, 5]]);

        let err = PathDisjoint.prepare(&family, &MaxTerms(2)).err();
        let ok = PathDisjoint.prepare(&family, &MaxTerms(3));

        assert_eq!(err, Some(Error::TooManyTerms { limit: 2 }));
        assert!(ok.is_ok());
    }

    #[test]
    fn stops_when_cancelled() {
        let family = paths(0, 3, vec![vec![0, 1, 3]]);

        let err = PathDisjoint.prepare(&family, &CancelNow).err().unwrap();

        assert_eq!(err, Error::Cancelled);
    }
}
