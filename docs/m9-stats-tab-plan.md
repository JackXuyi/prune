# M9 统计 Tab 方案（系统级资源总览，作为默认 tab）

> 状态：已实施（A0–A3），待真机走查
> 前置：M2/M5（内存与整机 CPU）、M4 S0（卷容量）、M7（端口）全部已落地
> 需求：把统计信息做成一个**单独 tab**，作为**默认 tab** 展示内存、硬盘占用的具体大小、CPU 占用百分比、端口占用数量

---

## 0. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 统计 tab 是**系统级总览**，不是应用级 | 四张大卡：内存 / 硬盘 / CPU / 端口，全部是整机读数 | 应用维度已有 M8 资源卡；用户要的「统计」是开机一眼看全的本机健康度 |
| 四个域里**三个零新增采集、一个加 0.1ms 轻量命令** | 内存/CPU 随内存常采已有；硬盘卷容量加 `disk_volume` 命令（statvfs 直读，不依赖全盘扫描）；端口复用 M7 扫描 | 默认 tab 不允许引入新的常驻开销 |
| 端口在统计 tab **只做一次性扫描，不挂 30s 定时轮** | 首次进入 tab 扫一轮（热扫实测 39.6 ms），切走即停 | 沿用 M7「tab 不可见开销为 0」的验收原则；定时轮仍只在端口 tab |
| 默认 tab 从「内存 · 应用」改为「统计」 | `useState<MainTab>("stats")`，`ui://tab` 跳转语义不变 | 打开 Prune 先看全局，再下钻；这是资源监视器的常规信息架构 |
| 硬盘「已用」标注为**粗略值** | 已用 = 总量 − 用户可用（statvfs f_bavail），脚注声明与 Finder 有 purgeable 差异 | macOS 没有权威「已用」口径，不标注就是假事实 |
| 点击卡片切到对应下钻 tab | 内存→概况、硬盘→存储、端口→端口 tab；CPU 无下钻页 | 总览 → 明细是同一个心智模型，不用再找 tab |

---

## 1. 需求理解与边界

**用户问题**（每个 UI 决策对照检查）：

1. 「我这台机器现在整体什么状态？」——打开 Prune 第一眼，不用切 4 个 tab 拼答案
2. 「内存用了多少？硬盘还剩多少？」——具体大小 + 百分比，不是趋势图
3. 「有多少端口被占着？」——数量级即可，明细下钻端口 tab

**边界**：

- ✅ 做：一个系统级总览 tab（四卡布局）+ 默认 tab 切换 + 卡片点击下钻
- ❌ 不做：应用排行（M8 已有）、流量统计（macOS 无公开 API）、GPU/能耗（边界外）、把「内存 · 概况」tab 合并进来（下钻语义仍需要它，合并留到后续评估）

---

## 2. 现状诊断（代码级）

| 域 | 现状 | 缺口 |
|---|---|---|
| 内存 | `MemSnapshot.physicalTotal / physicalUsedBytes / freeBytes / swap / pressure` ✅ 常采 10 s 推送 | 无（统计卡直接读 memStore） |
| CPU | `MemSnapshot.cpu: CpuInfo` ✅（M5：usage 每核平均 0–100 + 60 点 history） | 无（首轮 null 是既定语义，显示「采样中」） |
| 硬盘 | `VolumeInfo`（statvfs）✅ 但**只在全盘扫描装配报告时计算**（disk/mod.rs:776）；`diskReport()` 读的是持久化报告，首装/未扫描时可能为 null 或 volume 过期 | 加一个 `disk_volume` 轻量命令直调 `volume_info(home)`（statvfs ~0.1 ms，不触发扫描） |
| 端口 | `PortSnapshot` ✅ + `hydrate()`（只读）+ `refresh()`（触发扫描）+ 可见性门控 ✅ | 统计 tab 首次进入需要**主动扫一轮**才有数字，且不能挂定时轮 |

---

## 3. 口径定义（正确性核心）

### 3.1 内存卡

| 字段 | 口径 | 措辞 |
|---|---|---|
| 主读数 | `physicalUsedBytes` / `physicalTotal` | 「已用 X / 共 Y」+ 百分比 |
| 副读数 | 可用 `freeBytes`（macOS Pages free ≠ Windows AvailPhys，沿用概况 tab 的 hint 文案） | 「可用 X」 |
| 副读数 | swap 已用百分比（≥75% warn 色，≥90% danger，沿用概况 tab 阈值） | 「swap 已用 N%」 |
| 空值 | `physicalUsedBytes` 旧快照 undefined → 主读数显示「—」，**不是 0** | |
| 脚注 | footprint / RSS 口径徽标沿用 header（该域读数与内存 tab 同源） | |

### 3.2 硬盘卡

| 字段 | 口径 | 措辞 |
|---|---|---|
| 主读数 | `volume.totalBytes − volume.userAvailableBytes` / `volume.totalBytes` | 「已用 X / 共 Y」+ 百分比 |
| 副读数 | 用户可用 `userAvailableBytes`（主 UI 读数，与概况一致） | 「可用 X」 |
| 副读数 | 可回收 `DiskReport.totals.reclaimable`（报告存在才显示） | 「可回收约 X · 明细见存储 tab」 |
| **假事实声明** | macOS 的「已用」不守恒：f_bavail 扣除 purgeable，Finder 的「可用」也含 purgeable，两者对得上但「已用」没有权威值 | 脚注：「按 statvfs 粗略计算，含系统占用，与 Finder 显示可能有出入」 |
| 空值 | statvfs 失败（`volume == null`）→ 整卡显示「容量不可读」，**不显示 0/0 GB** | |
| 数据源 | 新命令 `disk_volume()`：直调 `prune_core::disk::volume_info(home)`，**不依赖扫描**；每次进入 tab / 点刷新时取一次 | |

### 3.3 CPU 卡

| 字段 | 口径 | 措辞 |
|---|---|---|
| 主读数 | `cpu.usage`（每核平均 0–100，M5 C0） | 「N%」 |
| 迷你走势 | `cpu.history`（60 点 @10 s）用现有 Sparkline | |
| 副读数 | 核数；macOS 加 load 1/5/15（Windows `load == null` 隐藏） | 「8 核 · load 2.1」 |
| 空值 | 首轮 `cpu == null` → 主读数「采样中…」（10 s 后自动到，M5 首见语义） | |
| **口径红线** | 整机 usage（每核平均）与进程/家族的单核口径**不守恒**，统计卡只呈现整机口径，不把两者并排放 | |

### 3.4 端口卡

| 字段 | 口径 | 措辞 |
|---|---|---|
| 主读数 | `entries.length`（去重后的 (port, proto) 监听条目数） | 「N 个监听端口」 |
| 副读数 | 关注层条数 / 对外暴露（`exposure === "any"`）条数，沿用 M7 口径 | 「关注层 X · 对外 Y」 |
| 副读数 | 活跃连接总数（`connsByPid` 全 pid established 求和） | 「活跃连接 Z」+ CLOSE_WAIT > 0 时 warn 提示 |
| 权限说明 | `coverage.note` 非空必须显示（覆盖面受限 ≠ 端口少）；未扫描时显示「尚未扫描」而不是「0 个端口」 | |
| 采集 | 首次进入统计 tab：`portSnapshot()` 为 null 则 `refresh()` 一轮（一次性，**不挂 30s 定时轮**）；切走 tab 不扫描；统计 tab 内点「刷新」会重扫一轮 | |

---

## 4. 布局（960×640，2×2 大卡）

```
┌──────────────────────┬──────────────────────┐
│ 内存                  │ CPU                   │
│ 9.2 GB / 16 GB  58%  │ 23%   ▁▂▄▃▂▁▂▄       │
│ ▓▓▓▓▓▓▓░░░░░░░      │ 8 核 · load 2.1       │
│ 可用 6.8G · swap 41% │ （点击无下钻）          │
├──────────────────────┼──────────────────────┤
│ 硬盘                  │ 端口                   │
│ 318 GB / 494 GB 64%  │ 27 个监听端口           │
│ ▓▓▓▓▓▓▓▓░░░░        │ 关注层 9 · 对外 3       │
│ 可用 176G · 可回收 12G│ 活跃连接 205            │
└──────────────────────┴──────────────────────┘
（每卡右上角小箭头 + 「查看明细」→ 切到对应 tab；CPU 卡除外）
口径脚注：按 statvfs 粗略计算……（折叠为 ⓘ tooltip，不占版面）
```

- 卡片即按钮（CPU 卡除外）：hover 高亮 + 右上 chevron，点击 `setTab` 下钻——**只导航，不放任何清理/结束动作**（破坏性操作入口不进统计 tab）
- 主读数字号加大（text-2xl 级）、等宽 tabular-nums，百分比与占用条同色系
- 占用条复用现有 CompareBar / Progress 样式语言，阈值色沿用各域既有 tone

---

## 5. 分期与改动清单

### A0 类型与命令（Rust + IPC）
- `src-tauri/src/commands.rs`：新增 `#[tauri::command] fn disk_volume() -> Option<VolumeInfo>`（同步、直调 core，无需 spawn_blocking——statvfs 微秒级）
- `src/lib/ipc.ts`：`diskVolume()`；浏览器态走 devFixture
- `src/lib/devFixture.ts`：`devVolume()` 样本

### A1 StatsBoard 组件 + tab 注册（纯前端）
- 新建 `src/components/StatsBoard.tsx`：四卡布局 + 口径脚注 + 下钻导航
- `src/windows/MainWindow.tsx`：`MainTab` 加 `"stats"` 并设为 `useState` 默认值；Tabs items 首位插入「统计」；`ui://tab` 白名单加 `"stats"`；刷新按钮分流（统计 tab = memRefresh + diskVolume 重取 + 端口一次性重扫）

### A2 端口一次性扫描门控
- `MainWindow.tsx`：`tab === "stats"` 时 `initPorts()`（含快照读取，无则扫一轮），**不调 `setActive(true)`**（30s 定时轮仍只归端口 tab）；切走时同样保证不扫
- 验收依赖 M7 既有门控机制，无 Rust 改动

### A3 文档同步
- README：能力表 + 首屏截图说明；`prune-feature-spec-v2.md` §2.1 边界；`prune-tauri-technical-plan.md` 命令表补 `disk_volume`

---

## 6. 假事实规避清单

| 陷阱 | 规避 |
|---|---|
| 硬盘「已用」被当成权威值 | 脚注声明 statvfs 粗略口径 + purgeable 差异 |
| 端口未扫描显示「0 个」 | 未扫描 = 「尚未扫描」，有 coverage.note 必须显示 |
| CPU 首轮 null 被当成 0% | 「采样中…」，10 s 后自动填充 |
| 整机 CPU% 与进程 CPU% 被并排做减法 | 统计卡只有整机口径 |
| 端口「对外暴露」被当成安全漏洞结论 | 沿用 M7 措辞：只是绑定范围事实 |
| 统计卡变成清理入口 | 只读 + 仅导航；白名单/确认流程不经过这里 |

---

## 7. 验收清单

1. 启动 Prune 直落「统计」tab，四卡在 1s 内出数（CPU 卡允许「采样中…」≤10s）
2. 内存/硬盘主读数与「概况」「存储」tab 同源一致（同一 store，不出现两套数字）
3. 首次进入统计 tab 触发且仅触发一次端口扫描；切走后无 30s 定时轮（对照 M7 验收方式）
4. 点击内存/硬盘/端口卡切到对应 tab；CPU 卡无跳转
5. `npx tsc --noEmit` 0 错误；`cargo test --workspace` 全绿；浏览器 fixture 走查 + `pnpm tauri dev` 真机确认四卡与下钻
6. 未扫描/不可读的域显示「尚未扫描 / 不可读」，全卡不出现编造的 0

---

## 8. 工作量

约 0.6 人天：A0 0.15 / A1 0.3 / A2 0.05 / A3 0.1。无新依赖、无定时器、无 Rust 采集新域（disk_volume 是对既有 `volume_info` 的一行包装）。
