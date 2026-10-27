# M3 · B6 CLI + MCP + A6 验收记录

> 日期：2026-10-27 · 提交基线：M3/B5 之后（`a705728`）
> 方案：`m3-cli-mcp-plan.md` · 判定核心：`m3-leak-plan.md` / `m3-leak-acceptance.md`

## 1. 交付物

| 项 | 内容 |
|---|---|
| `prune-cli` | 单二进制 `prune`：`scan` / `reclaim` / `mem` / `mem --watch` / `mcp`；arg 手写（D-C1），不引 clap |
| `prune mcp` | newline-delimited JSON-RPC 2.0 stdio server；三工具 `mem_snapshot` / `mem_leaks` / `disk_report` |
| A6 | `scan` 输出对 L4 Docker 附命令提示（`docker system prune` 等 3 条），代码层无执行路径 |
| 文档 | README CLI 段改「已实现」+ 路线图 M0~M3 勾选；方案 `m3-cli-mcp-plan.md` |

## 2. 单测

- prune-cli **16 项**（arg 解析 5 / JSON-RPC 编解码 6 / SeriesAccum 2 / 工具定义 1 / 格式化 2）
- prune-core 84 项 + prune-app 7 项不受影响，workspace 全绿
- `cargo build --release` 零警告

## 3. 真机实测（release 构建）

| 命令 | 结果 |
|---|---|
| `prune mem --swapped` | ✅ 物理 16.0 GB / swap 9.4/10.0 GB（94%）/ 压力 Warning；Cursor 6.9 GB（含换出 4.1 GB），与 GUI 同量级；家族口径标注 `footprint` / `≈ RSS` 正确（`is_exact` 要求全家族深探，混合口径为下界，与 GUI 一致） |
| `prune scan` | ✅ 主数字 51.5 GB（L1 6.5 + L2 24.5 + L3 20.5，Docker 3.0 另列）；A1 按平台分布 + 浪费比 6.91:1；A3 四管理器 29.4 GB；A6 三条命令提示可见 |
| `prune reclaim`（默认） | ✅ dry-run：256 项 / 6.5 GB 全列明，尾行提示「追加 --yes 执行」；未动任何文件 |
| `prune mem --watch --interval 2` | ✅ 每 60s 判定一次，`暂无泄漏嫌疑（观测 20 进程：数据不足 11、良性排除 9、未达标 0）`——如实显示数据不足（验收标准 4） |
| `prune mcp` 握手 | ✅ `initialize` 返回 protocolVersion 2024-11-05；`notifications/initialized` 静默；`tools/list` 三工具带 description + inputSchema |
| `tools/call mem_leaks` | ✅ 读真实序列库：observed 171 / insufficient 56 / benign 57 / below 58 / items 0（0 误报与 B5 验收一致）；`now` 取库内最大 ts |
| `tools/call bogus` | ✅ `error: unknown tool: bogus` |

`prune reclaim --yes` 未实测执行（真删 6.5 GB，与 M1 验收同理由留给人工）；护栏在 M1 已有 66 项单测覆盖（白名单拒家目录、20GB 熔断、NeedsExplicitConfirm）。

## 4. 实测挖出并修复的问题

| # | 问题 | 修复 |
|---|---|---|
| 1 | watch 判定按轮数触发（`round % 30`），但每轮实际耗时含深探 > interval，判定永远迟到（65s 未触发一次） | 改为按实际时间触发（`ts - last_verdict >= 60`） |
| 2 | watch 时间戳用 `ts % 86400` 是 UTC，比本地慢 8h | 改 `libc::localtime`（prune-core 已有该依赖） |
| 3 | `prune mem` 深探 Top 20 后家族仍显示 `≈ RSS` 引发误判怀疑 | 核实为口径设计使然（家族 `is_exact` 需全深探），非 bug；输出已带口径列与说明行 |

## 5. 验收标准对照（方案 §6）

| # | 标准 | 结果 |
|---|---|---|
| 1 | scan 主数字与 GUI 同量级 + A6 提示 | ✅ 51.5 GB vs GUI 55.26 GB（数据日变，量级一致）；A6 命令可见无执行路径 |
| 2 | 不带 --yes 绝不删除 | ✅ 默认 dry-run + NeedsExplicitConfirm 双闸 |
| 3 | mem --swapped 与 GUI 口径一致 | ✅ Cursor 6.9 GB（含换出 4.1 GB） |
| 4 | watch 数据不足显式显示 | ✅ 「数据不足 11」 |
| 5 | mcp 握手 + tools/call 读真实库 | ✅ 全链路通 |
| 6 | JSON-RPC 编解码单测 | ✅ 6 项（解析/错误/未知方法/通知静默/包装/错误码） |

## 6. 已知边界

- MCP `mem_leaks` 依赖 GUI 已采集的序列库；GUI 从未运行过则返回 `unavailable`（不冒充无泄漏）
- CLI `reclaim` v1 只做 L1 死重条目；L2/L3 整库清理只进 GUI（命令行脚本误用风险不对称，D-C4）
- `NSPopover` 原生化推迟：M0 tray 定位实测可用，objc2 FFI 成本高收益纯体验，留待真实定位失败反馈再投入
