# Prune · CPU 归因（M5）详细方案

> 状态：C0 已实施（2026-11-03）；C1/C2/C3 待做 · 前置：M1 磁盘 / M2 内存 / M3 泄漏 / M4 域分离 / W1–W3 跨平台已完成
> 一句话：**CPU 不加新域、不加新 tab，做成「应用的第二个维度」**——同一行同时看到「内存 3.2 GB · CPU 12%」，回答现有视图答不出的两个问题。

## 0. 结论先行

1. **数据几乎零成本**：`Scanner::scan()` 每 10 s 已经调用 `refresh_processes`，而 sysinfo 0.39.6 的该方法**默认带 `with_cpu()`**（`sysinfo/src/common/system.rs:309-324`）——CPU 早就取回来了，只是被 `ProcMem` 丢掉了。整机 CPU 只需一次 `refresh_cpu_specifics`，本机实测 **0.08–0.15 ms**。
2. **不做成新资源域**：M4 确立的域分离依据是「用户动作不同」（内存=减压、存储=腾空间）。CPU 的可用动作和内存完全一致（看进程、结束进程），且 CPU% 是**应用这个实体的属性**，不是并列的域。因此不加 CPU tab，把 CPU 补进现有应用列表 + 首屏一个整机读数。
3. **三个必须处理的正确性坑**（不处理就是撒谎，比不显示更糟）：
   - **0 不等于 0%**：sysinfo 的进程 CPU 是差分值，新进程/首轮没有基线时保持 0（`sysinfo/src/unix/apple/macos/process.rs:286-305`）。必须用「已预热 + 首见」标记把它变成 `null`，UI 显示 `—`。
   - **单核口径会 >100%**：一个 4 线程进程可以是 320%（= 3.2 核）。UI 必须能显示 >100%，并给「约 3.2 核」的换算。
   - **进程合计 ≠ 整机**：本机实测进程合计 70.9%（单核口径）而整机 15.6%，因为整机含内核/驱动时间。UI 绝不能把两个数并排放着让用户做减法。
4. **四阶段 C0→C3，约 1.5 人天**，每阶段独立可验收（数据层 → 面板 → 主窗口 → 安全与文档）。
5. **明确不做**：温度 / 风扇 / 功耗 / 频率 / 每核热力图 / CPU tab / CPU 历史入库 / 「一键降温」（理由逐条写在 §11）。

## 1. 需求理解与边界

### 1.1 要回答的两个问题

| 用户当下的问题 | 现在能不能答 | CPU 补上之后 |
|---|---|---|
| 「Cursor 内存一直在涨，它是在正常工作还是在漏？」 | 只能看到内存曲线，看不到它在不在干活 | 同一行看到 `3.2 GB · CPU 12%`：有 CPU = 在跑任务；CPU 近 0 而内存在涨 = 更可疑 |
| 「风扇在响 / 机器发烫 / 电池掉得快，是谁干的？」 | 完全没有入口 | 按 CPU 排序的应用榜 + 进程明细 |

### 1.2 边界（写死，评审时不要再扩）

| ✅ 做 | ❌ 不做 |
|---|---|
| 进程级 CPU%（单核口径，与活动监视器一致） | 每核热力图、P/E 核区分 |
| 家族（应用）级 CPU 合计 + 按 CPU 排序 | 温度、风扇转速、功耗、实时频率 |
| 整机 CPU 使用率 + load average + 逻辑核数 | CPU 历史入库（5 min 粒度对 CPU 无意义） |
| 进程累计 CPU 时间（详情页，回答「它一共烧了多少分钟」） | 独立「CPU」tab / 独立资源域 |
| 系统关键进程在 CPU 榜上的禁杀保护 | 任何「优化 / 加速 / 降温 / 省电」按钮或文案 |

## 2. 现状诊断（代码级，可复查）

### 2.1 CPU 数据已经在手里，只是被丢掉了

| 事实 | 证据 |
|---|---|
| 采样器每 10 s 只取内存字段 | `crates/prune-core/src/mem/process.rs:107-147`，`ProcMem` 里 `rss: p.memory()`，没有 `p.cpu_usage()` |
| 但 `refresh_processes` 默认就采 CPU | `sysinfo/src/common/system.rs:309-324`：`ProcessRefreshKind::nothing().with_memory().with_cpu().with_disk_usage()...` |
| macOS 上一次 `proc_pidinfo(PROC_PIDTASKINFO)` 同时返回内存与 CPU 累计时间 | `sysinfo/src/unix/apple/macos/process.rs`（`pti_resident_size` 与 `pti_total_user/system` 同一个结构体）→ **拿 CPU 不增加 syscall** |
| 实测：只调 `refresh_processes`（不调 `refresh_cpu_*`）就有 158/358 个进程 CPU 非零 | 临时探针（已删）：`processes-only refresh: 21.3 ms · nonzero=158` |

结论：C0 的主要工作不是「采集」，而是**把已有字段搬运、聚合、标注语义**。

### 2.2 整机 CPU 从未被刷新

- 全仓 grep 无 `refresh_cpu_*` / `global_cpu_usage`。
- sysinfo 文档明确：`global_cpu_usage()` 必须先 `refresh_cpu_specifics` / `refresh_cpu_usage`（`sysinfo/src/common/system.rs:487-501`），否则是初始化时的静态值。
- 实测：`System::new_all()` 立刻读到的 26.68% 是**初始化采样值，不是 10 s 窗口均值**（同一台机器 3 s 后连续两次 refresh 分别是 15.6% / 15.0%）→ 首轮同样必须标 `null`。
- 成本可忽略：`refresh_cpu_specifics` 三次实测 **0.153 / 0.082 / 0.105 ms**。

### 2.3 前端零 CPU 字段，排序写死在 Rust

| 位置 | 现状 |
|---|---|
| `src/types/mem.ts` | `ProcMem` / `FamilyMem` / `MemSnapshot` 均无 CPU 字段 |
| `src/components/AppMemoryList.tsx:166` | 主窗口行 grid `[26px_1fr_112px_86px]`，只有相对占用条与总量 |
| `src/windows/MainWindow.tsx`（apps tab 表头） | 4 列：`# / 应用 / 相对占用 / 总量` |
| `crates/prune-core/src/mem/mod.rs:367` | `aggregate()` 结尾 `out.sort_by_key(Reverse(total))`——**排序只有内存一个维度**，前端拿到的是已排好序的数组 |

### 2.4 本机实测基线（2026-11-02 18:40，Apple M2 / 16 GB）

| 项 | 实测值 |
|---|---|
| 逻辑核 / 物理核 | 8 / 8（`hw.perflevel0.physicalcpu=4`、`perflevel1=4`，即 4P+4E） |
| 整机 CPU | 15.6% / 15.0%（连续两轮，3 s 间隔） |
| 单核百分比逐核 | `[31.4, 28.6, 25.5, 21.2, 7.6, 2.6, 2.6, 0.3]`（和 = 125.1 → /8 = 15.6%，与整机一致） |
| 进程 CPU 合计 | 70.9%（单核口径），即 8.9% 折算整机——**明显小于整机**（内核/驱动时间不在进程里） |
| load average | 3.99 / 4.18 / 5.15 |
| 进程数 | 358 |
| 现有 `scan()` 耗时 | 19.8 ms（首轮）/ 33.3 ms（次轮） |

两条从基线里直接读出的结论，写进口径定义：
- 整机 = 每核平均；逐核高低的形态与 4P+4E 一致，但 sysinfo **不暴露 P/E 归属**，所以不做「性能核/能效核」标注。
- load 4.0 / 8 核 = 50% 饱和度，但 CPU 只有 15%——macOS 的 load 含不可中断 I/O。**load 只能当「排队长度」事实值展示，不能换算成 CPU 使用率。**

## 3. 口径定义（本方案的正确性核心）

| 字段 | 含义 | 量纲 / 范围 | macOS | Windows | `null` 语义 |
|---|---|---|---|---|---|
| `proc.cpu` | 进程 CPU，近 10 s 均值 | 单核百分比，0–100×核 | `proc_pidinfo` 累计时间差分 ÷ 窗口 | `GetProcessTimes` 差分（sysinfo 内部） | 首轮 / 新出现进程 / 采集失败 → UI `—` |
| `proc.cpuTimeMs` | 进程累计 CPU 时间 | ms（可远大于墙钟） | `pti_total_user+system` | `GetProcessTimes` 累加 | 0 表示确实为 0 |
| `family.cpu` | 家族成员 CPU 之和 | 单核百分比 | 派生 | 派生 | 全部成员未知 → `null`；部分未知 → **下界**（`≥`） |
| `cpu.usage` | 整机 CPU | 0–100 | 每核平均（`host_processor_info`） | PDH `\Processor(_Total)\% Processor Time` | 采集失败 / 首轮 → 隐藏整块 |
| `cpu.load` | 1/5/15 min 排队长度 | 浮点 | `getloadavg` | 不支持 → `null` | 隐藏 |
| `cpu.cores` | 逻辑核数 | 整数 | `cpus().len()` | 同 | — |

### 3.1 「0」与「未知」必须分开（P0 级正确性）

sysinfo 的进程 CPU 是差分：`total_existing_time > 0` 才赋值，否则保持 0（`sysinfo/src/unix/apple/macos/process.rs:286-305`）。因此**新启动的进程在第一次被看到时一定是 0**，而它可能正在满核编译。

方案：

```rust
pub struct Scanner {
    sys: System,
    /// (pid, start_time) → 已见过；用于区分「首次出现」与「真的 0%」
    seen: std::collections::HashSet<(i32, u64)>,
    /// 首轮不产出整机 CPU（new_all 的初值是初始化采样，不是 10 s 窗口）
    cpu_warmed: bool,
}
```

- 首次出现的进程 → `cpu = None`；已在 `seen` 里 → `Some(cpu_usage)`（含真实的 0.0）。
- `pid` 复用由 `start_time` 区分（与 `merge_deep` / `merge_workspace` 同一套防串台规则）。
- 整机：首次 `scan()` 只建立基线、不产出 `cpu.usage`；第二轮起正常。
- 前端：`null` → `—`，**绝不渲染成 0%**（否则「它没在跑」是假事实）。

### 3.2 单核百分比与 >100%

- 进程与家族一律用**单核口径**（与活动监视器同语义）：`320%` 表示「吃掉 3.2 个核」。
- 文案：`320%` 右侧灰字 `约 3.2 核`；tooltip：`单核百分比，多线程进程可超过 100%`。
- 家族合计上限是 `cores × 100`，不需要额外裁剪。

### 3.3 「10 秒均值」的语义与代价

- 采样与内存同频（10 s），所以 CPU 是**10 s 窗口的平均占用**，看不出 200 ms 级尖峰。
- 想抓尖峰需要 1 s 采样：`scan()` 实测 20–33 ms，10 倍频会把常驻开销从 ~0.3% 抬到 ~3%，**不做**。
- UI 必须在能看到 CPU 数字的地方标注「近 10 秒均值」（首屏一行小字即可，不必每行都标）。
- 与活动监视器（约 5 s 窗口）比对时允许 ±5 个百分点偏差，验收按此判定（§12）。

### 3.4 不承诺守恒

`sum(进程) ≠ 整机`（实测 70.9% vs 15.6%，口径还差一个 ÷核数）。UI 里**不同时呈现两个数**：整机读数只在首屏，进程/家族号只在列表，避免用户做减法得出「Prune 算错了」。

## 4. 数据模型变更

### 4.1 Rust（`crates/prune-core/src/mem/`）

```rust
// mod.rs
pub struct ProcMem {
    // ...现有字段
    /// 近 10 s 均值，单核百分比；None = 首见/采集失败（不是 0）
    #[serde(default)]
    pub cpu: Option<f32>,
    /// 累计 CPU 时间（ms）
    #[serde(default)]
    pub cpu_time_ms: u64,
}

pub struct FamilyMem {
    // ...现有字段
    pub cpu: Option<f32>,        // 已知成员之和；可能为下界
    pub cpu_covered: usize,      // 有 CPU 读数的成员数（对齐 deep_covered 的语义）
}

pub struct WorkspaceMem {
    pub cpu: Option<f32>,        // Cursor：AI 项目 120% / prune 项目 30%
}

/// 整机 CPU 读数（新增，不塞进 SystemMem：那是内存口径模块）
pub struct CpuInfo {
    pub usage: f32,
    pub cores: u32,
    pub load: Option<Load>,      // Windows: None
    pub history: Vec<f32>,       // 最近 60 点（10 min），sparkline 用
    pub at: i64,
}

pub struct MemSnapshot {
    // ...现有字段
    #[serde(default)]
    pub cpu: Option<CpuInfo>,    // 旧快照反序列化为 None，UI 自动隐藏
}
```

聚合规则（`aggregate()`）：
- `cpu` 只累加 `Some` 的成员；`cpu_covered < process_count` → UI 前缀 `≥`（与 footprint 下界的处理保持同一套语言）。
- 工作区分组同理。
- 排序仍由前端决定（Rust 保持按内存降序的默认顺序，不改协议）。

### 4.2 TypeScript（`src/types/mem.ts` + `src/lib/format.ts`）

| 文件 | 改动 |
|---|---|
| `types/mem.ts` | 镜像 `cpu?: number \| null`、`cpuTimeMs: number`、`cpuCovered: number`、`CpuInfo`；`MemSnapshot.cpu: CpuInfo \| null` |
| `lib/format.ts` | 新增 `formatCpu(v: number \| null, cores?: number): string`：`null → "—"`；`<1 → "<1%"`；`≥100` 取整并可选加「约 N 核」 |
| `lib/devFixture.ts` | 补 CPU 字段，保证浏览器态能迭代 UI |

`cpuTimeMs` 的 payload 代价：358 进程 ≈ +2.8 KB JSON，可接受（`cmd` 仍然不序列化）。

## 5. 采样与成本

### 5.1 fast round 的新顺序

```
refresh_cpu_specifics(nothing().with_cpu_usage())   ← 新增，0.1 ms
        ↓
scanner.scan()  (refresh_processes，已含 CPU)        ← 已有，20–33 ms
        ↓
读取 cpu.usage / cores / load → 追加 ring buffer
        ↓
assemble() → MemSnapshot{ ..., cpu }  → emit mem://snapshot
```

| 项 | 现状 | 改后 | 结论 |
|---|---|---|---|
| fast round 耗时 | 20–33 ms（`scan` 为主） | +0.1 ms | 增量 < 0.5% |
| 新增 syscall | — | 1 次 `host_processor_info`（macOS）/ PDH 查询（Windows） | 可忽略 |
| 深探轮 | 5 min | 不动 | CPU 不需要深探 |
| 事件协议 | `mem://snapshot` | payload 增加 `cpu` 字段 | 前端同时更新，无版本协商 |

### 5.2 ring buffer（代替入库）

- `MemState` 增加 `cpu_hist: Mutex<VecDeque<f32>>`，每轮 push，cap = 60（10 min）。
- 随 `CpuInfo.history` 下发，复用 `src/components/Sparkline.tsx`。
- **不入库**：现有 `mem_process` 表是 5 min 粒度、7 天保留，为 CPU 建同样精度没有意义；10 min 环形缓冲已够回答「刚才是不是一直在烧」。

### 5.3 Windows 特别注意

- 进程 CPU / 整机 CPU 都走同一个 sysinfo API，**不新增 FFI**（W2 的 `win.rs` 不动）。
- `System::load_average()` 在 Windows 返回全 0 → 必须在 Rust 侧就变成 `None`，不能让它显示成「负载 0.0」这种假事实。
- 验收沿用 `scripts/check-windows.sh --build`（0 error + 产出 PE32+ exe）。

## 6. UI 设计

### 6.1 面板（380×560，hero 高度不变）

```
┌──────────────────────────────────────┐
│ Prune [footprint]        [刷新] [打开]│
├─ 内存 ───────────────────────────────┤
│ ● 压力正常  已用 12.1/16 GB   CPU 15% │ ← CPU 右端对齐，null 时整块不渲染
├─ 存储 ───────────────────────────────┤
│ ⚡ 45.2 GB 可回收        剩余 98 GB   │
├──────────────────────────────────────┤
│ [应用] [概况]      内存⇄CPU  12 个已深探│
│ Cursor            3.2 GB · 12%        │
│ ▓▓▓▓▓▓▓░░░░░  11 进程 · 精确          │
│ Chrome            1.8 GB · 34%        │
│ 某游戏客户端       0.4 GB · 18%        │
└──────────────────────────────────────┘
注：按 CPU 排序时，「某游戏客户端」这类小体积但高 CPU 的行必须能进前 10（见 §6.1 关键坑）。
```

- **hero 内存行右端**加 `CPU 15%`（`text-[11px]`，等宽数字），不新增行高（M4 的 ≤76px 预算不变）。
- 应用列表 `compact` 行尾在内存数字后补 ` · 12%`；**行内不画第二个 Meter**（380px 会挤）。
- 排序切换放在 tabs 行右侧（替换现在只有「12 个应用 · 11 个已深探」的位置，两者并列：`内存 ⇄ CPU` 小按钮 + 计数）。
- **关键坑**：`limit={10}` 的截断必须发生在排序**之后**。现在是 `families.slice(0, limit)`，若按 CPU 排序仍先切内存 Top10，CPU 最高的应用（常常是小体积的 Node/游戏进程，如实测的 18% 但只有 0.4 GB）会直接消失。
- 点击行 → 打开该家族「当前排序维度」最重的进程（现在写死为内存最重，需按排序维度切换）。

### 6.2 主窗口（960×640，apps tab）

```
#   应用                相对占用      CPU      总量
01  Cursor              ▓▓▓▓▓▓░░░    12%      3.2 GB
02  Chrome              ▓▓▓░░░░░░    34%      1.8 GB
```

- grid：`[26px_1fr_112px_86px]` → `[26px_1fr_96px_62px_86px]`（CPU 列 62px，右对齐等宽）。
- 表头「相对占用 / CPU / 总量」中 **CPU 与 总量可点击排序**（默认内存降序，点击 CPU 切换为 CPU 降序，再点反向）。排序状态只在前端，Rust 协议不变。
- 进程明细（overview 右栏）每行补 CPU%；详情里补 `累计 CPU 1h 12m`（`cpuTimeMs`）。

### 6.3 文案硬约束（沿用 M2/M3 的克制原则）

| 允许 | 禁止 |
|---|---|
| 「CPU 12%（近 10 秒均值）」 | 「CPU 占用过高」 |
| 「320%（约 3.2 核）」 | 「建议关闭以提升性能」 |
| 「系统进程 · 不可结束」 | 「优化 / 加速 / 降温 / 省电」任何动词 |
| 「累计 CPU 1h 12m」 | 「该应用正在浪费 CPU」 |

理由：高 CPU 可能是编译、导出、游戏、模型推理——**这是用户的正当使用，不是浪费**。Prune 只提供事实与排序，判断权交回用户。

## 7. 安全联动（P1，容易被忽略）

CPU 榜会把 `kernel_task` / `WindowServer` 顶到第一屏，用户的第一反应是「结束它」。现在的护栏只有 `pid <= 1` 与自身（`src-tauri/src/commands.rs` 的 `kill_process`），不足以覆盖 CPU 榜带来的新诱惑。

| 项 | 做法 |
|---|---|
| 保护名单 | `kernel_task, launchd, WindowServer, loginwindow, syslogd, opendirectoryd, coreaudiod, bluetoothd, powerd, mds, mds_stores, distnoted`（**精确匹配进程名**，不做子串，避免误伤 `WindowServerHelper` 之类） |
| 后端 | `kill_process` 在现有护栏后追加名单校验，返回「系统关键进程，不允许结束」 |
| 前端 | 名单内的行：`系统` 徽标 + 不渲染右键「结束进程」入口；CPU 榜仍然展示它们（它们是事实） |
| 单测 | 名单命中 / 不误伤 `XXXHelper` / 自身 / pid≤1 四条路径 |

## 8. 分期与任务分解

### C0 · 数据层（~0.5 天）

| 文件 | 改动 | 验收 |
|---|---|---|
| `crates/prune-core/src/mem/process.rs` | `Scanner` 增 `seen` / `cpu_warmed`；`scan()` 返回 `ScanResult { procs, cpu_usage: Option<f32>, cores: u32 }`；填 `cpu` / `cpu_time_ms` | 首轮 `cpu=None`；第二轮有值；新进程首见 `None` |
| `crates/prune-core/src/mem/mod.rs` | `ProcMem/FamilyMem/WorkspaceMem/MemSnapshot` 加字段；`CpuInfo`；`aggregate` 累计 `cpu` + `cpu_covered` | 单测：求和、下界标记、全未知→`None` |
| `src-tauri/src/sampler.rs` | fast round 内先 `refresh_cpu_specifics`；ring buffer 60 点；`assemble` 填 `cpu` | 实跑日志打印整机 CPU 与耗时 |
| `src/types/mem.ts` / `src/lib/format.ts` | 镜像类型 + `formatCpu` | `tsc --noEmit` 0 error |

### C1 · 面板（~0.5 天）

| 文件 | 改动 |
|---|---|
| `src/panels/MenuBarPanel.tsx` | hero 内存行右端 CPU 读数；排序状态 `mem \| cpu`；tabs 行排序切换 |
| `src/components/AppMemoryList.tsx` | `sortBy` 入参（默认 `mem`），**排序后再 slice**；compact 行尾 CPU；点击打开「当前维度最重」进程 |

验收：CPU 排序下，内存排名 20 名开外但 CPU 第一的应用必须出现在面板前 10。

### C2 · 主窗口（~0.3 天）

| 文件 | 改动 |
|---|---|
| `src/windows/MainWindow.tsx` | apps 表头 5 列 + CPU 可排序；overview 进程行 CPU%；详情累计 CPU 时间 |
| `src/components/Sparkline.tsx` 复用 | CPU 60 点 sparkline 挂在整机读数旁（可选项，见 §9-4） |

### C3 · 安全与文档（~0.2 天）

| 项 | 内容 |
|---|---|
| 护栏 | §7 保护名单 + 单测 |
| `README.md` | 「❌ 不做通用硬件监控」改写为「❌ 不做温度/风扇/功耗/实时频率；CPU 只做进程级归因」 |
| `docs/prune-feature-spec-v2.md` §2.1 | 边界表补一行 CPU 归因 |
| `docs/prune-tauri-technical-plan.md` §2 | 模块表加 CPU 口径（在 `mem/` 内，不新建模块） |
| `docs/windows-support-plan.md` | 补 CPU / load 的平台差异 |

## 9. 待确定问题（评审拍板）

| # | 问题 | 默认建议 |
|---|---|---|
| 1 | 整机 CPU 放面板 hero 内存行右端，还是 footer？ | hero：首屏就要能答「机器现在忙不忙」 |
| 2 | 面板 compact 行是否常显 CPU%？ | 常显（` · 12%`，约 34px）；只有 `null` 时不占位 |
| 3 | 家族 CPU 用单核和（可 >100%）还是折算整机占比？ | 单核和，与活动监视器/进程行同语义；tooltip 给「约 N 核」 |
| 4 | 是否做 60 点 CPU sparkline？ | 做：数据已在 ring buffer，UI 复用现有组件（~30 行） |
| 5 | CPU 是否入库做 7 天趋势？ | 不做：5 min 粒度对 CPU 无意义，且要 schema 迁移 |
| 6 | 是否标「持续高 CPU」？ | 本期不做：编译/导出/游戏都合法，误判成本高于收益 |

## 10. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 首轮/新进程的 0 被当真实值 | 高 | 高（「它没在跑」是假事实） | `seen` + `cpu_warmed` → `null`；单测锁定 |
| 用户误读 >100% | 高 | 低 | 「约 N 核」换算 + tooltip |
| 结束系统进程（kernel_task / WindowServer） | 低 | 高（系统失稳） | §7 名单 + 前端不渲染入口 |
| Windows 上 load 恒 0 被当「空闲」 | 中 | 中 | Rust 侧转 `None`，UI 隐藏 |
| 排序切换后「CPU 高的应用消失」 | 中 | 中 | 排序先于 slice，C1 专门验收 |
| pid 复用导致 CPU 串台 | 低 | 中 | CPU 不跨轮缓存，按轮直读；`seen` 用 `(pid,start_time)` |
| 采样开销回归 | 低 | 低 | 预算 +5 ms，实测 +0.15 ms；fast round 日志留耗时 |
| 旧快照/旧前端缺字段 | 中 | 低 | 全部 `#[serde(default)]` + TS 可选字段 |

## 11. 不做项（含理由，避免二次讨论）

| 不做 | 理由 |
|---|---|
| 每核热力图 / P-E 核标注 | sysinfo 只有逻辑核顺序，不暴露 perflevel；8 条无标签的条带是噪音。实测逐核数据只能看出「前 4 核更忙」 |
| 温度 / 风扇 / 功耗 / 实时频率 | 需 SMC / `powermetrics`（root）；`pmset -g therm` 在本机（Apple Silicon）只返回三条 "No ... recorded"，拿不到 `CPU_Speed_Limit` |
| 独立 CPU tab | 内容与「应用」列表 90% 重叠；M4 的域分离依据（动作不同）在这里不成立 |
| CPU 历史入库 | 见 §9-5 |
| 「持续高 CPU」判定 / 一键降温 | 与「只陈述事实、不替用户下结论」的定位冲突（M3 措辞硬约束同理） |
| 1 s 高频采样抓尖峰 | 常驻开销 ×10，收益是「看到瞬时峰值」，与 10 s 内存轮不同频，反而不利于对照 |

## 12. 验收标准

| # | 项 | 通过条件 |
|---|---|---|
| 1 | 单测 | `cargo test --workspace` 全绿；新增 ≥6 条（首见 None、聚合求和、下界标记、全未知 None、名单匹配、load 置 None） |
| 2 | 首轮语义 | 冷启动第一帧所有进程 CPU 显示 `—`；第二轮（≤10 s）出现数值 |
| 3 | 新进程语义 | 启动一个 `yes > /dev/null` 后 ≤20 s 内该进程从 `—` 变为接近 100%（单核） |
| 4 | 口径校验 | 与活动监视器同一进程比对，偏差 ≤5 个百分点（10 s 均值 vs 5 s 窗口） |
| 5 | >100% 展示 | 多线程进程（如 `yes` 多开或编译）显示 `3xx%` + 「约 3.x 核」 |
| 6 | 排序正确性 | 按 CPU 排序时，CPU 第一的应用出现在面板/主窗口首位，且不被 `limit` 截掉 |
| 7 | 整机读数 | 面板 hero 与主窗口读数同源（同一 snapshot 字段），刷新间隔内不跳变 |
| 8 | 跨平台 | `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` 0 error；`check-windows.sh --build` 产出 exe；Windows 上 load 不显示 |
| 9 | 类型/构建 | `tsc --noEmit` 0 error；`vite build` 通过 |
| 10 | 性能 | fast round 耗时增量 ≤5 ms（实测预期 <0.5 ms）；快照 payload 增量 ≤4 KB |

## 13. 与上层文档的关系

| 文档 | 需要改什么 |
|---|---|
| `README.md` | 「不做」表格里的「通用硬件监控」下加限定：**不做温度/风扇/功耗/频率；CPU 只做进程级归因** |
| `docs/prune-feature-spec-v2.md` §2.1 | 边界表补一行，避免后续评审反复问「为什么不显示温度」 |
| `docs/prune-tauri-technical-plan.md` §2 | 模块表补 CPU 口径说明（`mem/process.rs` + `mem/mod.rs`，**不新建 cpu 模块**——它和内存同源同轮） |
| `docs/m2-memory-plan.md` §4 | 数据模型处加一句「CPU 字段见 M5」，保持跨文档一致 |
| `docs/windows-support-plan.md` | 新增 CPU 口径小节：同为 sysinfo、load 不可用、无新增 FFI |

## 14. 提交拆分（沿用仓库 commit 规范）

| 阶段 | 提交 |
|---|---|
| C0 | `feat(mem): 进程与家族 CPU 口径（首见/首轮标 null，单核百分比）` |
| C1 | `feat(panel): 应用列表 CPU 列与内存/CPU 排序切换` |
| C2 | `feat(main): 主窗口 CPU 列、排序与累计 CPU 时间` |
| C3 | `fix(safety): 系统关键进程禁杀名单` + `docs: CPU 归因方案与边界修订` |
