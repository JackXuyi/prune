# M19 端口冲突保镖（F4）方案

> 灵动岛事件通道的第三类事件：关注端口从空闲变为被占、或同端口出现多个持有者时，
> 从刘海里长出一条「端口 3000 被 node 占用 · prune 项目」的提报。

## 一、背景与目标

**背景**：开发者最高频的资源问题是「谁占着 3000」。现状要主动切到端口 tab 才能看；
灵动岛已有 CPU / 内存两类事件通道（M15），端口数据与关注度规则（M7）全部现成，
只缺「状态变化 → 提报」这一环。

**目标**：机器上发生端口状态变化时，用户**不打开主窗口**就知道谁占的、属于哪个项目。

**成功标准**：

| # | 标准 |
|---|------|
| S1 | 关注端口（`is_watched` 命中）从空闲→被占，30 s 内弹出提报，含端口 / 进程 / 项目归属 |
| S2 | 同端口多持有者（SO_REUSEPORT / 冲突）出现时提报「冲突」 |
| S3 | 应用启动后的第一轮扫描只做基线，不连发存量端口（否则开机会弹十几条） |
| S4 | 同一端口冷却 10 分钟，dev server 重启横跳不刷屏 |
| S5 | 端口 tab 打开时不重复扫描（复用 PortState 的热快照） |
| S6 | `cargo test -p prune-core` 全绿 + `tsc` 0 错误 |

## 二、不做什么（本期边界）

- 不拦截 / 不结束占用进程（提报只指路，操作在端口 tab）—— 与「Docker 只给命令不执行」同一条纪律
- 不做自定义端口清单 UI（`WATCHED_PORTS` + `is_watched` 规则已覆盖，配置化留后续）
- 不给端口事件单独的设置分区：一个开关 `island.port_guard`（默认开）挂在灵动岛主区
- Windows 行为与 macOS 一致（扫描走 `win_ports` 同源接口），不做平台特化

## 三、方案

### 3.1 判定逻辑（prune-core，纯函数可单测）

落在 `crates/prune-core/src/island.rs`（与 CpuMonitor / MemMonitor 同址，职责一致）：

```
PortGuardMonitor
  baseline: bool                 // 第一轮只建基线
  occupied: HashSet<u16>         // 上一轮被占的关注端口（TCP）
  conflicts: HashSet<u16>        // 上一轮的多持有者端口
  cooldown: HashMap<u16, u64>    // 每端口 10 min 冷却（防重启横跳）

observe(now_ms, entries: &[PortEntry]) -> Vec<PortGuardAlert>
  1. 汇总本轮：TCP 且 listeners 非空 → occupied；owner_count() > 1 → conflicts
  2. 首轮：写基线，返回空（S3）
  3. 后续轮：
     - conflicts 新增 → Conflict 告警（优先于 NewListen）
     - occupied 新增且 e.watched → NewListen 告警
     - 每告警过一次的端口进入冷却；冷却中不再告警；端口转空闲即解除
  4. 端口转空闲：从 occupied / conflicts 移除（重新武装）
```

告警类型：

```rust
enum PortGuardKind { NewListen, Conflict }
struct PortGuardAlert { port, kind, comm, pid, project_label, exposure, owners }
```

### 3.2 采样（src-tauri，30 s 一拍，独立循环）

新增 `port_guard_loop`（挂在 `island::spawn_loops`）：

| 条件 | 行为 |
|------|------|
| `island.enabled` 关 或 `island.port_guard` 关 | 整拍跳过，常驻开销 0（与 M7 可见性门控同一纪律） |
| 端口 tab 活跃且 PortState 快照 < 20 s | 复用 `PortState.snapshot()`，不扫（S5，防两轮 lsof 并行） |
| 其余 | `spawn_blocking` 走 `net_ports::scan_cached`，**复用 PortState 的 ProjectCache**（热扫 ~50 ms，冷扫 cwd 缓存跨轮共享） |

频控：端口事件独立 `RateLimiter`（6/小时）+ 每端口 10 min 冷却。
不并进 sample_loop 的全局 Gates——端口事件天然稀疏，独立预算足够，避免为共享闸做跨循环重构。

### 3.3 事件 payload（`island://alert`，`kind: "port"`）

```json
{ "kind": "port", "port": 3000, "comm": "node", "pid": 5821,
  "projectLabel": "prune", "exposure": "loopback", "owners": 1,
  "conflict": false, "notchH": 33 }
```

`deliver()` 复用现有通道（显示 → 120 ms → emit；前端未就绪走 PENDING 补发），后触发者抢占前者。

### 3.4 前端（src/panels/Island.tsx）

- 折叠态三段式：插头图标 + 标签「端口」（冲突时「冲突」）+ 读数 `:3000`；强调色 `#0A84FF`（蓝，与 CPU/内存的橙/红区分，标签是颜色之外的第二识别通道）
- 展开态：大号 `:3000` + 副标题「被 node 占用」/「双持有者冲突」+ 明细行（项目归属 / 暴露面 / 持有者数）；暴露面为 `any` 时加「对外可达」提示（安全语义，M7 同口径）
- 点击：`openMain(undefined, "ports")` 直达端口 tab（其余事件仍开总览）

### 3.5 设置（两端同改，防 serde 断契约）

- `island.port_guard: bool`（默认 true）进 `Cfg` / `read_cfg` / `IslandSettings` get+set
- 前端 `SettingsBoard` 的 `IslandSettings` 接口 + DEFAULTS 同步加 `portGuard`，灵动岛主区加一行 Switch
- 后端为必填字段的字段，前后端必须同一次变更内落地（否则 `island_settings_set` 直接反序列化失败）

## 四、里程碑拆解

| # | 内容 | 验收 |
|---|------|------|
| T1 | prune-core：`PortGuardMonitor` + 单测（基线不弹 / 新占用弹 / 冲突弹 / 冷却 / 转空闲重武装） | `cargo test -p prune-core` |
| T2 | src-tauri：`port_guard_loop` + `fire_port` + 设置键 + PortState 快照复用 | `cargo check` |
| T3 | 前端：Island.tsx port 渲染 + 点击跳端口 tab + SettingsBoard 开关 | `tsc` 0 错误 |
| T4 | 实测：起/停一个 dev server，验证弹报 → 冷却 → 重武装全链路 | 手工验证记录 |

## 五、风险与对策

| 风险 | 对策 |
|------|------|
| lsof 每 30 s 一轮的常驻开销 | 热扫 ~50 ms（0.17% 占空比），且端口 tab 活跃时复用快照不双扫 |
| 项目归属（cwd 查询 20 ms/进程）放大开销 | 复用 PortState 的 ProjectCache，热路径只有新增进程 |
| `is_watched` 的 any-scope / has_project 规则面较宽，理论上噪声大 | 30 s 节拍 + 10 min 每端口冷却 + 6/h 频控三道闸兜底；实测过宽再收紧到 `WATCHED_PORTS` 白名单 |
| 双 lsof 并行（guard 轮撞上端口 tab 手动刷新） | tab 活跃时优先复用快照；残余重叠只是读命令并行，无一致性问题 |
