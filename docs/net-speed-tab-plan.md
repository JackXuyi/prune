# 网络 Tab 方案（下载 / 上传速度统计 + 每日流量落库）

> 状态：**已实施（2026-11-06，A0a–A4）**，真机走查待做
> 前置：M10 应用网速（`nettop` 按进程差分）、M13 菜单栏总网速（`sysinfo::Networks` 2 s 常驻）、M9 统计 tab
> 需求：新增一个**网络下载 / 上传速度**的统计 tab —— 本机当前速率、走势、网卡分布、按应用排名、**今日流量**，一屏看完
> 修订：v2 按用户拍板改为「今日流量落 SQLite + 接受菜单栏口径纠错」（本版）

---

## 0. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 做**新 tab「网络」**，位置在「存储」之后、「端口」之前 | 与「端口」相邻成网络域：先看流量、再看监听 | 7 个 tab 仍在 960 px 头部装得下（现有 6 个约占 370 px） |
| 主读数用 `sysinfo::Networks` **计数器差分**，不用 `nettop` | 零权限、跨平台、< 1 ms/拍；`nettop` 14.5 ms/拍只配「按应用」区块 | 主数字必须常驻好看，不能依赖一个每秒 spawn 子进程的私有 CLI |
| **复用现有 2 s 常驻轮**，改自适应节拍：tab 可见 1 s / 不可见 2 s | 同一份状态、同一个循环；不新起第二个采样器 | 两套循环必然出现「菜单栏数字和 tab 数字不一致」 |
| 主数字**只累加物理网卡**，虚拟口（utun/awdl/bridge…）单列不计入 | VPN 隧道流量会与物理口**重复计一次**，不分就翻倍 | `utun3` 的字节数不是额外流量，它已经在 `en0` 上算过一次 |
| **今日流量落 SQLite**：`net_daily` 一天一行；另给「本次会话」两个数 | 用户要的是「今天用了多少」，重启不能归零 | 会话数只回答「这次开了多久」，回答不了「今天」 |
| 落库节奏：内存**逐拍**累加，每 **60 s** 增量 UPSERT 一次；跨天先 flush 再切日 | 1 s 一写让 WAL 每秒抖动；60 s 的丢帧上限可接受且能写清楚 | 写的是「增量相加」而不是覆盖，天然幂等、重启不清零 |
| 「今日」口径固定为**「Prune 运行期间累计」**，文案与 tooltip 写死 | 系统计数器只给「开机至今」，**无法回填 Prune 未运行的时段** | 多写一句限定，就少一个「这就是全天流量」的假事实 |
| 落库了就要看得见：网络 tab 给一条**近 7 天柱**（下/上堆叠） | 否则用户无法判断落库是否在工作 | 缺数据的日子留空灰显，不画成 0 柱 |
| 日期用 `libc::localtime_r`（Unix）/ `GetLocalTime`（Windows），**不引 chrono** | 只为「今天」加一个日期库不划算；`libc` 已是 core 依赖、Windows feature 已开 | 抽 `local_day(ts) -> "YYYY-MM-DD"` 纯函数，单测只断言格式与稳定性 |
| 先修 **P0 计时 bug**：`run_total_speed` 用 `started.elapsed()`（refresh 耗时）当分母 | 分母是 0.1–1 ms 而不是 2 s，速率被放大约 10³ 倍 | 新 tab 的主数字与菜单栏同源；**用户已确认接受修复后数值变小**（详见 §2.2 / §11） |
| 「按应用」区块复用 `netStore`（nettop），门控合并为「资源 tab **或** 网络 tab 可见」 | 两个 tab 抢同一个 `set_active`，各自 effect 会互相关闸 | 必须收敛成一个开关，否则从资源 tab 切到网络 tab 会互相把对方关掉 |
| 只读 + 只导航 | 不提供限速 / 断网 / 杀进程入口 | Prune 的破坏性动作只在存储与进程明细里，网络 tab 不是第八种工具 |

---

## 1. 需求理解与边界

**用户问题**（每个 UI 决策都要能对上其中一条）：

1. 「现在到底跑多快？」——下载 / 上传两个大字，一眼可读，不用心算
2. 「刚才那波是不是很大？」——近 5 min 走势 + 本次会话峰值（带时刻）
3. 「是这块网卡在跑，还是走了 VPN？」——网卡分布，物理口合计 = 主数字
4. 「谁在偷跑带宽？」——按应用降序 Top 8，指向「应用资源」tab 深看
5. 「今天一共用了多少？」——今日 ↓/↑（落库，重启不清零）+ 近 7 天柱
6. 「这次开了多久、用了多少？」——会话累计 ↓/↑（与今日并排，两者含义不同）

**边界**：

- ✅ 做：系统级实时速率 + 走势 + 网卡分布 + 按应用 Top N + 今日/会话累计 + 峰值 + 近 7 天柱 + 口径脚注
- ✅ 做：`net_daily` 落 SQLite（本地日期、仅物理口、仅 Prune 运行期间、保留 90 天）
- ❌ 不做：按域名 / 对端拆流量、抓包、防火墙、限速、流量导出、跨设备同步、Windows 按进程网速（需 ETW）
- ❌ 不做：把「连接数」混进来 —— 连接多 ≠ 流量大（端口 tab 已有准确措辞，两处不能自相矛盾）

---

## 2. 现状诊断（代码级）

| 能力 | 现状 | 缺口 |
|---|---|---|
| 总网速采样 | `net_sampler.rs:298 run_total_speed`：2 s 常驻，`sysinfo::Networks`，排除回环 | 只存 `(down, up)` 一个二元组；无历史、无网卡维度、无事件、无累计 |
| 总网速状态 | `net_sampler.rs:258 TotalNetState { total: Mutex<Option<(f64,f64)>> }` | 无 `status`（ok/warmup/unavailable）、无累计、无峰值 |
| 前端消费 | 只有 `tray.rs:438 update_title_network` 写菜单栏标题 | **前端完全没有入口**：无 IPC 命令、无事件、无类型 |
| 按进程网速 | `NetSpeedState`（1 s `nettop`）+ `net://speed` + `netStore`，门控在 `MainWindow.tsx:142`（仅资源 tab） | 门控写死在单 tab 上；新增 tab 会与它互相关闸 |
| 持久化 | `prune_core::db::Store`（`prune.db`，SQLite WAL）已服务内存序列 | 只有 `mem_process` 一张表；无流量表、无按日聚合能力 |
| 日期能力 | 无 `chrono` / `time` 依赖 | 需要本地日期（`YYYY-MM-DD`）做「今日」切分 |
| 走势 | `Sparkline.tsx` 自绘，只有 `accent` / `warn` 两色 | 上下行需要两个可区分的颜色（第二条曲线） |
| 语言一致性 | 统计页端口卡写着「本机没有 per-process 字节计数口径」（`StatsBoard.tsx:358`） | M10 之后这句话已经过时，需要一起改 |

### 2.1 已有可复用的东西（不重造）

- `formatSpeed()`（`types/net.ts:269`）：B/s → `KB/s`/`MB/s`，直接复用
- `sysinfo` 已在 `src-tauri/Cargo.toml` 依赖里（M13 引入），**不新增依赖**
- `is_loopback_iface()`（`net_sampler.rs:252`）+ 两个单测：回环判定基线
- `netStore` / `netSpeedOfPids()`：按 pid 聚合到家族，按应用排名直接调
- `prune_core::db::Store`：`open` 时 `execute_batch(SCHEMA)`，往 `SCHEMA` 追加 `CREATE TABLE IF NOT EXISTS` 即可让老库自动补表
- `libc`（core 依赖）、`windows-sys` 的 `Win32_System_SystemInformation`（已启用）：本地日期不需要新依赖

### 2.2 P0：现有计时口径是错的（必须先修，用户已确认接受）

```rust
// net_sampler.rs:303-307（现状）
tick.tick().await;
let started = std::time::Instant::now();
nets.refresh(true);                                  // 耗时 ~0.1–1 ms
let Some((down, up)) = sum_total_speed(&nets, started.elapsed().as_secs_f64()) else { ... };
```

`NetworkData::received()` 的语义是「**自上次 refresh 起**收到的字节」（sysinfo 0.39.6 文档原文），
所以分母应当是**两次 refresh 的真实间隔（2 s）**，而不是 `refresh()` 自身耗时。
现状等价于「2 s 的流量 ÷ 1 ms」，数字被放大 10³ 量级 —— 菜单栏上那些 `2.1M` 很可能是 `2 KB/s`。

修法（同时拿到更稳的口径）：

- 用 `total_received()` / `total_transmitted()`（**自开机累计**）自行差分，自己记上一拍时刻；
- 分母 = `now - prev_at`，漏拍 / 延迟 tick 都不会失真；
- 负增量（网卡重置、计数器回绕）按 0 处理，不取绝对值；
- 新出现的接口首拍没有基线 → 记 0 且该接口标 `warmup`，**不拿开机累计值当速率**。

A0 第一步就是实测确认（对照 `nettop` 与 `ifconfig` 计数器 10 s 增量），把「放大了多少倍」写进提交说明与 release note。

---

## 3. 口径定义（正确性核心）

### 3.1 速率

| 项 | 口径 |
|---|---|
| 采集 | `Networks::refresh(true)` → 遍历接口读 `total_received/total_transmitted` |
| 速率 | `Σ(物理口 total 增量) / Δt`，`Δt = now − prev_at`，单位字节/秒 |
| 分母下限 | `Δt < 0.2 s` 时丢弃该拍（重复 tick / 时钟异常），不除零 |
| 时钟回拨 / 负增量 | 该接口这一拍记 0，并把该接口基线重置为当前累计值 |
| 接口消失 | 从列表移除（`refresh(true)` 已负责），同时清掉它的 prev，不留僵尸行 |

### 3.2 网卡分类（决定主数字是否可信）

| 类型 | 判定（名称小写前缀） | 计入主数字 | 展示 |
|---|---|---|---|
| `physical` | 不在下面任何一类 | ✅ | 网卡列表主行 |
| `loopback` | `lo` / `lo0` / `loopback*` | ❌ | 完全不显示 |
| `tunnel` | `utun*` / `tun*` / `tap*` / `wg*` / `ipsec*` / `ppp*` | ❌ | 「虚拟接口」折叠区 |
| `virtual` | `bridge*` / `awdl*` / `llw*` / `ap*` / `anpi*` / `vmenet*` / `veth*` / `docker*` / `vEthernet*` | ❌ | 「虚拟接口」折叠区 |

- **为什么不分就错**：开 VPN 时 `utun3` 与 `en0` 统计的是同一批字节（一份明文、一份密文），相加等于双计
- 虚拟口不丢信息：折叠区显示当前速率，一行小字注明「不计入主数字，避免与物理口重复」
- 已知局限写进脚注：无法从名称/MAC 判定「哪块是主网卡」，所以不做「主网卡」结论，只按速率排序

### 3.3 三种状态（假事实红线）

| 状态 | 触发 | UI |
|---|---|---|
| `warmup` | 首拍 / 刚从不可用恢复 / 全部接口都是新出现 | 「采样中…」（< 2 s），**不显示 0** |
| `ok` | 有基线且读到接口 | 正常数字；本拍为 0 时显示 `0 B/s` + 「本拍无流量」 |
| `unavailable` | 连读不到（接口列表为空 / 系统调用失败）连续 3 拍 | 「不可读：{note}」，整页不出现编造的 0 |

`ok` + 0 与 `unavailable` 必须严格区分 —— M8/M10 已经踩过「两者都显示 0 是同一个谎」。

### 3.4 聚合

| 指标 | 口径 | 措辞 |
|---|---|---|
| 峰值 | 本次会话内单拍最大值 + 时刻（不随窗口变化） | 「峰值 ↓ 8.1 MB/s · 18:02」 |
| 平均 | 当前窗口内**时间加权**均值（`Σ rate×Δt / ΣΔt`） | 「近 5 min 平均 640 KB/s」 |
| 会话累计 | `Σ max(0, Δtotal)`，仅物理口，自 Prune 启动 | 「本次会话 ↓ 812 MB」 |
| 今日累计 | 见 §3.5（落库，跨重启不清零） | 「今日 ↓ 12.4 GB · ↑ 1.2 GB」 |
| 走势 | 环形缓冲 900 点；`history(seconds)` 按时间窗口取，降采样到 ≤ 120 点（**桶内均值**，不是最大值） | 「近 5 min · 各自刻度」 |

### 3.5 今日流量（落库口径）

| 项 | 口径 |
|---|---|
| 定义 | 物理网卡 `total_received/transmitted` 的**正增量**之和，按**本地日期**归集 |
| 起算 | 当天 00:00 之后 **Prune 运行期间**的增量；Prune 未运行的时段**无法回填**（系统计数器只给「开机至今」） |
| 跨天 | 每拍比较 `local_day(now)`；变化 → 先把 pending flush 到旧日，再切新日从 0 计。切天那一拍归**新日**，误差 ≤ 1 拍（1–2 s） |
| 落库节奏 | 内存逐拍累加（UI 每秒准）；每 60 s + 跨天时把「上次之后的新增」UPSERT 一次（增量相加） |
| 幂等 | SQL 是 `down_bytes = down_bytes + excluded.down_bytes`，同一批增量重复提交才会双计 → 只由单一采样线程提交，单测覆盖 |
| 崩溃 | 最多丢 60 s；退出时 best-effort flush 一次（A0 收尾，Tauri `RunEvent::Exit`） |
| 库不可用 | `today = null`，UI 显示「今日流量不可用（存储打不开）」，会话累计与实时速率照常 |
| 保留 | `net_daily` 保留 90 天，flush 时顺带清理（每天最多跑一次） |
| 时区 | 本地时区；跨时区旅行可能把一天切成两段，记录在案不处理 |
| 对账 | 与活动监视器「已接收/已发送」对不上是正常的：它按接口含虚拟口、且覆盖全天；我们只算物理口、只算运行期间 |

### 3.6 与「按应用」的口径差

| 维度 | 主数字 | 按应用 |
|---|---|---|
| 数据源 | `sysinfo` 网卡计数器 | `nettop -P -t external`（按进程聚合） |
| 排除 | 回环 + 虚拟口 | 回环（`-t external`） |
| 量级差异 | 加密开销、隧道、系统级无主流量 | 只能看到有 socket 的进程 |

脚注一句话：「按应用合计与主数字不必相等（一份按网卡、一份按进程），差 5% 属正常」。

---

## 4. 数据契约

### 4.1 Rust 结构

```rust
// 纯数据 + 纯函数：放 crates/prune-core/src/net/traffic.rs（CLI 后续可复用）
pub enum IfaceKind { Physical, Tunnel, Virtual, Loopback }

pub struct DayTraffic {
    pub day: String,        // 本地日期 YYYY-MM-DD
    pub down_bytes: u64,
    pub up_bytes: u64,
}

pub struct NetTotalSample {
    pub at: i64,                    // 秒
    pub status: NetTotalStatus,     // ok | warmup | unavailable
    pub note: Option<String>,       // 非空 = 必须显示给用户的原因（含落库失败）
    pub down_bps: f64,              // 物理口合计
    pub up_bps: f64,
    pub interval_ms: u64,           // 上一拍真实间隔
    pub ifaces: Vec<IfaceRate>,     // 物理口在前，虚拟口在后
    pub session_down_bytes: u64,
    pub session_up_bytes: u64,
    pub peak_down_bps: f64,
    pub peak_up_bps: f64,
    pub peak_at: i64,
    /// 今日（Prune 运行期间累计）。None = 未落库 / 库不可用 —— **不是 0**
    pub today: Option<DayTraffic>,
    /// 上次成功落库时刻（0 = 还没落过）
    pub today_persisted_at: i64,
}
```

serde `rename_all = "camelCase"`，与 `src/types/net.ts` 一一对应（改一侧必须同步另一侧，沿用文件头注释的约定）。

### 4.2 IPC

| 命令 / 事件 | 签名 | 说明 |
|---|---|---|
| `net_total_snapshot` | `() -> Option<NetTotalSample>` | `null` = 还没采过（**不是「没有流量」**） |
| `net_total_history` | `(seconds: u32) -> Vec<NetTotalPoint>` | 窗口 60 / 300 / 900，降采样到 ≤ 120 点 |
| `net_traffic_recent` | `(days: u32) -> Vec<DayTraffic>` | 近 N 天升序；缺失日不返回（前端留空灰显） |
| `net_total_set_active` | `(active: bool) -> ()` | 只改节拍（1 s ↔ 2 s），不清基线 |
| `net://total` | 每秒广播 `NetTotalSample` | 前端订阅即得实时尾巴；payload 约 1–2 KB |

`net_total_set_active` 与 `net_speed_set_active` 语义不同，命名要让人一眼分清：
前者是**系统口径、常驻**（只是提速），后者是**按进程口径、开关**（不动就完全不 spawn `nettop`）。

### 4.3 SQLite（`crates/prune-core/src/db.rs`）

```sql
CREATE TABLE IF NOT EXISTS net_daily (
    day        TEXT PRIMARY KEY,   -- 本地日期 YYYY-MM-DD
    down_bytes INTEGER NOT NULL,
    up_bytes   INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
```

方法：

| 方法 | 语义 |
|---|---|
| `net_add(day, down_delta, up_delta, now)` | 增量 UPSERT：`ON CONFLICT(day) DO UPDATE SET down_bytes = down_bytes + excluded.down_bytes, ...` |
| `net_day(day) -> Option<DayTraffic>` | 启动时读回今日基线 |
| `net_recent(days) -> Vec<DayTraffic>` | 7 日柱数据（升序，只返回有记录的日） |
| `net_prune_before(day)` | 保留期清理（90 天） |

- 挂在既有 `prune.db`：往 `db.rs` 的 `SCHEMA` 常量追加建表语句，`Store::open` 的 `execute_batch` 会自动给老库补表
- 注入：`lib.rs:76-92` 里 `store.clone()` 一份给 `net_sampler::TotalNetState::new(store)`（原 `Arc` 被 `MemState::new` 吃掉，必须先 clone）
- 单测（`open_in_memory`）：同一天两次累加 = 相加；跨天两行独立；重开 store 读回同值（模拟重启）；`net_recent` 缺失日不出现；保留期删除

---

## 5. 布局（960×640）

```
┌────────────────────────────────────────────────────────────────────┐
│ 网络                                        实时 1 s · 仅物理网卡  │
│                                                                    │
│  ↓ 下载  1.2 MB/s                    ↑ 上传  100 KB/s              │
│  ▁▂▃▅▇▆▅▃▂▁▂▄▆█▇▅▃▂▁▂▃▅▇   ┄┄┄┄┄┄┄┄┄┄┄┄┄┄  （两条独立走势）     │
│                                                                    │
│  峰值 ↓ 8.1 MB/s · 18:02    平均 ↓ 640 KB/s                        │
│  今日 ↓ 12.4 GB / ↑ 1.2 GB   本次会话 ↓ 812 MB / ↑ 96 MB           │
├────────────────────────────────────────────────────────────────────┤
│ 近 7 天（下 / 上）                                                 │
│  ▂  ▃  ▁  ▅  █  ▄  ▌     10-30 … 11-04 · 已记录 6/7 天           │
├────────────────────────────────────────────────────────────────────┤
│ 网卡（物理口合计 = 上面的数字）                                    │
│  en0          ↓ 1.2 MB/s   ↑ 100 KB/s   ▓▓▓▓▓▓▓░░░░   812 MB       │
│  en5          ↓ 0 B/s      ↑ 0 B/s      ░░░░░░░░░░░   0 B          │
│  ▸ 虚拟接口 3 个（utun3 · awdl0 · llw0）不计入主数字               │
├────────────────────────────────────────────────────────────────────┤
│ 按应用（nettop · 仅非回环）                          ↓ 速率降序    │
│  Cursor       ↓ 980 KB/s  ↑ 12 KB/s                                │
│  WeChat       ↓ 120 KB/s  ↑ 4 KB/s                                 │
│  ▸ 其余 9 个应用                                                   │
└────────────────────────────────────────────────────────────────────┘
口径：网卡计数器差分（排除回环与虚拟口）；「今日」为 Prune 运行期间累计（本地日期，每分钟落库）；
按应用来自 nettop，两者不必相等。
```

**交互与视觉规则**：

- 上下行两条 `Sparkline` **各自归一化**，并在图上标注「各自刻度」——量级常差 10 倍，共享纵轴会把上传压成直线
- 下行用 `accent`（`#34d399`），上行用新增 `info`（`#7dd3fc`）；`Sparkline` 增加 `tone="info"`，不改现有调用
- 今日 / 会话并排但**必须能分清**：今日文案带「今日」+ hover tooltip「Prune 运行期间累计，未运行时段不计」；会话带「本次会话」
- 7 日柱：下行为主柱、上行为叠加上段（同色系深浅），柱宽按可用宽度均分；缺失日画 1 px 基线灰条 + 不显示数值，底部标「已记录 N/7 天」
- 落库失败（连续 2 次 UPSERT 报错）：今日数字右侧出现 `⚠ 未落库`，并把 `note` 带到页面脚注，不假装成功
- 网卡按当前速率降序；速率与累计都为 0 的物理口排到最后，仍显示（有线口常驻 0 是事实，不是噪音）
- 虚拟接口默认折叠（一行小字 + 展开），避免一排 `awdl0 0 B/s` 抢注意力
- 按应用 Top 8 + 「其余 N 个」展开；数字与「应用资源」卡同源（`netSpeedOfPids`）
- 空态文案分三种：`warmup`「采样中…」/ `ok 且 0`「本拍无流量」/ `unavailable`「不可读：{note}」
- 页面内**不放任何按钮**（除头部统一的刷新）；点击应用行跳到「应用资源」tab 对应卡（可选，A3）

---

## 6. 分期与改动清单

### A0 数据层 + 采样器（含 P0 修复）— 0.35 人天

**A0a 日期与存储**
- 新建 `crates/prune-core/src/net/traffic.rs`，并在 `net/mod.rs` 加 `pub mod traffic;`
  - `local_day(ts: i64) -> String`：Unix 走 `libc::localtime_r`（读 `tm_year/mon/mday` 直接格式化），Windows 走 `windows_sys::Win32::System::SystemInformation::GetLocalTime`（feature `Win32_System_SystemInformation` 已在 `prune-core/Cargo.toml` 启用）；**不引 chrono**
  - `DayTraffic`、`IfaceKind`、`classify_iface(&str) -> IfaceKind`
  - 单测：格式 `^\d{4}-\d{2}-\d{2}$`；同一秒多次调用稳定；`ts + 86400` 必得不同日；不硬编码时区期望（避免 CI 时区差异）
- `crates/prune-core/src/db.rs`：`SCHEMA` 追加 `net_daily` 表 + `net_add / net_day / net_recent / net_prune_before` + 5 项单测

**A0b 采样器**
- `src-tauri/src/net_sampler.rs`：重写 `TotalNetState`
  - `ring: Mutex<VecDeque<NetTotalPoint>>`（900）、`prev: Mutex<Option<(HashMap<String, Totals>, Instant)>>`、`session`、`peak`、`status`、`active`
  - 今日：`today: Mutex<Option<DayTraffic>>` + `pending: Mutex<(u64, u64)>` + `persisted_at`
  - `run_total_speed`：tick 500 ms，按 `active` 选 1 s / 2 s 目标间隔；到点才采一拍
  - **修复分母**：用 `total_received/total_transmitted` 差分 + `prev_at`（§2.2）
  - 每拍：更新 session/today/pending/peak/ring → `emit("net://total")` → `tray::update_title_network`（同一份数字，不再各算一遍）
  - 每 60 s 或跨天：`store.net_add(today, pending…)`，成功后清 pending、写 `persisted_at`；失败累加 `flush_failures` 并在 `note` 里体现
  - 纯函数抽出：`diff_totals`、`downsample(points, secs)`、`accumulate_today(prev_day, now_day, delta)`
- 新增命令 `net_total_snapshot` / `net_total_history` / `net_traffic_recent` / `net_total_set_active`
- `src-tauri/src/lib.rs`：`:65-66` 注册命令、`:87-92` `store.clone()` 后 manage `TotalNetState`、`:119-124` 保持单一循环；`RunEvent::Exit` best-effort flush
- **验收动作**：与 `nettop` 合计 / `ifconfig` 10 s 增量对齐（误差 < 20%），把修复前后倍数写进提交说明

### A1 前端类型 / IPC / store — 0.2 人天
- `src/types/net.ts`：`IfaceKind`、`IfaceRate`、`NetTotalStatus`、`NetTotalSample`、`NetTotalPoint`、`DayTraffic`、`ifaceKindLabel()`
- `src/lib/ipc.ts`：四个封装；`openMain` 的 tab 联合类型加 `"net"`
- 新建 `src/stores/netTotalStore.ts`：`sample` / `history` / `days` / `init()`（快照 + 历史 + 7 日 + 订阅 `net://total` 追加，按 `at` 去重，环形上限 900）/ `refresh()` / `setActive()`
- `src/lib/devFixture.ts`：`devNetTotal()` + `devTrafficDays()`（含 en0 / utun3、6 天数据，浏览器可单独走查 UI 与 7 日柱）

### A2 `NetBoard` 组件 — 0.5 人天
- 新建 `src/components/NetBoard.tsx`：hero 双读数 + 双走势 + 四个聚合指标（峰值 / 平均 / 今日 / 会话）+ 7 日柱 + 网卡列表（虚拟折叠）+ 按应用 Top 8 + 口径脚注
- 7 日柱：组件内联 `DayBars`（纯 SVG，不引图表库），缺失日灰条、`已记录 N/7 天`
- `src/components/Sparkline.tsx`：`tone` 增 `"info"`
- 复用 `useNetSpeedStore` + `useMemStore` 做按应用排名；`netStore` 为 `null`/`unavailable` 时该区块降级但**不影响主数字与今日**
- 组件内不订阅、不采样（数据全部来自 store），保持「谁可见谁负责」的既有分工

### A3 tab 注册与门控 — 0.15 人天
- `src/windows/MainWindow.tsx`
  - `:36` `MainTab` 加 `"net"`
  - `:205-220` Tabs 插入「网络」（hint：「本机下载 / 上传速率、走势、网卡分布与今日流量」）
  - `:88-106` `ui://tab` 白名单加 `"net"`
  - `:142-153` **门控合并**：`const netVisible = tab === "resources" || tab === "net"`，一个 effect 管 `setNetActive`；网络 tab 另外 `setTotalActive(true)` 提节拍，切走 `setTotalActive(false)`
  - `:161-196` `busy` / `onRefresh` / `refreshLabel` 增加网络分支（刷新 = 重取快照 + 历史 + 7 日，不打断采样）
- 门控失败只降节拍/耗电，不报错（与 `netStore` 同纪律）

### A4 文档同步 — 0.15 人天
- README「它做什么 / 不做什么」：✅ 加「网络 tab：下载 / 上传速率 + 走势 + 网卡分布 + 今日流量（本地日期、落 SQLite、仅 Prune 运行期间）+ 按应用 Top N」；❌ 列保留「按域名/对端拆流量」，删掉「累计流量统计」（已做）
- README 路线图补 **M14**；release note 写明菜单栏网速口径纠错（数值会变小）
- `docs/prune-feature-spec-v2.md` §2 边界表同步（累计流量从 ❌ 移到 ✅，附口径限定）
- `StatsBoard.tsx:358` 过时措辞改掉（「连接多 ≠ 流量大」保留，删「本机没有 per-process 字节计数口径」）

### A5（可选，默认不做）统计页加网络卡 — 0.15 人天
- `StatsBoard` 从 2×2 变 2+3（网络卡 + 一行占位说明），主读数 ↓/↑ + 32 px 走势，点击下钻网络 tab
- 风险：破坏 M9 已验收的四卡版式；等真实使用反馈再决定

---

## 7. 假事实规避清单

| 陷阱 | 规避 |
|---|---|
| 用 `refresh()` 耗时当分母 | 改成 `now − prev_at`；A0 必须实测对齐（§2.2） |
| VPN 开着时主数字翻倍 | 虚拟口不计入主数字，单列展示并注明原因 |
| 「今日」被读成「全天流量」 | 文案固定「Prune 运行期间累计」+ tooltip；脚注声明无法回填未运行时段 |
| 首拍 / 恢复首拍显示 0 | `warmup` → 「采样中…」 |
| 读不到显示 0 | `unavailable` + `note`，整页不出现编造数字 |
| 库不可用却显示「今日 0 B」 | `today = null` → 「今日流量不可用（存储打不开）」；`DayTraffic{0,0}` 才是「今天没流量」 |
| 重复 flush 双计 | flush 只提交上次之后的新增，SQL 增量相加；单测覆盖连续两次 flush |
| 跨天把前一天的量算进新一天 | 每拍比较 `local_day`；变化先 flush 旧日再切；误差 ≤ 1 拍并写进注释 |
| 落库失败被吞掉 | 失败计数进 `note`，UI 显示「⚠ 未落库」，不假装成功 |
| 7 日柱把「Prune 没运行的日子」画成 0 | 缺数据日画灰条不画柱，底部标「已记录 N/7 天」 |
| 新接口首拍把开机累计当速率 | 该接口首拍记 0，并把当前累计设为基线 |
| 睡眠唤醒 / 漏拍 → 尖峰 | 用累计计数器差分 + 真实 Δt，天然免疫；Δt < 0.2 s 丢弃 |
| 峰值随窗口变化导致「峰值」名不副实 | 峰值定义为**本次会话**最大，与窗口解耦 |
| 两条曲线共享纵轴 → 上传被压平 | 各自归一化 + 图上标注「各自刻度」 |
| 按应用合计与主数字不等被当成 bug | 脚注固定解释两套口径 |
| 与「端口」tab 抢同一个 `set_active` | 合并为 `netVisible` 一个开关 |
| 网络 tab 长成第二个系统监控工具 | 只读、只导航；无按钮、无限速、无断网 |

---

## 8. 风险与取舍

| 风险 | 影响 | 处置 |
|---|---|---|
| 菜单栏数字修复后**变小**（可能小 1000 倍） | 用户会以为「网速统计坏了」 | **已确认接受**；提交说明 + release note 写明「此前口径错误」，网络 tab 上线即新口径 |
| 用户对「今日」数值有异议（对比活动监视器/路由后台） | 信任受损 | 脚注写清三条差异：只算物理口、只算运行期间、按本地日期；不做「校准」 |
| SQLite 每秒写放大 | WAL 抖动、耗电 | 60 s 一次单行 UPSERT；WAL 自动 checkpoint；不做每拍写 |
| 每日一行长期增长 | 体积 | 保留 90 天，flush 路径顺带清理 |
| 时区/旅行把一天切成两段 | 日统计有断点 | 记录为已知限制，不做补偿 |
| `sysinfo` 接口命名差异（`en0` / `en5` / Thunderbolt 网桥） | 黑名单漏项 → 双计或少算 | 单测覆盖 macOS/Windows 常见名；未知名称默认 `physical`（宁可多算真实物理口） |
| 1 s 事件的 IPC 开销 | 主窗口隐藏时也在广播 | payload ~1–2 KB/拍；只在窗口存活时监听；窗口不可见停发留 TODO，不首期做 |
| nettop 与 sysinfo 对不上 | 用户质疑「哪个是真的」 | 两处各自标注来源，不做「校正」 |
| 采样循环与 tray 标题刷新耦合 | 修改采样器可能影响菜单栏 | 标题仍由 `update_title_network` 单点写入，数字来自同一拍样本 |

---

## 9. 验收清单

1. 切到「网络」tab，≤ 1 s 出当前速率（首拍允许「采样中…」，≤ 2 s）
2. 下载 100 MB 文件：主数字与活动监视器 / `nettop` 合计同量级（±20%），**不出现 GB/s 级数字**
3. 菜单栏数字与 tab 主数字**同一时刻一致**（同源）
4. 断开 Wi-Fi：显示 `0 B/s` + 「本拍无流量」，不是「不可读」
5. 开 VPN 期间，主数字不因 `utun*` 重复计数而接近翻倍；虚拟接口在折叠区可见
6. 拔插 USB 网卡 / 新建 VPN：新接口不产生巨大尖峰；消失的接口不留残留行
7. `nettop` 不可用（改名 PATH 模拟）：按应用区块降级为「不可读」，主数字与今日照常
8. 切走网络 tab 后 `pgrep nettop` 为空；切到「应用资源」tab 后按应用仍有数（门控未互相关闸）
9. 合盖 30 min 唤醒：首拍不出现异常峰值
10. **重启 Prune**：「今日」不清零（读回同值），重启期间的流量不计入（不出现回填的假数字）
11. 把系统时间调到 23:59 观察跨天：前一天的量留在旧日，新日从 0 起；`net_daily` 出现两行
12. 把 `prune.db` 设为只读 / 删除：今日显示「不可用」，会话与实时照常，无 panic
13. `sqlite3 prune.db "select * from net_daily"`：每分钟 `updated_at` 变化，`down_bytes/up_bytes` 单调不减
14. 7 日柱：缺数据日灰显，底部「已记录 N/7 天」文案与库内行数一致
15. `npx tsc --noEmit` 0 错误；`cargo test --workspace` 全绿；浏览器 fixture 与 `pnpm tauri dev` 真机各走查一遍
16. 页面无任何写操作入口；「连接多 ≠ 流量大」与端口 tab 措辞不冲突

---

## 10. 工作量

| 阶段 | 内容 | 人天 |
|---|---|---|
| A0a | 日期纯函数 + `net_daily` 表与方法 + 单测 | 0.15 |
| A0b | 采样器重写 + P0 修复 + 今日累加/落库 + 命令 + 实测对齐 | 0.2 |
| A1 | 类型 / IPC / store / fixture | 0.2 |
| A2 | `NetBoard` + 7 日柱 + Sparkline 第三色 | 0.5 |
| A3 | tab 注册 + 门控合并 + 刷新分流 | 0.15 |
| A4 | README / spec / release note / 过时措辞 | 0.15 |
| **合计** | | **≈ 1.35** |
| A5（可选） | 统计页网络卡 | +0.15 |

无新依赖（日期用 `libc` / `windows-sys`，SQLite 用既有 `rusqlite`）、无新权限、无新的常驻进程（复用 M13 的 2 s 循环，只在 tab 可见时提速到 1 s）。

---

## 11. 决策记录（2026-11-05 拍板）

| # | 议题 | 结论 |
|---|---|---|
| 1 | tab 名称 | **「网络」**（不用「网速」：这一页还含走势、网卡与今日流量，命名要宽一档） |
| 2 | 累计口径 | **每日流量落 SQLite**（`net_daily`）；同时保留「本次会话」并排展示，两者含义不混 |
| 3 | 菜单栏数字 | **接受修复后数值变小**（口径纠错）；release note 说明 |
| 4 | 其余 | 按方案推荐：物理口口径、1 s/2 s 自适应、900 点环形缓冲、只读 + 只导航、A5 统计页卡片默认不做 |

遗留（不影响开工，A2 走查后定）：
- 7 日柱与「今日/会话」两行的上下顺序，按 960×640 实际观感微调
- 若用户后续要看 30 天趋势，`net_recent(days)` 已支持，只需 UI 换成 30 根柱

---

## 12. 实施记录（2026-11-06）

### 12.1 落地文件

| 阶段 | 文件 | 说明 |
|---|---|---|
| A0a | `crates/prune-core/src/localtime.rs`（新） | 本地日期纯函数。**顺手合了一处重复实现**：`swap::local_day_start` 原本自己写了一遍 `localtime_r`，现在两处共用本模块 |
| A0a | `crates/prune-core/src/net/traffic.rs`（新） | `DayTraffic` / `IfaceRate` / `NetTotalSample` / `IfaceKind` / 接口名分类 |
| A0a | `crates/prune-core/src/db.rs` | `net_daily` 建表 + `net_add`（增量 UPSERT）/ `net_day` / `net_recent` / 90 天清理 + 4 项单测 |
| A0b | `src-tauri/src/net_sampler.rs` | `TotalNetState` 重写：累计计数器差分、900 点环形缓冲、会话累计、峰值、今日累加与 60 s 落库、`net://total` 事件 |
| A0b | `src-tauri/src/lib.rs` | 注册 4 个命令、注入 store、**退出前 flush** 未落库的量 |
| A1 | `src/types/net.ts` / `src/lib/ipc.ts` / `src/stores/netTotalStore.ts`（新） / `src/lib/devFixture.ts` | 类型、4 个 IPC 封装、store（快照 + 历史 + 7 日 + 事件追加）、开发态 fixture |
| A2 | `src/components/NetBoard.tsx`（新） / `Sparkline.tsx` | 页面；`tone="info"`（`#6aa9ff`）给上传线 |
| A3 | `src/windows/MainWindow.tsx` | tab 注册、`ui://tab` 白名单、门控合并、刷新分流 |
| A4 | `README.md` / `prune-feature-spec-v2.md` / `StatsBoard.tsx` | 边界表、里程碑、口径段落；修掉「本机没有 per-process 字节计数口径」这句已过时的话 |

### 12.2 与方案的三处偏差（都是实施中发现方案不够准）

1. **走势窗口 300 s，不是 900 s**：环形缓冲仍是 900 点（切走再回来曲线不变短），但 UI 只取近 5 min——
   960×640 里 15 min 曲线会被压成一条噪声带，反而看不出「现在快不快」。标注与取值必须一致，已在 store 顶部写死。
2. **`localtime` 抽成公共模块**：方案原打算在 `traffic.rs` 里再写一份 FFI，实施时发现 `swap.rs` 已有同样实现，
   两份跨平台 `unsafe` 各自演化迟早对不上（一处改了时区语义另一处没改）。合并成一个模块，两边都调它。
3. **门控必须合并成一个 effect**：方案只说「两个 tab 都要 nettop」，没写实现细节。若各写一个 effect 各自
   `setActive(false)`，在「应用资源 → 网络」之间切换时后执行的会把前一个刚开的开关关掉，出现「进了页面但按应用一直空」。
   现在一个 effect 管一个开关：`visible = tab === "resources" || tab === "net"`。

### 12.3 P0 修复证据

方案 §2 的推断在实施时拿到了实测数：同一时刻 `nettop` 2.0 MB/s，M13 那版口径算成 **2914 MB/s（约 1457 倍）**。
修复后两口径差 < 3%（差值来自 nettop 的进程归属与网卡层的封装开销）。这条写进了 `net_sampler.rs` 头部注释，
避免以后有人「优化」回用 `received()` 除以 refresh 耗时。

### 12.4 验收状态

| 项 | 状态 |
|---|---|
| `cargo test --workspace` | ✅ 全绿：224 passed / 2 ignored（core 172 + app 34 + cli 18），其中网络相关 26 项 |
| `npx tsc --noEmit` | ✅ 0 错误 |
| 浏览器 fixture 走查（960×640） | ⚠️ 部分：首屏 ✅（双主读数 / 双走势各自归一化 / 7 日柱含缺失日灰条）；网卡展开、按应用、脚注未走查（dev server 被中断） |
| `pnpm tauri dev` 真机走查 | ⬜ 待做（需看菜单栏数字真实量级、睡眠唤醒后不跳变） |
| 真机 `pgrep nettop` 切走为空 | ⬜ 待做 |
