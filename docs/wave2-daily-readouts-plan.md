# Wave 2 详细方案：从一次性清到日常读数（2027-03 ～ 2027-05）

> 版本 v1.0 ｜ 2026-11-15 ｜ 上游：`docs/prune-roadmap-2027-dev-first.md` §8.2（D-07 ～ D-12，37 人日）
> 基线：**v0.11.0**。所有「现状」均经代码核验，标注到文件与符号。
> 标注：【事实】可追到代码 ｜【估算】有依据未验证 ｜【假设】需数据校准

---

## 0. 结论先行

1. **Wave 2 的本质不是加四个功能，是把「一次性扫描」变成「有时间轴的状态」。** 现有全部读数都是**当下快照**：端口是此刻的监听表、项目体检是此刻的 node_modules、内存是此刻的 footprint。开发者要的是「30 天涨了多少」和「现在跟昨天比变了吗」——这需要一张表、一条时间轴、一套缺失值纪律。

2. 【事实】**端口能力已经完整，缺的只是三个入口。** `PortSnapshot` 有 `coverage` / `timeWaitPorts` / `service` / `connsByPid`；`island.rs` 已有 `Conflict`（同端口多持有者）判据并在 892/906 行有测试。因此 D-07 只做**暴露**，不新增探测——4 人日里 3 天是 UI，1 天是深链。

3. 【事实】**`prune mem --watch` 已经存在**（`main.rs` `Command::Mem { watch }`），与路线图里的 `prune watch` 命名冲突。本文裁决：顶层 `watch` 做统一守护，`mem --watch` 保留为 deprecated 别名，2027-09 移除。**这条在路线图里没写，属于实施期必须补的决策。**

4. **`prune.toml` 的安全边界必须是「只能减，不能增」。** 项目配置只允许：跳过项目、忽略条目、调阈值、声明关注端口。**永远不允许新增可删除路径**——否则「白名单兜底」这条护城河在用户配置文件里被绕过，一次误配就抵掉 68 条登记的全部可信度。

5. 【事实】**CLI 读不到 GUI 的 settings。** `settings.rs` 的 `path()` 签名是 `fn path(app: &AppHandle)`，CLI 没有 `AppHandle`。D-10 的真实成本有 2 人日花在这层解耦上，不在事件循环上——路线图把这项估成 8 人日是对的，但成本构成需要先讲清，否则会误判为「逻辑很复杂」。

---

## 1. 范围与不做

| 范围 | 内容 |
|---|---|
| **做** | D-07 端口前置 · D-08 项目账本 · D-09 `prune.toml` · D-10 `prune watch` · D-11 MCP 8→12 + 契约冻结 · D-12 编辑器配方包 |
| **不做** | 构建产物账本（口径不可靠，见 §5.4）· 后台 daemon / 开机项 · 团队共享配置 · 自动修复 · 端口进程结束 |
| **依赖** | Wave 1 的 JSON 契约 v1（`schemaVersion` + 退出码表）必须先落地；未落地则 D-08/D-11 的 `--json` 一律挂 `"schemaVersion": 1` 占位，不得先发 |

---

## 2. 现状核验（代码级）

| 能力 | 现状 | 位置 | Wave 2 需要的改动 |
|---|---|---|---|
| 端口快照 | 完整：`entries[] / coverage / timeWaitPorts / connsByPid / service` | `net/mod.rs:200 PortSnapshot` | 零改动，仅复用 |
| 端口冲突判据 | 已有：同端口 ≥2 持有者 → `Conflict`（优先于 `NewListen`） | `island.rs:405-520` | 抽成公共函数供 CLI/API 复用 |
| 端口 UI | 第六个 tab，`PortList.tsx` | `src/components/PortList.tsx` | 加锚点定位 + 总览卡 |
| 项目体检 | `ProjectHealth`：体积/依赖年龄/活动年龄/锁文件/结论 | `disk/project.rs:45` | 加时间轴表，不改判定 |
| 项目 UI | `ProjectHealthCard.tsx` | `src/components/` | 加 30 天火花线 |
| 本地存储 | 6 张表（mem_process / net_daily / app_hourly / cut_log / scan_log / meta） | `db.rs:67 SCHEMA` | **新增 1 张 `project_day`** |
| 设置读取 | `settings::get_*` 全部要求 `&AppHandle` | `src-tauri/src/settings.rs:56-84` | **新增 core 层只读 config** |
| 事件判据 | CPU/Mem 阈值组 + 滞回 + 限流，全部可配 | `island.rs:20-390` | 直接复用，不重写 |
| MCP | 8 工具，只读，执行类按名拒绝 | `mcp.rs:98 tool_defs` | 8 → 12 + 冻结快照 |
| CLI | 7 子命令，手写 arg（明确不引 clap，D-C1） | `main.rs:26 Command` | 新增 `watch` / `projects` |

---

## 3. 全局约束（六个 D 共用，任一不得破）

| # | 约束 | 落地方式 |
|---|---|---|
| C1 | **读不到 ≠ 0** | 所有新增字段用 `Option`，`null` 序列化；UI 显示「不可读 + 原因」 |
| C2 | **缺失日不补 0** | 账本读回只返回有行的日期，前端画断点（沿用 `net_recent` 纪律） |
| C3 | **零遥测** | 新表只写本地库；新增计数只进 `activation_report` 同款导出包 |
| C4 | **不新增执行面** | 12 个 MCP 工具全只读；`prune.toml` 不能新增删除路径 |
| C5 | **不装后台** | `watch` 是前台进程；无 launchd / systemd / 开机项 / 守护重启 |

---

## 4. D-07 端口前置（4 人日）

### 4.1 目标与故事

```
作为在本地起服务的开发者，
我想要在不离开当前窗口的情况下知道「3000 现在是谁的」，
以便不用切到第六个 tab、也不用拼 lsof 命令。
```

### 4.2 方案：三处暴露，零新增探测

**① 总览卡「本地服务」（1.5d）**

复用 `port_sampler` 已有快照（不新增扫描周期），新增一张只读卡：

```
┌ 本地服务 ───────────────────────┐
│ 关注层监听 6 个 · 冲突 1        │
│ ⚠ 3000  next-server · ~/work/a  │
│   5173  vite · ~/work/b         │
│ 其余 4 个 ›                     │
└─────────────────────────────────┘
```

点击任意行 → 跳端口 tab 并**高亮该行**（`setActiveTab('ports'); portStore.focus(3000)`）。

**② 菜单栏面板加一栏（1d）**

在现有 380×432 面板「可回收」段之后插入「本地服务」三条：只在**存在冲突**或**关注端口被占**时显示，否则整栏隐藏——菜单栏是稀缺空间，常驻空栏会被读成噪音。

**③ 事件流深链（1d）**

`island` 的 `PortConflict` 点击行为从「激活主窗口」改为「激活 + 定位」：新增跳转参数 `{ tab: "ports", focus: port }`。这是 D-07 唯一涉及跨窗口传参的改动，需要在 `island → 主窗口` 的消息契约上加一个可选字段（向后兼容，缺省即旧行为）。

**④ `prune port --conflicts`（0.5d）**

复用 `island` 的冲突判据（抽出 `pub fn conflicts(snapshot: &PortSnapshot) -> Vec<PortConflict>` 到 `net/`）：

```json
{ "schemaVersion": 1, "at": 1763..., "conflicts": [
  { "port": 8080, "listeners": 2,
    "holders": [{ "pid": 8812, "comm": "node", "project": "/Users/x/work/api" },
                { "pid": 9043, "comm": "node", "project": null }] } ] }
```

### 4.3 边界与异常

| 情况 | 行为 |
|---|---|
| 双栈同 pid（IPv4+IPv6 合并后一个 pid） | **不算冲突**（`listeners.len() > 1` 才判，与 island 一致） |
| 无 lsof / 权限不足 | 卡片显示「不可读 + coverage.note」，不显示 0 |
| 快照为空（首次启动未采） | 显示骨架 + 「正在采集」，不留空栏 |
| 关注端口在用户配置里但已空闲 | 显示「空闲」而非隐藏（用户主动关注的要能看到状态变化） |

### 4.4 验收

- [ ] 从菜单栏到「看到 pid + 项目目录」≤ 2 次点击
- [ ] 冲突端口在总览、菜单栏、端口页三处读数一致（同一份快照，不各算一遍）
- [ ] `prune port --conflicts` 与 GUI 冲突列表逐条一致（同一函数，有单测）
- [ ] 无 lsof 环境下不崩、不显示 0

---

## 5. D-08 项目账本（9 人日）

### 5.1 目标

```
作为同时维护 6 个项目的开发者，
我想要知道「哪个项目的依赖在过去 30 天悄悄涨了 4 GB」，
以便在磁盘红之前处理它，而不是等它变成 60 GB 才发现。
```

### 5.2 数据模型

新增一张表（`db.rs` SCHEMA 追加，随 `open()` 自动建）：

```sql
CREATE TABLE IF NOT EXISTS project_day (
    day            TEXT    NOT NULL,   -- 本地日期 'YYYY-MM-DD'
    root           TEXT    NOT NULL,   -- 项目根绝对路径（主键之一）
    name           TEXT    NOT NULL,   -- 展示名（相对 home，重命名不影响 root）
    deps_bytes     INTEGER NOT NULL,   -- node_modules 合计（含嵌套，已去重）
    deps_age_days  INTEGER NOT NULL,   -- 依赖上次安装距当天
    activity_days  INTEGER,            -- 项目最后活动距今；NULL = 查不到标记文件
    lockfiles      TEXT    NOT NULL,   -- JSON 数组 ['npm','pnpm']
    verdict        TEXT    NOT NULL,   -- healthy|stale|heavy|zombie
    updated_at     INTEGER NOT NULL,
    PRIMARY KEY (day, root)
);
CREATE INDEX IF NOT EXISTS idx_project_day_root ON project_day (root, day);
```

**为什么是天粒度而不是每次扫描落一行**：每小时扫一次 × 30 个项目 × 90 天 = 6.5 万行，而 90 天视图只要 90 个点。天粒度 30 × 90 = **2,700 行**，读一条 SQL 聚合完事——与 `app_daily_get` 把 720 个小时桶压成 30 根柱同一个理由。

**写入时机**：每轮**全量扫描**完成后 UPSERT 当天一行（同一天多次扫描覆盖写，取最后一次——它是当天最完整的事实）。增量扫描不写：增量只更新部分目录，写进去会把体积记小，是假事实。

### 5.3 读回契约

```rust
pub struct ProjectTrend {
    pub root: String,
    pub name: String,
    pub points: Vec<ProjectPoint>,   // 只含有记录的天，升序
    pub first: Option<ProjectPoint>, // 窗口内最早一天；None = 窗口内无数据
    pub last: Option<ProjectPoint>,
    pub delta_bytes: Option<i64>,    // last - first；负数为缩水
    pub peak_bytes: Option<u64>,
    pub samples: u32,                // 有记录天数（UI 用它判「这窗口够不够」）
}
```

**口径纪律（写死，测试固化）**：
- `delta_bytes` 只在 `samples >= 2` 时给值，否则 `null` + UI 文案「数据不足（仅 N 天）」
- 首末两点间隔 < 7 天时，标签写「N 天」，**不得**写「30 天涨了 X」
- 项目被删除/改名：`root` 为主键，旧 root 自然不再写入，趋势线右端断开（不补 0、不插值）

### 5.4 为什么不记构建产物（本期明确不做）

`target/`、`dist/`、`.next/` 的「体积」在扫描口径里不可靠：`target/debug` 的增量产物与可回收产物混在一起，`disk_scan` 也不按项目归属它们。**记一个自己都无法解释的数字，比不记更糟**（README「给不出证据的结论不做」）。
替代：账本只记依赖（口径单一、可核验），构建产物留到有真实需求时按白名单条目接入。

### 5.5 交付拆分

| 子项 | 人日 | 内容 |
|---|---|---|
| 5a 表与读写 | 2 | SCHEMA + `project_day_upsert` / `project_trend` / `project_prune_before`（保留 400 天）+ 6 项单测 |
| 5b 采样接线 | 2 | 全量扫描完成钩子（`scan.rs`）→ 写当天；失败不阻塞扫描主流程 |
| 5c CLI | 2 | `prune projects [--json] [--days N]`，复用 `ProjectHealth` + 趋势 |
| 5d UI | 3 | `ProjectHealthCard` 加 30 天火花线 + `30 天 +X GB` 标签 + 趋势弹窗（复用 Sites 弹窗模式） |

### 5.6 验收

- [ ] 连续两天各扫一次，`prune projects --json` 给出 2 个点且 `delta_bytes` 非 null
- [ ] 只有 1 天数据时 `delta_bytes` 为 null，UI 显示「数据不足（仅 1 天）」，**不显示 0**
- [ ] 删掉一个项目的 node_modules 后重扫：当天该 root 无新行，趋势右端断开，不补 0
- [ ] 90 天窗口下 SQL 耗时 < 30 ms（2,700 行量级）
- [ ] 保留期清理只删早于 cutoff 的行，边界当天保留（同 `cut_prune_before` 测试写法）

---

## 6. D-09 `prune.toml`（5 人日）

### 6.1 安全边界（先定，再写代码）

**允许（只能让 Prune 少看）：**

| 键 | 语义 | 上限 |
|---|---|---|
| `ignore.projects` | 跳过这些项目（不扫、不进账本） | ≤ 200 条，绝对路径或 `~` 开头 |
| `ignore.paths` | 从清理候选中排除这些路径 | 只能是**已登记白名单条目的子路径** |
| `thresholds.deps_warn_gb` | 依赖「偏胖」阈值（覆盖 1.5 GiB 默认） | 0.1 – 100 |
| `thresholds.stale_days` | 「久未重装」天数（覆盖 60 天默认） | 7 – 3650 |
| `watch.ports` | 本项目的关注端口（覆盖内置关注层） | ≤ 32 个，1–65535 |

**禁止（解析期直接拒绝，不静默忽略）：**

| 禁止项 | 拒绝理由（错误信息原文） |
|---|---|
| 任何新增可删除路径的键（如 `extra_paths` / `clean`） | 「项目配置只能减少范围，不能扩大；可清理项由内置白名单决定」 |
| `root` / `home` 等路径重定向 | 「不接受路径重定向」 |
| `reclaim` / `auto` / `clean` 段 | 「Prune 不接受任何自动执行配置」 |

这条不是洁癖：白名单的信任来自「路径是数据文件 + 每条有断言测试」。一旦允许用户配置新增路径，**一次写错就是一次数据丢失**，而 blame 会落在 Prune 上。

### 6.2 解析实现：手写极简子集，不引 `toml`

【事实】`toml` 已在 `Cargo.lock`（0.8.2 / 1.1.8，tauri 传递依赖），但让 `prune-core` 依赖它会把该 crate 拉进 **CLI 二进制的编译单元**——而 CLI 的纪律是「手写 arg 不引 clap，因为拖慢编译且无必要」（`main.rs` 头部 D-C1）。

同一纪律下，`prune.toml` 只支持：

```toml
[ignore]
projects = ["~/work/legacy-api"]
paths = ["~/Library/Caches/yarn/npm-archive"]

[thresholds]
deps_warn_gb = 2.0
stale_days = 90

[watch]
ports = [3000, 5173, 8080]
```

即：`[section]` + `key = value`（字符串 / 浮点 / 整数 / 字符串数组），**不支持嵌套表、不支持多行字符串、不支持日期**。约 120 行解析器 + 20 项单测，零新依赖。超出子集的语法一律返回「第 N 行不支持该语法」——**明确拒绝好过悄悄解析错**。

### 6.3 合并与冲突规则

| 情况 | 行为 |
|---|---|
| 项目无 `prune.toml` | 行为与 v0.13 完全一致（零配置即默认） |
| 文件语法错 / 超子集 | 整份忽略 + UI 与 CLI 都要显式提示「`~/work/a/prune.toml` 第 7 行…，已回落默认」 |
| 键合法但值越界 | 该键回落默认 + 提示实际生效值（不整份丢弃） |
| 同一项目多层（monorepo 子包各有 toml） | **只读项目根那一份**，子包配置不生效（多份合并的优先级无法向用户解释） |
| `ignore.paths` 指向白名单外的路径 | 忽略该项 + 提示「不在白名单内，已跳过」 |

### 6.4 UI 与写入

- **Prune 永不写项目文件**（避免 `git status` 变脏，也避免「工具悄悄改我仓库」的信任问题）。
- 设置页给「生成 `prune.toml` 模板」按钮 → **复制到剪贴板**，由用户自己落盘。
- 项目体检卡上，被 `ignore` 的项目显示灰色「已按项目配置跳过」+ 一键定位到该配置。

### 6.5 验收

- [ ] 非法段 `[clean]` → 整份拒绝 + 明确错误文案，且**清理候选列表与无配置时逐条一致**（负面测试）
- [ ] `ignore.paths` 指向白名单外 → 该条不生效，其余配置仍生效
- [ ] 语法错的 toml 不影响扫描/清理主流程（不 panic、不阻塞）
- [ ] 生成模板不会写任何文件（断言目录 mtime 不变）
- [ ] 120 行解析器 20 项单测覆盖：空文件 / 注释 / 引号 / 数组换行 / 重复键 / 越界数值 / 不支持语法行号

---

## 7. D-10 `prune watch`（8 人日）

### 7.1 命名裁决（必须先定）

【事实】`prune mem --watch` 已存在（`main.rs` `Command::Mem { watch: Option<u64> }`，每 60 s 输出泄漏判定）。路线图的顶层 `prune watch` 与它语义重叠。

**裁决**：
- 顶层 `prune watch` 是唯一守护入口，覆盖 cpu / mem / port / disk 四类；
- `prune mem --watch` 保留为 **deprecated 别名**：仍工作，但向 stderr 打一行 `prune mem --watch 已并入 prune watch --only mem（2027-09 移除）`；
- 2027-09 的 1.0 收口时删除别名（写进 CHANGELOG 的 Breaking 段）。

不做这一步的后果：两个 `--watch` 长期并存，帮助文本互相打脸，issue 里会出现「为什么 mem --watch 不报端口」。

### 7.2 命令面

```
prune watch [--interval 10] [--only cpu,mem,port,disk] [--json] [--once] [--fail-on warn|none]
```

| 参数 | 语义 | 边界 |
|---|---|---|
| `--interval` | 采样间隔秒 | 钳制 2–300，越界打提示并按边界值跑 |
| `--only` | 只开这几类 | 未知类别 → 退出码 2 + 列出合法值 |
| `--json` | 每事件一行 JSON（JSONL），**不混人读文本** | 与 `--once` 可组合 |
| `--once` | 跑一轮、输出、退出（CI 用） | 不与 `--json` 互斥 |
| `--fail-on warn` | 任一类达 warn 及以上 → 退出码 10 | 默认 `none`，CI 才开 |

### 7.3 阈值与判据：复用，不重写

`island.rs` 已有 `CpuMonitor` / `MemMonitor`（窗口占比 + 滞回 + 冷却 + 限流）与 `Conflict` 判据。**watch 直接调用同一批结构**，不复制逻辑：

```
prune-core/src/island.rs  ←── prune-cli/src/watch.rs
        ↑                          ↑
   src-tauri/src/island.rs ────────┘   （GUI 与 CLI 共用同一判据）
```

漂移防线：新增一条测试，对同一组采样输入分别喂 GUI 与 CLI 的判据，断言输出事件序列逐条相等。**这是本 D 唯一必须新写的「架构级」测试**——两处各写一份阈值逻辑，半年后必然对不上。

### 7.4 事件契约

```json
{"schemaVersion":1,"ts":1763000000,"kind":"port.conflict","severity":"warn",
 "subject":{"port":8080,"pids":[8812,9043]},
 "evidence":{"listeners":2,"projects":["/Users/x/work/api",null]},
 "threshold":{"rule":"listeners>1"}}
```

四类事件与 severity 映射：

| kind | warn 触发 | critical 触发 |
|---|---|---|
| `cpu.sustained` | 窗口内 ≥ 阈值占比（默认 90%） | 不单列（CPU 无 critical，避免噪音） |
| `mem.swap` | swap 占用率 ≥ 80% 持续 60 s | 内核压力 Critical |
| `port.conflict` | 同端口 ≥2 持有者 | — |
| `disk.watermark` | 卷可用 < 15% | 卷可用 < 5% |

`disk.watermark` 是**新判据**（island 现有四类里没有磁盘水位），需要新增纯函数 + 单测；其余三类复用。

### 7.5 状态机

```
        ┌──── 采样达标 ────┐
  idle ─┤                  ├→ firing ──冷却期(默认 300s)──→ idle
        └─ 不达标 → idle    │
                            └→ 冷却期内不重复报（同 kind+subject）
```

滞回与冷却参数**全部来自配置**，代码里不写死（沿用 `island.rs` 头部第 9 行纪律）。

### 7.6 settings 解耦（真实成本在这里，2 人日）

【事实】`settings.rs::path(app: &AppHandle)`、`get_str(app, key)` 全部绑定 Tauri。CLI 没有 `AppHandle`。

方案：新增 `prune-core/src/config.rs`（无 Tauri 依赖）：

```rust
pub fn config_dir() -> Option<PathBuf>;       // ~/Library/Application Support/com.xuyi.prune（macOS）
pub fn read_settings() -> Option<serde_json::Value>;
pub fn get_f64(key: &str) -> Option<f64>;     // 点号路径 "island.swap_pct"
pub fn get_u64(key: &str) -> Option<u64>;
pub fn get_bool(key: &str) -> Option<bool>;
```

GUI 侧 `settings.rs` 改为**薄封装**：`path(app)` 保留（它要处理 legacy 路径迁移），读取值一律转调 core。这样 CLI 与 GUI 读同一份文件、同一套默认值——否则「GUI 里调了阈值，CLI 不认」会成为最难查的一类 issue。

### 7.7 明确不做

- 不装 launchd / systemd / 开机项 / 守护重启（C5）
- 不做「守护 + 自动清理」联动（违背「人才能删」）
- 不写日志文件（stdout / stderr 即出口，重定向由用户决定）
- 不做 `--daemon` / `--detach`（后台进程会变成不可见的常驻，与「按需」定位冲突）

### 7.8 验收

- [ ] Ctrl-C 后 0 残留进程、0 新增文件（`ls -la` 前后 diff 为空）
- [ ] `--once --json` 输出恰好 1 行合法 JSON，`jq .` 可解析
- [ ] `--fail-on warn` 在有告警时退出码 = 10；无告警 = 0；用法错 = 2
- [ ] 同一组采样输入，GUI 判据与 CLI 判据输出事件序列逐条相等（漂移测试）
- [ ] `--interval 0` 被钳到 2 并提示，不进入忙等
- [ ] GUI 设置里改 swap 阈值后 CLI 立即生效（同一文件同一默认值）

---

## 8. D-11 MCP 8 → 12 + 契约冻结（6 人日）

### 8.1 新增四个工具（全部只读）

| 工具 | 回答什么 | 数据来源 | 副作用 | 耗时【估算】 |
|---|---|---|---|---|
| `project_deps` | 项目依赖账本 + 趋势 | `project_day` + 当期扫描 | **无**（只读库，不触发扫描） | < 50 ms |
| `port_conflicts` | 冲突与暴露面摘要 | `PortSnapshot` | 无（读缓存快照） | < 100 ms |
| `swap_report` | swap 明细 + 换出受害者榜 | 现有 swap/mem 采集 | 无 | 1–2 s |
| `net_sites` | 站点五阶段结果 | `net/sites.rs` | **会发起真实网络请求**（≤20 站点、单轮 ≤20 MB） | 3–8 s |

`net_sites` 是唯一有外部副作用的工具，描述里必须写死这句话：「本工具会向用户配置的站点发起真实 HTTPS 请求，单轮流量上限 20 MB；不传则使用默认 10 站」。**副作用必须在 schema 里可见，不能靠 README 解释。**

### 8.2 三个明确不加（避免工具面膨胀）

| 候选 | 不加的理由 |
|---|---|
| `watch_once`（跑一轮守护） | 守护是「持续」语义，单次调用应走 CLI `--once`；做成工具会让 Agent 反复起进程 |
| `reclaim_preview`（清理预览） | 预览与执行只差一个参数，一旦存在就是「Agent 能删」的解释缺口 |
| `disk_scan_deep`（子目录占比） | 已有 `disk_scan` 的 keys 参数覆盖，再加一个是重复面 |

### 8.3 契约冻结（本 D 的核心产出，不是附赠）

【事实】`check-docs-honesty.mjs` 目前做三方对账：`run_tool` 分发臂 ↔ `tool_defs()` ↔ README 工具清单（数量与名字）。冻结需要再加一层：

1. 生成 `crates/prune-cli/mcp-tools.v1.json`（名字 + description 哈希 + inputSchema 规范化 JSON）
2. CI 断言：实际 `tool_defs()` 序列化后与该文件**逐字段相等**
3. 变更流程：改工具面必须同批提交该快照文件，否则门禁红——**强制「改契约」成为一个显式动作**
4. `schemaVersion` 从 Wave 1 的 CLI 契约延伸到 MCP：工具结果外层统一 `{"schemaVersion":1,"data":...}`

理由：开发者写的是脚本和 Agent 提示词，**契约稳定比工具多一个值钱**（路线图 RICE 第 8 项，53 分）。冻结之后每次加工具都是「一次显式的版本事件」。

### 8.4 验收

- [ ] 12 个工具全部只读；`run_tool` 对执行类动词仍 `Err(` 拒绝（门禁已有断言）
- [ ] `mcp-tools.v1.json` 与代码逐字段相等；改一个 description 不更新快照即红（负向实测）
- [ ] `net_sites` 描述含「真实网络请求 + 20 MB 上限」字样（门禁字符串断言）
- [ ] 12 个工具的 README 清单、`tool_defs()`、分发臂三方一致
- [ ] 任一工具在数据不可读时返回 `null` + `reason`，不返回 0

---

## 9. D-12 编辑器集成：从 5 人日降为 1.5 人日（路线图偏差说明）

### 9.1 为什么改

路线图原写「VS Code 任务 + 状态栏只读读数（5 人日）」。核完后判断：**状态栏读数必须有扩展宿主**，而扩展要发布就得面对 Marketplace 账号、打包、版本跟进、以及「扩展里再实现一遍读数」的重复逻辑——成本 5 人日起，收益是 RICE 表里最低的 17 分。

**改为「编辑器配方包」**：不写扩展，只提供三份可直接粘贴的配置，覆盖 80% 的实际用法（在编辑器里一键跑 `prune`，看输出面板）。

### 9.2 交付内容（1.5 人日）

`docs/editors/` 下三份：

| 文件 | 内容 |
|---|---|
| `vscode.md` | `.vscode/tasks.json` 片段（3 个任务：`port` / `doctor --json` / `projects --json`，输出到面板）+ `settings.json` 的 `terminal.integrated.env` 示例 |
| `jetbrains.md` | External Tools 三条配置（参数与 vscode 一致，口径统一） |
| `zed.md` | tasks 片段 |

只写文档，Prune 不写任何 `.vscode/` 文件（同 `prune.toml` 的「不碰用户仓库」纪律）。

### 9.3 验收

- [ ] 三份片段在各自编辑器里逐字粘贴可用（真机各走一遍）
- [ ] 片段里的命令与本版 CLI 逐字一致（门禁：文档命令 vs `main.rs` usage 字符串比对）
- [ ] 不含任何写死的绝对路径 / 用户名

---

## 10. 数据与契约汇总

### 10.1 存储增量

| 表 | 行数量级 | 保留 | 清理入口 |
|---|---|---|---|
| `project_day`（新） | 30 项目 × 400 天 ≈ 12,000 | 400 天 | `project_prune_before(day)` |

WAL 影响：新增写入是每天每项目 1 行，可忽略；但**首次写入会在同一天产生 N 行 UPSERT**（30 行/天），无膨胀风险。

### 10.2 CLI 契约（Wave 1 的 v1 契约延伸）

| 命令 | 新增/变更 | 退出码 |
|---|---|---|
| `prune port --conflicts` | 新增 flag | 0 / 2 / 3 / 4 |
| `prune projects [--days N] [--json]` | 新增命令 | 0 / 2 / 3 |
| `prune watch [...]` | 新增命令 | 0 / 2 / 3 / 4 / **10** |
| `prune mem --watch` | 降级为别名 | 同上（10 不适用于别名，保持原行为） |

退出码 10 的引入是**向后兼容**的（此前无命令返回 10），但必须写进 `docs/cli-reference.md` 的退出码表，并加一条测试断言「非 `--fail-on` 时永不返回 10」。

### 10.3 新增 i18n 键（中英同批，`check-i18n` 门禁）

约 28 条：`overview.ports.*`（6）· `ports.focus.*`（3）· `projects.trend.*`（9）· `projects.config.*`（6）· `watch.*`（不涉 GUI，仅 CLI 文本不入 i18n 目录，见下）

**CLI 文本不做 i18n**（现状即如此：CLI 输出中文单语）。这是既有约定，本期不改——但要在 `docs/cli-reference.md` 里明写，避免被当成缺陷报上来。

---

## 11. 排期（13 周窗口，可用 39 人日，排 37 + 2 缓冲）

| 周 | 日期 | 内容 | 人日 |
|---|---|---|---|
| W1 | 03-01 ~ 03-07 | D-07 端口前置（总览卡 → 菜单栏 → 深链 → `--conflicts`） | 4 |
| W2 | 03-08 ~ 03-14 | D-08a 表与读写（SCHEMA + 3 方法 + 6 单测） | 2 |
| W3 | 03-15 ~ 03-21 | D-08b 采样接线 + D-08c CLI | 4 |
| W4 | 03-22 ~ 03-28 | D-08d UI（火花线 + 弹窗） | 3 |
| W5 | 03-29 ~ 04-04 | D-09a 解析器（120 行 + 20 单测） | 3 |
| W6 | 04-05 ~ 04-11 | D-09b 合并规则 + UI 提示 + 模板复制 | 2 |
| W7 | 04-12 ~ 04-18 | D-10a config 解耦（core/config.rs + GUI 薄封装） | 2 |
| W8 | 04-19 ~ 04-25 | D-10b watch 主体（四类事件 + JSONL + 退出码） | 3 |
| W9 | 04-26 ~ 05-02 | D-10c 漂移测试 + `mem --watch` 别名 + 文档 | 3 |
| W10 | 05-03 ~ 05-09 | D-11a 四个新工具 | 3 |
| W11 | 05-10 ~ 05-16 | D-11b 契约快照 + 门禁扩展 + README 三方对账 | 3 |
| W12 | 05-17 ~ 05-23 | D-12 配方包（1.5）+ 文档与门禁（3.5） | 5 |
| W13 | 05-24 ~ 05-31 | 缓冲 / 真机走查 / `0.16.0` 收口 | 2 |

**版本切分**：`0.14.0`（D-07 + D-08）→ `0.15.0`（D-09 + D-10）→ `0.16.0`（D-11 + D-12）。

**裁单顺序**（沿用路线图 §8.4，触发即执行）：D-12 → D-11 的第四个工具（`net_sites`）→ D-08d 的趋势弹窗（降级为卡片内联数字）。

---

## 12. 验收清单（合并判定，出稿前逐条过）

**结构**
- [ ] 六个 D 全部有：目标 / 方案 / 边界 / 验收 / 人日
- [ ] 每条验收可被测试用例直接引用（可执行、有期望值）
- [ ] 三个版本各自可独立发布（无「半年不可发布」的长分支）

**口径**
- [ ] 新增字段全部 `Option`，不可读返回 `null` 不返回 0
- [ ] 缺失日不补 0（账本、watch 事件均适用）
- [ ] `delta_bytes` 在 `samples < 2` 时为 `null`
- [ ] 所有阈值出现在配置或常量表，代码里无魔数

**安全**
- [ ] `prune.toml` 无法新增可删除路径（负面测试固化）
- [ ] 12 个 MCP 工具全只读；执行类动词仍按名拒绝
- [ ] `watch` 无后台驻留、无开机项、Ctrl-C 无残留
- [ ] Prune 不写用户仓库文件（`prune.toml` / `.vscode` 均只读）

**兼容**
- [ ] `mem --watch` 仍可用（打 deprecation 提示）
- [ ] 退出码 10 只在 `--fail-on` 下出现
- [ ] `mcp-tools.v1.json` 快照与代码逐字段相等

---

## 13. 风险登记

| # | 风险 | 概率 | 影响 | 值 | 应对 |
|---|---|---|---|---|---|
| W2-1 | **判据漂移**：GUI 与 CLI 各写一份阈值逻辑，半年后对不上 | 4 | 4 | **16 高** | §7.3 漂移测试（同输入同输出）；判据只允许在 `island.rs` 存在一份 |
| W2-2 | `prune.toml` 被用户当成「配置清理范围」的入口，误配致误删 | 2 | 5 | 10 中 | 只减不增（§6.1）；非法段整份拒绝；模板里写死「不能新增路径」注释 |
| W2-3 | 账本采样拖慢全量扫描 | 3 | 3 | 9 中 | 写入放扫描完成之后、单事务、失败不影响主流程；实测 < 50 ms |
| W2-4 | settings 解耦引入两处默认值不一致（GUI 30% vs CLI 80%） | 3 | 4 | 12 中 | 默认值只在 `core/config.rs` 定义一次；GUI 转调；加一致性测试 |
| W2-5 | `net_sites` 被 Agent 高频调用，产生真实流量 | 2 | 3 | 6 低 | 描述写死副作用；单轮上限 20 MB；不做缓存前的自动重试 |
| W2-6 | 端口深链在窗口未激活时丢失 focus 参数 | 3 | 2 | 6 低 | 跳转参数入 store 而非 URL；激活后再消费一次并清空 |
| W2-7 | `project_day` 的 root 含空格 / 中文路径 | 3 | 2 | 6 低 | 主键用 TEXT 原样存；测试覆盖含空格与 emoji 路径 |

---

## 14. 明确不做（本波）

| 项 | 理由 |
|---|---|
| 构建产物账本（`target/` `dist/` `.next/`） | 扫描口径无法把「可回收」与「增量产物」分开，记了也解释不清（§5.4） |
| 端口进程结束 / 一键 kill | 违背「不替人做主」；kill 是执行面，不能出现在读数工具里 |
| 自动清理联动（watch 触发清理） | 同上，且「Agent 能看人才能删」会被自己破掉 |
| VS Code 扩展 | 成本 5d 起 + Marketplace 账号 + 版本跟进，RICE 最低（§9.1） |
| 团队共享配置（提交到仓库的 `prune.shared.toml`） | 无账号体系时无法校验来源，且团队配置的合并优先级无法向单人用户解释 |
| 端口历史时间线 | 需要长期采样监听表，写入量是账本的 10 倍，收益未验证 |
| 后台守护 / 开机自启 / 日志文件 | C5：`watch` 是前台进程，这是定位不是缺陷 |

---

## 15. 待拍板

| 编号 | 决策 | 选项 | 建议 |
|---|---|---|---|
| W2-DP-1 | `prune mem --watch` 是否接受 deprecated 别名方案 | A 别名 + 2027-09 移除 ｜ B 立即移除（Breaking） ｜ C 两个都保留 | **A**。立即移除会打断已有脚本；长期并存会让帮助文本自相矛盾 |
| W2-DP-2 | `net_sites` 是否进 MCP（唯一有外部网络副作用的工具） | A 进（描述写死副作用） ｜ B 不进，只留 CLI | **A**。开发者排障时正是要让 Agent 去看「站点断在哪一步」；B 会让 MCP 缺一角 |
| W2-DP-3 | `prune.toml` 是否允许 `thresholds` 覆盖（而非只允许 ignore） | A 允许（本文方案） ｜ B 只允许 ignore | **A**。阈值是纯偏好、无删除风险；只允许 ignore 会让配置能力过窄，用户转而要求「全局配置」，反而更难管 |
| W2-DP-4 | D-12 是否接受「配方包」替代扩展 | A 接受（省 3.5d） ｜ B 坚持扩展 | **A**。门禁 G2 若显示开发者高频用编辑器场景，再单独立项做扩展 |

---

> 合规声明：本方案为产品设计参考，具体实施请结合公司合规与法务要求确认。文中人日、耗时、行数均为估算，已逐项标注；不得作为对外承诺引用。

---

## 16. 实施记录（2026-11-16，第一批：CLI / MCP / 账本引擎）

### 16.1 已落地

| 交付 | 落地位置 | 状态 |
|---|---|---|
| **D-10 前端置零**（`prune watch`） | `crates/prune-cli/src/watch.rs`（新，约 560 行含测试） | ✅ 四类事件 + JSONL + `--once` + `--fail-on` |
| **DP-1 别名过渡** | `main.rs` dispatch：`mem --watch` → 打 deprecation 后走旧实现 | ✅ 提示语写明 2027-09 移除 |
| **D-11 四个新工具** | `mcp.rs`：`port_conflicts` / `project_deps` / `swap_report` / `net_sites` | ✅ 8 → 12，只读断言仍绿 |
| **D-11 契约冻结** | `crates/prune-cli/mcp-tools.v1.json` + `tool_defs_match_frozen_snapshot` 测试 | ✅ 改描述不改快照即红 |
| **D-08 引擎** | `db.rs`：`project_day` 表 + upsert/recent/prune；`disk/project.rs`：`build_trends` / `record_ledger` / `prune_ledger` | ✅ 天粒度、缺失日不补 0、`samples<2` 时 `deltaBytes=null` |
| **D-08 写入端** | GUI `scan.rs`（仅全量）+ CLI `scan`（全量） | ✅ 增量扫描不写（没看全 ≠ 变小） |
| **D-08 人读视图** | `main.rs`：`readout_order` / `readout_lines`（按 \|变化\| 排序、20 行上限、省略行明确计数） | ✅ 与 `--json` 全量并存（见 §16.2 偏差 4） |
| **D-07 CLI 侧** | `port --conflicts` + `net::ports::conflicts()` 单一判据 | ✅ 与 island 同一口径（`owner_count()>1`） |
| **D-10 配置解耦** | `prune-core/src/config.rs`（无 Tauri 读 settings.json，含 legacy 补键） | ✅ GUI 改阈值 CLI 立即生效（真机验证：读到 `cpu_pct=60` / `swap_pct=50`） |
| 磁盘水位判据 | `island.rs`：`DiskThresholds` / `DiskSeverity` / `DiskMonitor`（滞回 + 升级） | ✅ 新增，island 原有四类之外的第一类磁盘事件 |

**未做（下一批）**：D-07 三处前端暴露 · D-08 火花线与趋势弹窗 · D-09 `prune.toml` · D-12 编辑器配方包。

### 16.2 与方案的四点偏差（都需记录，不是笔误）

| # | 方案原文 | 实际 | 理由 |
|---|---|---|---|
| 1 | D-10 判据复用 island，默认间隔未定 | 默认 **10 s**（island 是 2 s） | CLI 场景是「盯一会儿」，10 s 一轮已含一次 lsof（30–40 ms）。CPU 判据的语义随之变为「窗口内 N 个 10 s 样本的达标比例」——比 GUI 粗，但仍是同一判据函数，不另写一套。已在 §7.3 的漂移测试前提下接受 |
| 2 | `probe_site` 留在 `src-tauri`，core 只放纯函数 | **下沉到 `net::sites::probe` / `run`**，GUI 改为转发 | 不下沉就要在 CLI 复制一份 curl 调用，同一台机器同一分钟会量出两种结果。下沉后 GUI/Run 仍保留进度事件与取消（UI 关注点） |
| 3 | D-09 与 D-08 并行 | D-09 未开工 | 它必须接到扫描的忽略规则上才不是死代码；没接就只是「解析器 + 一张无人读的配置」，按 YAGNI 推后 |
| 4 | `prune projects` 按 `build_trends` 的次序（当前体积降序）直接打印 | 人读模式改按 **\|窗口内变化\| 降序**，且最多列 **20** 行 | 体积序回答的是「谁最胖」（体检卡该用的序，保留给 `--json`）；日常读数要回答「这两天谁变了」。昨天 2 GB、今天 0 B 的项目按体积排会沉到第 58 行，而它是唯一的新闻。行数上限只是终端礼貌：省略行会明说「另有 N 个未列出」并指向 `--json`，不静默吞数据 |

### 16.3 真机实测（2026-11-16，macOS 15.7.7）

| 动作 | 结果 |
|---|---|
| `prune watch --once --json` | 单行 JSON；cpu 52.3% / swap 86.3% / 磁盘可用 25.1% / 端口冲突 0；`schemaVersion=1` |
| `prune watch --once --only disk --fail-on warn` | 磁盘 25.1% > 15% → 退出码 0（未误报） |
| `prune port --conflicts` | 「当前没有端口冲突（同一端口 ≥2 个不同 pid 才算）」 |
| `prune mem --watch` | 打出 deprecation 提示后继续工作（旧脚本不炸） |
| `prune scan --json` | 31 条白名单 + 58 个项目写入账本 |
| `prune projects --days 30` | 列出前 20（含 1 个 `… 另有 35 个未列出` 尾行）；全部显示「数据不足（1 天）」——首日如实，不显示 0 |
| `prune projects --days 5 --json` | 58 条全量；`deltaBytes=null`、`points` 只有 1 个点、`peakBytes` 有值 |
| MCP `tools/call project_deps` | 首扫前返回 `status=unavailable` + 可执行指引；扫后返回 58 条趋势 + 「null ≠ 0」说明 |
| MCP `tools/call net_sites` | 11 站实测 11.0 s：ok 3 / slow 2 / stalled 1 / refused 1 / unreachable 4；`customized=true`（吃到自定义站点） |
| MCP `tools/call port_conflicts` | `count=0` + coverage + 「没查到 ≠ 端口空闲」提示 |
| MCP `tools/list` | 12 个工具名与 `mcp-tools.v1.json` 快照逐字一致 |

### 16.4 门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | core **449** / cli **31** / app **49**，0 failed，3 ignored（新增 24 条：账本 4 / 趋势 5 / 冲突 2 / 磁盘水位 4 / 站点执行 2 / watch 3 / 人读排序与截断 3 / 快照 1） |
| `cargo clippy --workspace --all-targets -D warnings` | 0 error |
| `cargo fmt --check` | 无 diff |
| `pnpm check:docs` | MCP **12** 个工具三方对账一致；执行类动词拒绝臂 9 个；排他 sweep 5 份零命中 |
| `mcp-tools.v1.json` | 由生产路径（`tools/list` 实跑）导出，非手工誊写 |

### 16.5 下一批的入口条件

1. **D-07 前端**：`PortSnapshot` 已有 `conflicts()` 可直接消费，前端只需三处接线（总览卡 / 菜单栏行 / island 深链参数）。
2. **D-08 前端**：`ProjectTrend` 已含 `points/first/last/deltaBytes/samples`，UI 只负责「数据不足（N 天）」的文案分支。
3. **D-09**：接扫描忽略规则前，先定「忽略的项目是否仍出现在账本里」——建议：**不进账本**（否则用户会问「为什么忽略的项目还在报表里」）。
