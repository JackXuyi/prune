# F18 闲置 dev server 提报（灵动岛）方案

> 竞品依据（2026-11-18 联网核实）：Vitals 1.11（2026-11-11）的 Developer Tab 实测发现
> 「跑了 16 小时、占 3.8 GB 的 dev server」是作者宣传的核心场景，验证了该需求真实高频。
> Prune 的数据层（M7 项目归属 + `PortSnapshot.conns_by_pid` 每 pid ESTABLISHED 计数）
> 已全部现成，缺的只是「闲置判定 → 灵动岛提报」一环。

## 一、背景与目标

**背景**：AI 编码 agent 并行跑任务后留下大量僵尸 dev server 是 2025–2026 新涌现的痛点
（DevCheck / PortKill / LeftOpen / Portero / PortAura 一年内十余个新品全在做这件事）。
Prune 已有端口保镖（F4，端口**变化**提报），但「一直被占着、却没人访问」的**闲置**状态
从未提报——用户不打开主窗口永远不知道。

**目标**：项目进程监听的 dev 端口持续零连接 + 零 CPU 消耗超过阈值时，
灵动岛长出一条「:3000 已闲置 32 分钟 · prune 项目」的提报。

**⚠️ 路线图对齐（先读）**：`prune-roadmap-2027h1.md` §1 结论 4 已裁决
「在四类事件的留存贡献被验证之前，**冻结第五类事件**」。本功能是**新事件类**，
因此本期以 **opt-in（默认关闭）** 落地：不改变冻结裁决的效力，给愿意尝鲜的用户通道；
M-B 访谈判定通过后（或用户访谈中点名此需求）再翻默认值。

**成功标准**：

| # | 标准 |
|---|------|
| S1 | 有项目归属的 TCP listener，连续 30 min 每 30 s 轮的 ESTABLISHED 计数恒为 0 且进程 CPU 均值 < 1% → 提报，含端口 / 进程 / 项目 / 闲置时长 |
| S2 | 同一闲置会话只提报一次；会话结束（出现连接或端口释放）后重新武装 |
| S3 | 每端口冷却 60 min + 全局频控（复用 island.rs 的 RateLimiter 模式），重启横跳不刷屏 |
| S4 | 无项目归属的监听（系统服务 / GUI 应用内部端口）永不触发 |
| S5 | `island.idle_server` 设置键默认 **false**；开启后行为符合 S1–S3 |
| S6 | `cargo test -p prune-core` 全绿 + `tsc` 0 错误 |

## 二、不做什么（本期边界）

- 不自动结束进程 / 不弹「一键停掉」按钮（纪律同 F4：提报只指路，操作在端口 tab）
- 不做跨机器 / 局域网维度的闲置判定
- 不把闲置判定接进 CLI watch / MCP（先在 GUI 事件通道验证价值，被验证后再扩形态）
- 默认关闭（见上），不进首扫引导链路，不加新设置分区（挂在灵动岛主区一行 Switch，F4 同款）

## 三、方案

### 3.1 判定逻辑（prune-core，纯函数可单测）

落在 `crates/prune-core/src/island.rs`（与 PortGuardMonitor 同址，同构）：

```rust
pub struct IdleServerMonitor {
    arming: HashMap<u16, Instant>,  // 进入「持续零连接」观测的起点（按 port）
    reported: HashSet<u16>,         // 本会话已提报的端口
    cooldown: HashMap<u16, u64>,    // 提报后 60 min 冷却
}

/// 输入全部现成：
///   entries: &[PortEntry]          —— project_label 归属（M7）
///   conns: &HashMap<i32, u32>      —— PortSnapshot.conns_by_pid 的 ESTABLISHED 计数
///   pid_cpu_pct: &HashMap<i32, f32> —— sampler 轮的进程 CPU（缺省视为 0，不阻塞判定）
pub fn observe(&mut self, now_ms: u64, entries: &[PortEntry],
               conns: &ConnIndex, pid_cpu: &CpuIndex) -> Vec<IdleServerAlert>
```

判定序列：

1. 过滤：TCP listener 且 `project_label.is_some()`（S4 由口径直接保证）
2. 每轮：该端口全部持有者的 `established == 0` 且 CPU < 1% → 端口进入/维持 arming；
   任一条件破坏 → 移出 arming 并清 reported（重武装，S2）
3. arming 满 30 min 且未 reported 且不在冷却 → `IdleServerAlert { port, pid, project_label, idle_minutes }`
   并进入 reported + 冷却（S2/S3）
4. 首轮只建基线不提报（F4 的 S3 纪律同款，防止开机连发存量）

阈值常量集中定义可调：`IDLE_MINUTES = 30`、`CPU_FLOOR = 1.0`、`COOLDOWN_MINUTES = 60`。

### 3.2 采样接入（src-tauri）

- 复用现有 30 s 端口轮（`port_sampler.rs` / PortState 快照），**不新增 lsof 轮次**；
  `conns_by_pid` 已在每轮快照里（net/ports.rs L55：LISTEN 与 ESTABLISHED 一起拿）
- 进程 CPU：优先复用 sampler 轮已有数据；缺失时该轮按 0 处理并在单测覆盖该分支
  （宁可漏报不可误报：established == 0 是主判据，CPU 是辅助）

### 3.3 事件 payload 与前端

- 灵动岛事件新增 `kind: "idle_server"`（与 port / cpu / mem / lan 并列），payload 字段
  `port / pid / comm / project_label / idle_minutes / exposure`
- `src/panels/Island.tsx` 折叠态标签「闲置」+ 读数 `:3000`；展开态副标题
  「已闲置 32 分钟 · 无访问连接」+ 明细行（项目 / 进程 / 暴露面）；点击 `openMain(undefined, "ports")`

### 3.4 设置（两端同改，防 serde 断契约）

- `island.idle_server: bool`（**默认 false**，S5）进 `Cfg` / `read_cfg` / `IslandSettings` get+set
- 前端 `SettingsBoard` 的 `IslandSettings` 接口 + DEFAULTS 同步加 `idleServer`，
  开关文案注明「实验功能，默认关闭」

## 四、里程碑拆解

| # | 内容 | 验收 |
|---|------|------|
| T1 | prune-core：`IdleServerMonitor` + 单测（30 min 触发 / 中途有连接清零重武装 / 已提报不重报 / 冷却 / 首轮基线 / 无项目归属不触发 / CPU 数据缺失分支） | `cargo test -p prune-core` |
| T2 | src-tauri：接入 30 s 轮 + `fire_island` 事件 + 设置键（默认 false） | `cargo check` |
| T3 | 前端：Island.tsx 渲染 + 点击跳端口 tab + SettingsBoard 开关 | `tsc` 0 错误 |
| T4 | 实测：起 vite dev server → 断开所有连接 → 30 min 后提报 → 访问一次 → 重武装 | 手工验证记录 |

## 五、风险与对策

| 风险 | 对策 |
|------|------|
| 误报（HMR 空闲期 / 长连接池静默） | 30 min 窗口 + established 主判据（HMR 断开仍会有零星连接则不触发）+ opt-in 默认关 |
| 与 PortGuard 事件打架（先弹「被占用」又弹「闲置」） | 状态机互斥：arming 中的端口不受 NewListen 重报冷却影响，两类事件字段可区分，实测观察后调优 |
| 每 pid CPU 采集放大开销 | 只对 arming 中的进程补采（≤ 3 个），主判据不依赖 CPU |
| 违背「冻结第五类事件」裁决 | 本方案显式以 opt-in 落地并在设置文案标注实验功能；默认值翻转必须等 M-B 判定，写入本文档作为门禁 |

## 六、验收清单

- [ ] 30 min 零连接 + 低 CPU 触发提报，含端口 / 进程 / 项目 / 时长（S1）
- [ ] 同会话只报一次，会话结束后重新武装（S2）
- [ ] 60 min 冷却 + 频控，重启横跳不刷屏（S3）
- [ ] 系统服务 / 无归属端口零触发（S4，单测含负例）
- [ ] `island.idle_server` 默认 false，两端同改无 serde 断裂（S5）
- [ ] 无「结束进程」入口，操作仍收口在端口 tab
- [ ] 首轮基线不弹存量（单测覆盖）
- [ ] `cargo test -p prune-core` 全绿 + `tsc` 0 错误（S6）
