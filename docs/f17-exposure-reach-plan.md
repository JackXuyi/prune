# F17 暴露面可达实测（reach test）方案

> 竞品依据（2026-11-18 联网核实）：LeftOpen / PortKill / Vitals 的「对外可达」全部是
> **按绑定地址推断**（`0.0.0.0` ⇒ 推断可达），无一真正测一次。
> Prune 已有 `BindScope`（M7 口径纪律：「绑定面是安全语义的唯一来源」），
> 本期把「推断」升级为「实测 + 推断」双通道，仍守住「只读观测器」边界。

## 一、背景与目标

**背景**：端口 tab 对 `Any`（`0.0.0.0` / `*`）绑定标「对外可达」，但这只是绑定面推断——
端口可能被本机防火墙规则挡住、可能 `connect` 立即被拒（假活）。用户面对安全语义时，
「推断」与「实测过」的可信度不同。

**目标**：端口 tab 对任一监听端口提供用户显式触发的「测一下」按钮，
从本机经**非 loopback 网卡地址**对端口发起一次 TCP connect，报告分层结果。

**成功标准**：

| # | 标准 |
|---|------|
| S1 | `Loopback` 绑定的端口不提供按钮（本机可达是定义使然，测了没有信息量） |
| S2 | `Any` / `Interface` 绑定端口可测，结果四态分开显示：`Connected` / `Refused` / `Timeout` / `NoRoute`，不合并为「通 / 不通」 |
| S3 | 结果口径诚实：显示「同机经网卡地址回连」≠「其他设备可达」，tooltip 写明差异 |
| S4 | 探针超时 ≤ 2 s，UI 有进行中状态；重复点击防抖（进行中禁用） |
| S5 | `cargo test -p prune-core` 全绿 + `tsc` 0 错误 |
| S6 | 不产生任何常驻轮询 / 后台自动探测（仅在点击时执行一次） |

## 二、不做什么（本期边界）

- 不做局域网内第二台设备发起的真·跨设备实测（需设备协同，本期不引入）
- 不做自动周期探测、灵动岛自动提报（安全语义误报代价高，保持用户显式触发，与 probe.rs L1 纪律一致）
- 不做 UDP 探测（UDP 无连接语义，测不出「可达」）
- 不写系统防火墙配置、不改任何网络状态
- 不给 `why`（F16）自动带上实测结果（两个能力独立，`why` 保持零 IO）

## 三、方案

### 3.1 探针核心（prune-core，纯逻辑可单测）

落在 `crates/prune-core/src/net/reach.rs`（新建，与 `probe.rs` 同域但职责不同：
probe 测「端口活着没」，reach 测「从网卡面进来通不通」）：

```rust
pub enum ReachOutcome { Connected, Refused, Timeout, NoRoute, NoIfaceAddr }

pub struct ReachResult {
    pub tried_addr: String,   // 实际回连的地址（本机某个非 loopback 网卡 IP）
    pub outcome: ReachOutcome,
    pub elapsed_ms: u64,
    pub scope_inferred: BindScope, // 推断面，与实测并列展示
}

/// addr 来源：复用 net/host.rs 的本机网卡地址枚举（取第一个非 loopback IPv4）。
/// IO 收敛在 execute() 一个入口，判定/展示逻辑为纯函数。
pub fn execute(port: u16, timeout_ms: u64) -> ReachResult;
pub fn classify(io_error_kind) -> ReachOutcome; // 纯函数，单测重点
```

分层判定（延续 net-diag / net-sites 的「断点分开」纪律）：

| 观测 | 判定 | 展示 |
|------|------|------|
| connect 成功 | `Connected` | 「实测可达（经 192.168.x.x）」 |
| `ECONNREFUSED` | `Refused` | 「端口拒连：无进程在监听（假活嫌疑）」 |
| 超时 | `Timeout` | 「超时：可能被防火墙拦截」 |
| `ENETUNREACH` / `EHOSTUNREACH` | `NoRoute` | 「无路由」 |
| 本机无非 loopback 地址 | `NoIfaceAddr` | 「无网卡地址可测（未联网）」，按钮置灰 |

### 3.2 Tauri 命令（src-tauri）

- `commands.rs` 增加 `port_reach_test(port: u16) -> ReachResult`（一次 connect，无状态）
- 不入轮询循环、不写库、不进事件通道

### 3.3 前端（src/components/PortList.tsx）

- 监听行的操作区加「测一下」入口，仅 `scope !== Loopback && scope !== Unknown` 时出现（S1）
- 结果行紧随其后：实测结果 + 推断面并列展示，两者不一致时（如推断 Any 实测 Refused）
  以实测为主文案，推断面降为副行——这是本功能的核心信息价值
- tooltip 固定一句口径说明：「同机回连实测，不等价于局域网其他设备可达」（S3）

## 四、里程碑拆解

| # | 内容 | 验收 |
|---|------|------|
| T1 | prune-core：`net/reach.rs` + `classify` 纯函数单测（四种 io error 映射） | `cargo test -p prune-core` |
| T2 | src-tauri：`port_reach_test` 命令 | `cargo check` |
| T3 | 前端：按钮 + 四态展示 + 进行中防抖 + 口径 tooltip | `tsc` 0 错误 |
| T4 | 实测：vite dev server（Any）→ Connected；停掉后 → Refused；随机高位端口 → Timeout 或 Refused | 手工验证记录 |

## 五、风险与对策

| 风险 | 对策 |
|------|------|
| macOS 应用防火墙对本机回连的处理随版本变化 | 结果只报观测事实（四态），不写「一定被谁拦了」的结论 |
| 用户误读「同机回连 = 别的设备也能连」 | S3 口径 tooltip + 副行保留推断面，两个口径永远并列 |
| 探针被滥用（循环点击） | 进行中禁用 + 一次一轮，无后台重试 |
| 双栈端口（IPv4+IPv6）测哪个 | 取端口快照里的第一个非 loopback 地址，结果行写明 tried_addr，不隐去 |

## 六、验收清单

- [ ] Loopback 端口无「测一下」入口（S1）
- [ ] 四态结果分开显示，无「通/不通」合并表述（S2）
- [ ] 实测与推断并列，冲突时实测为主、推断为副（S3）
- [ ] 超时 ≤ 2 s，进行中禁用重复触发（S4）
- [ ] 无任何常驻探测路径（grep 确认 reach 仅由命令触发）（S6）
- [ ] 结果不写「被墙 / 被防火墙拦截」类断言，只报观测事实
- [ ] `cargo test -p prune-core` 全绿 + `tsc` 0 错误（S5）
