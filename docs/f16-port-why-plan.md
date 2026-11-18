# F16 端口白话解释（`prune why <port>`）方案

> 竞品依据（2026-11-18 联网核实）：DevCheck 的 `why 5000` 是端口治理品类中最受称赞的单点功能——
> 一句话告诉用户「这是 AirPlay 占的，你可以在系统设置里关掉，或者换 5001」。
> Prune 端口域（M7 反查 + F4 保镖）数据齐全，缺的只是「把证据翻译成人话」这一层。

## 一、背景与目标

**背景**：`prune port` 与端口 tab 给的是结构化表格（端口 / 进程 / 项目 / 暴露面），
用户仍需自己解读：5000 是谁？是系统服务还是 dev server？为什么杀了又复活？
AI agent（MCP）侧同样缺一个「直接可读结论」的工具，`port_listeners` 返回的是原始行。

**目标**：`prune why <port>` 输出一段白话结论 + 证据行；MCP 同步暴露 `prune_why` 工具，
让 Claude Code / Cursor 能直接问「3000 是什么、能不能动」。

**成功标准**：

| # | 标准 |
|---|------|
| S1 | `prune why 5000` 在本机（AirPlay Receiver 开启）输出「系统服务 ControlCenter（AirPlay 接收器）占用，不是 dev server，杀掉会复活，关闭路径：系统设置·通用·隔空播放接收器」 |
| S2 | dev 端口（有项目归属）输出「node 占用 · prune 项目 · 绑定 127.0.0.1（仅本机）」，并给出项目目录 |
| S3 | 端口无监听时区分三种情况：查询失败（Coverage 差额）/ 确认空闲 / TIME_WAIT 残留，不输出「空闲」误导 |
| S4 | 双持有者冲突时输出冲突解释与两个持有者 |
| S5 | MCP `prune_why` 返回同源结论（JSON），`tool_defs` 冻结快照测试更新后全绿 |
| S6 | `cargo test -p prune-core` 全绿 + `tsc` 0 错误（如涉及前端提示文案复用） |

## 二、不做什么（本期边界）

- 不做「一键关闭 AirPlay / 杀进程」动作——只解释、指路，操作纪律与 F4 相同（提报只指路）
- 不做联网查询端口含义（离线规则库，L0 静态推断；L1 探针仍走 probe.rs 用户显式触发）
- 不给端口 tab 加新的可视化区块：`why` 结论以tooltip / 详情行形式复用现有 PortList 渲染
- 不改 `port_listeners` / `port_conflicts` 两个既有 MCP 工具的输出契约

## 三、方案

### 3.1 规则库（prune-core，纯函数可单测）

落在 `crates/prune-core/src/net/why.rs`（新建，与 `service.rs` 同域）：

```rust
pub struct WhyVerdict {
    pub port: u16,
    pub headline: String,        // 白话一句话（i18n key + 参数，不硬编码中文）
    pub evidence: Vec<String>,   // 证据行：进程 / pid / 项目 / 绑定 / Coverage
    pub action_hint: Option<String>, // 指路（不执行）：如系统设置路径或项目目录
    pub kind: WhyKind,           // SystemService / DevServer / Conflict / Free / Unknown
}

pub fn why(port: u16, snapshot: &PortSnapshot, rules: &[PortRule]) -> WhyVerdict
```

规则分层（全部 L0 静态，无 IO）：

1. **知名系统服务表**：5000/7000（AirPlay 接收器 · ControlCenter）、5353（mDNS · mDNSResponder）、
   137/138（NetBIOS）、631（CUPS）、22（ssh）、445（smbd）等，附关闭路径 hint。
   依据 `service.rs` 既有静态推断扩展，判定键 = `(port, comm)` 双匹配，防误报（普通进程占了 5000 ≠ AirPlay）。
2. **dev server 识别**：`project_label.is_some()` 即视为项目进程，headline 走项目模板。
3. **冲突**：复用 `ports.rs::conflicts()` 结论。
4. **兜底 Unknown**：规则不命中时只报观测事实（进程 / 绑定 / Coverage），不猜用途。

### 3.2 CLI（crates/prune-cli）

- `main.rs` 增加 `why` 子命令：`prune why 5000 [--json]`，内部复用 `cmd_port` 的扫描路径
- **契约纪律**：`cli_contract_matches_frozen_snapshot` 测试与 `docs/cli-reference.md` 同一变更内更新
- 退出码遵循现有枚举（0 正常 / 非 0 扫描失败），「没查到 ≠ 空闲」口径写入输出尾行

### 3.3 MCP（crates/prune-cli/src/mcp.rs）

- `tool_defs()` 增加 `prune_why`（readonly），`run_tool` 分发到同一 `why()` 纯函数
- `tool_defs_match_frozen_snapshot` / `tool_defs_have_readonly_tools` 两个冻结测试同步更新

### 3.4 前端（可选顺带）

- `PortList.tsx` 详情行追加 headline 一行（复用 `why()` 结论，随快照计算，不另起请求）

## 四、里程碑拆解

| # | 内容 | 验收 |
|---|------|------|
| T1 | prune-core：`net/why.rs` 规则库 + `why()` + 单测（S1–S4 四类各 1 正例 + 1 负例） | `cargo test -p prune-core` |
| T2 | CLI：`why` 子命令 + 契约冻结测试 + cli-reference.md | `cargo test -p prune-cli` |
| T3 | MCP：`prune_why` 工具 + 冻结快照更新 | `cargo test -p prune-cli` |
| T4 | 前端：PortList 详情行 headline（可选，随车） | `tsc` 0 错误 |
| T5 | 实测：本机 5000 / 一个 vite dev server / 一个随机端口 / 一个 root 端口 | 手工验证记录 |

## 五、风险与对策

| 风险 | 对策 |
|------|------|
| 系统服务表误报（用户自己的进程占了知名端口） | `(port, comm)` 双匹配；comm 不符只走 Unknown，不输出服务结论 |
| 规则库变成维护负担 | 只收「高频 + 有明确关闭路径」的服务（首批 ≤10 条），不追求大全 |
| 输出文案硬编码中文，破坏 i18n | headline 走 i18n key（复用 prune-core/src/i18n 现有机制） |
| MCP 冻结快照漂移 | 冻结测试在同一次变更内更新，CI 门禁兜底 |

## 六、验收清单

- [ ] `prune why 5000` 命中 AirPlay 规则且给出关闭路径（S1）
- [ ] `prune why <dev端口>` 给出项目归属与绑定面（S2）
- [ ] 无监听端口区分「确认空闲 / Coverage 差额 / TIME_WAIT」三种输出（S3）
- [ ] 冲突端口输出双持有者（S4）
- [ ] `prune why --json` 与 MCP `prune_why` 字段一致（S5）
- [ ] 规则不命中时输出 Unknown + 观测事实，无编造用途
- [ ] 全文无「一键关闭 / 自动杀进程」表述
- [ ] `cargo test -p prune-core -p prune-cli` 全绿，冻结快照已更新

## 七、实现记录（2026-11-18）

- 落点：`prune-core/src/net/why.rs`（规则表 7 条 + `why()` 纯函数，13 单测）/ `prune-cli` `why` 子命令 + `docs_json::why_doc` / MCP `prune_why`（`mcp-tools.v1.json` 经 `PRUNE_DUMP_MCP_TOOLS=1` 重导出，新增 dump 测试防手写）。
- 与方案的两处偏差（有意为之）：
  1. `WhyKind` 增加 `KnownService`（引用 `service.rs` L0 推断，headline 标「疑似」）——plan 原五类会把这些已知服务的结论错误归入 Unknown，违背「引用已采集证据」的初衷；
  2. 文案未走 i18n key：遵循 CLI 中文单语决策（W1-DP-6），与 `port_note` 既有惯例一致。
- 对账门禁全绿：`check:docs`（MCP 13 工具三方对账 / cli-reference 11 子命令双向）/ `check:i18n` / 契约冻结测试 / clippy 0 新警告。
- GUI 入口补齐（同日晚）：`port_why` 命令（快照优先采样器缓存，冷启动才现场扫）+ PortList 行内「为什么？」按钮 + 行下结论展示（headline 按类别上色：SystemService/Conflict=warn，DevServer/KnownService=accent，Free/TimeWait/Unknown=中性；证据行与指路压暗）。方案 §五 T3 的「可选，随车」项落地，不再挂待办。verdict 正文沿用核心层中文单语文案（W1-DP-6），按钮/标题走 i18n 双语。
