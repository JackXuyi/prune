# F24 · swap 增速量化方案（f24-swap-rate-plan）

> 2026-11-19 ｜ P2 第 1 项 ｜ 基线 v0.15.0（CLI 12 子命令 / MCP 13 工具）

## 一、背景

11-18 四域深挖的需求池将「swap 增速量化」列为 P2。当前 swap 能力的缺口：

- **系统级**（`swap::SwapInfo`）：总量 / 已用 / swapfile 个数 / 今日新增——全是**当下快照**，回答不了「换出在变快还是变慢」「从昨天起多挤了多少」。
- **按进程**（MCP `swap_report` 的 `topSwapped`）：谁**此刻**被换出最多，但没有**时间维度**——「node 这一小时又多换了 200 MB」这种增速信息缺失。

竞品核实（2026-11-18）：macmon 有瞬时 swap 速率读数（内核级、无进程归属），Vitals 有 swap 占用无增速；**按进程的换出增速排行无人做**。

## 二、关键代码事实（2026-11-19 核验）

1. `db.mem_process` 表（L80）**每进程 `swapped` 已入库 7 天**（`RETENTION_SECS`），且主键含 `start_time`——pid 复用可校验。**增速 = 相邻样本差分，零新增采集。**
2. `db.rs L301` 的 `mem_report_rows`（F20 加）不查 `start_time` 与 `footprint`——增速差分需要**新查询方法**或改签名（表列齐全，只差 SELECT）。
3. `swap::collect(None)` 产出系统级 `SwapInfo`；macOS 口径 `sysctl vm.swapusage` + swapfile 目录，Windows 走 sysinfo pagefile。
4. **Windows 无 per-process swapped 口径**（`win.rs L47-48`：`swapped` 一律 `None`，UI 隐藏换出读数）——增速榜在 Windows 是空集，这是口径事实不是缺陷。
5. MCP `swap_report`（mcp.rs L821）已有「swap 明细 + 换出受害者榜」工具；CLI 侧 **swap 无子命令**（只有 MCP 有）。

## 三、成功标准

1. `prune swap` 一条命令回答三个问题：现在挤了多少（明细）、**谁在加速挤**（增速榜）、系统整体换出净速率。
2. 增速全部来自**既有序列差分**，不新增任何采集轮次、不新建时序表。
3. 中间时段没跑 GUI → 空窗如实标注（复用 F20 gap 纪律），不让读者把「没数据」当「没换出」。
4. Windows 上增速榜为空且文案说明口径缺失，不显示 0。

## 四、不做什么

- **不建 `swap_series` 表**：按进程 swapped 已在 `mem_process`，系统级增速用进程差分聚合近似（见 §五 DP-1），不为系统级一条曲线单开存储。
- **不做 swap-in 逐笔事件流**：内核无此只读接口，伪造口径违背「口径不撒谎」。
- **不做 swap 预测 / 告警**（「照这个速度 3 小时后爆」）：增速外推是猜，灵动岛第五类事件冻结裁决依旧有效。
- **MCP 不加新工具**：`swap_report` 载荷加 rates 摘要即可，工具面 13 冻结。

## 五、方案

### DP-1 系统级增速 = 进程差分聚合（近似口径，诚实标注）

系统级换出净速率 **不**直接差分 `SwapInfo.used`（快照字段无历史），而是 Σ(每进程 swapped 差分)。偏差来源：**进程退出**会把它名下的 swapped 从下一轮快照里清零——表现为负差分。处置：聚合行拆两个数字——`sumDelta`（含退出释放）与 `activeDelta`（只算窗口两端 `start_time` 一致、即存活至今的进程），文案写明「含已退出进程释放的换出量」口径。**不夹 0、不隐藏负值**：负差分 = 发生了 swap-in 或释放，是信息不是脏数据。

### DP-2 增速窗口与产出形态

- 窗口：`--hours N`（默认 24，上限 168=7d 保留期）。窗口内每进程取**首末两个样本**差分 ÷ 时长 = 速率（MB/h），另给 1h 短窗（若样本足够）作「当下趋势」。
- 产出：`ProcSwapRate { pid, name, family, firstSwapped, lastSwapped, deltaBytes, rateMbH, samples, startConsistent }`，按 `deltaBytes` 降序，默认榜 10 条，`--all` 不设限（JSON 全量）。
- 样本 <2 的进程不产增速（宁缺勿错）；窗口两端不足（GUI 刚装 / 中途没跑）→ `coverage` 段标注窗口实际覆盖区间，复用 F20 的 gap 语义。

### DP-3 CLI 新子命令 `prune swap`（12→13）

```
prune swap [--json] [--hours N] [--all]
```

- 三段输出：**明细**（复用 `swap::collect` + 内存压力，与 MCP swap_report 同源）、**增速榜**（DP-2）、**系统级**（DP-1 双口径）。
- 文本模式中文单语（W1-DP-6 惯例）；`--json` 顶层键 `schemaVersion / swap / usedRatio / pressure / rates / systemRate / coverage`，进契约冻结。
- 为什么开新子命令而不是 `mem --rate` flag：swap 与 mem 在 MCP 层就是两个工具（分工注释明确「那边给家族聚合、这边回答谁被换出」），CLI 与 MCP 同构比省一个子命令重要。

### DP-4 MCP `swap_report` 加 rates 摘要

载荷追加 `rates: { top: [5 条], system: {...}, windowHours: 24 }`——Agent 问「最近谁在把机器往磁盘上挤」时不必再跑 CLI。工具描述同步更新，`mcp-tools.v1.json` 重导出。

### DP-5 GUI：换出受害者榜加 Δ 列

内存 tab 换出榜每行加「Δ24h」（同一 db 查询，与 CLI 同一聚合函数）；swap 使用率读数旁加「近 1h 净增速」小字。**不做新卡片、不开新 tab**——增速是既有榜的增量维度，不是新功能面。

## 六、里程碑

| 里程碑 | 内容 |
|---|---|
| M-A | core：`mem/swap_rate.rs` 纯函数（差分 / 聚合 / 窗口）+ db 查询方法（带 start_time）+ 单测（含 pid 复用、负差分、退出释放、gap） |
| M-B | CLI `prune swap` 子命令 + 契约三处同变更（contract / cli-reference / README 13→13 行） |
| M-C | MCP 载荷 rates + 描述更新 + 快照重导出；GUI Δ 列与增速行 + i18n 双语 |

## 七、边界风险

| 风险 | 处置 |
|---|---|
| 退出进程产生负差分污染聚合 | DP-1 双口径拆分 + 文案标注；单测覆盖 |
| pid 复用跨窗口错配 | 差分两端校验 `start_time` 一致才计入（`startConsistent` 字段） |
| 样本稀疏误读为「零增速」 | gap/coverage 段如实标注实际覆盖区间 |
| Windows 全空 | 增速榜返回空数组 + note 说明口径（与 win.rs 既有纪律一致） |

## 八、验收清单

- [ ] `prune swap` 文本 / `--json` 契约键冻结测试绿；`--hours` 上限 168
- [ ] MCP swap_report 含 rates、工具描述更新、快照重导出
- [ ] GUI 换出榜 Δ 列与 CLI 同数字（同一聚合函数实测核对）
- [ ] Windows 构建增速段空 + note（编译期 cfg 即可验证）
- [ ] 门禁全绿：core/cli 测试、tsc、check:i18n、check:docs（13 子命令对账）
