# M19 局域网设备时间线方案：加入时间 / 最后活跃 / 新设备标识

> 需求：设备卡显示**最后活跃时间**，详情弹窗完整显示**加入时间**（首次发现）；加入 10 分钟内显示「新设备」标识。
> 状态：方案已确认（2026-11-12），未开工。对应代码：`crates/prune-core/src/net/lan.rs`、`src-tauri/src/lan.rs`、`src/components/LanDeviceCard.tsx`。

## 0. 结论先行

| 项 | 决策 |
|---|---|
| 本质改动 | 给 LAN 扫描加一层**跨轮次设备历史**（持久化 `lan_history.json`），扫描后 merge 回填 `first_seen` / `last_seen` / `is_new` |
| 计算位置 | **Rust 侧算好，前端零推导**（与 `randomized_mac` 等 flag 同口径） |
| 设备身份 key | MAC 可用且非随机化 → `mac:<MAC>`；否则回退 `ip:<IP>` |
| 新设备阈值 | `NEW_WINDOW_SECS = 600`（10 分钟），**const 定义，不做设置项**（已确认） |
| 首扫基线 | history 为空（冷启动首扫）时**全员不标新**，本轮作为基线（避免满屏「新」） |
| 相对时间基准 | 以 `snapshot.at` 为基准算「x 前」，不用 `Date.now()`（快照口径，页面停留不漂移） |
| 落点 | 卡片：标题行「新」badge + 时间行**只显示「活跃 x 前」**；详情弹窗：加入时间 + 最后活跃两行完整时间 |

## 1. 现状与差距

当前链路（M18 已落地）：

```
lan_scan (src-tauri)                          LanBoard (前端)
  → lan::snapshot_scan_cb                        ← lanStore.scan / lanSnapshot
    → merge() 合成 LanDevice（一次性，无记忆）      ← LanDeviceCard 渲染
  → persist_cache 写 lan.json（单文件覆盖写）
```

差距：

1. `LanDevice` 无 `first_seen` / `last_seen` 字段——每轮扫描都是「第一次见面」。
2. `lan.json` 只存最近一轮快照，没有跨轮次的历史层。
3. 前端 `LanDeviceCard` 没有时间展示位，`lib/format` 只有 `formatClock` 没有「x 前」相对时间。

## 2. 数据模型

### 2.1 Rust `LanDevice` 增量（crates/prune-core/src/net/lan.rs）

```rust
pub struct LanDevice {
    // ...现有字段不动...
    /// 首次发现时间（Unix 秒；来自历史层，冷启动首扫 = 本轮 at）
    pub first_seen: i64,
    /// 最后活跃时间（本轮扫描仍在网 → = 本轮 at；离线遗留 = 上次在线轮次）
    pub last_seen: i64,
    /// 新设备标识（服务端算好；前端只判真假，不重复推口径）
    pub is_new: bool,
}
```

前端 `src/types/lan.ts` 同步三字段（`number` / `boolean`），注释保持「改这里必须同步 Rust 侧」铁律。

### 2.2 历史层文件 `lan_history.json`

`paths.rs` 仿 `lan_cache_path()` 加 `lan_history_path()`，同目录：

```json
{
  "version": 1,
  "devices": {
    "mac:aa:bb:cc:dd:ee:ff": { "first_seen": 1762900000, "last_seen": 1762910000, "ip": "192.168.0.115", "cidr": "192.168.0.0/24" },
    "ip:192.168.0.10":       { "first_seen": 1762890000, "last_seen": 1762910000, "ip": "192.168.0.10",  "cidr": "192.168.0.0/24" }
  }
}
```

只存身份 + 时间 + 定位信息，**不落 services / vendor / hostname**（隐私最小面；这些每轮扫描都能重取）。写盘沿用 `persist_cache` 的 tmp + rename 原子写套路。

### 2.3 设备身份 key 策略（本方案最关键的设计点）

| 场景 | key | 理由 |
|---|---|---|
| MAC 可用且非随机化 | `mac:<MAC>` | DHCP 换 IP 不算新设备；同设备跨网段仍可认 |
| MAC 为 null（查不到） | `ip:<IP>` | 唯一可用的稳定锚 |
| 随机化 MAC | `ip:<IP>` | MAC 会轮换，用它反而每次都「新设备」 |
| 同 MAC 多 IP | mac key 更新 `ip` 字段 | DHCP 续租换地址，视为同一台 |

已知限制（写进 `notes` 不藏）：随机化 MAC + DHCP 双变化的设备无法追踪，会被误判为新设备——这是局域网被动观测的物理上限，与「随机化 MAC 约一半设备」的现实（spec §9）一致。

## 3. 合并算法（新函数 `merge_history`）

调用点：`src-tauri/src/lan.rs` 的 `lan_scan` 在拿到快照后、`persist_cache` 前：

```text
1. load_history()                       # 读盘失败 = 空 history（当首扫）
2. baseline_existed = !history.devices.is_empty()
3. for device in snapshot.devices:
     key = device_key(device)           # §2.3 策略
     match history.get(key):
       Some(e)  → first_seen = e.first_seen; last_seen = snapshot.at; 更新 e.ip
       None     → first_seen = last_seen = snapshot.at; 插入
4. for device: device.is_new = baseline_existed
              && (snapshot.at - device.first_seen) <= NEW_WINDOW_SECS
5. 回收：last_seen 距今 > 30 天剔除；总量 > 1024 按 last_seen 最旧剔除
6. write_history()                      # 原子写
7. 首扫时 notes.push("本轮为首次扫描，已建立设备基线；后续扫描将标记新接入设备。")
```

口径说明：

- **`is_new` 判定基准是 `snapshot.at` 不是墙钟**：缓存快照（`lan_snapshot` 读盘）重启后打开，「10 分钟内加入」的判断不会因为隔了两天而失真——快照里是什么就是什么。
- **首扫基线规则**（§0）：冷启动时所有设备 `first_seen == at`，若不设基线判断，第一次用 Prune 的用户会看到满屏「新」。基线首扫静默，notes 里一句话说明。
- **CLI / MCP 口径**：`snapshot()`（CLI `cmd_lan`）同样接 `merge_history`——读同一份 `lan_history.json` 并写回，保证 GUI / CLI / MCP `lan_devices`（spec §12 读缓存文件）三端时间线一致。MCP 读的 `lan.json` 字段天然带齐，无需额外改动。

## 4. 前端设计

### 4.1 卡片（LanDeviceCard）

```
┌──────────────────────────────────────────────┐
│ [tile] 设备名  [自己] [新]              ●     │   ← 标题行：is_new 时追加 accent「新」badge
│         HuaweiTechno                         │   ← 副标题行不动
│ [192.168.0.115] [连了我:80] [服务tag]         │
│   活跃 5 分钟前                                │   ← 新增时间行：只显示最后活跃（text-[10px] text-ink-faint）
└──────────────────────────────────────────────┘
```

- 「新」badge 复用 `badgeSelf` / `badgeUplink` 的样式 token（`bg-accent/[0.12] text-accent`），优先级排在「自己 / 上联」之后。
- **卡片时间行只显示「活跃 x 前」**（已确认）：加入时间信息密度低（大多为「n 天前」且不变），放详情弹窗完整展示；卡片保持最小视觉面。
- 时间行独立一行而非塞进 tag 区：tag 区会 flex-wrap 折行，时间信息混进去视觉权重不稳；固定放最后一行，弱色弱字号，不抢安全信号（「连了我」warn 描边）的注意力。
- 离线设备：时间行照常显示（「活跃 2 天前」正是用户需要的信息），`dimmed` 逻辑不变。

### 4.2 详情弹窗

`<dl>` 追加两个 `DetailRow`（放在「设备类型」之后、「服务」之前）：

| 字段 | 展示 |
|---|---|
| 加入时间 | `3 天前` + 次行小字绝对时间 `11-09 14:32` |
| 最后活跃 | `5 分钟前` + 次行小字绝对时间 `11-12 13:30` |

绝对时间用现有 `formatClock` 扩展一个带月日的变体，跨天信息不丢。

### 4.3 相对时间 `formatAgo`（lib/format.ts）

| 差值 | 文案 |
|---|---|
| < 60s | 刚刚 |
| < 60min | n 分钟前 |
| < 24h | n 小时前 |
| < 30 天 | n 天前 |
| ≥ 30 天 | 直接显示日期 |

i18n keys 全量走 `check:i18n` 门槛（en/zh 两份），相对时间函数签名带 `locale`，与 `phaseText` 同型——不存文案只存 key。

## 5. 边界与口径汇总

| 边界 | 处理 |
|---|---|
| 冷启动首扫 | 基线规则：全员不标新，notes 说明 |
| 读 history 失败 / 文件损坏 | 当空 history（当首扫），不阻塞扫描主流程 |
| 缓存快照重启后查看 | 字段已在 `lan.json` 里，`is_new` 以快照时刻为准，不失真 |
| 跨网段（换 Wi-Fi） | mac-key 设备照常认；ip-key 设备要求 cidr 相同才算命中，跨网段 ip-key 视为新锚（entry 保留，cidr 字段区分） |
| 历史膨胀 | 30 天不活跃剔除 + 总量 cap 1024，超出按 last_seen 最旧淘汰 |
| 随机化 MAC 轮换 | 已知限制，无法追踪，notes 不额外提示（§2.3 已有「· 随机」标注在先） |
| 隐私 | history 只存 ip / mac / 时间 / cidr，不出本机，不进任何上报 |

## 6. 实施步骤（T1–T5）

| 阶段 | 内容 | 验收 |
|---|---|---|
| T1 | Rust：`LanDevice` 三字段 + `paths::lan_history_path` + `merge_history` 模块 + 单测 | `cargo test` 全绿：首扫基线 / 命中保留 first_seen / 窗口内 is_new / ip-key 回退 / 30 天回收 / cap 淘汰 |
| T2 | IPC 接线：`lan_scan` 与 CLI `snapshot()` 均过 `merge_history`；`persist_cache` 不动 | 手动扫两轮：第二轮时间不重置、首轮 notes 有基线说明 |
| T3 | 前端：`types/lan.ts` 同步 + `formatAgo` + i18n keys（en/zh） | `tsc --noEmit` 0 错误；`check:i18n` 通过 |
| T4 | UI：卡片「新」badge + 「活跃 x 前」时间行；详情弹窗加入时间 + 最后活跃两行 | 空态 / 离线 / 随机化 MAC / 自己 / 上联五类卡片走查 |
| T5 | 回归 + CHANGELOG + 版本 bump | 手动双轮扫描截图对比 |

## 7. 已确认决策（2026-11-12）

1. **新设备阈值**：`NEW_WINDOW_SECS = 600`（10 分钟），const 定义，**不做设置项**。
2. **卡片时间行**：只显示「活跃 x 前」；**加入时间只进详情弹窗**。
