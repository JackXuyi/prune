# Prune · 内存监测（M2）实现方案

> 2026-10-23 · 上游：`prune-feature-spec-v2.md` 模块 B / `prune-tauri-technical-plan.md` §1 §5.1
> 本文只解决「B1 footprint 口径 / B2 swap 仪表 / B3 家族聚合 / B4 换出受害者榜」怎么落地。
> **B5 泄漏判定不在本期**（需 30 min+ 序列积累），但本期把序列存储打好底。

---

## 0. 结论先行

| # | 结论 |
|---|---|
| 1 | 数据源全部实测可用：`footprint` 单次 **0.09 s**，12 路并发 **0.27 s**（40 进程预算 < 1.5 s 有 5 倍余量） |
| 2 | **降级判定不能看退出码**：`footprint` 读不到进程时退出码仍为 **0** 且不生成 JSON（实测 pid 1）→ 必须靠「JSON 是否产出 + 字段是否完整」判定 |
| 3 | 采样用双频：**10 s RSS 全量**（廉价筛选器）+ **5 min footprint Top N**（付费深探），符合技术方案 §1.3 |
| 4 | 前端只做**读数与曲线**，不做「释放内存」类动作——越界即变成伪科学产品 |
| 5 | 本期落在现有 `panel` 面板 + `main` 主窗口，不新增窗口形态 |

---

## 1. 本期范围

| 做 | 不做 |
|---|---|
| B1 `phys_footprint` 口径（含 swapped）+ 口径标注 | B5 泄漏嫌疑判定（M3，需序列 + 回归） |
| B2 swap 仪表（已用/上限/swapfile 数/今日新增） | 任何「清理内存 / 加速」动作 |
| B3 按家族聚合（Cursor 67 进程 → 1 行） | 进程终止、kill 操作 |
| B4 换出受害者榜（存活但 97% 在磁盘） | CLI / MCP（M3） |
| 序列落库，为 B5 铺路 | 全盘扫描（M1 磁盘侧） |

---

## 2. 实测基线（2026-10-23 09:45，本机只读核查）

| 项 | 实测值 | 备注 |
|---|---|---|
| `footprint -p <pid> --swapped -j` 单次 | **0.09 s** | 输出 22 个 category，含 `dirty/swapped/clean/wired` |
| 12 路并发总耗时 | **0.27 s** | 12/12 全部产出 JSON |
| 系统进程 pid 1 | ❌ 读不到，**退出码 0**，无 JSON | `footprint: Unable to find any processes matching…` |
| swap | total 14336 MB / used 13725 MB / free 610 MB | `sysctl vm.swapusage` |
| swapfile | **14 个 × 1 GB**（`/System/Volumes/VM/`） | 编号不连续（swapfile12/15-17 已回收）→ 只能数个数，不能看最大编号 |
| 压缩内存 | 436383 pages × 16 KB ≈ **6.66 GB** | `vm_stat` 「Pages occupied by compressor」 |
| 颠簸累计 | Pageins 78.17M / Swapins 43.01M / Swapouts 50.36M | 需取**速率**而非累计值 |
| 压力等级 | `kern.memorystatus_vm_pressure_level = 2` | 语义需与活动监视器校准（D-M3） |
| `memory_pressure` 工具 | 可用，当前 free 39% | 可作交叉校验 |

**页面大小 16384 B（16 KB）** —— 所有 vm_stat 页数换算必须读 `hw.pagesize`，不能写死 4096。

---

## 3. 架构

```
prune-core/                       src-tauri/                    前端
├ mem/mod.rs     快照装配   →    sampler.rs  10s / 5min 线程   →  stores/memStore.ts
├ mem/process.rs sysinfo 枚举           │  emit("mem://snapshot")      (Zustand)
├ mem/footprint.rs 命令调用+解析         │  emit("mem://deep")      →  panels/MenuBarPanel  内存区块
├ swap/mod.rs    sysctl + swapfile     commands.rs  invoke           →  windows/MainWindow  双栏
└ db/mod.rs      rusqlite 序列          mem_snapshot / mem_processes
                                        mem_history / mem_refresh
```

数据流：Rust 后台常采（无 WebView 也运行）→ 内存中保存最新快照 → 前端打开时 `invoke` 直接取（零等待）→ 之后靠 `emit` 增量刷新。

---

## 4. 数据模型

```rust
// prune-core
pub struct MemSnapshot {
    pub physical_total: u64,          // hw.memsize
    pub compressor_bytes: u64,        // vm_stat × pagesize
    pub pages_free_bytes: u64,
    pub swap: SwapInfo,
    pub pressure: Pressure,           // Normal / Warning / Critical + 原始 level
    pub thrash: Thrash,               // swapin/swapout 每秒速率
    pub families: Vec<FamilyMem>,     // B3 聚合
    pub top_swapped: Vec<ProcMem>,    // B4 榜首（按 swapped 降序，取前 5）
    pub deep_updated_at: Option<i64>, // 上次 footprint 轮次时间戳
}

pub struct SwapInfo {
    pub total: u64, pub used: u64, pub free: u64,
    pub file_count: usize,
    pub new_today: usize,             // 按 mtime 当日统计（D-M6）
}

pub struct ProcMem {
    pub pid: i32, pub ppid: i32, pub name: String, pub exe: Option<String>,
    pub family: String,
    pub rss: u64,
    pub footprint: Option<u64>,       // None = 未深探或失败
    pub swapped: Option<u64>,         // footprint 的 categories[].swapped 求和
    pub source: MemSource,            // Footprint | Rss(fallback)
}
```

前端 `src/types/mem.ts` 镜像同一结构（serde camelCase）。

---

## 5. 采样与并发

| 频率 | 数据源 | 范围 | 实测/预算成本 |
|---|---|---|---|
| 10 s | `sysinfo` refresh | 全量进程（RSS/ppid/exe/name）+ vm_stat + swapusage + swapfile 列表 | < 30 ms，CPU < 1% |
| 5 min | `footprint -p --swapped -j` | Top N（RSS > 80 MB 或非系统进程，上限 40） | 12 路 0.27 s → 40 进程 8 路并发 ≈ 1.3 s |
| 用户点开某进程 | 单次 footprint | 1 个 | 0.09 s |

实现要点：

1. **Tokio 单任务双定时器**，不 spawn 裸线程；每次采样前后记录耗时，超预算打日志。
2. **Semaphore(8)** 控制 footprint 并发，单次调用超时 **3 s** 即放弃并降级。
3. **pid 复用防护**：命中的进程记录 `start_time`，每次深探后校验，不一致则丢弃该点（避免序列串台）。
4. 深探结果写 SQLite（5 min 粒度、仅 Top N），供 M3 泄漏判定直接消费。
5. 首次启动无历史时先跑一轮 10 s 采样，deep 轮在后台补，前端先用 RSS 渲染 + 骨架占位。

---

## 6. IPC 契约

| 类型 | 名称 | 说明 |
|---|---|---|
| command | `mem_snapshot()` → `MemSnapshot` | 取内存中最新快照（零 IO） |
| command | `mem_processes()` → `Vec<ProcMem>` | 全量进程（家族聚合前的明细） |
| command | `mem_history(pid, minutes)` → `Vec<Point>` | 序列（读 SQLite） |
| command | `mem_refresh()` → `()` | 手动触发一轮 10 s + 深探 |
| event | `mem://snapshot` | 10 s 全量快照 |
| event | `mem://deep` | 5 min 深探完成（footprint/swapped 更新） |

事件用 `emit` 全局广播即可（窗口数 ≤ 2），无需 Channel 精细订阅。

---

## 7. 降级链（**本期最关键的正确性要求**）

```
footprint 调用
├─ 命令不存在 / 沙箱拦截        → 全局降级：全部 source=Rss，顶部提示「当前为 RSS 口径，不含换出量」
├─ 退出码 0 但 JSON 缺失        → 单进程降级 Rss（实测 pid 1 走这条，绝不能当成功）
├─ JSON 解析失败 / 无 processes[] → 同上 + 打日志（只依赖 footprint / categories[].swapped 两个字段）
├─ 超过 3 s 超时                → 同上
└─ 成功                        → footprint = p.footprint, swapped = Σ categories[*].swapped
```

UI 必须显式标注口径来源：`RSS 口径（不含换出量）` 徽标，绝不静默混用两种读数。

---

## 8. UI 设计

### 8.1 面板（380×560，首屏「一、内存」区块）

```
┌─────────────────────────────┐
│ 压力 ● 警告     压缩 6.7 GB  │  ← 压力点：绿/黄/红
│ swap 13.4 / 14.0 GB (96%)   │  ← 仪表条
│ ↳ 正在吃掉磁盘 14 GB · +0 今日│
│ 换出受害者                    │
│  next-server  3.35 GB (97%) │  ← 点击进主窗口检视器
│  tsserver     2.42 GB (99%) │
│  followup     1.77 GB (97%) │
└─────────────────────────────┘
```

- 主数字仍是「可回收 GB」（磁盘侧），内存只做第二区块，不抢首屏主位（功能方案 §3.1）
- 术语硬约束：禁止出现「释放/加速/优化内存」

### 8.2 主窗口（960×640 双栏）

- 左：家族列表（Cursor 4.26 GB / 67 进程 / Chrome 2.15 GB / 31 进程…），点击展开到进程
- 右：检视器 —— footprint vs RSS 对照条形、swapped 占比、sparkline（自绘 SVG，不引图表库）
- 顶部：口径徽标 + 上次深探时间 + 手动刷新

---

## 9. 性能与自耗预算

| 项 | 预算 | 校验方式 |
|---|---|---|
| 后台常驻内存（无 WebView） | < 60 MB | `footprint -p <self>` |
| 10 s 轮次 CPU | < 1% | 采样耗时日志 + 活动监视器 |
| footprint 轮次 | < 1.5 s / 40 进程 | 实测 12 路 0.27 s，留 5 倍余量 |
| SQLite 体积 | < 50 MB / 7 天 | 只存 Top 40 × 5 min × 7 天 |
| 前端重渲染 | 10 s 一次全量快照，列表用 memo 分片 | — |

---

## 10. 任务分解

| ID | 任务 | 产出 | 依赖 |
|---|---|---|---|
| T-201 | `prune-core/mem` 数据模型 + sysinfo 枚举 + 家族归类 | 单测：枚举耗时、家族聚合正确性 | — |
| T-202 | `mem/footprint.rs`：调用、并发、超时、JSON 解析、降级 | 单测：pid 1 降级、假 JSON 解析 | T-201 |
| T-203 | `swap/`：vm.swapusage + swapfile 列表 + 今日新增 | 单测：解析 sysctl 输出 | — |
| T-204 | 系统总览：vm_stat 页数换算、颠簸速率、压力等级 | 单测：pagesize 非 4096 场景 | — |
| T-205 | `db/`：rusqlite 建表 + 序列写入/查询（5 min × Top N） | 单测：写入去重、查询窗口 | T-201 |
| T-206 | `sampler.rs`：双频调度 + 事件推送 + 复杂度收敛 | 手动跑 5 分钟观察日志 | T-202 T-203 T-204 |
| T-207 | `commands.rs`：4 个 IPC 命令 + 前端 `types/mem.ts` 契约 | 前后端类型对齐 | T-206 |
| T-208 | 前端 `memStore` + 面板内存区块 | 面板可见 swap 仪表与受害者榜 | T-207 |
| T-209 | 主窗口双栏 + 检视器 + SVG sparkline | 家族展开到进程 | T-208 |
| T-210 | 口径验收：与活动监视器抽检 20 进程，偏差 < 10% | 验收记录进 `docs/m2-memory-acceptance.md` | T-209 |

粗排：T-201~T-205 可并行（2 天）→ T-206~T-207（1 天）→ T-208~T-209（2 天）→ T-210（0.5 天）。

---

## 11. 待确认决策点

| ID | 决策 | 我的建议 |
|---|---|---|
| **D-M1** | 采样间隔是否可配 | 先写死 10 s / 5 min，设置页留到 M3；过频是耗电元凶 |
| **D-M2** | 家族归类规则 | 内置规则表（Cursor/Chrome/Electron/Node/微信…）+ 兜底按 exe 目录名聚合，规则表可后续扩展 |
| **D-M3** | 压力等级映射 | 用 `kern.memorystatus_vm_pressure_level`（本机实测=2），但**必须先与活动监视器校准**再定 Normal/Warning/Critical 阈值 |
| **D-M4** | 历史保留期 | 5 min 粒度 + 只存 Top 40 + 保留 7 天（约 8 万行，< 20 MB），够 M3 回归用 |
| **D-M5** | WebView 即用即毁是否本期做 | 本期不做（先保证读数正确），M2 收尾再按实测自耗决定 |
| **D-M6** | 「今日新增 swapfile」判定 | 以文件 mtime 落在本地当日为准 + 记录上一次数量做差值兜底（现存 swapfile 编号不连续，不能按最大编号推算） |

---

## 12. 风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| `footprint` 输出格式随系统版本变化 | 🟠 | 只依赖 `footprint` 与 `categories[].swapped`；解析失败即降级 RSS |
| 退出码 0 但实际失败（已实测） | 🔴 | 以 JSON 存在与字段完整性为唯一成功判据 |
| pid 复用导致序列串台 | 🟠 | 记录 start_time 校验 |
| 压力等级语义与活动监视器不一致 | 🟠 | T-210 校准，偏差大则改用页率推导 |
| 16 GB 机器上自耗 | 🟡 | 预算 60 MB；不引图表库；WebView 按需销毁 |
| 采样耗电 | 🟡 | 面板隐藏时降到 30 s；电池供电降频（M3） |

---

## 13. 验收标准

1. next-server 这类进程显示 **footprint 3.35 GB**，而不是 RSS 113 MB，并标注「其中 97% 已换出」
2. swap 面板读数与 `sysctl vm.swapusage` **完全一致**；swapfile 数量与 `/System/Volumes/VM/` 实际文件数一致
3. 抽检 20 个进程，footprint 与活动监视器「内存」列偏差 **< 10%**（写入验收记录）
4. 连续运行 30 分钟，10 s 轮次无堆积、自身 CPU 均值 < 1%
5. 拔掉深探能力（临时改坏路径）时，UI 明确显示「RSS 口径」而不是空白或 0

---

## 14. 与上层文档的关系

| 上游 | 本方案处理 |
|---|---|
| 功能方案 §4 模块 B（B1/B2/B3/B4） | §1 范围 + §8 UI |
| 功能方案 §6 B5 泄漏判定 | **不做**，本期只落序列存储（T-205） |
| 技术方案 §1.3 采样架构 | §5 双频采样，实测校准了并发预算 |
| 技术方案 §5.1 footprint 调用 | §7 降级链（补充了「退出码不可信」这条实测发现） |
| 技术方案 §7 WebView 内存 | D-M5 决定本期不做，先保读数正确 |
