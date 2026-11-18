# netavail

Network availability analysis in Rust: minimal path sets, minimal cut sets, and
source-to-destination availability for network topologies.

Status: early. Minimal path sets (`dfs`) and cut-set availability (`cut_disjoint`) are
ported and checked against reference answers; cut set algorithms come next. See
[docs/architecture.md](docs/architecture.md) and [docs/decisions.md](docs/decisions.md).

**Demo:** <https://michaelchennn.github.io/netavail/> runs the Rust code as WebAssembly
in your browser.

## Command line

```sh
cargo run --release -p netavail-cli -- algorithms
cargo run --release -p netavail-cli -- run tests/reference/Germany_17/topology.json --out results.csv
cargo run --release -p netavail-cli -- verify tests/reference/Germany_17/topology.json tests/reference/Germany_17
```

`run` writes one row per node pair with the time of each stage; `--sets-dir DIR` also
exports the sets. Speed compared with the original C++: [tools/cpp_baseline](tools/cpp_baseline/README.md).

## Origin

netavail is a Rust rewrite of the algorithms from these open-source tools:

- [PyRBD++](https://github.com/shakthij98/PyRBD_plusplus)
- [PyRBD3](https://github.com/shakthij98/PyRBD3)
- [Fast-MCS](https://github.com/shakthij98/Fast-MCS)

## Citing

If you use netavail in research, please cite the paper behind each algorithm you use.
GitHub's "Cite this repository" button gives the entries from [CITATION.cff](CITATION.cff).

- S. Janardhanan, Y. Chen, C. Mas-Machuca, "PyRBD++: An Open-Source Fast Reliability
  Block Diagram Evaluation Tool", RNDM 2025.
- S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, "Leveraging Minimal Path Sets
  for Reliability Block Diagram Evaluation", IEEE FNWF 2025.
  [IEEE Xplore](https://ieeexplore.ieee.org/document/11317672)
- S. Janardhanan, Y. Chen, W. Kellerer, C. Mas-Machuca, "Fast-MCS: A Scalable
  Open-Source Tool to Find Minimal Cut Sets", arXiv:2602.16686, 2026.

A per-algorithm mapping will be added as each algorithm is ported.

## License

MIT. See [LICENSE](LICENSE).
