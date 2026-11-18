// Times the PyRBD-Suite C++ code on the same work as the criterion benches.
//
// Usage: bench <prepared.txt> <paths|cuts:METHOD|mcs|mcs-pairs> [repeats]
//   paths      sets::minimalpaths on every pair src < dst
//   cuts:M     sets::minimalcuts (paths included) with method M on every pair src < dst
//   mcs        mcs::toProbaSet + probaSetToAvail at p = 0.9 .. 0.9999 on every cut family
//   mcs-pairs  one line per family: src dst cuts terms ms (single run)
// Prints the median time in ms and a checksum to compare with the Rust side.

#include <pyrbd_core/availability/mcs.hpp>
#include <pyrbd_core/sets.hpp>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <fstream>
#include <functional>
#include <iostream>
#include <sstream>
#include <string>

using namespace pyrbd_core;

struct Family {
    NodeID src, dst;
    std::vector<Set> sets;
};

static double ms_since(std::chrono::steady_clock::time_point t0) {
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
}

static double median_ms(int repeats, const std::function<void()>& work) {
    std::vector<double> times;
    for (int r = 0; r < repeats; ++r) {
        auto t0 = std::chrono::steady_clock::now();
        work();
        times.push_back(ms_since(t0));
    }
    std::sort(times.begin(), times.end());
    return times[times.size() / 2];
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: bench <prepared.txt> <paths|mcs|mcs-pairs> [repeats]\n");
        return 2;
    }
    std::ifstream in(argv[1]);
    std::string mode = argv[2];
    int repeats = argc > 3 ? std::stoi(argv[3]) : 10;

    // Same +1 offset as bindings.cpp offsetAdjIn: node 0 is reserved
    int n, m;
    in >> n >> m;
    AdjList adj(n + 1);
    for (int i = 0; i < m; ++i) {
        int u, v;
        in >> u >> v;
        adj[u + 1].push_back(v + 1);
        adj[v + 1].push_back(u + 1);
    }
    int count;
    in >> count;
    std::vector<Family> families(count);
    for (auto& f : families) {
        int k;
        in >> f.src >> f.dst >> k;
        f.src += 1;
        f.dst += 1;
        f.sets.resize(k);
        for (auto& s : f.sets) {
            int len;
            in >> len;
            s.resize(len);
            for (auto& v : s) {
                in >> v;
                v += 1;
            }
        }
    }

    if (mode == "paths") {
        long total = 0;
        double t = median_ms(repeats, [&] {
            total = 0;
            for (int s = 1; s <= n; ++s)
                for (int d = s + 1; d <= n; ++d)
                    total += sets::minimalpaths(adj, s, d).size();
        });
        std::printf("paths %.3f ms, %ld paths\n", t, total);
    } else if (mode.rfind("cuts:", 0) == 0) {
        std::string method = mode.substr(5);
        // combination prints [DEBUG] lines to std::cout; keep them out of the timing output
        std::ostringstream sink;
        auto* old = std::cout.rdbuf(sink.rdbuf());
        long total = 0;
        double t = median_ms(repeats, [&] {
            total = 0;
            for (int s = 1; s <= n; ++s)
                for (int d = s + 1; d <= n; ++d) {
                    total += sets::minimalcuts(adj, s, d, n, method).size();
                    sink.str("");
                }
        });
        std::cout.rdbuf(old);
        std::printf("cuts:%s %.3f ms, %ld cuts\n", method.c_str(), t, total);
    } else if (mode == "mcs" || mode == "mcs-pairs") {
        std::map<int, double> probs;
        std::vector<ProbabilityMap> maps;
        for (double p : {0.9, 0.99, 0.999, 0.9999}) {
            for (int v = 1; v <= n; ++v) probs[v] = p;
            maps.emplace_back(probs);
        }
        if (mode == "mcs-pairs") {
            for (const auto& f : families) {
                auto t0 = std::chrono::steady_clock::now();
                auto terms = mcs::toProbaSet(f.src, f.dst, f.sets);
                std::printf("%d %d %zu %zu %.3f\n", f.src - 1, f.dst - 1, f.sets.size(),
                            terms.size(), ms_since(t0));
                std::fflush(stdout);
            }
            return 0;
        }
        double sum = 0;
        double t = median_ms(repeats, [&] {
            sum = 0;
            for (const auto& f : families) {
                auto terms = mcs::toProbaSet(f.src, f.dst, f.sets);
                for (const auto& map : maps) sum += mcs::probaSetToAvail(f.src, f.dst, map, terms);
            }
        });
        std::printf("mcs %.3f ms, availability sum %.12f\n", t, sum);
    } else {
        std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
        return 2;
    }
    return 0;
}
