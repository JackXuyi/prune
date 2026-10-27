# Prune · 磁盘可回收探测（M1）实现方案

> 2026-10-26 · 上游：`prune-feature-spec-v2.md` 模块 A / §5 A1 详设 / §7 安全分级；`prune-tauri-technical-plan.md` §5.3~5.5
> 本文只解决「A1 跨平台二进制探测 / A2 缓存统计 / A3 重复检测 / 🟢 一键清理 + 安全护栏」怎么落地。
> 实测基线直接复用 `docs/research/reclaim-scan-report.md`（2026-10-22，54.85 GB）与 `reclaim-scan.sh`。
> **A4（node_modules 活跃度）/ A5（模拟器）/ A6（Docker）不在本期**（见 D-K4 / D-K3）。

---

## 0. 结论先行

| # | 结论 |
|---|---|
| 1 | **A1 的精确删除 v1 只覆盖 Yarn v6 缓存**：实测 334 个跨平台条目全部在 Yarn（目录名自带平台标识，删目录即删条目，零解析风险）；npm `_cacache` 是 content-addressed 哈希库、pnpm store 同理，**条目级删除不可行**，只做统计 + 整库清理 |
| 2 | 删除一律走 `trash` crate（废纸篓），**不调 `npm cache clean` 等外部命令**——白名单 + 废纸篓 + dry-run + 熔断四条护栏全部是代码层约束，不是 UI 提示 |
| 3 | 扫描范围是**定点白名单目录**（约 20 个根），不是全盘；M0 遗留的「6336 个 node_modules 扫描性能」验证折入 T-301 探针实测定生死 |
| 4 | 首屏主数字「可回收 GB」= L1 + L2 + L3 合计；**L4 Docker 不计入主数字**（不可行动的数字不进首屏，只留命令入口） |
| 5 | 本期新增 `prune-core` 的 `disk/` `classify/` `reclaim/` 三个模块，`mem/swap/db` 不动；菜单栏面板已有内存区块，磁盘区块 + 主数字接在现面板上 |

---

## 1. 本期范围

| 做 | 不做 |
|---|---|
| A1 跨平台二进制探测（Yarn 条目级精确判定 + 清理） | A4 node_modules 活跃度（M2+，需 mtime 扫描与 UI 分级） |
| A2 包管理器缓存统计（npm/Yarn/pnpm/Bun/Cargo/Homebrew 等定点目录） | A5 模拟器 / Playwright 明细（M2+） |
| A3 三管理器重复检测（总量层面 + 合并建议文案） | A6 Docker（L4 永远只给命令，M3） |
| 🟢 L1/L2 一键清理 + 全套安全护栏 | L3 逐项确认清理（等 A4 的「N 天未改动」数据） |
| 菜单栏首屏主数字 + 可回收分层 | mtime 增量扫描（v1 全量按需，见 D-K5） |
| 扫描进度事件 + 每周静默刷新 | CLI / MCP（M3） |

---

## 2. 实测基线（2026-10-22，本机只读，可复现）

| 项 | 实测值 | 对本方案的意义 |
|---|---|---|
| 可回收合计 | **54.85 GB** | 首屏主数字的期望量级 |
| L1 跨平台二进制 | **7.74 GB**（334 条 / Yarn 内） | A1 主战场；darwin-arm64 仅 0.93 GB，浪费比 8.3:1 |
| L2 其余缓存 | 22.47 GB | npm 7.87 + Yarn 14.29 + pnpm 7.09 = 29.25 GB 占 53% |
| L3 需判断 | 21.69 GB（node_modules 17.0 GB + Simulator 3.49 GB + Playwright 1.2 GB） | 只统计展示，不清理 |
| L4 Docker VM | 2.95 GB | 不计入主数字 |
| shell 版扫描耗时 | **~5 分钟**（逐个 `du`，曾被打断） | Rust 并行目标 < 20 s（技术方案 §10 #4） |
| A1 误判后果 | arm64 进程加载不了 linux/win32/android 二进制 | 误判风险实际为零；darwin-x64 在有 Rosetta 时需降级 🟡 |

**Yarn v6 目录名形态**（A1 判定的输入）：`~/Library/Caches/Yarn/v6/npm-@next-swc-darwin-arm64-13.5.1-<hash>/...`——包名、平台、版本都在首层目录名里，**枚举首层目录即可完成判定**，不需要进内容。

---

## 3. 架构

```
prune-core/                          src-tauri/                     前端
├ disk/mod.rs    定点扫描+体积统计 →  scan.rs   后台扫描调度          →  stores/diskStore.ts
├ disk/whitelist.rs 白名单常量表        │  emit("disk://progress")      (Zustand)
├ classify/mod.rs A1 平台判定(纯函数)   │  emit("disk://done")      →  panels/MenuBarPanel 主数字+磁盘区块
├ reclaim/mod.rs 白名单校验+废纸篓  →   commands.rs                    →  windows/MainWindow 可回收 tab
└ (mem/ swap/ db/ 不动)                 disk_scan / disk_report
                                        reclaim_preview / reclaim_execute
```

数据流：启动后 30 s 后台扫一次（进度事件）→ 报告缓存于 Rust 侧 → 前端 `invoke` 直接取 → 每周静默刷新 + 手动刷新。清理走 preview（dry-run 列表）→ 用户确认 → execute → 重新计算报告。

---

## 4. 数据模型

```rust
// prune-core
pub struct DiskReport {
    pub scanned_at: i64,
    pub scan_elapsed_ms: u64,
    pub l1: Vec<CacheGroup>,          // A1：按平台分布，Yarn 组带明细
    pub l2: Vec<CacheGroup>,          // A2：按管理器/目录
    pub l3_summary: Vec<CacheGroup>,  // A4 之前的粗统计（总量 + 目录数）
    pub dup: Option<DupSummary>,      // A3：三管理器并存
    pub reclaimable_total: u64,       // L1+L2+L3 合计（主数字）
}

pub struct CacheGroup {
    pub key: String,                  // "yarn" / "npm" / "simulator" ...
    pub label: String,                // "Yarn 缓存"
    pub root: String,
    pub total_bytes: u64,
    pub entry_count: usize,
    pub deadweight: Option<DeadweightStats>, // 仅 A1 组非空
}

pub struct DeadweightStats {
    pub bytes: u64,                   // 🟢 永远加载不了的部分
    pub keep_bytes: u64,              // ✅ darwin-arm64 可用部分
    pub entry_count: usize,
    pub by_platform: Vec<PlatformStat>,
    pub top: Vec<PackageHit>,         // Top 10 体积包（@next/swc 这类）
    pub ratio: f64,                   // 浪费比 deadweight/keep
}

pub enum Platform { Linux, Win32, Win, Android, Musl, Freebsd, DarwinX64, DarwinArm64 }
pub enum Verdict { Deadweight, NeedsConfirm, Keep }   // 🟢 / 🟡 / 保留
```

前端 `src/types/disk.ts` 镜像同一结构（serde camelCase），与 `types/mem.ts` 同约定。

---

## 5. 扫描策略

### 5.1 定点白名单（编译期常量，硬护栏的一部分）

| 组 | 路径 | 级别 | 处理 |
|---|---|---|---|
| Yarn | `~/Library/Caches/Yarn/v6` | 🟢 L1+L2 | 首层目录枚举 → A1 判定；条目级 trash |
| npm | `~/.npm/_cacache` | 🟢 L2 | 只测体积；整库 trash（D-K2） |
| pnpm | `~/Library/pnpm/store`（+ 旧路径 `~/.local/share/pnpm/store`） | 🟢 L2 | 只测体积；整库 trash |
| Bun | `~/.bun/install/cache` | 🟢 L2 | 体积 + 整库 |
| Cargo | `~/.cargo/registry` | 🟢 L2 | 体积 + 整库 |
| Homebrew | `~/Library/Caches/Homebrew` | 🟢 L2 | 体积 + 整库 |
| DerivedData / Xcode 缓存 / CocoaPods / Composer / Gradle / Maven / Go mod / Playwright / Cypress / Electron 缓存 | 见 reclaim-scan.sh 清单 | 🟢/🟡 | 体积统计；清理级别按 §7 分级 |
| Simulator | `~/Library/Developer/CoreSimulator/Devices` | 🟡 L3 | 只统计（D-K4） |
| node_modules | `~` 下深度 8、`-prune` 语义 | 🟡 L3 | **只计数与总量**，不逐项展示（T-301 顺带实测耗时） |

实现：`ignore::WalkBuilder` 并行遍历，`git_ignore(false)`（缓存目录本就在 .gitignore 语义之外，要物理大小）、`hidden(false)`；体积累加 `metadata.len()`（`st_blocks×512` 与 `du` 口径有差，报告对齐时注明用哪种——**选 `len()`，UI 口径一致即可**）。

### 5.2 触发时机（D-K5）

| 时机 | 行为 |
|---|---|
| 启动后 30 s | 后台全量扫一轮（避开启动竞争），进度事件推送 |
| 每周静默一次 | 常驻定时器，更新菜单栏数字 |
| 用户手动刷新 | 面板/主窗口按钮 |
| 清理完成后 | 增量重算受影响组（不重扫全盘） |

v1 不做 mtime 增量：全量预算 < 20 s、每周一次，增量收益不抵复杂度。
（**A7 追加**：2026-10-27 该决策被推翻——每小时静默增量 + 每周全量校准已落地，见 `docs/a7-incremental-plan.md` 与 `docs/a7-incremental-acceptance.md`；手动刷新与启动首扫保持全量。）

---

## 6. A1 分类器（`classify/`，纯函数，单测重仓）

```rust
pub fn platform_of(name: &str) -> Option<Platform>;
// token 表：linux / win32 / win / android / musl / freebsd / darwin-x64 / darwin-arm64
// 注意 token 边界：'-linux-' 必须是独立段，防止包名本身含 "linux"（如 `some-linux-tools`）误判

pub fn classify(platform: Option<Platform>, arch: Arch, has_rosetta: bool) -> Verdict {
    match platform {
        Some(p) if p.is_foreign() => Verdict::Deadweight,            // linux/win32/win/android/musl/freebsd
        Some(Platform::DarwinX64) if arch == Arch::Arm64 && !has_rosetta => Verdict::Deadweight,
        Some(Platform::DarwinX64) => Verdict::NeedsConfirm,          // 有 Rosetta → 🟡 不进默认清理
        Some(Platform::DarwinArm64) | None => Verdict::Keep,
    }
}
```

- 架构：`uname -m`（`std::env::consts::ARCH` 在编译期可能被交叉编译干扰，运行时取 `uname` 为准）。
- Rosetta（D-K6）：检测 `/usr/libexec/rosetta/oahd` 进程是否在运行，比查安装目录可靠。
- **误判负例单测必须包含**：包名含平台词但非平台包（`node-linux-x64` 反例 vs `liblinux`）、`darwin-arm64` 不能被判为 deadweight、无平台 token 的普通包 Keep。
- 真机抽样核对（M0 #4 关闭项）：随机抽 100 个 Yarn 条目人工核对，写入验收记录。

**产出文案口径（照抄功能方案 §5.2，不改写）**：

```
🟢 7.74 GB · 你的 Mac 永远加载不了
334 个跨平台二进制缓存包 · 浪费比 8.3 : 1
最大的一个：@next/swc 缓存了 25 个版本×平台组合（你只需要 3 个）
```

---

## 7. 清理层（`reclaim/`，四条代码层硬护栏）

```rust
pub fn reclaim_execute(items: &[ReclaimItem], confirmed: bool) -> Result<ReclaimReport, ReclaimError> {
    for it in items {
        ensure_in_whitelist(&it.path)?;        // 1. 规范化后必须落在白名单根内，越界直接 Err
        ensure_verdict_cleanable(&it)?;        // 2. 🟡(NeedsConfirm) 不允许走默认通道
    }
    let total = total_bytes(items);
    if total > 20 * GB && !confirmed { return Err(NeedsExplicitConfirm); }  // 3. 熔断
    trash::delete_all(items.iter().map(|i| &i.path))?;                       // 4. 废纸篓，无 rm
    Ok(report)
}
```

| 护栏 | 实现 | 单测 |
|---|---|---|
| 白名单 | 路径 `canonicalize()` 后前缀匹配白名单根；拒绝符号链接逃逸 | 白名单外路径 / `../` 穿越 / symlink 指向 home → 全 Err |
| 废纸篓 | 仅 `trash` crate，代码里不出现 `remove_dir_all` 对用户路径的调用 | — |
| Dry-run | `reclaim_preview` 返回前 50 项 + 总量，UI 先列后删 | — |
| 熔断 | 单次 > 20 GB 必须带 `confirmed=true` | 19.9 GB 直接过 / 20.1 GB 无确认 → Err |
| 不提权 | 全程无 root 请求；拿不到的目录直接跳过并在报告标注 | — |
| 清理中保护 | pnpm/npm 清理前不检测运行中安装（v1 从简），废纸篓兜底可恢复 | — |

术语硬约束（继承功能方案 §12）：文案永远说「可再生资源 / 永远加载不了」，**永不出现**「加速 / 优化 / 释放 / 清理垃圾」。

---

## 8. IPC 契约

| 类型 | 名称 | 说明 |
|---|---|---|
| command | `disk_report()` → `Option<DiskReport>` | 最新缓存报告（无则前端显示「尚未扫描」） |
| command | `disk_scan()` → `()` | 触发后台扫描（幂等，扫描中重复调用忽略） |
| command | `reclaim_preview(keys)` → `ReclaimPreview` | dry-run：条目列表 + 总量 + 熔断提示 |
| command | `reclaim_execute(keys, confirmed)` → `ReclaimReport` | 执行清理并重算报告 |
| event | `disk://progress` | `{ phase, groups_done, groups_total, bytes_so_far }` |
| event | `disk://done` | 扫描/清理完成，携带新报告 |

扫描单飞闸门：复用 M2 的 `DeepInFlight` 模式（`AtomicBool` + Drop 自动释放），手动刷新撞上周定时器时后来者跳过。

---

## 9. UI

### 9.1 菜单栏面板（首屏，380 宽）

```
┌─────────────────────────────┐
│ ⚡ 54.9 GB 可回收    ● 绿    │  ← 主数字（L1+L2+L3），点开进主窗口
│ 🟢 7.7 GB  跨平台二进制      │  ← 杀手位置顶
│ 🟢 22.5 GB 包管理器缓存      │
│ 🟡 21.7 GB 项目依赖/模拟器   │
│ 🔴 Docker · 只给命令         │  ← 不计入主数字（D-K3）
├─────────────────────────────┤
│ （原内存区块：压力/swap/受害者）│
└─────────────────────────────┘
```

### 9.2 主窗口新增「可回收」tab

- 左列：A1 卡片置顶（分布表 + Top 10）→ A2 分组 → A3 合并建议卡（「三个包管理器并存 29.25 GB，保留一个即可」）
- 右侧检视器：选中组的明细（Yarn 组可展开到条目级）+ [ 预览清理 ] → dry-run 列表 → [ 确认移入废纸篓 ]
- 顶部：扫描时间 + 口径说明 + 手动刷新；🟡 NeedsConfirm 条目默认不勾选

---

## 10. 性能与自耗预算

| 项 | 预算 | 校验方式 |
|---|---|---|
| 全量白名单扫描 | **< 20 s**（技术方案 §10 #4；shell 版 5 min） | T-301 探针实测 wall time |
| 扫描期间 UI | 不卡顿（后台线程 + 进度事件） | 事件间隔 < 1 s |
| 扫描峰值内存 | < 200 MB（只累计 u64，不缓存路径树） | 探针观测 |
| 常驻增量 | 扫描结束后归零（报告 ~KB 级） | — |
| 每周静默扫描耗电 | 每周一次可忽略 | — |

---

## 11. 任务分解

| ID | 任务 | 产出 | 依赖 |
|---|---|---|---|
| T-301 | `examples/disk-probe` 探针：白名单并行扫描 + node_modules 计数，实测耗时 | **M0 #2 关闭**（< 20 s 或调 maxdepth/并行度） | — |
| T-302 | `disk/whitelist.rs` 白名单常量 + `disk/mod.rs` 定点扫描与体积统计 | 单测：体积累加、缺失目录跳过 | T-301 |
| T-303 | `classify/`：平台 token 表 + `classify()` + Rosetta 检测 | 单测含误判负例；**真机抽样 100 条人工核对（M0 #4 关闭）** | — |
| T-304 | A2 统计汇总 + A3 三管理器重复检测 + Top 10 包 | 报告结构与 reclaim-scan-report 对齐 | T-302 |
| T-305 | `reclaim/`：白名单校验 + trash + dry-run + 熔断 | 四条护栏单测全绿 | T-302 T-303 |
| T-306 | `scan.rs` 调度（启动 30s / 每周 / 手动，单飞闸门）+ 进度事件 + 4 个 command | 5 分钟日志观察 | T-304 T-305 |
| T-307 | `types/disk.ts` + `diskStore` + 面板主数字与磁盘区块 | 面板可见「⚡ X GB 可回收」 | T-306 |
| T-308 | 主窗口「可回收」tab + A1 清单 + 清理交互（preview → execute） | 端到端清理一次 Yarn 条目 | T-307 |
| T-309 | 验收：清理后 `yarn install` 无失败 + 数字对 10-22 报告复核 | 记录进 `docs/m1-disk-acceptance.md` | T-308 |

粗排：T-301/T-303 可立即并行（0.5 天）→ T-302/T-304/T-305（1.5 天）→ T-306（0.5 天）→ T-307/T-308（1.5 天）→ T-309（0.5 天）。

---

## 12. 待确认决策点

| ID | 决策 | 我的建议 |
|---|---|---|
| **D-K1** | A1 条目级删除的覆盖范围 | **v1 只做 Yarn v6**（目录名即元数据，零解析风险）；npm cacache / pnpm store 条目级不可行也不值得（见 D-K7） |
| **D-K2** | 整库清理实现 | **直接 trash 目录本体**（`~/.npm/_cacache` 等，下次安装自动重建），不调 `npm cache clean` / `pnpm store prune` 外部命令——可控、可审计、离线、行为一致 |
| **D-K3** | 首屏主数字口径 | **L1+L2+L3 合计**，分层行展示；L4 Docker 不计入（未运行 daemon 时无法拆明细，不可行动的数字不进主位） |
| **D-K4** | A4/A5 是否本期做 | **不做**。M1 收口纯 L1/L2；L3 的「N 天未改动」判定与逐项确认 UI 留给 M2+ |
| **D-K5** | 扫描触发与增量 | 启动后 30 s 一轮 + 每周静默 + 手动；**v1 不做 mtime 增量**（全量 < 20 s，增量不抵复杂度） |
| **D-K6** | Rosetta 判定方式 | 检测 `/usr/libexec/rosetta/oahd` 进程存在；失败视为「无 Rosetta」（宁可 🟢 误报也不漏报浪费） |
| **D-K7** | npm cacache 是否解析 index-v5 做条目级 A1 | **不做**。要解析 `make-fetch-happen` key + 映射 content hash + 删两个库，复杂度/收益比差；整库 trash 已覆盖其价值 |
| **D-K8** | 体积口径 | ~~用 `metadata.len()`（逻辑大小）~~ **实测推翻 → 改用 `st_blocks×512`（物理块）**：Docker.raw 是稀疏文件，按 `len()` 报出 1099 GB 的假数字。详见 §16 |

---

## 13. 风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| Yarn 目录名平台误判（包名含平台词） | 🔴 | token 边界匹配 + 负例单测 + 100 条人工抽样；废纸篓可恢复兜底 |
| darwin-x64 误删（Rosetta 下 x86 进程需要） | 🟠 | 有 Rosetta 一律降 🟡，不进默认清理通道（§7 护栏 2） |
| 扫描比预算慢（> 20 s） | 🟠 | T-301 先探针后实施；可调 maxdepth / 限 node_modules 仅计数 |
| pnpm store 清理撞上进行中的安装 | 🟡 | v1 废纸篓兜底；报告注明「清理时避免正在 install」 |
| 与 10-22 报告数字漂移（缓存自然增长） | 🟡 | 验收允许 ±合理漂移，只核对同组相对关系与 A1 占比 |
| 术语越界变成「清理工具」形象 | 🟡 | 文案硬约束写入 reclaim/ 与 UI 组件的公共常量，code review 门禁 |

---

## 14. 验收标准

1. 首屏显示「⚡ X GB 可回收」，分层行与 `reclaim-scan-report` 同组相对关系一致（绝对值允许自然漂移）
2. A1 真机抽样 100 条人工核对，**判定正确率 100%**（含 darwin-arm64 不误杀）
3. 清理 Yarn 跨平台条目后，任一真实项目 `yarn install` 成功且不再重下 darwin-arm64 包
4. 白名单外路径调用 `reclaim_execute` 直接 `Err`（单测覆盖 `../`、symlink、绝对路径注入）
5. 单次 > 20 GB 无 `confirmed` 参数时熔断报错，UI 出现二次确认
6. 全量扫描 < 20 s，期间面板可正常打开、内存区块读数不受影响
7. 拔掉 Yarn 缓存目录（改名模拟不存在）时，A1 卡片显示「未发现」而不是 0 或报错

---

## 15. 与上层文档的关系

| 上游 | 本方案处理 |
|---|---|
| 功能方案 §4 模块 A（A1/A2/A3/A7） | §1 范围 + §5 扫描策略（A7 增量降级为每周全量，D-K5） |
| 功能方案 §5 A1 详设 | §6 分类器 + §9.2 文案照抄 |
| 功能方案 §7 安全分级 + §7.2 六条护栏 | §7 清理层（六条全落，护栏实现到代码层） |
| 功能方案 §10 M0 #2/#4 | T-301 / T-303 关闭，验收记录进 m1-disk-acceptance.md |
| 技术方案 §5.3 并行扫描 / §5.4 分类器 / §5.5 安全删除 | §5 / §6 / §7，依赖 `ignore` `rayon` `trash` 首次引入 |
| 实测档案 reclaim-scan.sh / reclaim-scan-report.md | §2 基线 + §5.1 白名单清单来源 |

---

## 16. 实施偏差记录（2026-10-26，按实测回扫）

方案确认后实施过程中被实测推翻或调整的点，逐条留痕，避免"文档写的和代码跑的不一样"。

| # | 方案原定 | 实际落地 | 原因 |
|---|---|---|---|
| 1 | **D-K8** 体积用 `metadata.len()`（逻辑大小） | **改为 `st_blocks × 512`（物理块）** | Docker.raw 是稀疏文件：按 `len()` 累计出 **1099 GB** 的假数字。物理块口径下同机实测 3.17 GB，与 `du` 一致 |
| 2 | §5 扫描依赖 `ignore` + `rayon` 并行 | **自建工作队列线程池，未引入这两个 crate**；只新增 `trash` | 定点白名单目录不需要 gitignore 语义；`ignore` 的 walker 在 pnpm store（44 万文件）上不解决单目录退化问题。首版按"首层分叉"并行，遇 pnpm store 只有 3 个顶层目录 → 退化为单线程 30 s；改工作队列后并发 12 → 全量 15.6 s |
| 3 | §4 数据模型 `CacheGroup` 无「可清理」标记 | **新增 `cleanable: bool`** | `node-modules` 是 55 个目录的聚合项，`root` 记的是 home。原实现把它当成整项交给 `preview`，生成了一条 `path=/Users/xuyi`、17 GB 的待删项——白名单能拦住执行，但预览就先吓到用户。聚合项现在置 `false`，只统计不提供一键清理 |
| 4 | §14 验收 #2「A1 抽样 100 条人工核对」 | 抽样改为**含平台词 50 条 + 不含 50 条** | 死重只占全库 4%，纯等距抽样 100 条只命中 4 条死重，既查不出误杀也查不出漏判。清单见 `docs/research/m1-a1-sample.md` |
