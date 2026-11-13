# f13 · M29 MCP 只读深化 + 「AI 只读、人执行」信任叙事

> 2026-11-13 · 差异化主线：不追 Vitals 的工具广度，把「AI 只读 + 人执行 + 白名单兜底」做成代码层可验证的信任卖点。

## 1. 现状

MCP server（`prune mcp`）已有 6 个只读工具：`mem_snapshot` / `mem_leaks` / `disk_report` / `port_listeners` / `app_resources` / `lan_devices`。roadmap M29 指定的残余：

1. **`disk_scan`（细分视图）**：`disk_report` 是全盘浪费结构（~15 s，重），缺一个「白名单逐条现在多大、里面什么占的」的轻量查询。
2. **`net_diag`（诊断结论）**：`port_listeners` 给的是原始条目，Agent 还要自己推「谁在对外暴露」；缺一个直接给结论的工具。
3. **信任叙事**：「只读」目前只是实现事实，没有做成产品主张（README / 工具描述 / 拒绝行为三处落地）。

## 2. 工具设计

### 2.1 `disk_scan` —— 白名单逐条细分（只读，秒级）

- 入参：`keys?: string[]`（过滤，缺省全量）、`children?: boolean`（是否列一级子目录，缺省 false）、`childrenTop?: number`（默认 8，clamp 1–20）
- 实现：`whitelist::resolve(home)` → `parallel_map` 并发 `tree_size`（复用磁盘扫描自建线程池，CoreSimulator 3.5 GB 量级秒级）→ 子目录用 `first_level_sizes` 排序截断
- 返回：每条 `{key, label, level, cleanable, path, exists, sizeBytes, children}` + summary（`cleanableBytes` = L1/L2 和、`confirmBytes` = L3 和、present 计数）
- 未安装的条目 `exists=false, sizeBytes=0`（与 GUI resolve 自动跳过同口径）
- L4（容器类）只列不计量——它们本来就不进清理通道

### 2.2 `net_diag` —— 端口暴露面诊断结论（只读）

- 入参：`port?: number`（缺省 = 全量）
- 实现：`net::ports::scan(None)` → 新增纯函数 `diag_conclusions(entries, invisible, time_wait)` 产结论（可直接单测）
- 结论语义（对标 BindScope 的「安全」注释，不发明新口径）：
  - `exposure=Any`（0.0.0.0/[::]）→ **attention**：「同网段设备可达」，附进程名与项目归属
  - `exposure=Interface`（绑定具体网卡地址）→ **attention**（局域网同样可达，只是没对全部网卡开放）
  - 全部 Loopback → **ok**：「所有可见监听仅本机可达」
  - `coverage.invisible > 0` → **note**：权限差额，「没查到」≠ 端口空闲
  - `time_wait > 0` → **note**：内核残留连接，无持有进程
- 返回：`exposed[]`（Any/Interface 明细）+ `conclusions[]` + coverage/timeWait/scanMs

## 3. 只读底线（代码层，不是文档层）

1. **`run_tool` 显式拒绝执行类请求**：`reclaim` / `clean` / `execute` / `delete` 等命中即报错，错误信息写明「MCP 通道只读，清理执行只能由人在 GUI 确认」——不是笼统的 unknown tool，让 Agent（和读代码的人）直接看到底线。
2. **测试固化**：工具名单断言扩到 8 个；新增断言——任何工具名不得含 clean/delete/remove/execute/reclaim/kill/write 词根；`run_tool` 对执行类名字必须报「只读」错而非 unknown。
3. **描述层声明**：每个工具 description 保留「只读」字样；两个新工具在返回 JSON 里带 `readonly: true` 与 note 字段，Agent 的上下文里每条结果都自带底线说明。

## 4. 信任叙事（README「CLI 与 MCP」段）

主张一句话：**Prune 是唯一把「AI 只读、人执行」写进代码层的系统工具**——Agent 能看（8 个只读工具）、人才能删（GUI 白名单 + 废纸篓 + 20 GB 熔断）；MCP 通道对执行类请求在分发层直接拒绝并有测试固化。与 Vitals 的「AI 请求 → 人确认」对比：Prune 连请求执行的门都没有，权限面为零。

不做：MCP 写通道（任何形式）、`prune reclaim --yes` 的 MCP 包装、GUI 远程触发——都是把信任叙事自己拆掉。

## 5. 交付物与验收

1. `mcp.rs`：2 个新工具 + `diag_conclusions` 纯函数 + 显式拒绝 + 3 组新测试
2. README「CLI 与 MCP」段重写（8 工具 + 叙事 + 底线）
3. CHANGELOG 0.8.5 补 M29 段；roadmap 文档 M29 标 ✅（对比表更新留给 Vitals storage map 上线触发）
4. 门禁：cargo test / clippy -D warnings / fmt / tsc / pnpm build
