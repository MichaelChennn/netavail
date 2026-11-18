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

## Mid-size topologies

Cut disjoint grows fast with the number of cuts. On Norway (27 nodes, 51 edges) the
C++ `toProbaSet` needs 29 s for pair 0-8 alone (283 cuts → 16 211 terms) and did not
finish the first 10 pairs in 2 minutes; Rust needs 9.0 s for the same pair. These
topologies are left out of the benches and of routine `netavail verify` runs.
