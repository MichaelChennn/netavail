#!/usr/bin/env bash
# Builds the PyRBD-Suite C++ baseline into target/cpp_baseline/ and runs it on one topology.
#
# Usage: tools/cpp_baseline/run.sh <topology.json> [Mincutset_<T>.csv] [repeats]
# Env:   PYRBD_SUITE (default ~/Workspace/PyRBD-Suite), CXX (default: conda env pyrbd_suite g++)
# PyRBD-Suite is only read, never modified.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
suite="${PYRBD_SUITE:-$HOME/Workspace/PyRBD-Suite}/packages/pyrbd_core/src"
cxx="${CXX:-$HOME/miniconda3/envs/pyrbd_suite/bin/x86_64-conda-linux-gnu-g++}"
out="$root/target/cpp_baseline"
mkdir -p "$out"

# Same flags as the PyRBD-Suite CMake build; mcs.cpp includes omp.h
if [[ ! -x "$out/bench" || "$here/bench.cpp" -nt "$out/bench" ]]; then
  "$cxx" -std=c++20 -O3 -march=native -fopenmp -I "$suite/include" \
    "$here/bench.cpp" "$suite/common.cpp" "$suite/sets/minimalpaths.cpp" \
    "$suite/availability/mcs.cpp" -o "$out/bench"
fi

topo="$1"
name="$(basename "$topo" .json)"
python3 -I "$here/prepare.py" "$topo" ${2:+"$2"} > "$out/$name.txt"
"$out/bench" "$out/$name.txt" paths "${3:-10}"
if [[ -n "${2:-}" ]]; then
  "$out/bench" "$out/$name.txt" mcs "${3:-10}"
fi
