# F25 · prune trace 任务成本核算方案（f25-trace-plan）

> 2026-11-19 ｜ P2 第 2 项 ｜ 基线 v0.15.0（CLI 12 子命令 / MCP 13 工具）

## 一、背景

11-18 四域深挖的需求池将「prune trace 任务成本核算」列为 P2。开发者的日常反复问一个问题：**「跑一次 `pnpm install` / `next build` / 起一个 dev server，到底吃了这台机器多少资源？」** 现状工具都只答一半：

- `/usr/bin/time -v`：峰值 RSS + CPU 时间，**没有**换出、没有端口、没有进程树（只算直接子进程）；
- 活动监视器：人肉盯，无法留存对比；
- Prune 自己的内存/端口视图是**全机视角**，没有「这次任务」的归属边界。

这是 roadmap「CLI/MCP 一等公民，CLI 作证据台」路线的直接落点：trace 的产出天然是**可贴进 issue 的证据单**。

## 二、关键代码事实（2026-11-19 核验）

1. `mem::process::Scanner`（持久枚举器，复用 `System`）每轮产出 `ProcMem`：**pid / ppid / name / rss / swapped / `cpu_time_ms`（累计 CPU 时间）/ start_time**——进程树归属、CPU 差分、换出增量所需字段全部现成。
2. CPU 口径先例：`ProcMem.cpu` 是近 10 s 差分均值且**首见为 None 不是 0**（sysinfo 差分窗口）——trace 的 CPU 用 `cpu_time_ms` 末值−首值，无此坑。
3. `net::ports::scan` 一轮 40–140 ms（f16 实测）；watch 已有「每轮顺带端口扫描」的先例，开销可控。
4. `watch.rs` 三条边界纪律已成文：**不装后台 / 不写日志文件 / stdout 即出口 / 只读**——trace 全盘继承。
5. footprint 深探一轮 1–2 s（Top 20）——trace 逐轮深探不可行，见 DP-3。
6. 契约现状：CLI 12 子命令，MCP 13 工具冻结。

## 三、成功标准

1. `prune trace -- pnpm install` 结束后输出一张**成本单**：墙钟、CPU 时间与利用率、峰值 RSS、换出增量、新绑定端口、按命令聚合的 top 后代。
2. 子进程退出码**透传**——`prune trace -- cmd && next` 的 CI 链可直接替换裸命令。
3. Ctrl-C 中断同样产出成本单（中断前的消耗也是成本）。
4. 归属边界诚实：收养/脱离进程树的后代**不计入且标注**，不冒充全量。

## 四、不做什么

- **不做 MCP 工具**：`prune_trace` 需要执行任意命令——MCP 工具面是纯查询（13 个全部只读），开执行口违背面冻结与「没有一扇执行的门」叙事（README L290）。Agent 要观测任务成本 → 引导其跑 CLI。
- **不做后台 / 计划任务 trace**：前台、随子进程生灭，watch 纪律同款。
- **不做性能归因**（火焰图 / syscall 追踪）：成本核算回答「花了多少」，不回答「为什么慢」——后者是 dtrace/instruments 的领域。
- **GUI 本期不做**：CLI-only；trace 结果是文本/JSON，贴 issue 即消费。
- **不内置任何任务清单**：Prune 不自带「常见任务」列表诱导执行，`--` 后面是什么就观测什么。

## 五、方案

### DP-1 语法与执行权边界

```
prune trace [--json] [--out <PATH>] [--interval N] -- <command> [args...]
```

- `--` 后**必须**有命令，缺省用法错退出 2（与 `--emit-script`/`--yes` 互斥先例同款）。
- 直接 `spawn` 不经 shell（避免注入面与 shell 包装噪音）；命令找不到 → 退出 4 并说明。
- 与 D-15 `--emit-script`（打印命令永不执行）不冲突：trace 的角色是**观测壳**（同 `/usr/bin/time`），执行的是用户显式传入的命令；Prune 自身不存在任何「替用户决定执行什么」的清单。文档写明这条边界。

### DP-2 采样与进程树归属

- spawn 前先建**基线轮**（全进程快照，含端口绑定集）；`Scanner` 每 `--interval`（默认 2s）一轮。
- **归属规则**：本轮进程表中 ppid 链可追溯到 root pid 的集合（含 root）。逐轮增量记录：新入树的进程记「加入时刻 + 首见 name/exe」。
- **收养缺口如实标注**：孙进程被 reparent 到 launchd（pid 1）后从树中脱落——成本单 `coverage` 段写明「被系统收养的后代不计入，实际消耗略高于本单」。
- **短命进程**：存活 < 一个采样间隔的孙进程可能整轮漏采——成本单注明采样粒度（interval），不做「补估」。
- trace 自身进程树（prune 及其祖先）从归属集合排除。

### DP-3 指标与口径（成本单字段）

| 指标 | 口径 | 来源 |
|---|---|---|
| `wallMs` | 子进程 spawn→reap 墙钟 | std |
| `exitCode` | 子进程退出码（透传） | wait |
| `cpuTimeMs` | 树内各进程 `cpu_time_ms` 末−首求和；多线程可超墙钟 | Scanner |
| `cpuUtil` | cpuTimeMs ÷ wallMs ÷ 核数，>100% 说明并行度 | 推导 |
| `peakRssBytes` | 树内 RSS 逐轮 max（**采样粒度下的峰值**，标注 interval） | Scanner |
| `swapDeltaBytes` | 树内 swapped 末−首求和；负值 = 发生 swap-in，不夹 0 | Scanner |
| `portsBound` | trace 期间新绑定的 (port, proto, comm)（对基线轮 diff）；列「还在监听」与「已释放」两段 | ports::scan |
| `topDescendants` | 按 cpuTimeMs 降序 top 5，含命令行首段 | Scanner |

- **不做 footprint 承诺**（DP-3 关键决策）：深探 1–2 s/轮不可逐轮付，子进程死后拿不到——成本单只承诺 RSS 峰值口径，footprint 字段**不出现在成本单里**，避免与内存页口径混淆（f19 的同源纪律反向应用：不同口径不并列展示）。
- 端口轮默认开（dev server 场景正是端口证据）；`--no-ports` 关闭。

### DP-4 输出通道与 JSON 契约

- **文本模式**：子进程 stdout/stderr **原样透传**（trace 是观测壳不是管道中间件），成本单打到 **stderr**——`prune trace -- cmd > build.log` 的重定向习惯不被破坏。
- **`--json` 模式**：成本单 JSON（一行）走 **stderr**，子进程输出仍透传 stdout/stderr；`--out <PATH>` 时 JSON 写文件、stderr 只留一行路径回执。顶层键：`schemaVersion / cmd / wallMs / exitCode / cpu / rss / swap / ports / topDescendants / coverage / intervalMs`——进契约冻结（13→14 子命令）。
- 为什么 JSON 走 stderr 不走 stdout：stdout 属于被观测命令（透传纪律优先），成本单是 trace 自己的产出——`time` 走 stderr 的同款先例。

### DP-5 信号与中断

SIGINT/SIGTERM → 转发给子进程进程组 → 等 reap（上限 5s，超时 `SIGKILL` 仅针对**用户自己的子进程**——这是透传用户意图，不是 Prune 主动杀）→ 照常产出成本单，`interrupted: true` 字段标注。trace 自身被二次 Ctrl-C 才直接退出（此时成本单丢弃——用户明确不要了）。

## 六、里程碑

| 里程碑 | 内容 |
|---|---|
| M-A | core：`trace.rs`（spawn + 归属纯函数 `tree_of` + 成本单聚合）+ 单测（树归属 / 收养缺口 / 短命进程 / CPU 差分 / 端口 diff） |
| M-B | CLI `trace` 子命令 + 信号转发 + 契约三处同变更（12→13） |
| M-C | 真机实测三场景：`sleep`（最短成本单）、`npm install`（CPU+RSS 主场）、`vite dev` + Ctrl-C（端口 + 中断单） |

## 七、边界风险

| 风险 | 处置 |
|---|---|
| 收养后代漏算 | coverage 段如实标注「略高于本单」；不做补估 |
| 短命进程整轮漏采 | 采样粒度写进成本单；spawn 前基线轮 + reap 后补采一轮缩小漏窗 |
| CPU 时间含「继承前」消耗（fork 自长命父进程） | 按进程**加入树时刻**起算增量；首见即树内新生的才全计 |
| 用户命令是守护进程不退出 | 成本单永远不主动结束观测——文档写明「trace 随子进程生灭」，Ctrl-C 是唯一出口 |
| 契约扩张 | 12→13 子命令与 contract / cli-reference / README 同批；MCP 不动 |

## 八、验收清单

- [ ] `prune trace -- sleep 1` 产出最短成本单；exitCode 透传（`-- false` → 退出码 1）
- [ ] Ctrl-C 中断单带 `interrupted: true` 且数字非空
- [ ] JSON 走 stderr、子进程 stdout 透传（重定向实测）
- [ ] 契约冻结测试绿；check:docs 13 子命令对账
- [ ] 门禁全绿：core/cli 测试、clippy `-D warnings`（CI 同款）、tsc、check:i18n、check:docs
