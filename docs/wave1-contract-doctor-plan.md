# Wave 1 详细方案：契约 v1 · `prune doctor` · `prune mcp install` · 文档

> 版本 v1.0 ｜ 2026-11-16 ｜ 上游：`docs/prune-roadmap-2027-dev-first.md` §8.1（D-03 ～ D-06，23 人日）
> 基线：**v0.11.0**。所有「现状」均经代码核验，标注到文件与符号。
> 标注：【事实】可追到代码 ｜【估算】有依据未验证 ｜【假设】需数据校准

---

## 1. 结论摘要

四件事是一条链，不是四个独立特性：

```
D-03 契约 v1 ──► D-04 doctor（第一个「消费者」：结论必须建立在可信读数上）
      │                   │
      └──► D-06 文档 ◄────┘
                │
                ▼
        D-05 mcp install（分发：把「接得进」变成一条命令）
```

三个关键判断（落地前请先确认，它们决定 70% 的工作量）：

1. **契约不是从零设计，而是「补齐 + 冻结」**。现状 8 个 JSON 输出点里 **3 个已有 `schemaVersion`**，其余 5 个是裸结构体直出——D-03 的主体工作量在**统一信封 + 快照门禁**，不在协议设计。
2. **`prune doctor` 的 30 秒预算是硬约束，决定了它不能靠「每次都全盘扫」**。磁盘报告目前没有 CLI 可读的缓存，本方案选择「默认实时扫一次白名单（~15s）+ 其余三域用轻量实时探测」，不新增缓存状态源（决策 W1-DP-3）。
3. **`prune mcp install` 是 Prune 少见的「写用户文件」动作**，必须与 `prune.toml` 的纪律同构：默认 dry-run、只改已知键、写前备份、坏 JSON 拒绝而不是覆盖（决策 W1-DP-4）。

**版本**：`0.13.0`。**排期**：4 周 23 人日（§7）。

---

## 2. 基线事实（逐项核验）

### 2.1 JSON 输出与 `schemaVersion` 现状

| 命令 | 输出位置 | `schemaVersion` | 顶层形态 |
|---|---|---|---|
| `scan --json` | `main.rs:598` | ❌ | `DiskReport` 裸直出（GUI IPC 同一结构） |
| `mem --json` | `main.rs:762` | ❌ | `{physicalTotal, swap, pressure, families, topSwapped}` |
| `port --json` | `main.rs:884` | ❌ | `{port, entries, coverage, timeWait, timeWaitPorts, scanMs}` |
| `port --conflicts --json` | `main.rs:838` | ✅ | `{schemaVersion, at, conflicts, coverage}` |
| `projects --json` | `main.rs:432`（`projects_unavailable`：517） | ✅ | `{schemaVersion, from, days, projects}` |
| `app --json` | `main.rs:1314` | ❌ | `{family, processCount, memoryBytes, …, ports, disk}` |
| `lan --json` | `main.rs:1183`（缓存同体写盘：1173） | ❌ | `LanSnapshot` 裸直出 |
| `watch --once --json` | `watch.rs:364` | ✅ | `{schemaVersion, ts, mode, …}` |
| `watch --json`（事件流） | `watch.rs:198 / 245 / 280 / 325` | ✅ | JSONL 每行一个事件 |

**结论**：契约 v1 已完成约 40%（3/9 输出点）；缺口集中在 `scan` / `mem` / `port`（默认分支）/ `app` / `lan` 五处，且 `lan` 有一个额外复杂度——**缓存文件与 CLI 输出是同一份结构**（1173 写缓存、1183 打印），信封不能直接加在结构体上。

### 2.2 退出码现状

| 码 | usage 承诺（`main.rs:332`） | 代码事实 |
|---|---|---|
| 0 | 正常 | 全部命令 |
| 2 | 用法错 | `main.rs:342`（`parse_args` Err）、`app` 歧义匹配（`main.rs` cmd_app） |
| 3 | **权限不足** | **全仓没有任何 `return 3`**【事实】——文档承诺与代码事实脱节 |
| 4 | 探测失败 | `watch.rs:353`、`watch.rs:498`（两类探测失败） |
| 10 | watch 达 warn（仅 `--fail-on`） | `watch.rs` `fail_on_warn` 分支 |

**3 从未被返回**是本轮必须收口的诚信问题：要么实现它，要么从 usage 删掉（本方案选前者，见 §3.3）。

### 2.3 `prune doctor` 可用的数据源

| 域 | 现有能力 | 成本 | 失败形态 |
|---|---|---|---|
| 磁盘 | `prune_core::disk::scan(&home, progress)`（`main.rs:560` cmd_scan 实测约 15s，31 条白名单） | ~15s | 权限跳过目录记入 notes，不算整体失败 |
| 内存 | `mem::system::read_pressure()` + `mcp::watch_round(20)`（`main.rs:748-752` cmd_mem，1–2s） | ~2s | `read_pressure` 返回 `None` → 该域 unknown |
| 端口 | `net::ports::scan(None)`（约 40–140 ms，`PortSnapshot` 带 `coverage.note`） | <0.2s | lsof 不可用 / 权限差额 → note 必须如实呈现 |
| 网络 | `net::diag::run_diag(at, locale) -> NetDiag`（`diag.rs:559`；字段 `status/note/dns/lan/wan/jitter_ms/gateway/captive/verdict/ssid`） | ~1.5s | `status` 失败 + `note` 原因（`diag.rs:167-182` 注释明确「读不到就 None，不下假结论」） |
| （项目） | `DiskReport.projects` + `config_notes`（D-09 已落地） | 0（随磁盘域） | 无独立失败形态 |

**判据复用**（不新造阈值）：

| 结论 | 判据来源 |
|---|---|
| 磁盘水位 | `island.rs:667-671` `DiskThresholds`：可用 <15% warn、<5% critical |
| 内存压力 | `mem::system::read_pressure()` 的级别 |
| 端口冲突 | `net::ports::conflicts()`（`owner_count() > 1`，与 GUI/island 同一判据） |
| 端口暴露 | `exposure == BindScope::Any` 计数（`main.rs:1141-1144` cmd_lan 同口径） |
| 网络结论 | `NetDiag.status` / `verdict` / `captive` |

### 2.4 MCP 客户端配置事实（2026-09 核验，来源见 §9）

| 客户端 | macOS | Windows | 顶层键 | 条目形态 |
|---|---|---|---|---|
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `%APPDATA%\Claude\claude_desktop_config.json` | `mcpServers` | `{command, args}` |
| Cursor | `~/.cursor/mcp.json`（项目级 `.cursor/mcp.json`） | `%USERPROFILE%\.cursor\mcp.json` | `mcpServers` | `{command, args}` |
| VS Code | `~/Library/Application Support/Code/User/mcp.json` | `%APPDATA%\Code\User\mcp.json` | **`servers`**（且每项需 `"type": "stdio"`） | 见下 |

**唯一的坑**：VS Code 用 `servers` 而不是 `mcpServers`，且条目要带 `"type": "stdio"`——从别处抄来的块是合法 JSON、不报错、不加载（来源 2、3 均单列此坑）。

**不确定项**：Claude Desktop 的 Linux 路径官方未发布（第三方写 `~/.config/Claude/…`）——本方案对 Linux 走「dry-run 显示路径 + 用户确认」，不静默写（§5.4）。

### 2.5 文档与门禁现状

- `docs/cli-reference.md` **不存在**（`ls docs/*cli*` 只有 m3 的 plan/acceptance）。
- README 章节顺序（`README.zh-CN.md`）：功能叙事在前（第 45–208 行），**CLI 在 210 行、MCP 在 229 行、安装 246 行**——开发者要「装 → 跑 → 接」得滑过 200 行产品文案。
- `scripts/check-docs-honesty.mjs` 已有检查 A（MCP 工具三方对账）、B（排他表述 sweep）、C（编辑器配方 ↔ usage，本批新增）。
- MCP 契约快照 `crates/prune-cli/mcp-tools.v1.json` + `tool_defs_match_frozen_snapshot`（`mcp.rs:987`）是**本方案要复用的范式**：快照从生产路径导出，不手写。

---

## 3. D-03 契约 v1（6 人日）

### 3.1 契约定义：统一信封，不引入 `data` 包装

**规则**（v1 冻结，之后只增不改）：

1. 每个 `--json` 输出的**顶层**必含 `"schemaVersion": 1`；
2. 其余键保持现有领域形状（**不**改成 `{schemaVersion, data}` 双层包装）；
3. 新增字段允许；删字段、改类型、改语义 = 破坏性，必须提 `schemaVersion` + 主版本；
4. `--json` 输出永远是**一个** JSON 文档（`watch` 事件流除外——它是 JSONL，每行自带 `schemaVersion`）。

**为什么选「顶层平铺」而不是 `{data}` 包装**（决策 W1-DP-1）：

- 已发布的 `projects` / `port --conflicts` / `watch` 都是平铺形状，改成包装等于**现在**就制造一次破坏性变更，与「冻结契约」的目的相反；
- Agent（MCP）读的是领域键（`conflicts` / `projects` / `entries`），包装层只是多一跳；
- 快照门禁只需断言「顶层键集合 + schemaVersion 值」，平铺更容易被脚本消费。

### 3.2 五处缺口的修法

| 命令 | 做法 | 注意 |
|---|---|---|
| `scan` | 新增 `fn scan_doc(&DiskReport) -> Value`：`{"schemaVersion":1, ...report 字段平铺}`（用 `serde_json::to_value` 后插入键，或 `#[serde(flatten)]` 包装结构） | **不改 `DiskReport` 结构体**——它同时是 GUI IPC 载荷，加字段会污染前端类型与旧报告反序列化 |
| `mem` | 在现有 `json!({...})` 顶部加 `"schemaVersion": 1` | 纯增量 |
| `port`（默认分支） | 同上 | `--conflicts` 分支已合规，保持 |
| `app` | 同上 | |
| `lan` | **信封与缓存分离**：缓存文件继续写裸 `LanSnapshot`（`main.rs:1173`，GUI/MCP 读同一份）；CLI 打印用 `lan_doc(&snap)` 包信封 | 直接把字段加进 `LanSnapshot` 会让缓存文件与 MCP `lan_devices` 结果都多一个键，扩散面比收益大 |

**产出函数化**：把每处 `json!` 抽成 `fn xxx_doc(…) -> serde_json::Value`（`crates/prune-cli/src/docs_json.rs` 新模块），CLI 打印与快照测试共用同一函数——**快照必须来自生产路径**（复用 `mcp-tools.v1.json` 的纪律，`mcp.rs:981-996`）。

### 3.3 退出码：给 3 一个明确的产生条件

现状「3 从未返回」不能靠改文档糊过去——权限不足是开发者最需要区分的状态（「没查到」≠「没有」）。

**语义收敛**（决策 W1-DP-2）：

| 码 | 语义 | 产生条件（唯一判据） |
|---|---|---|
| 0 | 结论已出 | 包括「读到部分数据 + 在 `coverage.note` 里说明差额」 |
| 2 | 用法错 | `parse_args` 失败、必填参数缺失、`--only` 未知域 |
| 3 | **权限不足** | 主输出为空**且**根因是权限：lsof 不可用/被拒、`statvfs` 拒绝、目录遍历全被拒 |
| 4 | 探测失败 | 主输出为空且根因不是权限：超时、命令不存在、输出无法解析 |
| 10 | watch 达 warn | 仅 `--fail-on warn` |

**实现**：`main.rs` 加

```rust
/// CLI 退出码（v1 契约的一部分，`usage()` 的退出码行由它派生）
#[derive(Clone, Copy)]
pub enum ExitCode { Ok = 0, Usage = 2, Permission = 3, ProbeFailed = 4, WatchWarn = 10 }
```

并加一条测试断言 `usage()` 文本与枚举逐项一致（防止再出现「文档写了、代码没有」）。

**边界纪律**：`coverage.note` 非空但主数据有内容时**仍然返回 0**——「部分不可见」是数据事实，不是进程失败；把它升级成退出码会让 CI 脚本对每台机器都报错。

### 3.4 契约快照与门禁

新增 `crates/prune-cli/cli-contract.v1.json`：

```json
{
  "schemaVersion": 1,
  "commands": {
    "scan":     { "keys": ["schemaVersion", "scanned_at", "scan_elapsed_ms", "groups", "totals", "…"] },
    "mem":      { "keys": ["schemaVersion", "physicalTotal", "swap", "pressure", "families", "topSwapped"] },
    "port":     { "keys": ["schemaVersion", "port", "entries", "coverage", "timeWait", "timeWaitPorts", "scanMs"] },
    "port_conflicts": { "keys": ["schemaVersion", "at", "conflicts", "coverage"] },
    "app":      { "keys": ["schemaVersion", "family", "processCount", "memoryBytes", "…"] },
    "lan":      { "keys": ["schemaVersion", "…"] },
    "projects": { "keys": ["schemaVersion", "from", "days", "projects"] },
    "watch_once": { "keys": ["schemaVersion", "ts", "mode", "…"] }
  },
  "exitCodes": { "0": "ok", "2": "usage", "3": "permission", "4": "probe_failed", "10": "watch_warn" }
}
```

**导出方式**：新增 `prune --dump-contract`（隐藏命令，仅测试用？—— 不，见下）……

更符合本仓已有纪律的做法：**测试内直接调 `docs_json::*` 产出样例结构**（用最小 fixture 构造 `DiskReport` / `PortSnapshot` 等），收集顶层键排序后与冻结文件逐字段比对：

```rust
#[test]
fn cli_contract_matches_frozen_snapshot() {
    let frozen: Value = serde_json::from_str(include_str!("../cli-contract.v1.json")).unwrap();
    let actual = docs_json::contract_shape(); // 由生产函数产出，不手写
    assert_eq!(actual, frozen, "CLI 契约已变更：同批更新 cli-contract.v1.json 并提升 schemaVersion");
}
```

**快照更新纪律**：改契约 → 同批改快照 + 在 CHANGELOG 写明「契约变更」；`schemaVersion` 只在破坏性变更时提升。

### 3.5 验收

- [ ] 8 个**命令级** JSON 输出点全部含 `schemaVersion: 1`（`watch` 事件流按 JSONL 单独计，每行已自带）
- [ ] `cargo test -p prune-cli` 含 `cli_contract_matches_frozen_snapshot` 与退出码一致性测试
- [ ] 人为制造权限失败（如 `--all` 且在受限终端）能返回 3，且 stderr 说明「哪一段看不到」
- [ ] `lan` 缓存文件**不含** `schemaVersion`（GUI/MCP 读取不受影响），CLI 输出**含**

---

## 4. D-04 `prune doctor`（8 人日）

### 4.1 定位

一句话：**给出「这台机器现在哪里不对劲」的结论，而不是再抛一遍原始读数。**
与现有命令的区别：

| 命令 | 回答的问题 |
|---|---|
| `prune scan` | 哪些目录可回收多少 GB |
| `prune mem` / `port` / `lan` | 某一域的原始读数 |
| **`prune doctor`** | **磁盘/内存/端口/网络四域的「结论 + 证据 + 下一步」，30 秒内出结果** |

### 4.2 人读形态（示意，数值为格式示例非实测）

```
Prune 体检 · 2026-11-16 15:02 · 用时 16.3s（磁盘 15.1s / 内存 0.9s / 端口 0.1s / 网络 1.4s）

磁盘  ⚠ 可用 8%（11.2 GB / 460.4 GB）
      证据：可回收 31.4 GB（node_modules 42 个 / 缓存 8 组）
            卷容量读自 statvfs；扫描覆盖 31 条白名单，跳过 3 个无权限目录
      下一步：prune reclaim --level green 预览 → GUI 存储页确认清理

内存  ✓ 压力正常（可用 3.2 GB / 16 GB，swap 18%）
      证据：pressure=normal；swap 2.9 GB / 16 GB

端口  ⚠ 1 处冲突
      证据：:3000 被 node(8412) 与 node(9031) 同时监听
            另有 3 个监听无法归属进程（权限差额，内核口径 28 / 已归属 25）
      下一步：prune port 3000 看持有者；GUI 端口页可一键定位

网络  ✓ 出口正常
      证据：DNS 12ms / 局域网 3ms / 外网 42ms（抖动 4ms）；网关 192.168.1.1
```

**硬约束**：任一项读不到 → 该域显示 `⊘ 不可读：<原因>`，**绝不显示 0**（`diag.rs:180` 的既有纪律原样继承）。

### 4.3 JSON 形态

```json
{
  "schemaVersion": 1,
  "at": 1763266920,
  "elapsedMs": 16300,
  "findings": [
    {
      "id": "disk.volume_low",
      "domain": "disk",
      "severity": "warn",
      "title": "可用空间 8%",
      "evidence": [{ "label": "available", "value": "11.2 GB" }, { "label": "ratio", "value": 0.08 }],
      "next": ["prune reclaim --level green"],
      "unreadable": null
    }
  ],
  "coverage": {
    "disk":  { "state": "ok", "skippedPaths": 3, "scannedAt": 1763266904 },
    "mem":   { "state": "ok" },
    "ports": { "state": "ok", "invisible": 3 },
    "net":   { "state": "ok" }
  }
}
```

`severity` 词表固定四个值：`ok` / `warn` / `bad` / `unknown`（`unknown` 只在 `coverage.state != ok` 时出现，防「缺数据被读成正常」）。

### 4.4 数据源与时间预算

| 步骤 | 来源 | 预算【估算】 | 超时后行为 |
|---|---|---|---|
| 磁盘 | `disk::scan(&home, progress)`（全量白名单） | 15s | `--only` 排除该域；或 `coverage.disk.state = "timeout"` |
| 内存 | `read_pressure()` + `watch_round(20)` | 2s | unknown + 原因 |
| 端口 | `net::ports::scan(None)` | 0.2s | unknown + `coverage.note` |
| 网络 | `net::diag::run_diag(now, locale)` | 1.5s | unknown + `NetDiag.note` |

**总预算 30s**：串行相加 ~19s，留 10s 抖动余量。`--only disk,mem` 之类可裁剪。

**为什么默认实时扫而不是读缓存**（决策 W1-DP-3）：CLI 侧没有磁盘报告缓存（GUI 的报告只在内存，DB 里只有项目账本），新增一份「最近报告文件」等于多一个状态源与一致性负担。doctor 是低频命令（一天几次），15s 可接受；等真机显示太慢再加 `--fast`（列为后续，不进本期）。

### 4.5 命令形态

```
prune doctor [--json] [--only disk,mem,port,net] [--timeout N]
```

- 退出码：`0` 结论已出（含 warn/bad）；`2` 用法错；`3`/`4` 全部域都不可读时按根因给
- `--only` 只跑指定域，`coverage` 里其余域标 `skipped`
- `--timeout` 单域超时秒数（默认按上表，上限 30）

### 4.6 判据表（结论生成规则，全部复用既有口径）

| finding id | severity 条件 | 证据字段 |
|---|---|---|
| `disk.volume_low` | 可用 <15% → warn，<5% → bad（`island.rs:667`） | available, ratio |
| `disk.reclaimable_high` | 可回收 ≥ 20 GB → warn【估算，待真机校准】 | reclaimableBytes |
| `disk.unreadable_paths` | 跳过目录 > 0 → ok（信息项，不报警） | skippedPaths |
| `mem.pressure` | `read_pressure()` warn/critical → warn/bad | pressure, availableBytes |
| `mem.swap_high` | swap 使用 > 70% → warn【估算】 | usedBytes, totalBytes |
| `port.conflict` | `conflicts()` 非空 → warn | port, pids, comms |
| `port.invisible` | `listenTotal - attributed > 0` → ok（如实信息项） | invisible, total |
| `net.verdict` | `NetDiag.status` 非 ok → warn/bad，附 `verdict` | dns/lan/wan ms, jitter, gateway |
| `net.captive` | `captive` 非 none → warn | captive |

### 4.7 验收

- [ ] 真机 `time prune doctor` ≤ 30s，输出含四域结论
- [ ] 断网 / 无 lsof 权限两种降级下：对应域显示原因，退出码 0，JSON `coverage.*.state` 非 ok
- [ ] `--json | jq '.findings[].severity'` 只含四个词表值
- [ ] 与 GUI 同口径抽查：`doctor` 的磁盘水位结论与菜单栏 island 事件一致（同机同刻）

---

## 5. D-05 `prune mcp install`（5 人日）

### 5.1 命令形态

```
prune mcp install [--client claude|cursor|vscode|all] [--write] [--json] [--force]
```

- **默认 dry-run**：只打印「将写入哪个文件、哪一段、diff」，不落盘（决策 W1-DP-4）
- `--write` 才执行；`--json` 输出结构化计划/结果
- 不提供 `--uninstall`（本期不做；手动删除一个键即可，写进文档）
- 只支持**用户级**配置（项目级 `.cursor/mcp.json` / `.vscode/mcp.json` 会让 `git status` 变脏，违反「不碰用户仓库」纪律）

### 5.2 写入内容（条目形态按客户端差异生成）

```jsonc
// Claude Desktop / Cursor（mcpServers）
{ "mcpServers": { "prune": { "command": "/abs/path/to/prune", "args": ["mcp"] } } }

// VS Code（servers + type）
{ "servers": { "prune": { "type": "stdio", "command": "/abs/path/to/prune", "args": ["mcp"] } } }
```

`command` 取 `std::env::current_exe()` 的**绝对路径**——不写 `prune` 裸名：客户端启动 MCP server 时用的 PATH 与本机 shell 不一致，写裸名是「安装成功但加载失败」的头号原因。

### 5.3 写入算法（七步，全部有测试）

1. 解析现有文件；**文件不存在** → 视为 `{}`，action = `create`
2. **JSON 非法** → action = `refused`，输出「先修好这份配置，Prune 不覆盖坏 JSON」；`--force` 也不覆盖（坏 JSON 大概率含用户手写内容，覆盖就是把人家的工作删了）
3. 取顶层键容器（`mcpServers` / `servers`）；存在但不是对象 → `refused` + 原因
4. 已有同名 `prune` 条目且与目标**等价** → action = `noop`（幂等，重复执行不产生备份噪音）
5. 有差异 → action = `update`；dry-run 输出 unified diff
6. `--write`：先备份 `<file>.prune-bak-<YYYYMMDD-HHMMSS>`（`0600`），再**原子写**（tmp + rename），保留原文件权限位
7. 写后读回解析：条目存在且顶层其他键**逐一相等**（不丢用户配置）→ 成功；否则用备份**回滚**并报错

### 5.4 安全边界

- 只写 §2.4 表中列出的路径，路径由代码内常量给出（`paths.rs` 风格），不接受用户传 `--path`
- 父目录不存在时：`create` 只在**父目录已存在**时进行（Claude Desktop 未安装 → 不替用户造目录树，报「未检测到客户端」）
- macOS 上 GUI 版 Prune 的 `current_exe()` 在 `.app/Contents/MacOS/` 内，路径含空格——JSON 字符串转义由 `serde_json` 负责，测试要覆盖
- 备份文件不清理（用户可自行删）；文档说明命名规则

### 5.5 验收

- [ ] 三客户端各一次：`--write` → 重启客户端 → MCP 面板可见 `prune` 且 **12 个工具全部列出**
- [ ] 首次调用成功率 100%：各客户端让 Agent 调一次 `mem_snapshot` 或 `port_conflicts`，返回正常 JSON
- [ ] 幂等：连跑两次，第二次 action = `noop`、不产生新备份
- [ ] 破坏性保护：手写一个带尾逗号的配置 → 命令拒绝且文件字节不变
- [ ] 零绝对路径泄漏：任何输出/文档不含具体用户名（dry-run 打印的路径除外，那是运行时事实）

---

## 6. D-06 文档（4 人日）

### 6.1 README 开发者入口重排

现状：产品叙事 160+ 行在前，CLI/MCP/安装在后（210/229/246 行）。
目标顺序（**只挪不加**，内容改写另算）：

```
1  一句话 + 徽章
2  安装（brew / 下载）              ← 开发者第一诉求
3  最初两分钟（doctor → scan → reclaim）
4  CLI（命令总表 + 指向 cli-reference）
5  MCP 接入（prune mcp install 三条命令 + 12 工具一览）
6  它做什么 / 不做什么
7  六个 tab / 菜单栏 / 清理三级 / 内存 / 网络（原有产品叙事）
8  支持平台 / 状态与路线图 / 文档 / License
```

同步 `README.zh-CN.md`（双语门禁不在 CI，但§6.3 会把「双语 README 的章节标题一一对应」加入检查）。

### 6.2 `docs/cli-reference.md`（新建，中文单语）

结构：

1. **命令总表**：与 `usage()`（`main.rs:315`）逐字对齐
2. 每命令一节：语法 / flags 表 / JSON 顶层字段表 / 退出码 / 示例（含 `--json | jq` 一行）
3. **退出码总表**：语义 + 产生条件（与 §3.3 表一致）
4. **JSON 契约**：`schemaVersion` 规则、只增不改、「破坏性变更需主版本」
5. **MCP 工具清单**：指向 `crates/prune-cli/mcp-tools.v1.json`（快照是唯一真源，文档不抄 12 段 schema）
6. 明写「CLI 输出中文单语，不做 i18n」（既有约定，防被当缺陷报）

### 6.3 门禁扩展（`scripts/check-docs-honesty.mjs` 增两条）

| 检查 | 内容 | 失败即红 |
|---|---|---|
| **D** | `cli-reference.md` ↔ `usage()` **双向**：文档出现的每个 `prune <子命令>` 必须在 usage 里；usage 的每个子命令必须有对应小节；文档里的 flag ⊆ usage flags | 子命令/flag 漂移 |
| **E** | `cli-reference.md` 退出码表 ↔ `main.rs` `ExitCode` 枚举：集合与语义双向相等 | 再出现「3 写了但没有」类脱节 |

检查 D 复用检查 C 的解析器（`prune <sub> <flags>` token 化），只是数据源从 `docs/editors/*.md` 换成 `docs/cli-reference.md`，并把「覆盖要求」从三客户端任务集换成「usage 全集」。检查 E 从 `main.rs` 里正则抠 `ExitCode` 枚举成员与 `usage()` 退出码行，两处比对。

**门禁覆盖声明**：`pnpm check:docs` 的输出行要追加 D/E 的结果摘要（与现有 A/B/C 同格式）。

### 6.4 验收

- [ ] `pnpm check:docs` 通过，且输出行含「cli-reference 对账：N 子命令 / M flag」「退出码 5 个双向一致」
- [ ] 故意把 cli-reference 里某命令改成 `prune doctorx` → 门禁红（负向验证，同检查 C 的做法）
- [ ] README 中英两版章节标题顺序一一对应（人工核对一次，之后靠 review）

---

## 7. 排期（4 周，23 人日）

| 周 | 内容 | 人日 |
|---|---|---|
| W1 | D-03a：`docs_json` 模块 + 五处信封补齐（含 lan 缓存分离）+ 退出码 enum | 3 |
| W1–W2 | D-06a：`cli-reference.md` 初版 + 门禁 D/E | 2 |
| W2 | D-03b：`cli-contract.v1.json` + 快照测试 + 退出码一致性测试 | 3 |
| W3 | D-04：doctor 主体（findings 模型 + 四域 + 人读/JSON + 降级路径） | 8 |
| W4 | D-05：mcp install（三客户端 + dry-run + 备份 + 回滚 + 测试） | 5 |
| W4 | D-06b：README 重排 + 文档收口 + 门禁全绿 | 2 |

**依赖**：D-06a 与 D-03a 并行（文档先行不阻塞代码）；D-04 依赖 D-03 的读数结构（但可先用裸结构开发，收口时接 `docs_json`）。
**版本**：`0.13.0` 一次发（D-03/04/05 + D-06）；`0.12.0` 留给 D-01/02（签名与 brew）。

---

## 8. 决策点汇总（开工前需拍板）

| # | 问题 | 选项 | 建议 | 理由 |
|---|---|---|---|---|
| W1-DP-1 | JSON 信封形状 | A 顶层平铺（本文） ｜ B `{schemaVersion, data}` ｜ C 只给 scan 补字段 | **A** | 已发布 3 个输出点是平铺；B 等于现在就破坏一次；C 不解决 mem/port/app/lan |
| W1-DP-2 | 退出码 3 是否实现 | A 实现（本文） ｜ B 从 usage 删除 ｜ C 保留不用 | **A** | 「权限不足」是可执行信息；删掉会让脚本无法区分「没权限」与「没问题」 |
| W1-DP-3 | doctor 磁盘数据来源 | A 新增最近报告缓存 ｜ B 每次实时扫（本文） ｜ C 默认缓存 + `--fresh` | **B** | CLI 无缓存现状；多一个状态源就多一类不一致；15s 在 30s 预算内 |
| W1-DP-4 | mcp install 默认行为 | A 默认 dry-run，`--write` 才落盘（本文） ｜ B 默认写 + 备份 ｜ C 交互确认 | **A** | 与「不悄悄改用户文件」一致；B 的失败模式是用户不知道被改了什么 |
| W1-DP-5 | 支持哪些客户端 | A 三客户端用户级（本文） ｜ B 三客户端 + 项目级 ｜ C 五客户端（+Windsurf/Claude Code） | **A** | 项目级会弄脏仓库；五客户端里 Zed 是 `context_servers`、Codex 是 TOML，格式分裂成本高，等有人要再说 |
| W1-DP-6 | cli-reference 语言 | A 中文单语（本文） ｜ B 中英双语 | **A** | 与「CLI 文案单语中文」既有约定一致；双语维护成本会拖垮 D-06 的 4 人日 |

---

## 9. 风险与未决问题

| 风险 | 影响 | 应对 |
|---|---|---|
| **RK-E 契约破坏**（路线图已识别） | 毁掉 CI 集成信任 | 快照测试 + `schemaVersion` 只增不改 + 破坏性变更进主版本（§3.4） |
| 客户端路径漂移 | `mcp install` 写到旧路径，用户以为装好了 | 路径表标注核验日期（2026-09）；每次发版前复核一次；dry-run 让用户**看见**路径；文档给「验证命令」 |
| Claude Desktop Linux 路径无官方来源 | 可能写错位置 | Linux 只给 dry-run 计划，`--write` 要求 `--force` 并在输出里写明「该路径未被官方文档确认」 |
| VS Code `servers` vs `mcpServers` 混淆 | 装完零工具、无报错 | 条目形态按客户端分流（§5.2）+ 真机三客户端验收（§5.5） |
| doctor 15s 扫描在冷缓存机器上更慢 | 超过 30s 承诺 | `--only` 裁剪 + `--timeout`；真机实测后再决定是否引入 `--fast` |
| 门禁 D/E 解析脆弱（正则抠 Rust 枚举） | 误报/漏报 | 检查 E 优先用「usage 退出码行 ↔ 文档表」比对；枚举正则只作为集合校验的补充 |

**未决**（不阻塞本期，记录待议）：
1. `doctor` 是否要出 `--fix` 建议脚本（与 DP-4「人才能删」冲突，本版明确**不做**）
2. `mcp install --uninstall` 的形态（手工删键即可，等反馈）
3. 契约快照是否也要覆盖 MCP 工具（已有 `mcp-tools.v1.json`，两套快照的版本号是否联动）

---

## 10. 验收清单（可判定，逐条可打勾）

### D-03
- [ ] 8 个命令级 `--json` 输出点全部含 `schemaVersion: 1`（`watch` 事件流按 JSONL 单独计）
- [ ] `cli-contract.v1.json` 由生产函数导出、测试逐字段比对
- [ ] 退出码 0/2/3/4/10 各有真实产生路径与测试；usage 行 ↔ 枚举一致
- [ ] `lan` 缓存文件无信封、CLI 输出有信封（同一份数据，两个出口）

### D-04
- [ ] 真机 `time prune doctor` ≤ 30s
- [ ] 任一域不可读 → 显示原因、`severity=unknown`、**不出现 0**
- [ ] JSON 与 `cli-contract.v1.json` 对账通过
- [ ] 与 GUI/island 同口径抽查一致（磁盘水位、端口冲突）

### D-05
- [ ] 三客户端各装一次，工具数 12，Agent 首次调用成功
- [ ] 幂等、坏 JSON 拒绝、备份可回滚三项各有测试
- [ ] 写入的是 `current_exe()` 绝对路径（含空格路径用例）

### D-06
- [ ] `docs/cli-reference.md` 存在且覆盖 usage 全集
- [ ] `pnpm check:docs` 全绿（A/B/C/D/E），负向验证各红一次
- [ ] README 中英两版新顺序落地，安装/CLI/MCP 进前 1/3

---

## 11. 与后续波次的接口

- **D-07～D-12（Wave 2，已落地）**：本期的 `schemaVersion` 规则是它们的既成事实（`projects` / `watch` 已合规）；D-09 的 `config_notes` 结构已按「结构化事实 + 前端翻译」落位，doctor 可直接展示。
- **D-11 MCP 冻结**：`mcp-tools.v1.json` 与本期 `cli-contract.v1.json` 是两套快照——建议在 `0.13.0` 的 CHANGELOG 里写清「MCP 工具面变更不影响 CLI 契约，反之亦然」。
- **D-14 报告导出**：`doctor --json` 的 `findings/coverage` 模型就是 `prune report --json` 的雏形，届时复用而不是另造。
- **D-16 1.0 收口**：本期的退出码表与契约快照是「契约稳定」的判定依据。

---

## 12. 实施记录（2026-11-16，W1 一次落库）

### 12.1 交付

| 项 | 状态 | 位置 |
|---|---|---|
| D-03a docs_json + 五处信封 + ExitCode | ✅ | `crates/prune-cli/src/docs_json.rs` / `main.rs` |
| D-03b cli-contract.v1.json + 快照测试 | ✅ | 快照由 `PRUNE_DUMP_CONTRACT=1 cargo test dump_contract` 从生产函数导出 |
| D-04 doctor | ✅ | `crates/prune-cli/src/doctor.rs`（真机三域 10.3s / 全量 23.5s 含 cargo 开销） |
| D-05 mcp install | ✅ | `crates/prune-cli/src/mcp_install.rs`（真机 dry-run 三客户端行为全对） |
| D-06 cli-reference + 门禁 D/E + README 重排 | ✅ | `docs/cli-reference.md` / `scripts/check-docs-honesty.mjs` / 双语 README |
| 版本 | ✅ | 0.11.0 → 0.13.0（bump-version.mjs 五文件同步）+ CHANGELOG |

### 12.2 决策与偏差（6 个决策点全按建议项落地）

1. **W1-DP-1 顶层平铺**：五处信封均为 `enveloped()` 平铺插入，`DiskReport` 结构体未动（GUI IPC 载荷不受污染）。
2. **W1-DP-2 退出码 3 实现**：产生路径为 `port --json` 主输出空且 `coverage.note` 含「权限不足」；`exit_code_for_empty()` 纯函数化并有三分支测试。偏差：usage 的 `reclaim` 行补了 `[--yes]`（真实 flag 此前没写进 usage）。
3. **W1-DP-3 doctor 实时扫**：磁盘域独立线程 + `mpsc.recv_timeout`，超时如实标 `timeout`（进程退出时扫描线程随之回收，CLI 无需 join）。
4. **W1-DP-4 默认 dry-run**：真机 dry-run 即验证了三种形态——Claude 未安装（拒绝）、Cursor 已有条目差异（更新）、VS Code 现有配置坏 JSON（拒绝且字节不变）。
5. **W1-DP-5 三客户端用户级**：`--force` 仅打印提示（坏 JSON 拒绝不设豁免）；Linux Claude 路径带「未被官方文档确认」note。
6. **W1-DP-6 cli-reference 中文单语**：门禁 D 复用检查 C 的解析器思路，token 预处理剥 `[ ] |` 与 `prune` 前缀。

### 12.3 偏差与未决

- **disk.unreadable_paths 未实现**：DiskReport 不含「跳过目录数」字段，为不污染 GUI IPC 载荷而不加；该项等扫描报告侧有数据源再补（记录为方案判据表的已知缺口）。
- **doctor 头部对 skipped 域显示「未运行」而非 0.0s**——「没跑」不是「瞬间完成」，同一纪律的输出侧延伸。
- **pct 双百分号教训**：`pct()` 返回带 `%` 的串，标题模板再拼一次字面 `%` → `94%%`；修掉后 doctor 真机输出正确。
- **快照含 doctor**：方案 §3.4 的 8 命令快照扩为 9（doctor 自带 schemaVersion，验收清单要求对账）。
- 未决 1/2/3（--fix、--uninstall、双快照联动）维持原状不进本期。

### 12.4 门禁结果

cargo test --workspace 全绿（core 485 / cli 50 / app 49）；clippy -D warnings 0；fmt 无 diff；tsc / check:i18n / check:docs（A–E 全部）/ build 全绿；门禁 D/E 各做了负向验证（`doctorx` 章节与退出码 3→7 均被拦下）。

### 12.5 发版插曲：0.13.0 的 Windows 编译失败 → 0.13.1

- **现象**：`v0.13.0` 的 Release workflow 里 macOS 两腿成功、Windows 腿红（4 个 E0433/E0425），`release-notes` 因此跳过 → Release 停留在草稿、未对外发布。
- **根因**：Wave 2 的 `panel_fit_height` 在**命令体**里直接写了 `crate::popover::remember_fit` / `is_popover_mode` / `fit_height` 与 `crate::tray::PANEL_H`，而 `popover` 模块与 `PANEL_H` 都是 `#[cfg(target_os = "macos")]`。本地 macOS 编译看不见，Windows 上这些符号根本不存在。
- **修法**：popover 分支整段包 `#[cfg(target_os = "macos")] { … return; }`（编译期消失，不是运行期 `if false`——后者仍然要求符号存在）；降级路径统一 `panel.set_size(PANEL_W, h)`；`PANEL_W` 去掉 cfg 门（跨平台都要用），`PANEL_H` 保持 mac 专属。顺带修 `net_diag` 的 `Read` / `Instant`——只在 mac 的 `networkQuality` 路径用，按平台收窄 import。
- **教训**：新增「跨平台命令 + 平台专属实现」时，`cfg` 必须落在**语句块**上，且 `cargo check --target x86_64-pc-windows-msvc` 在 mac 上跑不通（ring 的 C 部分缺 MSVC 头），只能靠 CI 的 Windows 腿兜底——本地门禁全绿不等于三平台可编。

