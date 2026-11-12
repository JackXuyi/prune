# M19 局域网设备时间线 技术规格（spec）

> 配套文档：[m19-lan-device-times-plan.md](./m19-lan-device-times-plan.md)（需求定位与取舍过程）、[m18-lan-devices-spec.md](./m18-lan-devices-spec.md)（M18 扫描链路本体，本文不重复）。
> 本文只写**怎么实现**：字段契约、算法、接线点、UI 契约、测试与提交顺序。
> 已确认决策（2026-11-12）：阈值 `NEW_WINDOW_SECS = 600`（const，不做设置项）；卡片时间行只显示「活跃 x 前」，加入时间只进详情弹窗。

---

## 1. 目标与本质

给 LAN 设备加**跨轮次记忆**：

| 需求 | 数据来源 |
|---|---|
| 加入时间（首次发现） | 历史层 `first_seen` |
| 最后活跃时间 | 历史层 `last_seen`（本轮仍在网 → = 本轮 `at`；离线遗留 = 上次在线轮次） |
| 新设备标识 | `at - first_seen <= 600s` 且**非首扫基线** |

本质：每轮扫描后把设备身份与时间写进 `lan_history.json`，下一轮扫描把历史回填进 `LanDevice`。**Rust 侧算好，前端零推导**（与 `randomized_mac` 同口径）。

## 2. 架构决策

### 决策 1：设备身份 key 升级为「稳定 MAC 优先」，修订 M18 的「IP 为准」口径

M18 的 UI 文案写「跨轮次同一台设备以 IP 为准」——那是**没有跨轮次记忆**时的诚实表述（当时根本没有聚合）。M19 引入真实记忆后，若仍以 IP 为准，手机 DHCP 换 IP（家里的常态）会天天产生假「新设备」，直接摧毁「新设备标识」的可信度。所以：

| 场景 | key | 理由 |
|---|---|---|
| MAC 可用且非随机化 | `mac:<MAC>` | DHCP 换 IP 不算新设备；跨网段仍可认 |
| MAC 为 null | `ip:<IP>@<cidr>` | 绑定 cidr：换网后的同 IP 是另一台设备 |
| 随机化 MAC | `ip:<IP>@<cidr>` | MAC 会轮换，用它反而每轮都是「新设备」 |
| 同 MAC 多 IP | mac key 更新 `ip` 字段 | DHCP 续租换地址，视为同一台 |

**UI 文案同步修订**（`lan.footer` / `lan.randomMacNote` / `lan.modal.randomMacNote`）：聚合口径改为「稳定 MAC 优先；随机 MAC / 无 MAC 以 IP + 网段为准」。这是口径变更，不是实现细节——必须让用户看得到。

已知限制（不藏）：随机化 MAC + DHCP 双变化的设备无法追踪，会被误判为新设备——被动观测的物理上限。

> 与 M23 `LanWatchMonitor`（island.rs）的关系：那是**会话内新面孔告警**——内存态、
> 重启即重置基线、IP 键（其注释已文档化取舍），两套机制定位不同不合并。M19 历史层
> 是**持久时间线**：跨重启、噪声会随天数累积，所以值得 MAC 优先。

### 决策 2：历史层独立文件，不塞进 `lan-snapshot.json`

| 方案 | 否决理由 |
|---|---|
| 在快照缓存里嵌 history | 快照是「一轮的产出」，历史是「跨轮次的状态」，生命周期不同；MCP 读快照时会背上一坨无关状态 |
| **`lan-history.json` 独立文件** | ✅ `paths.rs` 加 `lan_history_path()`，与 `lan-snapshot.json` 同目录同原子写套路 |

### 决策 3：`is_new` 以 `snapshot.at` 为基准，不是墙钟

缓存快照（`lan_snapshot` 读盘）重启后打开，「10 分钟内加入」的判断不能因为隔了两天而失真——快照里是什么就是什么。前端相对时间同理（见 §6.2）。

### 决策 4：首扫基线

冷启动时所有设备 `first_seen == at`，不设基线判断会让第一次用 Prune 的用户满屏「新」。规则：**history 落盘前为空 ⇒ 本轮全员不标新**，`notes` 加一句基线说明。

### 决策 5：向后兼容

`LanDevice` 三个新字段全部 `#[serde(default)]`：旧版 `lan-snapshot.json`（无新字段）反序列化不炸，`first_seen = last_seen = 0`、`is_new = false`，UI 显示 `—`。升级不丢缓存快照。

## 3. 数据结构

### 3.1 `LanDevice` 增量（`crates/prune-core/src/net/lan.rs`）

```rust
/// 首次发现时间（Unix 秒；来自历史层，冷启动首扫 = 本轮 at；旧缓存 = 0）
#[serde(default)]
pub first_seen: i64,
/// 最后活跃时间（本轮仍在网 → = 本轮 at；离线遗留 = 上次在线轮次；旧缓存 = 0）
#[serde(default)]
pub last_seen: i64,
/// 新设备标识（服务端算好，前端只判真假不推口径；旧缓存 = false）
#[serde(default)]
pub is_new: bool,
```

前端 `src/types/lan.ts` 同步（`firstSeen: number` / `lastSeen: number` / `isNew: boolean`），保留「改这里必须同步 Rust 侧」铁律注释。

### 3.2 历史层（新文件 `crates/prune-core/src/net/lan_history.rs`）

```rust
/// 新设备窗口（spec：const，不做设置项）
pub const NEW_WINDOW_SECS: i64 = 600;
/// 不活跃回收线
pub const RETAIN_SECS: i64 = 30 * 86_400;
/// 历史总量上限（超出按 last_seen 最旧淘汰）
pub const HISTORY_CAP: usize = 1024;

#[derive(Serialize, Deserialize)]
pub struct LanHistory {
    pub version: u32,                       // 当前 1
    pub devices: BTreeMap<String, LanHistoryEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct LanHistoryEntry {
    pub first_seen: i64,
    pub last_seen: i64,
    /// mac-key 设备 DHCP 换地址后的最新 IP
    pub ip: String,
    /// ip-key 的绑定网段（换网即换锚）；mac-key 也带上便于诊断
    pub cidr: Option<String>,
}
```

只存身份 + 时间 + 定位信息，**不落 services / vendor / hostname**（隐私最小面，每轮扫描都能重取）。文件不进任何上报，不出本机。

### 3.3 纯函数 API（全部可单测，IO 另拆薄壳）

```rust
/// 设备身份 key（决策 1 的唯一落点）
pub fn device_key(d: &LanDevice, cidr: Option<&str>) -> String;

/// 回填 first_seen / last_seen / is_new 并更新历史。
/// 返回 baseline_existed（本轮是否在已有历史上跑——false = 首扫基线）。
/// 快照不可用（at == 0 或 cidr == None）时调用方不得调用本函数。
pub fn apply_history(snap: &mut LanSnapshot, history: &mut LanHistory) -> bool;

/// 回收 + 淘汰（在 apply_history 内部调用，独立出来只为单测）
pub fn gc_history(history: &mut LanHistory, now: i64);

/// IO 薄壳：读盘（失败/损坏 = 空 history，不阻塞扫描）、原子写（tmp + rename）
pub fn load_history() -> LanHistory;
pub fn store_history(history: &LanHistory);

/// 一步到位：GUI / CLI 都调它。at==0 或 cidr==None 的不可用快照直接跳过。
pub fn record(snap: &mut LanSnapshot);
```

`paths.rs` 追加：

```rust
/// M19 设备历史层（docs/m19-lan-device-times-spec.md §3.2）：跨轮次身份 + 时间。
pub fn lan_history_path() -> Option<PathBuf> {
    Some(config_dir()?.join("com.xuyi.prune").join("lan-history.json"))
}
```

## 4. 算法：`apply_history`

```text
baseline_existed = !history.devices.is_empty()

for device in snap.devices:
    key = device_key(device, snap.cidr)
    match history.devices.get_mut(key):
        Some(e)  → device.first_seen = e.first_seen
                   device.last_seen  = snap.at
                   e.last_seen = snap.at; e.ip = device.ip; e.cidr = snap.cidr
        None     → device.first_seen = snap.at
                   device.last_seen  = snap.at
                   插入 LanHistoryEntry { first_seen: at, last_seen: at, ip, cidr }

for device in snap.devices:
    device.is_new = baseline_existed && (snap.at - device.first_seen) <= NEW_WINDOW_SECS

gc_history(history, snap.at)

if !baseline_existed:
    snap.notes.push("本轮为首次扫描，已建立设备基线；后续扫描将标记 10 分钟内新接入的设备。")
```

`gc_history`：`last_seen < now - RETAIN_SECS` 剔除；总量 > `HISTORY_CAP` 按 `last_seen` 升序淘汰到上限内。

不变式（单测锁定）：

1. 命中 entry 时 `first_seen` 永不被覆盖（时间线只增不改）。
2. 首扫基线时任何设备的 `is_new` 都是 false。
3. `is_new` 只依赖 `snapshot.at` 与 `first_seen`，不读墙钟。
4. 历史文件损坏 ⇒ 空历史 ⇒ 本轮当基线，**不阻塞扫描主流程**。

## 5. 接线点

| 调用方 | 位置 | 动作 |
|---|---|---|
| GUI | `src-tauri/src/lan.rs` `lan_scan`：`snapshot_scan_cb` 返回后、`persist_cache` 前 | `lan_history::record(&mut snap)`；`persist_cache` 不动（字段已在快照里） |
| GUI 重启 | `lan_snapshot` 读盘 | 零改动——字段已在 `lan-snapshot.json`，`#[serde(default)]` 兜旧文件 |
| CLI | `prune-cli/src/main.rs` `cmd_lan`：`snapshot()` / `snapshot_scan()` 返回后 | 同样 `record(&mut snap)`；三端口径一致 |
| MCP | `mcp.rs` `tool_lan_devices` | **零改动**——透传缓存 JSON，新字段自动带齐；工具描述补一句时间线字段说明 |

CLI 文本输出追加 `[新]` 标记（`is_new` 为 true 时设备行前缀）；`first_seen > 0` 时行尾附「加入/活跃」相对时长。CLI 无 i18n，中文硬编码与现有输出一致。

## 6. 前端契约

### 6.1 卡片（`LanDeviceCard`）

```
┌──────────────────────────────────────────────┐
│ [tile] 设备名  [自己] [新]              ●     │   ← is_new 时追加「新」badge（样式复用 badgeSelf token）
│         HuaweiTechno                         │   ← 副标题行不动
│ [192.168.0.115] [连了我:80] [服务tag]         │
│   活跃 5 分钟前                                │   ← 时间行：lastSeen > 0 才渲染
└──────────────────────────────────────────────┘
```

- `at`（`snapshot.at`）作为 prop 传入卡片与详情弹窗（`LanBoard` / `DeviceSection` / 离线折叠区逐层传），**不在卡片里读 store**——快照语义显式化。
- 时间行 `text-[10px] text-ink-faint`，独立一行不混进 tag 区；离线卡片照常显示（「活跃 2 天前」正是用户要的信息）。

### 6.2 详情弹窗

`<dl>` 在「设备类型」之后、「服务」之前追加两行：

| 字段 | 展示 |
|---|---|
| 加入时间 | `formatAgo(firstSeen, at)` + 次行小字 `formatDayClock(firstSeen)` |
| 最后活跃 | `formatAgo(lastSeen, at)` + 次行小字 `formatDayClock(lastSeen)` |

`firstSeen` / `lastSeen` 为 0（旧缓存）→ 显示 `—`。

### 6.3 `lib/format.ts`

```ts
/** 相对时间，锚定快照时刻而非墙钟（M19 spec 决策 3） */
export function formatAgo(tsSec: number | null, nowSec: number, locale: Locale = currentLocale()): string

/** `MM-DD HH:mm`（本地时区）；与 formatClock 同为 24 小时制 */
export function formatDayClock(tsSec: number | null, locale: Locale = currentLocale()): string
```

`formatAgo` 分档（档位是交互约定，与 `formatRelative` 同哲学）：`<60s` 刚刚（复用 `common.time.justNow`）/ `<60min` n 分钟前 / `<24h` n 小时前 / `<30 天` n 天前 / ≥30 天 `formatDayClock`。分钟/小时/天用 `Intl.RelativeTimeFormat`（与 `formatRelative` 同源），零新增时间单位 key。

### 6.4 i18n keys（`lan` namespace，zh/en 全量，过 `check:i18n`）

| key | zh | en |
|---|---|---|
| `lan.card.badgeNew` | 新 | new |
| `lan.card.lastActive` | 活跃 {ago} | Active {ago} |
| `lan.field.firstSeen` | 加入时间 | First seen |
| `lan.field.lastSeen` | 最后活跃 | Last active |
| `lan.field.timeMissing` | —（旧版快照无记录） | — (not in this snapshot) |

修订（口径变更，决策 1）：

| key | 修订后 zh |
|---|---|
| `lan.footer` | 尾句改「跨轮次「同一台设备」以稳定 MAC 优先；随机 MAC / 无 MAC 以 IP + 网段为准」 |
| `lan.randomMacNote` | 补「跨轮次聚合以 IP + 网段为准」 |
| `lan.modal.randomMacNote` | 同步修订括号内口径 |
| `lan.notes.baseline`（新增） | 首扫基线说明（Rust notes 是中文硬编码，前端不翻译 notes——与 M18 现状一致；此 key 仅留档备用，不实现） |

> 注意：M18 的 `notes` 本就是 Rust 侧中文硬编码直出（`snapshot_scan_cb`），M19 沿用，不引入 notes 翻译机制。

## 7. 测试清单

Rust（`lan_history.rs` 单测，全部纯函数）：

| 用例 | 断言 |
|---|---|
| `key_mac_stable` | 非 随机化 MAC → `mac:` 前缀；随机化 → `ip:` 前缀带 cidr |
| `first_scan_baseline` | 空 history ⇒ 全员 `is_new == false`，`first_seen == at`，notes 含基线说明 |
| `hit_preserves_first_seen` | 上一轮 entry 在 ⇒ `first_seen` 不变、`last_seen == 本轮 at` |
| `new_within_window` | `at - first_seen <= 600` 且 baseline 存在 ⇒ `is_new == true` |
| `not_new_beyond_window` | `at - first_seen == 601` ⇒ false |
| `mac_key_survives_ip_change` | 同 MAC 换 IP ⇒ 仍命中，entry.ip 更新 |
| `ip_key_binds_cidr` | 同 IP 换 cidr ⇒ 不命中，视为新锚 |
| `gc_retains_and_drops` | 30 天线内保留 / 超线剔除 |
| `cap_evicts_oldest` | 1024 + 10 ⇒ 总量 1024，被淘汰的是 last_seen 最旧 |
| `serde_default_legacy` | 无新字段的 JSON 反序列化 ⇒ 0 / 0 / false |

前端：`tsc --noEmit` 0 错误 + `check:i18n` 通过（类型体操锁 zh/en 对齐）+ `npm run build` 全绿。

## 8. 提交顺序（对齐逐条可编译门禁）

| # | 内容 | 门禁 |
|---|---|---|
| 1 | `feat(lan): LanDevice 增加 first_seen/last_seen/is_new（serde default 兼容旧缓存）` | cargo test（旧测试补新字段） |
| 2 | `feat(lan): 设备历史层 lan_history（key 策略 + apply_history + gc + 单测）` | cargo test |
| 3 | `feat(lan): GUI/CLI 接线 merge_history，CLI 输出新设备标记` | cargo build --workspace |
| 4 | `feat(lan): 前端时间线——formatAgo/formatDayClock + 卡片/弹窗 + i18n 口径修订` | tsc + check:i18n + build |
| 5 | `docs: m19 spec/plan + CHANGELOG + bump` | npm run bump |
