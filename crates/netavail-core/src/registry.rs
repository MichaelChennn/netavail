//! All algorithms, as listed in the browser, the CLI and the consistency tests (architecture.md §3.3; D26, D27).
//!
//! Adding an algorithm = one new file implementing its trait + one line here.

use crate::availability::AvailabilityAlgorithm;
use crate::sets::minimalcuts::CutAlgorithm;
use crate::sets::minimalpaths::PathAlgorithm;

/// Static description of an algorithm.
#[derive(Debug)]
pub struct AlgoInfo {
    /// Used in code, on the command line and in the browser.
    pub id: &'static str,
    /// Short UI name, at most 18 characters.
    pub name: &'static str,
    /// One sentence on how the algorithm works; shown as a tooltip.
    pub summary: &'static str,
    /// Bumped after a bug fix so cached results are recomputed.
    pub version: u32,
    /// Old names (D27).
    pub aliases: &'static [&'static str],
    pub citation: Option<Citation>,
}

/// The paper an algorithm comes from.
#[derive(Debug)]
pub struct Citation {
    pub text: &'static str,
    pub url: Option<&'static str>,
}

impl AlgoInfo {
    fn matches(&self, key: &str) -> bool {
        self.id == key || self.aliases.contains(&key)
    }
}

static PATHS: &[&dyn PathAlgorithm] = &[];
static CUTS: &[&dyn CutAlgorithm] = &[];
static AVAILABILITY: &[&dyn AvailabilityAlgorithm] = &[];

pub fn path_algorithms() -> &'static [&'static dyn PathAlgorithm] {
    PATHS
}

pub fn cut_algorithms() -> &'static [&'static dyn CutAlgorithm] {
    CUTS
}

pub fn availability_algorithms() -> &'static [&'static dyn AvailabilityAlgorithm] {
    AVAILABILITY
}

/// Looks up a path algorithm by id or old name.
pub fn find_path(key: &str) -> Option<&'static dyn PathAlgorithm> {
    PATHS.iter().copied().find(|a| a.info().matches(key))
}

/// Looks up a cut algorithm by id or old name.
pub fn find_cut(key: &str) -> Option<&'static dyn CutAlgorithm> {
    CUTS.iter().copied().find(|a| a.info().matches(key))
}

/// Looks up an availability algorithm by id or old name.
pub fn find_availability(key: &str) -> Option<&'static dyn AvailabilityAlgorithm> {
    AVAILABILITY.iter().copied().find(|a| a.info().matches(key))
}
