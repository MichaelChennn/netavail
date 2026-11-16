# 决策记录

记录已经确认、后续实现要遵守的决定。每条写明内容和原因。要推翻某一条时，不删除原条目：把它标成"已作废"，再新增一条说明原因。

状态：**已定** = 已确认；**待定** = 有倾向但还没最终确认。

---

## D1 用 Rust 重写 PyRBD-Suite（已定，2026-11-16）

用 Rust 重写 PyRBD-Suite 的 C++ 核心和 Python 胶水层，新仓库为 netavail。PyRBD-Suite 暂时冻结。

原因：
- 构建简单：cargo + maturin 取代 CMake + pybind11 + conda gcc 11。
- 跨平台多线程：rayon 不依赖编译器支持。OpenMP 在 Mac 上无法构建，因为 Apple clang 不支持它。
- 同一个 Rust 核心可以直接用于 Tauri 桌面 app 或浏览器 WASM。

不作为理由的：单纯追求性能。Rust 与 C++ 性能持平，算法本身才是耗时大头。

## D2 算法整体思路不变，允许一致性所需的小改动（已定）

只做等价翻译。为了得到一致结果，可以做小改动，例如显式规定排序顺序、把不稳定排序换成稳定排序。算法改进留到重写完成之后再做。

## D3 "结果一致"的定义（已定）

- 路集、割集：每个集合内部排序，集合列表再按（长度、字典序）排序，之后完全相等。
- 可用度：与标准答案的绝对误差 ≤ 1e-12。这个阈值是暂定的，第一轮对拍后可以调整。
- 中间表示不要求逐字一致，包括布尔表达式字符串和 SDP 项的顺序。

原因：现有 C++ 本身就做不到跨平台逐比特一致。一是 `sdp.cpp` 里 `std::sort` 对等价元素的顺序不确定；二是 `-march=native` 下可能生成 FMA 指令，影响浮点末位。

## D4 Cargo workspace 分层（已定）

一个仓库包含多个 crate：

- `netavail-core`：纯算法，不做 IO，也不依赖其它 crate。
- `netavail-io`：拓扑和结果的读写。
- `netavail-py`：Python 绑定。
- `netavail-cli`：命令行工具。
- 以后再加 app 层（Tauri 或 WASM）。

依赖方向只能向下。详见 [architecture.md](architecture.md)。

## D5 保留 Python 入口（已定）

用 PyO3 + maturin 构建，发布 abi3 预编译 wheel（Linux、macOS、Windows），研究者 `pip install netavail` 就能用，不需要编译器。Python API 尽量沿用 `pyrbd_suite` 的函数名。networkx 作为可选依赖，只出现在 Python 侧的薄包装里，Rust 不认识 networkx 对象。

App 本身不依赖 Python。

## D6 不使用 networkx / rustworkx 的替代品（已定）

算法只需要 `Vec<Vec<NodeId>>` 邻接表。networkx 原来只负责 JSON（node_link 格式）读写和重新编号，在 `netavail-io` 里自己写。以后需要通用图算法时用 petgraph。rustworkx 是给 Python 用的库，这里用不上。

## D7 节点状态用 `Lit(u32)` newtype，去掉 +1 偏移（已定）

编码为 `node << 1 | is_down`，占 4 字节，性能与 int 相同。节点 ID 内外统一从 0 开始，不再需要 0 号节点的 +1 偏移。

不用 `enum { Up(u32), Down(u32) }`：它占 8 字节，会让集合的内存翻倍，排序也会变慢。

## D8 `Lit` 的排序必须复刻 C++ 的有符号整数顺序（已定）

排序键为 `is_down ? -(node+1) : (node+1)`：先排不可用的节点，节点号从大到小；再排可用的节点，节点号从小到大。这和 C++ 内部用 1 起始的有符号 int 排序结果相同。SDP 的分解顺序依赖这个顺序，所以必须有单独的测试锁住。

## D9 标准答案来源（已定）

来源是 `~/Workspace/mincutset-availability-estimation/topologies/<Topo>/`，包含 Pathset、Mincutset、Availability、BoolExpr*.csv。

- 小于 1MB 的小拓扑复制到仓库的 `tests/golden/`，每次 `cargo test` 都跑。
- 大拓扑通过环境变量 `NETAVAIL_GOLDEN_DIR` 引用，测试标记为 `#[ignore]`，不进仓库。目录总共 9.2G，其中 `Germany_50/BoolExprSDP` 就有 8G。

PyRBD-Suite 的旧 benchmark CSV 只记录耗时和集合个数，只能用于性能对比。

## D10 第一批移植 `minimalpaths` 和 `mcs`（已定）

mcs 可用度计算直接用标准答案里的割集作为输入，这样它的测试不依赖任何割集算法。之后再移植割集算法、pathset 可用度和 SDP，SDP 放最后。

## D11 不考虑论文复现（已定）

每篇论文在 shakthij98 的仓库里都有对应实现。netavail 的目标是做一个好用、快速的后端和 app。

## D12 项目名 netavail（已定）

crates.io 和 PyPI 上都没有被占用。GitHub 上有一个无关的同名 ping 工具（riordanmr/netavail，0 星），已确认不介意。仓库地址：https://github.com/MichaelChennn/netavail

## D13 许可证 MIT，保留上游版权行（已定）

和三个上游仓库一致。LICENSE 原样保留 PyRBD++/PyRBD3 和 Fast-MCS 的版权行，再加上 netavail 本身的一行。

待办：以后请导师把 Yaxuan Chen 加进 Fast-MCS 的版权行；导师改了之后，同步更新这里的 LICENSE。

## D14 论文引用通过 CITATION.cff、README 和代码注释落实（已定）

MIT 本身不要求引用论文，引用的要求通过以下方式落实：

- `CITATION.cff` 列出三篇论文，GitHub 据此显示 "Cite this repository"。
- README 的 Citing 一节。
- 每个算法模块的文档注释写明出处论文。

待办：
- 确认算法和论文的对应关系。目前已知：`cnf_tree`、`shannon`、`multiplication` 来自 PyRBD3；`combination`、`combination_matrix`，以及 MCS 和 pathset 的可用度计算来自 PyRBD++。Fast-MCS 对应哪个算法还没确认。
- 确认 PyRBD3 论文（FNWF 2025）是否已正式发表，上游 README 里还写着 "submitted"。

---

## 尚未决定（见 architecture.md 第 7 节）

1. App 形态：Tauri 桌面 app / 浏览器 WASM / 服务端（axum）。
2. 首批移植是否包含链路故障（`to_link_graph`）。
3. Python API 完全照搬，还是借机整理。
4. 最低支持的 Python 版本（建议 3.9+）。
