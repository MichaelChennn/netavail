//! Same work as `tools/cpp_baseline/bench.cpp`, on the reference topologies:
//! - `paths`: dfs on every pair src < dst
//! - `cut_disjoint`: prepare + evaluate at p = 0.9 .. 0.9999 on every reference cut family
//!
//! Checksums are printed once so the totals can be compared with the C++ output.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use netavail_core::availability::{AvailabilityAlgorithm, CutDisjoint};
use netavail_core::sets::minimalpaths::{Dfs, PathAlgorithm};
use netavail_core::types::{SetFamily, SetKind};
use netavail_core::{Graph, NoControl, ProbabilityMap};
use netavail_io::Topology;
use netavail_io::reference::read_sets;

const TOPOLOGIES: [&str; 8] = [
    "Abilene",
    "Austria_24",
    "Germany_17",
    "HiberniaUk",
    "Spain",
    "Sweden",
    "dfn-bwin",
    "polska",
];
const PROBS: [f64; 4] = [0.9, 0.99, 0.999, 0.9999];

fn load(name: &str) -> (Topology, Vec<SetFamily>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference")
        .join(name);
    let read = |file: String| std::fs::read_to_string(dir.join(file)).unwrap();
    let topo = Topology::from_json(&read("topology.json".into())).unwrap();
    let cuts = read_sets(&read(format!("Mincutset_{name}.csv")), SetKind::Cuts).unwrap();
    (topo, cuts)
}

fn all_paths(g: &Graph) -> usize {
    let n = g.node_count();
    let mut total = 0;
    for src in 0..n {
        for dst in src + 1..n {
            total += Dfs.find(g, src, dst, &NoControl).unwrap().sets.len();
        }
    }
    total
}

fn all_availability(families: &[SetFamily], maps: &[ProbabilityMap]) -> f64 {
    let mut sum = 0.0;
    for f in families {
        let prepared = CutDisjoint.prepare(f, &NoControl).unwrap();
        sum += maps.iter().map(|p| prepared.evaluate(p)).sum::<f64>();
    }
    sum
}

fn bench(c: &mut Criterion) {
    for name in TOPOLOGIES {
        let (topo, cuts) = load(name);
        let g = topo.graph();
        let maps: Vec<_> = PROBS
            .iter()
            .map(|&p| ProbabilityMap::uniform(g.node_count() as usize, p))
            .collect();
        eprintln!(
            "{name}: {} paths, availability sum {:.12}",
            all_paths(g),
            all_availability(&cuts, &maps)
        );
        c.bench_function(&format!("paths/{name}"), |b| {
            b.iter(|| all_paths(black_box(g)))
        });
        c.bench_function(&format!("cut_disjoint/{name}"), |b| {
            b.iter(|| all_availability(black_box(&cuts), &maps))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
