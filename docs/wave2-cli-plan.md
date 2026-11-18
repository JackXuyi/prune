# Wave 2（CLI 深化）收尾方案

> 状态：方案定稿（2026-11-18），待实施。
> 上游：`prune-roadmap-2027-dev-first.md` §8.2（Wave 2 原计划）。
> 性质：**不是从零实施方案**。代码考古发现 Wave 2 主体已随 v0.14.0 提前落地，本文只对真实缺口立项。

## 1. 背景

路线图 §8.2 把 Wave 2 定义为 D-07～D-12，37 人日、分 0.14.0 / 0.15.0 / 0.16.0 三版。实际进度核对（2026-11-18，master = v0.14.0 tag）发现：

- **D-07 / D-08 / D-09 / D-10 / D-11 的主体代码已全部在库**（提交 `4b84822` prune.toml 核心侧、`632107a` watch + MCP 12 + projects 账本、`bb091a6` 契约+doctor+mcp install），且已随 v0.14.0 发布；
- 但 **CHANGELOG 0.14.0 一条都没提这些 CLI 变化**（0.14.0 的四段全是 GUI 侧：dock 图标 / popover / updater / 报告入口）；
- 路线图口径仍停留在"MCP 8 个工具"（五处）。

所以本文的任务是收尾：补记录、修口径、接一处真实缺口、把 D-12 的"不做"写成显式决策。

## 2. 现状盘点（对照路线图验收口径）

| 项 | 路线图验收口径 | 代码证据 | 结论 |
|---|---|---|---|
| D-07 端口前置 | 菜单栏→看到 pid+项目目录 ≤2 次点击 | StatsBoard 冲突排序卡；MenuBarPanel「本地服务」段（冲突行置顶）；MainWindow `ui://focus` 端口深链预填定位 | ✅ 完成 |
| D-08 项目账本 | 每项目「30 天涨 X GB」可回溯目录 | `project_day` 表 + `build_trends` + `cmd_projects`（读 GUI/扫描同一张表，CLI 不触发扫描——账本是历史记录不是现场重算） | ✅ 完成 |
| D-09 `prune.toml` | 损坏回落默认并告知；无配置行为不变 | `prune_toml.rs` 1095 行：5 键白名单（`ignore.projects/paths`、`thresholds.deps_warn_gb/stale_days`、`watch.ports`）、手写 TOML 子集、语法超子集整份忽略、键越界单键回落，均带 ConfigNote 告知；消费方 `disk/mod.rs`（ProjectConfigs::collect）、`disk/project.rs`、`net/ports.rs`（collect_watch_ports）；GUI 设置页「复制模板」+ PortList 空闲关注端口显示 | ✅ 完成 |
| D-10 `prune watch` | 不写 launchd；退出无残留 | `watch.rs` 652 行：cpu/mem/port/disk 四 Kind、JSONL、`--once`、`--fail-on-warn`、warn 即退出码 10；判据复用 `prune_core::island` 同款监视器；头注释三条边界（不装后台/不写日志/只读） | 🟡 主体完成，缺口见 §3.2 |
| D-11 MCP 12 + 冻结 | 只读断言存；门禁覆盖 12 个 | 12 工具；`mcp-tools.v1.json` 快照 + 逐字段对账测试（mcp.rs:981-994，改工具必须同批更新快照）；15 个测试含执行动词显式拒绝 | 🟡 冻结成立，口径未同步 |
| D-12 编辑器集成 | G2 门禁通过才做 | G2 = 2027-05-31 征募样本 ≥50 且 7 天内 ≥3 次调用 ≥30%——远未开始 | ❌ 按门禁设计不做 |

CLI 测试基线：`cargo test -p prune-cli` 50 通过 0 失败。

## 3. 剩余工作与设计决策

### W2-A CHANGELOG 与路线图口径补记（0.5 运营日）

- CHANGELOG 0.14.0 补四段：`prune watch`、`prune.toml`、MCP 12 工具、`prune projects` 账本。理由：CHANGELOG 门禁写明「所有可见变更记录于此」，这四项用户可见却缺席；latest.json 的 notes 在下版发布时自动同步，无需回改。
- 路线图五处「8 个」→ 12（§3 第 29 行、§7 第 75 行、§8.2 D-11 行、§8.1 架构图 127 行、G4 142 行），每处加脚注「12 个自 0.13.x 起（632107a）」，保留历史演进语义不冒充从来如此。

### W2-B watch 的 watch.ports 接线缺口（0.5 人日）

**事实**：`watch.rs:268` 端口采样走 `net::ports::scan(None)`，而 `collect_watch_ports` 只在 GUI 路径的 `scan_cached`（ports.rs:441）里调用。`prune watch --only port` 看不到用户配置的关注端口。

**影响面**：告警语义不受影响（Conflict / NewListen 判定不依赖 watched 列表）；缺的是输出里 entries 的 `watched` 标志与「配置了但空闲」呈现——watch.rs 头注释承诺「判据不复制、与 GUI 同源」，这条被打了折扣。

**DP-W2-1（默认即建议：选项 B）** watch 是机器级命令、没有项目缓存，`collect_watch_ports` 需要项目根来源。三个选项：

- A. 不接，文档写明「watch 的 watched 标志仅含 GUI 内存态」——最省，但同源承诺永久破缺；
- B. **watch 启动时从 CWD 向上找最近的项目根，读一份 `prune.toml` 只取 `watch.ports` 并入**——与「在项目目录里跑 watch」的用户心智一致，改动一个函数 + 一个键；
- C. watch 增加 `--config <path>` 参数显式指定——最正交，但为 32 个端口上限的配置引入一条新 CLI 面，过重。

选 B 时输出 meta 增加 `config_source` 字段（绝对路径或 null），「目录敏感」行为自解释。

**DP-W2-2（默认即建议：不做系统通知）** 路线图写「可选系统通知」。建议显式不做：watch.rs 头注释第 2 条边界「stdout/stderr 就是出口，重定向由调用方决定」——通知是第二个不可被 CI 消费的出口，与 JSONL 定位矛盾。在 `docs/cli-reference.md` watch 段写一句「系统通知为可选项，未实现；输出消费见 JSONL」。

### W2-C doctor 与 prune.toml 的边界声明（0.25 人日）

事实：`doctor.rs` 0 处引用 prune_toml——doctor 阈值是机器级常量（如 disk.reclaimable_high ≥ 20GB）。

**DP-W2-3（默认即建议：保持不接）** doctor 是机器体检，prune.toml 是项目级收窄，口径不同源；若 doctor 吃项目阈值，「结论卡可复核」会被「在哪个目录跑」破坏（同一台机器换个目录结论就变，违反 doctor 的机器级定位）。只补两句话：doctor.rs 头注释 + cli-reference「doctor 不读 prune.toml，阈值机器级」。

### W2-D D-12 显式不做声明（0.25 人日文档）

在路线图 D-12 行下补一段：触发条件 = G2（2027-05-31，样本 ≥50、二次调用率 ≥30%）；G1（2027-02-28，brew ≥300）不达则冻结 D-12 转修 doctor 结论质量。前置数据链 = D-02 tap 已 live → D-17 的 brew analytics 读取动作是 G1 的数据源。**在 G2 达标前，编辑器集成的任何代码投入都是违约**（这是路线图自己定的纪律，写成声明防止将来手痒）。

### W2-E 版本策略

路线图的三版计划（0.14.0 = D-07/08、0.15.0 = D-09/10、0.16.0 = D-11/12）实际坍缩：0.14.0 已带 D-07/08/09/10/11 主体。**建议不为收尾单独发版**：W2-A～C（约 1.25 人日）并入下一个含用户可见变更的版本（0.15.0）一起出，CHANGELOG 届时一次记全。

## 4. 代码落点

| 文件 | 改动 | 规模 |
|---|---|---|
| `CHANGELOG.md` | 0.14.0 补 watch / prune.toml / MCP 12 / projects 四段 | ~30 行 |
| `docs/prune-roadmap-2027-dev-first.md` | 五处 8→12 + 脚注；D-12 行下补不做声明段 | ~20 行 |
| `crates/prune-cli/src/watch.rs` | W2-B 选项 B：启动时从 CWD 向上找项目根读 prune.toml 的 watch.ports 并入 port 采样；meta 加 `config_source` | ~40 行 + 测试 |
| `crates/prune-core/src/prune_toml.rs` | 复用现有 `watch_ports()` 取值器（可能补一个 `find_up` 帮手） | ~15 行 |
| `docs/cli-reference.md` | watch 段补「通知未实现」「config_source 字段」；doctor 段补「不读 prune.toml」 | ~6 行 |

## 5. 边界与风险

| # | 风险 | 概率×影响 | 应对 |
|---|---|---|---|
| RK-W2-1 | watch 读 CWD 引入目录敏感行为：用户在错误目录跑 watch，关注端口集与预期不同 | 中×低 | 只读 watch.ports 一个键；meta 带 `config_source`；找不到项目根时行为与现状逐字节一致（DP-W2-1-B 的回落分支） |
| RK-W2-2 | 改路线图历史段（§3「事实清单」性质）被误读为篡改记录 | 低×低 | 只改数值 + 加时点脚注，不删历史叙述 |
| RK-W2-3 | CHANGELOG 补已发布的 0.14.0 造成「发布后改史」观感 | 低×低 | CHANGELOG 非发布产物（latest.json 已发出不受影响）；0.14.0 tag 后补记属于补文档不是改版本 |
| RK-W2-4 | D-12 边界声明后被「既然 watch 都做了不如顺手做 VS Code」式蔓延突破 | 中×中 | 声明里写死触发条件与 G1 冻结动作；这是路线图既有纪律，本文只是让它显式 |

## 6. 验收清单

| # | 验收 | 判定方式 |
|---|---|---|
| A1 | CHANGELOG 0.14.0 四段齐全 | `pnpm check:docs` 通过 |
| A2 | 路线图无「8 个工具」残留 | `grep -c "8 个" docs/prune-roadmap-2027-dev-first.md` = 0（含脚注历史处除外，逐处核对） |
| A3 | watch.ports 进 watch：在含 `watch.ports = [3000]` 的项目根下 `prune watch --only port --json --once`，输出 entries 含该端口的 watched 标志、meta 含 config_source | 新增单元测试覆盖解析与并入 |
| A4 | 无配置目录下行为回归：输出与 v0.14.0 逐字节一致（config_source = null 除外） | 新增回归测试 |
| A5 | cli-reference 两句边界声明 | `pnpm check:docs` 覆盖 |
| A6 | 全绿 | `cargo test -p prune-cli`（现基线 50 通过）不回归 |

## 7. 工作量与节奏

| 项 | 人日 |
|---|---|
| W2-A 补记 | 0.5 运营日 |
| W2-B watch 接线 | 0.5 |
| W2-C 边界声明 | 0.25 |
| W2-D D-12 声明 | 0.25（并入 A 的文档动作） |
| **合计** | **1.25 人日 + 0.5 运营日** |

路线图 Wave 2 原估 37 人日 / 缓冲 2——主体提前落地后，收尾只用掉缓冲的一部分。关键路径上真正挡路的仍是 Wave 1 的 D-01 签名公证（1.5 人日 + $99/年），Wave 2 收尾随时可插空做，不与任何项冲突。

## 8. 这份文件不做什么

- 不重述 D-07/08/09/10/11 已实现部分的设计（各自的模块头注释与 `docs/cli-reference.md` 已是真源）
- 不实施 D-12（W2-D 只写声明）
- 不动 GUI 侧代码（Wave 2 收尾全是 CLI + 文档）
- 不改 `mcp-tools.v1.json` 快照（冻结纪律：改工具才动，本次不动工具面）

## 9. 实施记录（2026-11-18）

**一处方案前提修正（比方案更重要）**：W2-B 的原始判断「`scan(None)` 不含 collect_watch_ports」**是错的**——`net::ports::scan` 委托 `scan_cached`（新建 cache），在线项目的 `watch.ports` 本来就在采样路径里。但顺着这条链核下去发现了**更严重的真缺口**：`PortGuardMonitor` 的 NewListen 告警依赖条目的 `e.watched`，而 watched 只覆盖**在线项目**——在还没起服务的项目里跑 `prune watch`，该项目的关注端口起服务那一刻**不会告警**。缺口从「展示问题」升级为「告警语义问题」，修复价值更高。

**落地清单**：

| 项 | 文件 | 内容 |
|---|---|---|
| W2-B 核心 | `prune-core/src/prune_toml.rs` | 新增 `find_watch_ports_upward`（CWD 向上找最近一份带 watch.ports 的配置；**就近损坏不向上降级**——「子目录坏了却用父目录的配置」无法向用户解释）+ 3 个测试 |
| W2-B 接线 | `prune-cli/src/watch.rs` | `cwd_watch_ports()` 启动解析一次（横幅与循环同源）+ `mark_extra_watched` 纯函数（只置位不撤销）+ 主循环与 `run_once` 两处并入 + 人读横幅显示配置来源 + `ports.watchedExtra` 字段（有配置才加键）+ 2 个测试 |
| W2-A | `CHANGELOG.md` | 0.14.0 补「CLI：Wave 2 主体」四段（watch / prune.toml / MCP 12 / projects）；新增 Unreleased 段记 W2-B |
| W2-A/D | `docs/prune-roadmap-2027-dev-first.md` | §3 第 3 条、§7 资产表、G4 三处口径 12 + 脚注 [^mcp12]；§3 事实清单 5 条与 D-11 标题的「8→12」为演进语义保留原样；D-12 行下新增显式不做声明段（G2 触发条件 + G1 冻结动作） |
| W2-C | `doctor.rs` 头注释 + `cli-reference.md` | doctor「不读 prune.toml」边界声明；watch 段补关注端口两层来源、watchedExtra 字段、「系统通知未实现（有意）」 |

**验收结果**：

- A1 ✓（check:docs 全过，MCP 三处对账 / 退出码双向相等仍绿）
- A2 ✓（残留「8 个」仅 2 处，均为有意保留的历史表述：脚注 + 版本区间行）
- A3 ✓ 实测：`withcfg/sub` 目录下 `watch --json --once --only port` → `watchedExtra: {ports:[19099,19098], source:.../prune.toml}`，顶层键集合不变
- A4 ✓ 实测 + 单测：无配置目录输出无 `watchedExtra`，ports 段键集与 0.14.0 一致；`mark_extra_watched` 空 extra 零改动
- 损坏就近不降级 ✓ 实测：broken/sub 损坏 + 父目录完好 → watchedExtra 不出现
- A6 ✓ `cargo test -p prune-cli` 52 通过（+2）、prune-core prune_toml 32 通过（+3）、`cargo fmt --check` 过（Release CI fmt 门禁）

**版本策略执行**：W2-B 记入 CHANGELOG Unreleased，随下个含用户可见变更的版本一起发（W2-E），不为收尾单独发版。
