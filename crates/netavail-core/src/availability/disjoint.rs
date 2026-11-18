//! Sum of disjoint products shared by the availability algorithms,
//! ported from PyRBD-Suite `availability/common.cpp`.

use crate::control::Control;
use crate::error::Error;
use crate::types::{Lit, Term};

/// Keeps the first set, makes every remaining set disjoint from it, repeats.
/// The returned terms are mutually exclusive and cover the union of `pending`.
pub(crate) fn disjoint_terms(
    mut pending: Vec<Term>,
    ctl: &dyn Control,
) -> Result<Vec<Term>, Error> {
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
    Ok(terms)
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
