"""One-off sanity check of the legacy reference answers before importing them.

For each small topology, checks:
  1. Pickle edges match PyRBD-Suite JSON edges (same node numbering).
  2. Reference path sets are exactly the chordless src-dst paths of the graph.
     With failing nodes, a path with a chord contains a shorter path's node set,
     so it is not minimal.
  3. Reference cut sets are minimal hitting sets of the reference path sets.
  4. Reference availability equals the exact availability of the reference
     path sets, with every node at the same p and perfect links.

Standard library only. Usage:
  python3 -I tools/check_reference.py <reference_topologies_dir> <json_topologies_dir>
"""

import ast
import csv
import json
import pickle
import sys
from functools import lru_cache
from pathlib import Path

TOPOLOGIES = [
    "Abilene", "Germany_17", "HiberniaUk", "Spain",
    "dfn-bwin", "polska", "Austria_24", "Sweden",
]
PROBS = ["0.9", "0.99", "0.999", "0.9999"]
# Reference CSVs round to 9 decimals (error up to 5e-10), plus float noise.
AVAIL_TOL = 1e-9


class _Stub:
    """Stand-in for any networkx class: stores pickled data, runs no code."""

    def __init__(self, *args):
        self.args = args

    def __setstate__(self, state):
        if isinstance(state, dict):
            self.__dict__.update(state)
        else:
            self.state = state


class _SafeUnpickler(pickle.Unpickler):
    """Maps networkx classes to _Stub and allows builtins.dict; blocks everything else."""

    def find_class(self, module, name):
        if module.startswith("networkx."):
            return _Stub
        if (module, name) == ("builtins", "dict"):
            return dict
        raise pickle.UnpicklingError(f"blocked global {module}.{name}")


def load_pickle_edges(path):
    # The pickle holds [graph, node positions, node names].
    with open(path, "rb") as f:
        g = _SafeUnpickler(f).load()[0]
    edges = {frozenset((u, v)) for u, nbrs in g._adj.items() for v in nbrs}
    return set(g._node), edges


def load_json_graph(path):
    data = json.loads(path.read_text())
    nodes = {int(n["id"]) for n in data["nodes"]}
    edges = {frozenset((int(e["source"]), int(e["target"]))) for e in data["edges"]}
    probs = {n["prob"] for n in data["nodes"]} | {e["prob"] for e in data["edges"]}
    return nodes, edges, probs


def read_sets(path, column):
    rows = {}
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            rows[(int(row["source"]), int(row["target"]))] = ast.literal_eval(row[column])
    return rows


def chordless_paths(adj, src, dst):
    """Node sets of all src -> dst paths with no edge between non-consecutive nodes."""
    found = set()

    def extend(path, on_path):
        last = path[-1]
        for nxt in adj[last]:
            # nxt may touch only the last node of the path, otherwise a chord appears.
            if nxt in on_path or any(v in on_path for v in adj[nxt] - {last}):
                continue
            if nxt == dst:
                found.add(frozenset(path + [nxt]))
            else:
                on_path.add(nxt)
                extend(path + [nxt], on_path)
                on_path.discard(nxt)

    extend([src], {src})
    return found


def is_simple_path(path, edges, src, dst):
    return (
        path[0] == src
        and path[-1] == dst
        and len(set(path)) == len(path)
        and all(frozenset(pair) in edges for pair in zip(path, path[1:]))
    )


def cuts_are_minimal_hitting_sets(cuts, paths):
    """Each cut meets every path, and dropping any node breaks that."""
    paths = [set(p) for p in paths]
    for cut in cuts:
        cut = set(cut)
        if not all(cut & p for p in paths):
            return False
        for node in cut:
            if all((cut - {node}) & p for p in paths):
                return False
    return True


def availability(paths, p):
    """Exact P(at least one path fully up), every node up with probability p."""
    q = 1.0 - p

    @lru_cache(maxsize=None)
    def rel(family):
        if not family:
            return 0.0
        if frozenset() in family:
            return 1.0
        counts = {}
        for path in family:
            for node in path:
                counts[node] = counts.get(node, 0) + 1
        x = max(counts, key=counts.get)
        up = frozenset(path - {x} for path in family)
        down = frozenset(path for path in family if x not in path)
        return p * rel(up) + q * rel(down)

    return rel(frozenset(frozenset(path) for path in paths))


def check(topo, ref_dir, json_dir):
    """Prints the findings for one topology; returns True if every check passes."""
    tdir = ref_dir / topo
    print(f"== {topo}")

    pk_nodes, pk_edges = load_pickle_edges(tdir / f"Pickle_{topo}.pickle")
    js_nodes, js_edges, js_probs = load_json_graph(json_dir / f"{topo}.json")
    print(f"  nodes: pickle {len(pk_nodes)}, json {len(js_nodes)}; "
          f"edges: pickle {len(pk_edges)}, json {len(js_edges)}")
    print(f"  edges identical: {pk_nodes == js_nodes and pk_edges == js_edges}")
    if pk_edges != js_edges:
        print(f"    only in pickle: {sorted(map(sorted, pk_edges - js_edges))[:10]}")
        print(f"    only in json:   {sorted(map(sorted, js_edges - pk_edges))[:10]}")
    print(f"  json prob values: {sorted(js_probs)}")

    pathsets = read_sets(tdir / f"Pathset_{topo}.csv", "pathsets")
    cutsets = read_sets(tdir / f"Mincutset_{topo}.csv", "mincutsets")
    n = len(js_nodes)
    print(f"  pairs: pathset {len(pathsets)}, cutset {len(cutsets)}, expected {n * (n - 1) // 2}")

    adj = {v: set() for v in js_nodes}
    for e in js_edges:
        u, v = tuple(e)
        adj[u].add(v)
        adj[v].add(u)

    invalid, mismatched = 0, []
    for (s, d), paths in pathsets.items():
        invalid += sum(not is_simple_path(p, js_edges, s, d) for p in paths)
        if {frozenset(p) for p in paths} != chordless_paths(adj, s, d):
            mismatched.append((s, d))
    print(f"  invalid paths: {invalid}")
    print(f"  pairs where paths != chordless paths: {len(mismatched)} {mismatched[:5]}")

    bad_cuts = [pair for pair, cuts in cutsets.items()
                if not cuts_are_minimal_hitting_sets(cuts, pathsets[pair])]
    print(f"  cut families that are not minimal hitting sets of the paths: {len(bad_cuts)}")

    worst = 0.0
    with open(tdir / f"Availability_{topo}.csv", newline="") as f:
        for row in csv.DictReader(f):
            paths = pathsets[(int(row["source"]), int(row["target"]))]
            for p in PROBS:
                diff = abs(availability(paths, float(p)) - float(row[f"{p} Availability"]))
                worst = max(worst, diff)
    print(f"  availability max |diff|: {worst:.3e} ({'OK' if worst <= AVAIL_TOL else 'MISMATCH'})")

    return (pk_nodes == js_nodes and pk_edges == js_edges
            and len(pathsets) == len(cutsets) == n * (n - 1) // 2
            and invalid == 0 and not mismatched and not bad_cuts
            and worst <= AVAIL_TOL)


def main():
    ref_dir, json_dir = Path(sys.argv[1]), Path(sys.argv[2])
    failed = [topo for topo in TOPOLOGIES if not check(topo, ref_dir, json_dir)]
    print(f"FAILED: {failed}" if failed else "ALL OK")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
