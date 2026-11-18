"""Converts a topology JSON (+ optional Mincutset CSV) to the plain text read by bench.cpp.

Output, whitespace separated, nodes 0-based:
  n m
  u v                      (m lines, edges in JSON order)
  P                        (number of cut families, 0 without a CSV)
  src dst k                (P blocks, each followed by k lines)
  len v1 v2 ...

Standard library only. Usage:
  python3 -I prepare.py <topology.json> [Mincutset_<T>.csv] > <out.txt>
"""

import ast
import csv
import json
import sys


def main():
    topo = json.load(open(sys.argv[1]))
    index = {node["id"]: i for i, node in enumerate(topo["nodes"])}
    edges = [(index[e["source"]], index[e["target"]]) for e in topo["edges"]]
    out = [f"{len(index)} {len(edges)}"]
    out += [f"{u} {v}" for u, v in edges]

    families = []
    if len(sys.argv) > 2:
        csv.field_size_limit(sys.maxsize)
        for row in csv.DictReader(open(sys.argv[2])):
            families.append((row["source"], row["target"], ast.literal_eval(row["mincutsets"])))
    out.append(str(len(families)))
    for src, dst, sets in families:
        out.append(f"{src} {dst} {len(sets)}")
        out += [" ".join(map(str, [len(s), *s])) for s in sets]
    print("\n".join(out))


main()
