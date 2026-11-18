# C++ baseline

Times the PyRBD-Suite C++ code (`pyrbd_core`) on the same work as
`crates/netavail-core/benches/reference.rs`, so the Rust port can be compared with it.
PyRBD-Suite is only read; everything is built into `target/cpp_baseline/`.

```sh
tools/cpp_baseline/run.sh tests/reference/Germany_17/topology.json \
    tests/reference/Germany_17/Mincutset_Germany_17.csv
cargo bench -p netavail-core --bench reference
```

- `paths`: `sets::minimalpaths` / `Dfs` on every pair src < dst.
- `cuts:<method>` / `cuts/<id>`: paths + minimal cuts on every pair src < dst
  (`bench <prepared.txt> cuts:cnf_tree`; the C++ `minimalcuts` finds the paths itself).
- `mcs` / `cut_disjoint`: `toProbaSet` + `probaSetToAvail` at p = 0.9, 0.99, 0.999, 0.9999
  on every reference cut family (src < dst).

The compiler defaults to the conda env `pyrbd_suite` (gcc 11.4), flags as in its CMake
build: `-O3 -march=native -fopenmp`. Both sides run single-threaded.

## Results (2026-11-18)

AMD Ryzen 7 5700X, WSL2, rustc 1.100.0. C++: median of 10 runs. Rust: criterion mean.
Path counts are equal on both sides and availability sums agree to 1e-11.

| topology   | paths C++ | paths Rust | cut_disjoint C++ | cut_disjoint Rust |
|------------|----------:|-----------:|-----------------:|------------------:|
| Abilene    |  0.361 ms |   0.028 ms |         0.462 ms |          0.199 ms |
| Austria_24 | 29.305 ms |   1.123 ms |         3.218 ms |          1.804 ms |
| Germany_17 |  4.360 ms |   0.290 ms |         9.067 ms |          3.928 ms |
| HiberniaUk |  0.280 ms |   0.032 ms |         2.684 ms |          1.457 ms |
| Spain      |  3.148 ms |   0.137 ms |         0.550 ms |          0.307 ms |
| Sweden     | 21.277 ms |   1.324 ms |       109.166 ms |         47.168 ms |
| dfn-bwin   |  0.562 ms |   0.020 ms |         0.004 ms |          0.001 ms |
| polska     |  1.008 ms |   0.057 ms |         1.454 ms |          0.724 ms |

The C++ `minimalpaths` rebuilds an `unordered_set` of forbidden nodes at every step,
which explains most of the path gap.

## Cut algorithms (2026-11-18)

Total time over **all pairs src < dst**, paths included, in **ms**. C++: one run (minutes
for combination); Rust: criterion mean. Archive: the old Python package from
`PyRBD-Suite/archive/benchmark/mincutsets/results/`, same totals. Cut counts are equal
on all sides.

| topology   | decision_tree C++ / Rust / archive | path_product C++ / Rust / archive | combination C++ / Rust | combination_bits C++ / Rust / archive |
|------------|---------------------:|--------------------:|----------------------:|---------------------------:|
| Abilene    |   0.63 / 0.17 / 7.4  |  0.51 / 0.12 / 4.7  |       3.98 / 0.12     |        5.86 / 0.22 / 41.9  |
| Austria_24 |  33.2 / 3.03 / 177.6 | 31.8 / 2.31 / 152.9 |    224 073 / 16.9     | 281 929 / 24.5 / 2 042 007 |
| Germany_17 |   6.45 / 1.73 / 57.9 |  5.37 / 0.96 / 37.4 |        758 / 6.0      |      1 114 / 9.8 / 6 168   |
| HiberniaUk |   0.63 / 0.29 / 11.5 |  0.62 / 0.28 / 6.5  |       22.5 / 0.20     |       91.5 / 0.22 / 194    |
| Spain      |   3.72 / 0.50 / 24.6 |  3.51 / 0.36 / 20.4 |        671 / 0.85     |        802 / 1.46 / 6 427  |
| Sweden     |  39.3 / 13.8 / 341.7 | 38.4 / 12.8 / 311.6 |    429 051 / 164      | > 900 000 / 401 / 3 971 308 |
| dfn-bwin   |   0.59 / 0.020 / –   |  0.63 / 0.021 / –   |       0.63 / 0.020    |       0.61 / 0.020 / –     |
| polska     |   1.61 / 0.48 / 14.3 |  1.57 / 0.40 / 12.0 |        9.9 / 1.8      |       11.4 / 2.2 / 98      |

`shannon` is within a few percent of `path_product` on both sides.

The combination gap is the number of combinations tried, not the test per combination.
The C++ tries every combination of size 2..ceil(n/2) of all nodes but src and dst
(Austria_24: ~855 million over all pairs). Rust only takes nodes on some path and stops
once no combination of the current size can grow into a minimal cut (Austria_24: 190 000,
mostly stopping at size 4 or 5; Sweden: 1.95 million against ~2 billion).

## Mid-size topologies

Cut disjoint grows fast with the number of cuts. On Norway (27 nodes, 51 edges) the
C++ `toProbaSet` needs 29 s for pair 0-8 alone (283 cuts → 16 211 terms) and did not
finish the first 10 pairs in 2 minutes; Rust needs 9.0 s for the same pair. These
topologies are left out of the benches and of routine `netavail verify` runs.
