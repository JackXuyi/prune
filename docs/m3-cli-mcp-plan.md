# M3 · B6 CLI + MCP + A6 实施方案

> B5 泄漏判定已独立成文（`m3-leak-plan.md`）。本文覆盖 M3 剩余：**CLI、MCP server、A6**。
> `NSPopover` 原生化**不在本轮**：M0 tray 实测可用（定位已固化），objc2 FFI 成本高、收益是纯体验优化，降级方案随时可启用——留待有真实定位失败反馈再投入。

## 1. 结论先行

- **单二进制双形态**：`prune` 既是 CLI 也是 MCP server（`prune mcp` 起 stdio 服务）。不引 clap / MCP SDK，arg 手写 + JSON-RPC 手写，全部复用 `prune-core`。
- **CLI 五条子命令**与 README 对齐：`scan` / `reclaim` / `mem` / `mem --watch` / `mcp`。
- **MCP 面向 AI Agent 查询**：三个工具 `mem_snapshot` / `mem_leaks` / `disk_report`。泄漏判定读 GUI 已采集的序列库（`~/Library/Application Support/com.xuyi.prune/prune.db`），**不自己采**——AI Agent 的查询是即时的，等不了 30 分钟序列。
- **A6**：`scan` 输出对 L4 Docker 组附 `docker system prune` 等命令提示，**永不执行**（与 GUI 口径一致，代码层就不存在执行路径）。

## 2. CLI 子命令规格

| 命令 | 行为 | 护栏 |
|---|---|---|
| `prune scan` | 全量扫描，输出主数字 / 分层 / A1 明细 / A6 提示；`--json` 机器可读 | 只读 |
| `prune reclaim [--level green]` | 预览清理清单；**默认 dry-run**；真执行需显式 `--yes` | 白名单 + 20GB 熔断（prune-core 代码层） |
| `prune mem` | 一次性内存读数：家族聚合表 + swap + 压力；`--swapped` 加换出列 | 只读 |
| `prune mem --watch [--interval N]` | 持续采样（默认 10s），进程内累积序列，每分钟输出一次泄漏判定 | **不落盘**——避免与 GUI 抢 SQLite 锁；watch 会话结束序列即弃 |
| `prune mcp` | stdio MCP server | 同上，只读工具 |

v1 的 `--level green` 即 A1 死重条目（L1，零风险）；L2/L3 整库清理不在 CLI 提供——误删代价与 GUI 不对称，命令行更容易被脚本误用。

## 3. MCP server 规格

- 传输：stdio，**newline-delimited JSON-RPC 2.0**（每行一个消息，无 Content-Length 头）。
- 方法：`initialize` / `notifications/initialized`（静默）/ `ping` / `tools/list` / `tools/call`；其余方法返回 `-32601`。
- 工具返回统一 `{ content: [{ type: "text", text: <JSON 字符串> }] }`——结构化结果序列化进 text，Agent 消费无歧义。
- `mem_leaks`：库不存在 → `LeakReport::unavailable("序列库不存在…")`；`now` 取库内最大 ts（同 leak-probe 的做法，避免历史库被误判为全部数据不足）。
- `disk_report`：现扫现报（~15s），工具 description 里注明成本，避免 Agent 高频轮询。

## 4. 决策点

| # | 决策 | 结论 | 理由 |
|---|---|---|---|
| D-C1 | arg 解析 | 手写，不引 clap | 5 条子命令，clap 拖慢编译且无必要 |
| D-C2 | watch 序列存储 | 进程内存，不落盘 | 与 GUI 共库会抢 SQLite 写锁；CLI 采集的数据本来就随会话结束失效 |
| D-C3 | MCP 泄漏数据源 | 只读 GUI 序列库 | 即时性要求；自采 30 min 不现实 |
| D-C4 | reclaim 真执行门槛 | `--yes` 显式 + 默认 dry-run | 脚本误用是命令行最大风险面 |
| D-C5 | NSPopover | 推迟 | 见文件头说明 |

## 5. 任务分解

- **T-501** CLI 主体（scan / reclaim / mem）+ arg 解析单测
- **T-502** `mem --watch`（序列累积 + 周期判定）
- **T-503** `prune mcp`（JSON-RPC 编解码单测 + 三工具）
- **T-504** 真机实测 + `docs/m3-cli-mcp-acceptance.md` + README 更新

## 6. 验收标准

1. `prune scan` 主数字与 GUI/dev fixture 同量级（55 GB 级），A6 提示可见且不含执行路径
2. `prune reclaim` 不带 `--yes` 绝不删除（默认 dry-run + NeedsExplicitConfirm 双闸）
3. `prune mem --swapped` 输出与 GUI 口径一致（footprint 含换出）
4. `prune mem --watch` 数据不足时显式输出「不足 N/30min」，不冒充无泄漏
5. `prune mcp` 完成 initialize 握手 + tools/list + tools/call（mem_leaks 读真实库）
6. JSON-RPC 编解码单测覆盖：请求解析、错误响应、未知方法
