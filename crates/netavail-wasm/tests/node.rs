//! Smoke test in Node (`wasm-pack test --node`): the wasm build matches the reference answers.
#![cfg(target_arch = "wasm32")]

use netavail_core::types::SetKind;
use netavail_io::reference::read_sets;
use netavail_wasm::Engine;
use netavail_wasm::wire::{PairReport, PlanSpec, SetKind as WireSetKind, Sets};
use tsify::{Ts, Tsify};
use wasm_bindgen_test::wasm_bindgen_test;

const TOPOLOGY: &str = include_str!("../../../tests/reference/Germany_17/topology.json");
const PATHS: &str = include_str!("../../../tests/reference/Germany_17/Pathset_Germany_17.csv");

#[wasm_bindgen_test]
fn every_pair_matches_the_reference_paths() {
    let mut engine = Engine::new(TOPOLOGY).unwrap();
    let plan = || -> Ts<PlanSpec> {
        let spec = PlanSpec {
            paths: "dfs".into(),
            cuts: None,
            availability: None,
        };
        spec.into_ts().unwrap()
    };

    for expected in read_sets(PATHS, SetKind::Paths).unwrap() {
        let (src, dst) = (expected.src, expected.dst);
        let report: PairReport = engine
            .run_pair(plan(), src, dst, vec![])
            .unwrap()
            .to_rust()
            .unwrap();
        let kind = WireSetKind::Paths.into_ts().unwrap();
        let sets: Sets = engine
            .sets(plan(), src, dst, kind)
            .unwrap()
            .to_rust()
            .unwrap();

        assert_eq!(report.error, None);
        assert!(report.times.paths_ms.unwrap() >= 0.0);
        let mut got = sets.sets.unwrap();
        got.iter_mut().for_each(|s| s.sort());
        got.sort();
        let mut want = expected.sets;
        want.iter_mut().for_each(|s| s.sort());
        want.sort();
        assert_eq!(got, want, "pair {src}-{dst}");
    }
}
