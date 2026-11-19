//! wasm-bindgen `Engine` for the browser (architecture.md §5; D31).
//!
//! The logic lives in [`session`] so it can be tested natively;
//! this file only converts between JS values and Rust types.

pub mod session;
pub mod wire;

use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use session::Session;
use wire::{
    AlgorithmList, PairReport, PairReports, PlanSpec, SetKind as WireSetKind, Sets, TopologyView,
};

#[wasm_bindgen]
extern "C" {
    /// Milliseconds; available in Worker scope and in Node.
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

#[wasm_bindgen]
pub struct Engine {
    session: Session,
}

#[wasm_bindgen]
impl Engine {
    /// Parses and checks a topology JSON (PyRBD-Suite frontend format).
    #[wasm_bindgen(constructor)]
    pub fn new(topology_json: &str) -> Result<Engine, JsError> {
        Ok(Engine {
            session: Session::new(topology_json, now).map_err(err)?,
        })
    }

    /// Switches topology; computed sets are kept if the graph is unchanged.
    pub fn load(&mut self, topology_json: &str) -> Result<(), JsError> {
        self.session.load(topology_json).map_err(err)
    }

    pub fn topology(&self) -> Result<Ts<TopologyView>, JsError> {
        Ok(self.session.topology().into_ts()?)
    }

    pub fn algorithms() -> Result<Ts<AlgorithmList>, JsError> {
        Ok(session::algorithms().into_ts()?)
    }

    /// One pair, every node at availability `p` for each entry of `probs`;
    /// with `link_prob`, every link can fail too (edge `i` is node `n + i`).
    #[wasm_bindgen(js_name = runPair)]
    pub fn run_pair(
        &mut self,
        plan: Ts<PlanSpec>,
        src: u32,
        dst: u32,
        probs: Vec<f64>,
        link_prob: Option<f64>,
    ) -> Result<Ts<PairReport>, JsError> {
        let report = self
            .session
            .run_pair(&plan.to_rust()?, src, dst, &probs, link_prob)
            .map_err(err)?;
        Ok(report.into_ts()?)
    }

    /// Re-evaluates every earlier pair whose sets `plan` already has, with new `probs`;
    /// `link_prob` picks the link graph's pairs, as in `runPair`.
    pub fn reevaluate(
        &mut self,
        plan: Ts<PlanSpec>,
        probs: Vec<f64>,
        link_prob: Option<f64>,
    ) -> Result<Ts<PairReports>, JsError> {
        let reports = self
            .session
            .reevaluate(&plan.to_rust()?, &probs, link_prob)
            .map_err(err)?;
        Ok(reports.into_ts()?)
    }

    /// `links` reads the sets computed with a `link_prob`.
    pub fn sets(
        &mut self,
        plan: Ts<PlanSpec>,
        src: u32,
        dst: u32,
        kind: Ts<WireSetKind>,
        links: bool,
    ) -> Result<Ts<Sets>, JsError> {
        let sets = self
            .session
            .sets(&plan.to_rust()?, src, dst, kind.to_rust()?, links)
            .map_err(err)?;
        Ok(sets.into_ts()?)
    }
}

fn err(message: String) -> JsError {
    JsError::new(&message)
}
