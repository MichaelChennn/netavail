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

> 2026-11-16 更新：crate 列表由 D21 取代。`netavail-py` 推迟，新增 `netavail-wasm`。

## D5 保留 Python 入口（已定）

用 PyO3 + maturin 构建，发布 abi3 预编译 wheel（Linux、macOS、Windows），研究者 `pip install netavail` 就能用，不需要编译器。Python API 尽量沿用 `pyrbd_suite` 的函数名。networkx 作为可选依赖，只出现在 Python 侧的薄包装里，Rust 不认识 networkx 对象。

App 本身不依赖 Python。

## D6 不使用 networkx / rustworkx 的替代品（已定）

算法只需要 `Vec<Vec<NodeId>>` 邻接表。networkx 原来只负责 JSON（node_link 格式）读写和重新编号，在 `netavail-io` 里自己写。以后需要通用图算法时用 petgraph。

> 2026-11-16 更正：现有拓扑 JSON 实际不是 node_link 格式，见 D24。rustworkx 是给 Python 用的库，这里用不上。

## D7 节点状态用 `Lit(u32)` newtype，去掉 +1 偏移（已定）

编码为 `node << 1 | is_down`，占 4 字节，性能与 int 相同。节点 ID 内外统一从 0 开始，不再需要 0 号节点的 +1 偏移。

不用 `enum { Up(u32), Down(u32) }`：它占 8 字节，会让集合的内存翻倍，排序也会变慢。

## D8 `Lit` 的排序必须复刻 C++ 的有符号整数顺序（已定）

排序键为 `is_down ? -(node+1) : (node+1)`：先排不可用的节点，节点号从大到小；再排可用的节点，节点号从小到大。这和 C++ 内部用 1 起始的有符号 int 排序结果相同。SDP 的分解顺序依赖这个顺序，所以必须有单独的测试锁住。

## D9 标准答案来源（已定）

来源是 `~/Workspace/mincutset-availability-estimation/topologies/<Topo>/`，包含 Pathset、Mincutset、Availability、BoolExpr*.csv。

- 小于 1MB 的小拓扑复制到仓库的 `tests/reference/`（2026-11-16 由 `tests/golden/` 改名），每次 `cargo test` 都跑。
- 大拓扑通过环境变量 `NETAVAIL_REFERENCE_DIR` 引用，测试标记为 `#[ignore]`，不进仓库。目录总共 9.2G，其中 `Germany_50/BoolExprSDP` 就有 8G。

PyRBD-Suite 的旧 benchmark CSV 只记录耗时和集合个数，只能用于性能对比。

## D10 第一批移植 `minimalpaths` 和 `mcs`（已定）

（2026-11-16 起改用新名：`minimalpaths` 即 `dfs`，`mcs` 即 `cut_disjoint`，见 D27。）

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

算法与论文的对应关系：

| 算法 | 引用论文 | 状态 |
|---|---|---|
| `cnf_tree`（割集） | Fast-MCS | 已确认（2026-11-16） |
| `shannon`、`multiplication`（割集）、SDP 可用度 | PyRBD3 | 推断：这些算法在旧的 pyrbd3 包里 |
| `combination`、`combination_matrix`（割集）、MCS / pathset 可用度 | PyRBD++ | 推断：这些算法在旧的 pyrbd_plusplus 包里 |
| `minimalpaths`（路集） | 待确认 | |

注意：`cnf_tree` 的旧实现放在 pyrbd3 包里，但应该引用的是 Fast-MCS 论文。所以"代码在哪个包"不能直接当作"引用哪篇论文"，两条推断项在移植对应模块时还要再确认一次。

表中是旧算法名，新旧名对应关系见 D27。

PyRBD3 论文已正式发表于 IEEE FNWF 2025：https://ieeexplore.ieee.org/document/11317672

## D15 App 先做浏览器版（WASM），桌面端以后再说（已定，2026-11-16）

core 编译成 WebAssembly，在浏览器的 Web Worker 里运行。整个 app 是纯静态文件，不需要服务器。

原因：用户打开网页就能用，不用安装任何东西。core 与形态无关，以后做 Tauri 桌面版可以直接复用。

约束：core 里不能用 `std::time::Instant`（在 wasm 下会 panic），也不能用 `std::thread`。计时只在适配层做。CI 每次都编译 `wasm32-unknown-unknown` 目标，一旦误用会立刻报错。

## D16 部署到 GitHub Pages（已定）

由 GitHub Actions 依次执行 `wasm-pack build` 和 `vite build`，再把产物发布到 Pages。

原因：仓库本来就是 public，不用另开服务。Vercel 默认构建环境里没有 Rust，还得多处理一步。

## D17 浏览器并行：多个 Worker 按节点对分工（已定）

不用 wasm 的共享内存多线程。共享内存需要 COOP/COEP 响应头，而 GitHub Pages 不能设置这两个头。各节点对的计算互相独立，按节点对拆给多个 Worker 就足够了。

## D18 链路故障（`to_link_graph`）不进首批（已定）

它只是一个图变换，放在 `transform.rs`，算法本身不知道链路的存在，阶段 4 再补。标准答案假设链路完美，首批用不上它。

## D19 阶段顺序：先 wasm + 网页，后 Python 绑定（已定）

阶段 3 改为：wasm 适配层 + 最小网页 + 部署到 Pages。Python 绑定往后挪。

## D20 命名：`sets/minimalcuts/` 与 `sets/minimalpaths` 对应（已定）

各割集算法放在 `sets/minimalcuts/` 目录下，不叫 `cuts/`。路集算法相应放在 `sets/minimalpaths/` 目录下（第一个是 `dfs.rs`）。

## D21 仓库顶层布局（已定，2026-11-16，更新 D4）

- `crates/netavail-core`：算法。不做 IO、不计时、不开线程，不依赖其它 crate。
- `crates/netavail-io`：拓扑 JSON、结果文件、参考答案的解析与对拍。
- `crates/netavail-wasm`：浏览器适配层（wasm-bindgen）。
- `crates/netavail-cli`：命令行。
- `web/`：前端（不是 Rust 包，所以不放在 `crates/` 里）。
- `tests/reference/`、`tools/`、`docs/`。

`netavail-py` 和 `apps/desktop` 等到要用时再建，不提前创建空目录。

io 从 core 里拆出来，是为了让 core 只处理内存数据：这样它容易测试，而且浏览器、CLI、Python 用的是同一份算法代码。

## D22 core 的基本类型（已定）

- `NodeId = u32`，从 0 开始。
- `NodeSet = Vec<NodeId>`：一条路径或一个割集，不带符号。
- `Term = Vec<Lit>`：可用度计算中的乘积项，带符号。两者分开，带符号的数据就不可能误传给需要路集的函数。
- `SetFamily { kind: Paths | Cuts, src, dst, sets }`：一个节点对的全部路集或割集，自带 src、dst、kind。
- 集合内部保持算法原样输出的顺序（路径顺序就是路由经过的顺序，SDP 可能依赖它）。另外提供 `normalize()`，按 D3 规范化后用于比较。
- `ProbabilityMap` 是一个数组，按 `Lit` 的内部值交错存放 `p, 1-p`，查询就是 `probs[lit.0]`，不需要分支。`1-p` 在构造时算一次，和 C++ 一样，所以浮点结果相同。

## D23 Graph（已定）

`Graph` 只表示无向简单图，内部是邻接表。邻居顺序等于边在文件中出现的顺序，照搬 Python `graph_to_adjlist` 的做法，因为 DFS 的输出顺序取决于它。`Graph::from_edges` 遇到自环、重复边或节点号越界时返回错误，并指出是哪条边。

## D24 拓扑文件格式（已定）

沿用 PyRBD-Suite 前端的 JSON 格式：`{name, nodes: [{id, label, prob, x, y}], edges: [{source, target, prob}]}`。交接文档里说它是 networkx node_link 格式，这一说法有误。已核对 `PyRBD-Suite/data/topologies/` 下全部 18 个文件：id 都是按顺序排列的 `"0"`…`"n-1"`，没有重复边，也没有自环，prob 全部是 1.0。

- 字符串 id 按在 `nodes` 数组里出现的顺序映射成 0…n-1，原始 id 和 label 保留下来，用于显示。
- 加一个可选字段 `"version": 1`，以后改格式时用它区分新旧版本。
- 暂时不支持 node_link 格式。networkx 图由以后的 Python 包装层自己转换。
- 坐标、label、名字、链路概率只存在 io 的 `Topology` 里，core 不知道它们。
- 概率有两个来源：`ProbabilityMap::from_nodes`（使用文件里每个节点的 prob）和 `ProbabilityMap::uniform(n, p)`（所有节点取同一个 p，参考答案就是这样算的）。

## D25 不引入数据库（已定）

网站是纯静态的，没有服务器。保存方式分三层：

1. 导出、导入 JSON 文件。这是唯一可靠的备份方式。
2. 浏览器 IndexedDB：按浏览器保存，清除网站数据后就没了。
3. 分享链接：把压缩后的拓扑写进 URL。

只保存拓扑，不保存计算结果。结果可能很大，而且随时可以重新算出来。只有将来需要账号、跨设备同步时，才考虑托管数据库。

## D26 算法接口：三类 trait + 注册表（已定）

```
Graph ──[PathAlgorithm]──► 路集 ──[CutAlgorithm]──► 割集
                            └──►[AvailabilityAlgorithm]◄──┘
```

- `PathAlgorithm::find(graph, src, dst, ctl) -> SetFamily`
- `CutAlgorithm::from_paths(paths, ctl) -> SetFamily`：割集算法以路集为输入。依据：现有 5 个割集算法开头都先调用 `minimalpaths`。这样同时要路集和割集时，路集只算一次；测试割集算法时，也可以直接用参考答案的路集作输入。将来如果出现直接在图上求割集的算法，再新增一个 trait。
- `AvailabilityAlgorithm::prepare(sets, ctl) -> Box<dyn Prepared>`，再调用 `Prepared::evaluate(probs) -> f64`。prepare 比较慢，和概率无关；evaluate 很快，可以对多组概率重复调用。如果传入的 kind 和 `input()` 不一致，运行时报错。
- `AlgoInfo { id, name, summary, version, aliases, citation }`。`version` 用来让缓存失效（D28），`aliases` 存旧名（D27）。
- 注册表 `registry.rs` 列出全部算法。网页、CLI、一致性测试都从这里读取。加一个新算法 = 新建一个文件 + 在注册表里加一行。
- 首批不设计算法参数（`combination` 的 `order` 一直用默认值），等真的需要时再加。
- 布尔表达式和 debug 输出先不做。

## D27 算法命名（已定）

id 用在代码和命令行里；name 是界面上的短名（≤ 18 字符）；summary 是一句话说明，作为鼠标悬停提示。旧名保留为别名。

| 类别 | 旧名（别名） | id | name |
|---|---|---|---|
| 路集 | `minimalpaths` | `dfs` | DFS |
| 割集 | `cnf_tree` | `decision_tree` | Decision tree |
| 割集 | `shannon` | `shannon` | Shannon expansion |
| 割集 | `multiplication` | `path_product` | Path product |
| 割集 | `combination` | `combination` | Combination |
| 割集 | `combination_matrix` | `combination_bits` | Combination (bits) |
| 可用度 | `mcs` | `cut_disjoint` | Cut disjoint |
| 可用度 | `pathset` | `path_disjoint` | Path disjoint |
| 可用度 | `sdp` | `path_sdp` | SDP |

`path_disjoint` 和 `path_sdp` 的名字是根据已读到的代码起的，移植时读完代码，如果名字不贴切再调整。界面先只做英文，core 里只存英文。

## D28 结果复用：图指纹和分层缓存 key（已定）

图指纹 = BLAKE3(`"netavail-graph-v1"` + n + 边数 + 排序后的边列表，每条边写成 (min, max)，整数都用 u32 小端序)。

- 参与指纹的：节点数、边集合（包括节点编号）。
- 不参与的：边的先后顺序、边的方向、概率、坐标、label、名字。边的顺序只影响集合的输出顺序，不影响集合的内容。
- 不用 Rust 自带的 `DefaultHasher`，因为它不保证跨 Rust 版本稳定，而指纹要存进浏览器和导出文件。

缓存 key 分层，每层都包含上一层的 key：

```
路集 key   = (图指纹, src, dst, 路集算法 id, version)
割集 key   = (路集 key, 割集算法 id, version)
prepare key = (集合 key, 可用度算法 id, version)
evaluate 不缓存
```

导出路集、割集时一并写入图指纹，导入时进行比对。旧的参考答案 CSV 没有指纹，导入时改做合法性检查（路径的每一步都有边，割集确实能切断 src 和 dst）。

`Graph::fingerprint()` 放在 core 的 `graph.rs`。

## D29 流水线（已定）

- `Plan { paths, cuts: Option, availability: Option }`，`Plan::new` 时检查搭配是否合法。
- 默认计算所有满足 i < j 的无序节点对，顺序和参考答案一致（`combinations(nodes, 2)`）。
- 计算的最小单位是一个节点对：`run_pair`（单线程）。`run_all` 在 feature `parallel` 打开时用 rayon，关闭时串行。
- 某一对出错只影响它自己。`PairResult` 默认只包含个数和可用度，集合本身按需再取。
- 动态分配：本地靠 rayon 的 work stealing；浏览器由主线程维护队列，哪个 Worker 空闲就发下一对给它。core 不知道 Worker 的存在。
- `Control` 是一个 trait，由适配层实现，方法都有默认实现：`check()`（取消）、`on_stage()`（计时）、`max_sets()`（上限）。浏览器默认设置上限；取消计算就是直接 terminate Worker。

## D30 缓存的位置（已定）

第一版只缓存在 Worker 内存里。主线程记录"哪一对是哪个 Worker 算的"，改概率时广播给所有 Worker，每个 Worker 只重新 evaluate 自己缓存的那些对。Worker 被 terminate 后缓存随之丢失，需要时重新计算。路集、割集持久化到 IndexedDB 以后再做。`Prepared` 无法序列化，只能留在内存里。

## D31 wasm 适配层（已定）

`netavail-wasm` 导出一个 `Engine` 类：`new(topology)`、`algorithms()`、`run_pair(plan, src, dst, scenarios)`、`reevaluate(scenarios)`、`sets(src, dst, kind)`。Plan 里写算法 id 字符串，由 Rust 在注册表里查找。各阶段的耗时由 `Control` 的实现用 `performance.now()` 记录。换拓扑时，如果指纹没变就保留缓存。

用 serde + tsify 根据 Rust 结构体生成 TS 类型，只维护一份类型定义。Worker 消息协议和 `Engine` 的方法一一对应。

## D32 前端先只做 demo（已定）

core 稳定之前，前端只是一个用来验证的 demo：原生 TS + Vite，单个页面，1 个 Worker，不用框架。功能：选示例或导入 JSON、选算法、设置概率、计算、显示进度、取消、结果表。它的作用是证明三件事：wasm 能编译、结果和参考答案一致、能部署到 Pages。

正式界面等 core 稳定后重新设计，demo 代码届时丢弃。D17 的多 Worker 并行到那时再实现。

## D33 CLI（已定）

用 clap 解析命令行参数，一共三个子命令：

- `algorithms`：列出注册表。
- `run`：计算并输出 CSV（包含每个阶段的耗时）。可以按参考答案的格式导出集合，并附带 `meta.json`（图指纹、算法版本）。
- `verify`：和参考答案对拍。

不做 `bench` 子命令。日常的耗时对比看 `run` 的输出就够了；正式的性能测试用 criterion。

浏览器里测到的时间取决于用户的硬件，只适合同一台机器上比较算法之间的相对快慢。正式的性能数据以本地 CLI 和 criterion 的结果为准。

## D34 测试分四层（已定）

1. 单元测试：`Lit` 排序、ProbabilityMap、吸收、图指纹规则。
2. 参考答案对拍：由注册表驱动，每个阶段都用参考答案作输入。对拍逻辑写在 `netavail-io::reference`，`cargo test` 和 `netavail verify` 共用。测试文件是 `crates/netavail-io/tests/reference.rs`。缺少的参考文件要明确列为跳过，不能悄悄当作通过。
3. 暴力验证：对节点数 ≤ 12 的小图，直接枚举 2ⁿ 种节点状态，算出精确的可用度和最小割集。配合 proptest 随机生成小图。这一层不依赖旧代码，能发现旧代码本身的 bug。
4. wasm 冒烟测试：`wasm-pack test --node`，结果要和本地编译版本一致。

`tests/reference/<Topo>/` 只放 `topology.json` 和 Pathset、Mincutset、Availability 三个 CSV，不放布尔表达式文件。阶段 1 搬数据前，要先核对 pickle 和 JSON 的节点编号是否一致，以及参考可用度是否按"所有节点同一个 p、链路完美"算出。

## D35 CI 和工具链（已定）

- `ci.yml`：在 Linux、macOS、Windows 上跑 `fmt --check`、`clippy -D warnings`、`test`。Linux 上还要跑 `--features parallel`、wasm32 目标检查、wasm 冒烟测试。
- `pages.yml`：push 到 main 后构建 demo，部署到 Pages。
- 用 `rust-toolchain.toml` 固定 Rust 版本。
- 大拓扑的参考答案只在本地，`--ignored` 测试只能在本地手动跑。

---

## 尚未决定

1. Python API 完全照搬，还是借机整理（做 Python 绑定时再定）。
2. 最低支持的 Python 版本（建议 3.9+）。
