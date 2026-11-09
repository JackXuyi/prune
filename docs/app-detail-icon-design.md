# 应用卡片图标 + 应用详情页 · 详细设计

> 版本 **v1.1** · 2026-11-09 · 承接 UI v4「Quiet Cockpit」S1–S5
> 范围：`应用` tab 的卡片图标化 + 卡片点击进入「应用详情」二级页 + **应用 7 天历史** + **复制诊断摘要**。
> 不改：安全护栏、dry-run 流程、既有 IPC 契约（仅**新增**图标 / 历史 / 剪贴板三类命令）。
>
> **v1.1 变更（对应四项决策）**
> 1. 卡片点击语义 = **整卡进详情**，取消卡内展开态（同 v1.0 推荐）
> 2. 详情页首版**含 7 天历史维度** → 新增 §5.5 数据层 + §5.6 图表规格（新增 1 张聚合表 + 1 条查询命令）
> 3. Windows 首版**全部字母徽标**（capability 门控，不硬编码平台判断）
> 4. 「复制诊断摘要」**首版就做** → 新增 §5.7（复用 `net_diag` 的 `pbcopy`/`Set-Clipboard` 通道，零新依赖）

---

## 1. 现状与问题

| 现状 | 问题 |
|---|---|
| 卡片只有文字标题（`Cursor` / `Chrome`），无视觉识别物 | 348 个应用、12 张首屏卡，靠读文字找目标，慢 |
| 点击卡片 → 卡内展开（磁盘三层 / 端口 / Top3 / 走势） | 展开态信息挤在 2 列卡宽里，磁盘三层与进程明细拥挤；跨卡对比时展开会打乱网格 |
| 明细能力分散在「内存 tab 的 ProcessInspector」 | 看一个应用的完整画像要跨 2 个 tab，且应用维度（磁盘三层 + 网速 + 端口）没有聚合页 |
| `ProcMem.exe` / `app_disk` / `connsOfPids` / `netSpeedOfPids` 数据都已存在 | 只在卡片摘要里用了一部分，详情页可零新增采集成本地上齐（**7 天历史除外**，见 §5.5） |

**结论**：图标解决「找得到」，详情页解决「看得全」，两者都是纯呈现层重构 + 一个图标提取命令。

---

## 2. 设计原则（沿用 v4 三条硬原则）

1. **图形优先**：图标 + 数字 + 微图表，文字降级为辅助。
2. **颜色即语义**：图标底色取域色的**低饱和染色**，不抢占域色本义（域色只给读数条/环）。
3. **有来有回**：卡片 → 详情 → 返回列表，保留排序/搜索/滚动位置；详情内可再下钻到进程明细（memory tab）与端口（ports tab），均有返回来源。
4. **假事实红线**：拿不到图标 → 字母徽标（**不显示系统占位灰图**）；拿不到读数 → `—` + 原因，不显示 0。

---

## 3. 图标方案（技术选型）

### 3.1 方案对比

| 方案 | 实现 | 视觉 | 成本 | 风险 |
|---|---|---|---|---|
| **A. Rust 提取 `.icns` → PNG 缓存（推荐）** | 从 `exe` 反推 `X.app/Contents/Resources/*.icns`，用系统 `sips -Z 64 -s format png` 转 PNG 落缓存目录，IPC 返回 `data:` URL | 真实应用图标，识别度最高 | 中（1 个命令 + 1 个 core 模块） | `sips` 不可用 / 无 icns 时降级；首次批量转换 ~几十 ms/个 |
| B. 纯前端字母徽标 | 取首字母 + 家族哈希色 | 统一、零成本 | 低 | 348 个应用里大量同字母，识别度低 |
| C. 引入 `icns` crate 解码 | 纯 Rust 解码 | 同 A | 中高（新依赖 + codec 维护） | 依赖体积、格式边界多 |

**决策**：A 为主，B 作为**永远存在的兜底**（所有平台/异常路径都落到 B）。Windows 首版全部走 B（`exe` 图标提取需要 winapi，V2 再做，capability 门控不硬编码）。

### 3.2 数据契约（新增）

```rust
// src-tauri/src/commands.rs
#[tauri::command]
pub async fn app_icons(items: Vec<IconQuery>) -> Result<HashMap<String, String>, String>
// IconQuery { family: String, exe: Option<String> }
// 返回：family -> "data:image/png;base64,..."（64×64）
// 失败语义：单个家族失败 → 不放进 map（前端落字母徽标）；命令本身不整体报错
```

- 缓存：`~/Library/Caches/<bundle-id>/icons/<fnv(exe)>.png` + 进程内 `Mutex<HashMap>`，TTL 同 `app_disk`（30 min）。
- 并发：8 路并行，单应用 300 ms 预算；超时按失败处理。
- 安全：只读系统文件，写只写自有缓存目录，不执行任何 `.app` 内二进制。

### 3.3 视觉规格

| 规格 | 卡片 | 详情页 |
|---|---|---|
| 尺寸 | 28 × 28 | 64 × 64 |
| 圆角 | 7px（≈ macOS squircle 观感） | 15px |
| 描边 | `inset ring 1px rgba(255,255,255,0.08)`（暗底防止深色图标糊掉） | 同左，1.5px |
| 失败兜底 | 首字母大写 + 家族哈希双色渐变（HSL 160–280 区间，饱和度 45%） | 同左，字号 26px |
| 加载态 | 保持字母徽标，图标到达后**交叉淡入** 180ms | 同左 |
| 无障碍 | `alt=""` + 卡片已有文本名（图标纯装饰） | `alt="Cursor 图标"` |

家族哈希取 `fnv1a(family) % 6` → 6 组预设渐变（与六域色同色温，避免突兀霓虹）。

---

## 4. 应用卡片改版

### 4.1 结构（尺寸标注）

```
┌──────────────────────────────────────────────────────────────┐ ← 1px hairline-soft，r=12
│ ┌────┐  Cursor                                    45 进程  › │  图标 28 · 名称 12.5/500 · 进程 10.5 mono
│ │icon│  — 端口 · 16 连接                                      │  副行 10 ink-faint（CPU 已在条形行）
│ └────┘                                                        │
│   CPU   8%   ▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░                  │  行高 18 · 标签 30 · 值 92 right · 条 3px
│   内存  ≈7.1 GB ▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░                  │
│   磁盘  975 MB  ▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░                  │
│   网速  ↓3.8k ↑120 B/s  ▓░░░░░░░░░░░░░░░░░░                  │
└──────────────────────────────────────────────────────────────┘
```

改动点（相对当前实现）：

| 项 | 现在 | 改后 | 理由 |
|---|---|---|---|
| 左侧 | `›` 折叠箭头 | **28px 应用图标**（`›` 移除） | 图标本身即「可点」暗示；折叠箭头与「进入详情」语义冲突 |
| 右侧 | 「45 进程」文本 | 「45 进程」+ `›` 尾部箭头（hover 右移 2px） | 明确的「进详情」方向暗示 |
| 点击 | 整卡展开（`aria-expanded`） | **整卡进入详情页**（`aria-label="打开 Cursor 详情"`） | 展开态与详情页信息重复，取消二义 |
| hover | 边框提亮 | 边框提亮 + 图标微放大 1.03 + `›` 右移 | 微动效叙事，尊重 reduced-motion |
| 键盘 | Enter/Space 展开 | Enter/Space 进入详情；`⌘↵` 保留原展开语义（兜底，不宣传） | 键盘可达 |

> 卡片不再有展开态 → `ResourceDetail` 组件整体迁移进详情页（磁盘三层按钮、端口 chips、Top3、60s 走势全部复用，不重写）。

### 4.2 列表头补充

- 搜索框左侧加 `显示应用图标` 开关？**不做**——图标是默认增强，不做可配项（减少决策噪音）。
- 排序 chip 增加「图标排序 12/348」不变。

---

## 5. 应用详情页（新增二级页）

### 5.1 布局（宽 960 内容区，单列滚动）

```
┌────────────────────────────────────────────────────────────────────────┐
│ ← 返回应用                                         ⌘R 刷新    ⋯ 更多   │ 吸顶 44px，滚动时保留
├────────────────────────────────────────────────────────────────────────┤
│ ┌────────┐  Cursor                                     [footprint 口径] │ Hero 88px
│ │ 64 icon│  /Applications/Cursor.app · 45 进程 · 16 连接               │
│ └────────┘  [在 Finder 显示]  [结束进程…]（危险，两步确认）[复制摘要]  │
├────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┐┌──────────────┐┌──────────────┐┌──────────────┐       │ 资源四联卡
│ │ CPU     8%   ││ 内存   ≈7.1GB││ 磁盘    975MB││ 网速  ↓3.8k  │       │ grid 4×1fr，gap 12
│ │ ▁▂▃▅▇▅▃▂▁▂▃ ││ ▓▓▓░░ swap   ││  本体/缓存/数据││ ↑120 B/s     │       │ 高 108
│ │ 近 10 min    ││ 12% 在磁盘    ││ 三层堆叠条    ││ 60s 扫描光    │       │
│ └──────────────┘└──────────────┘└──────────────┘└──────────────┘       │
├────────────────────────────────────────────────────────────────────────┤
│ 近 7 天                                       [24 h] [7 天] 覆盖 96%   │ 历史区，高 132
│  CPU  ┃ ▁▃▅▂▇▃▁  峰值 42% · 均值 9% · 今日 12%      ● 有记录 ○ 未运行  │ 三轨小图，各自量纲
│  内存 ┃ ▂▂▃▅▃▂▂  峰值 8.2 GB（11-07 15:00）· 今日 7.1 GB             │
│  流量 ┃ ▁▁▂▅▂▁▁  ↓ 合计 3.4 GB · ↑ 合计 210 MB                       │
├────────────────────────────────────────────────────────────────────────┤
│ 进程明细 (45)                                    [按内存 ▾] [搜索…]    │ 表格 1
│ ┄┄ 名称            pid    CPU    内存      换出    工作区      操作     │ 表头 10 ink-faint
│    Cursor Helper   8812   3%     1.2 GB   120 MB   ~/proj/a    ⋯      │ 行高 30，hover 底
│    ...                                                                 │
├────────────────────────────────────────────────────────────────────────┤
│ 端口与连接                                               [去端口页 →]  │
│  ● 5173 关注层 · Vite        ● 3000 对外 · Node     16 条 ESTABLISHED  │ chips
├────────────────────────────────────────────────────────────────────────┤
│ 工作区归属                                                [内存页 →]   │
│  ~/proj/a  12 进程 · 2.4 GB       ~/proj/b  6 进程 · 900 MB           │
├────────────────────────────────────────────────────────────────────────┤
│ 泄漏证据（仅命中时出现）                                                │
│  +240 MB / 30 min · R² 0.93 · 斜率 480 MB/h        [查看证据 →]        │
├────────────────────────────────────────────────────────────────────────┤
│ 脚注：口径一句话（footprint 含换出 / 7 天为小时聚合 / 网速 1s 差分）    │
└────────────────────────────────────────────────────────────────────────┘
```

### 5.2 分区规格与数据绑定

| 分区 | 内容 | 数据来源 |
|---|---|---|
| **吸顶栏** | 返回、刷新（复用父级 `refreshNonce`）、更多菜单（在 Finder 显示 / 复制路径 / 结束进程） | `revealPath`、`killProcess` |
| **Hero** | 64 图标、名称、`.app` 路径、进程数、连接数、口径 Badge、主操作按钮 | `Row`（family/members/conns/ports）+ `isExact` |
| **CPU 卡** | 大数字（单核口径）+ `AreaChart` 10 min 家族走势 | 新增前端 60 点滚动缓冲（按 family 聚合 `cpu`，10 s/点），与 StatsBoard 同款算法 |
| **内存卡** | `CountUp` 总量 + RSS/换出 双段 `StackBar` + 「N% 在磁盘」 | `FamilyMem.rss/footprint/swapped`、`swappedRatio` |
| **磁盘卡** | 本体/缓存/数据三层 `StackBar` + 三个数字 + 「重新测量」 | `appDisk(family, exe, force)`（已有 30 min 缓存） |
| **网速卡** | ↓/↑ 当前速率 + 60 s `AreaChart`（↓ domain-net / ↑ domain-cpu） | `netSpeedOfPids` + 详情页自持 60 点缓冲 |
| **近 7 天** | 三轨（CPU / 内存 / 流量）小时聚合走势 + 区间切换（24 h / 7 天）+ 覆盖率角标 | **新增** `app_history` 命令（见 §5.5），一次拉 8 天 |
| **复制摘要** | Hero 右侧按钮，生成 Markdown 摘要写入系统剪贴板，按钮 1.6 s 内变「✓ 已复制」 | **新增** `clipboard_write` 命令（复用 `net_diag` 的 `pbcopy` 通道，见 §5.7） |
| **进程表** | 名称/pid/CPU/内存/换出/工作区/操作（右键菜单复用 `ProcKillMenu`），点行 → 内存页 `ProcessInspector` | `processes.filter(family)`，排序默认内存降序 |
| **端口与连接** | 端口 chips（暴露面 Badge 语义色）+ 连接数；点击 → 端口 tab 并聚焦该端口 | `portsOfPid`、`exposureMeta`、`connsOfPids` |
| **工作区** | 路径 + 进程数 + 总量，点击 → 内存页对应家族 | `FamilyMem.workspaces` |
| **泄漏证据** | 仅 `leaksOfFamily` 命中时出现：净增长/R²/斜率 + 「查看证据」 | `snapshot.leaks` |

### 5.3 状态机

```
列表 ──点击卡片──▶ 详情（loading: 图标/磁盘卡骨架）
  ▲                    │
  └──← 返回 / Esc ─────┘
详情 ──点进程行──▶ 内存页 ProcessInspector（backTo=应用）
详情 ──点端口 chip──▶ 端口页（focusQuery=端口，backTo=应用）
```

- 返回列表：恢复原排序/搜索/滚动位置（状态留在 `AppResourceBoard`，详情只负责渲染）。
- `Esc` = 返回（详情页注册 keydown，输入框聚焦时不拦截）。
- 下钻到其他 tab 时沿用既有 `backTo` 机制（顶栏显示「← 返回应用」）。

### 5.4 动效（全部 `prune-anim` 守卫）

| 场景 | 动效 | 时长 |
|---|---|---|
| 进入详情 | 内容 `translateY(6px)→0` + 透明度 0→1 | 180ms ease-out |
| 图标到达 | 交叉淡入 | 180ms |
| 数字变化 | `CountUp` 滚动 | 400ms |
| 磁盘测量中 | 骨架条 shimmer（3 条） | 循环 1.2s |
| 表格行 hover | 底色 `white/[0.04]` | 120ms |
| 区间切换 | 三轨小图交叉淡入 + 柱高 160ms 过渡 | 160ms |
| 复制摘要 | 按钮文案换「✓ 已复制」+ 域色描边闪一下 | 1.6s 后复原 |

---

### 5.5 近 7 天历史（新增数据层）

#### 5.5.1 为什么不能直接复用现有表

| 现成数据 | 能否满足 7 天 | 原因 |
|---|---|---|
| `mem_process`（SQLite，5 min，7 天） | ❌ 只做 top-12 家族 | 深探只覆盖 `DEEP_MAX_FAMILIES=12` / `DEEP_MAX_PROCS=64`，348 个应用里 336 个没有序列；且只写有 footprint 的行 |
| `net_daily`（SQLite，日粒度） | ❌ 只有整机 | 表里没有 family 维度 |
| 前端 60 点环形缓冲 | ❌ 只在挂载时 | 10 min，卸载即丢 |
| 整机 CPU 环形缓冲 | ❌ 只 60 点且不入库 | `CPU_HISTORY_CAP=60`、注释明确「不入库」 |

**结论**：新增一张**按家族聚合**的小时表 `app_hourly`，用已经在跑的 fast 轮（10 s）与低频网速采样喂它，覆盖全部应用、写入量可控。

#### 5.5.2 表结构

```sql
CREATE TABLE IF NOT EXISTS app_hourly (
    family     TEXT    NOT NULL,
    bucket     TEXT    NOT NULL,   -- 本地时间 'YYYY-MM-DDTHH'（与 net_daily 的 day 同源 locatime 口径）
    rss_sum    INTEGER NOT NULL DEFAULT 0,  -- RSS 累加（字节，用于均值）
    rss_n      INTEGER NOT NULL DEFAULT 0,  -- 样本数
    rss_peak   INTEGER NOT NULL DEFAULT 0,
    cpu_sum    REAL    NOT NULL DEFAULT 0,  -- 单核口径 %，累加
    cpu_n      INTEGER NOT NULL DEFAULT 0,
    cpu_peak   REAL    NOT NULL DEFAULT 0,
    net_down   INTEGER NOT NULL DEFAULT 0,
    net_up     INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (family, bucket)
);
CREATE INDEX IF NOT EXISTS idx_app_hourly_bucket ON app_hourly (bucket);
```

- 量级：348 家族 × 24 桶 ≈ 8.4k 行/天，7 天 ≈ 59k 行（≈ 5–8 MB，WAL 下无压力）。
- 保留：`prune_before` 同步删除 `bucket < now-8d`（多留 1 天保「7 整天」完整）。
- 口径纪律：**RSS 而非 footprint** 进历史。footprint 深探是有成本的分层采样，用它做 7 天序列会出现「有 footprint 的点 ≠ 同一批应用」的断层；RSS 每 10 s 全量可得，序列连续。历史区脚注写明「历史为 RSS 口径，当前读数为 footprint」——两套口径不混着比。

#### 5.5.3 写入路径（零新增定时器）

```
fast 轮（10 s，已在跑）
  └─ 内存聚合器 AppAgg（Mutex<HashMap<family, Acc>>）
       · 每个家族：rss_sum += rss, rss_n += 1, rss_peak = max
       · 有 cpu 的家族：cpu_sum += cpu, cpu_n += 1, cpu_peak = max
       · 每 60 s 或跨小时 → flush 到 app_hourly（一条事务批量 UPSERT）

net 采样（1 s 可见时 / 60 s 后台低频，见 5.5.4）
  └─ 同上聚合器累加 net_down / net_up（按 pid→family 映射，映射由 fast 轮快照提供）
```

UPSERT 语义是**增量相加**（同 `net_daily`）：`rss_sum = rss_sum + excluded.rss_sum`，因此只允许单一写入者，重启后接着累加不清零。

#### 5.5.4 网速 7 天覆盖率的诚实处理

现状：`net_speed_set_active` 只在「应用」tab 可见时 1 s 采样，切走就停 —— 直接聚合会得到「只有你看着的时候才有流量」的假历史。

**决策**：新增**后台低频网速采样**（60 s 一轮，仅用于历史累加，不推 UI 事件）。

| 项 | 规格 |
|---|---|
| 触发 | 常驻 60 s 定时器（与 fast 轮同 loop，避免多一个线程） |
| 成本 | `nettop` 一轮 ≈ 60–120 ms，60 s 一次 ≈ 0.2% CPU，可接受 |
| 设置项 | `appHistoryNet`（settings.json，默认 `true`），关掉后流量轨显示「未开启后台采样」 |
| 覆盖率 | 前端按 `小时桶数 / 应有时长` 算「覆盖 96%」角标；< 60% 时角标转 warn 并在 tooltip 说明原因（Prune 未运行 / 采样关闭） |
| 网络不可读 | 该小时 `net_down/net_up` 保持 0 且 `updated_at` 有值，前端按 `rss_n==0 && cpu_n==0` 判「无记录」画灰柱，**不把 0 当「没流量」** |

#### 5.5.5 查询契约（新增命令）

```rust
// src-tauri/src/commands.rs
#[derive(Serialize)]
pub struct HistoryBucket {
    bucket: String,       // 'YYYY-MM-DDTHH'
    rss_avg: Option<u64>, // 无样本 = null（不是 0）
    rss_peak: Option<u64>,
    cpu_avg: Option<f32>,
    cpu_peak: Option<f32>,
    net_down: u64,
    net_up: u64,
    samples: u32,         // rss_n，用于覆盖率与「这一小时 Prune 在不在」
}

#[tauri::command]
pub async fn app_history(family: String, hours: u32) -> Result<Vec<HistoryBucket>, String>
```

- `hours` 首版固定传 `192`（8 天，前端只画 7 天，多一天用于跨时区/整点对齐）。
- **只返回有记录的小时**：缺失小时由前端补「灰柱」，绝不在后端补 0（同 `net_recent` 纪律）。
- 详情页挂载时拉一次；每 5 min（与 deep 轮同节拍）刷新一次；窗口不可见不刷新。

### 5.6 历史区图表规格

布局：一整行 `section`，高 132px，内含三轨（每轨 36px + 8px 间距），右上角区间切换 chip。

| 轨道 | 图形 | 主读数（右对齐） | 缺失处理 |
|---|---|---|---|
| **CPU** | 7 根柱（日聚合取 `max(cpu_peak)`）或 24 根柱（24 h 视图取每小时 `cpu_peak`），柱色 `domain-cpu`，峰值柱高亮 | `峰值 42% · 均值 9% · 今日 12%` | 当日无 `cpu_n` → 柱高 0 + 灰虚线底，tooltip「无记录」 |
| **内存** | 同柱形，色 `domain-mem`（蓝），柱内可用渐变表示 min→max | `峰值 8.2 GB（11-07 15:00）· 今日 7.1 GB` | 同上 |
| **流量** | 堆叠柱：下段 `domain-net`（↓）/ 上段 `domain-cpu`（↑ 紫，与其他页保持一致） | `↓ 合计 3.4 GB · ↑ 合计 210 MB` | 该小时 0 且 `samples==0` → 灰柱；否则 0 就是真 0 |

- 复用 `charts/AreaChart` 的坐标系与渐变，柱形新增 `BarSeries` 小组件（约 60 行，纯 SVG，不引依赖）。
- hover：柱上方浮 `MM-DD HH:00 · CPU 峰值 42% / 均值 8%`，一柱一条 tooltip；键盘 `←/→` 可遍历（`role="img"` + `aria-label` 概述整段）。
- 区间：`24 h` / `7 天` 两个 chip，默认 **7 天**（用户诉求是历史维度，先给全景）；切换不重新请求（一次拉 192 h，前端聚合）。
- 无数据（新装 / 关掉采样）：整块显示「还没有历史 · Prune 运行满一小时后出现」，不画空坐标系。

### 5.7 复制诊断摘要（首版就做）

#### 5.7.1 剪贴板通道

复用 `net_diag.rs` 已验证的通道（macOS `pbcopy` / Windows PowerShell `Set-Clipboard`，零新依赖），抽成通用命令：

```rust
#[tauri::command]
pub fn clipboard_write(text: String) -> Result<usize, String>  // 返回字符数
```

- 前端在 dev / 非 Tauri 环境退回 `navigator.clipboard.writeText`（`netDiagStore.copyText` 已有的三段兜底逻辑抽到 `lib/clipboard.ts` 复用）。
- 失败语义：命令返回 `Err` → 按钮显示「✗ 复制失败」+ tooltip 给原因，**不静默成功**。

#### 5.7.2 摘要内容（Markdown，前端拼装）

数据全在详情页手里，不新增采集命令；模板固定，方便用户贴到 issue：

```markdown
## Prune · 应用诊断摘要

**Cursor** · 45 进程 · 16 连接
- 路径：/Applications/Cursor.app
- 采集时间：2026-11-09 16:40（Prune v0.4.1 · macOS 15.1）

### 当前读数
| 指标 | 值 | 口径 |
|---|---|---|
| CPU | 8%（单核） | 8/100 核时 |
| 内存 | 7.1 GB | footprint，含换出 860 MB |
| 磁盘 | 975 MB | 本体 620 MB / 缓存 240 MB / 数据 115 MB |
| 网速 | ↓3.8 KB/s ↑120 B/s | 1 s 差分 |
| 端口 | 5173（关注层）、3000（对外） | lsof |

### 近 7 天（小时聚合，RSS 口径）
- CPU 峰值 42%（11-07 15:00）· 均值 9%
- 内存峰值 8.2 GB · 今日 7.1 GB
- 流量 ↓3.4 GB / ↑210 MB · 覆盖 96%

### 泄漏证据
- +240 MB / 30 min · R² 0.93 · 斜率 480 MB/h
# 未命中时写：无（未发现持续增长）

### 环境
- 采样：fast 10 s / deep 5 min / 历史保留 8 天
```

规则：
- 任何拿不到的项写「不可读（原因）」，**不写 0**（同 UI 红线）。
- 泄漏未命中写「无（未发现持续增长）」，不省略该节（否则用户以为漏了）。
- 摘要不含 pid 列表全文（45 行太长），只给进程数 + Top 3 进程名。

---

## 6. 组件拆解

| 组件 | 文件 | 职责 | 复用 |
|---|---|---|---|
| `AppIcon` | `components/AppIcon.tsx`（新） | 图标/字母兜底/加载交叉淡入；`size` 28/64 两档 | 卡片 + Hero + 列表 |
| `useAppIcons` | `lib/useAppIcons.ts`（新） | 批量取图标（一次 IPC）、家族→dataURL 映射、失败记 null | — |
| `AppDetail` | `components/appdetail/AppDetail.tsx`（新） | 详情页骨架 + 分区布局 | — |
| `ResourceTile` | `components/appdetail/ResourceTile.tsx`（新） | 四联卡外壳：标题 + 大数字 + 图表槽 + 脚注 | ×4 |
| `ProcTable` | `components/appdetail/ProcTable.tsx`（新） | 进程明细表（排序/右键/下钻） | `ProcKillMenu` |
| `HistoryTrack` | `components/appdetail/HistoryTrack.tsx`（新） | 三轨历史区：柱形 + 主读数 + 覆盖率角标 + 24h/7d 切换 + 空态 | `BarSeries` |
| `BarSeries` | `components/charts/BarSeries.tsx`（新） | 柱形序列（日/小时两档、缺失灰柱、峰值高亮、hover tooltip），纯 SVG ≈ 90 行 | `tone.ts` |
| `useAppHistory` | `lib/useAppHistory.ts`（新） | 拉 `app_history`（192 h）、5 min 刷新、24h/7d 聚合、覆盖率计算 | — |
| `buildAppSummary` | `lib/appSummary.ts`（新） | 把当前读数 + 历史 + 泄漏拼成 §5.7.2 的 Markdown；缺失项写原因 | `format.ts` |
| `writeClipboard` | `lib/clipboard.ts`（新，从 `netDiagStore` 抽出） | `clipboard_write` IPC → `navigator.clipboard` → `execCommand` 三段兜底 | — |
| `AppAgg` | `src-tauri/src/sampler.rs`（内嵌） | 家族 10 s 聚合器 + 60 s / 跨小时 flush 到 `app_hourly` | `prune_core::db` |
| `ResourceDetail` | 从 `AppResourceGrid` **迁移** | 磁盘三层 + 端口 chips + Top3 + 走势 | 详情页直接复用 |

---

## 7. 红线与空态对照

| 场景 | 行为 |
|---|---|
| 图标提取失败 / 无 `.app`（系统进程） | 字母徽标；不显示灰色问号图 |
| 磁盘未测量 | 磁盘卡显示「未测量 + 测量按钮」，不显示 0 |
| 网络未采样 | 网速卡「尚未采样」，不画空曲线 |
| CPU 首见无读数 | `—`（不是 `0%`） |
| 端口未扫描 | chips 区显示「尚未扫描，进入端口页扫一轮」 |
| 系统进程 / 不可结束 | 「结束进程」禁用 + tooltip 说明原因 |
| 泄漏未命中 | 证据区整块不渲染（沉默即正常） |
| **历史某小时无样本** | 灰柱 + tooltip「无记录（Prune 未运行）」；**不补 0、不连线** |
| **历史整体为空**（新装 / 采样关闭） | 「还没有历史 · 运行满一小时后出现」，不画空坐标系 |
| **历史覆盖率 < 60%** | 角标转 warn，tooltip 给原因（未运行 / 后台采样关） |
| **后台网速采样关闭** | 流量轨显示「未开启后台采样」+ 去设置链接，CPU / 内存轨正常 |
| **复制摘要失败** | 按钮「✗ 复制失败」+ 原因 tooltip，不静默成功 |

---

## 8. 无障碍

- 卡片：`role="button"`（或 `<button>`），`aria-label="打开 <家族名> 详情"`，焦点环 `2px domain-cpu/60`。
- 表格：真 `<table>` + `<th scope="col">`，行 `tabIndex=0`，Enter 下钻。
- 图标：装饰性 `alt=""`；Hero 图标 `alt="<名称> 图标"`。
- 对比度：字母徽标前景 `#0b0e11` 于 HSL(45% 明度) 底 ≥ 4.5:1；徽标文字 12.5px/600。
- 动效：全部走 `prune-anim`，`prefers-reduced-motion` 下禁用位移与交叉淡入。

---

## 9. 实施拆解（D1–D8，逐条可验收）

### 9.0 依赖与顺序

```
D1 图标管道 ─┐
             ├─▶ D2 卡片改版 ─┐
D3 历史数据层 ┘                ├─▶ D4 详情页骨架 ─┬─▶ D5 历史区 UI ─┐
                               │                   ├─▶ D6 明细区    ├─▶ D8 收尾
                               │                   └─▶ D7 复制摘要 ─┘
```

- D1 / D3 可并行（一个动 Rust+前端图标，一个动 Rust 采样+DB）。
- D4 依赖 D1（Hero 图标）与 D2（进入详情入口）。
- D5 / D6 / D7 相互独立，可在 D4 后并行。
- D8 最后，统一走查 + 文档回填。

| # | 任务 | 依赖 | 预估改动面 |
|---|---|---|---|
| D1 | 图标管道 | — | Rust 新模块 + 1 命令；前端 2 文件 |
| D2 | 卡片改版 | D1 | `AppResourceGrid.tsx` 单文件为主 |
| D3 | 历史数据层 | — | `db.rs` + `sampler.rs` + `net_sampler.rs` + 1 命令 + settings |
| D4 | 详情页骨架 | D1, D2 | 新目录 `components/appdetail/`（4 文件）+ `MainWindow` 路由 |
| D5 | 历史区 UI | D3, D4 | `BarSeries` + `HistoryTrack` + `useAppHistory` |
| D6 | 明细区 | D4 | `ProcTable` + 迁移 `ResourceDetail` |
| D7 | 复制诊断摘要 | D4（D5 数据可选） | `clipboard_write` + `lib/clipboard` + `buildAppSummary` |
| D8 | 收尾验收 | D5, D6, D7 | 动效/无障碍/文档 |

---

### D1 图标管道

**目标**：卡片与详情页拿到真实应用图标，失败全部落字母徽标。

**改动**
- `crates/prune-core/src/appicon.rs`（新）：`locate_icns(exe) -> Option<PathBuf>`（`.app/Contents/Resources/*.icns`，取 `CFBundleIconFile` 或首个 icns）；`convert(icns, size) -> Result<Vec<u8>>`（`sips -Z <size> -s format png <in> --out <tmp>`）；缓存写 `~/Library/Caches/<bundle-id>/icons/<fnv1a(exe)>-<size>.png`。
- `src-tauri/src/commands.rs`：`app_icons(items) -> HashMap<String,String>`（`data:image/png;base64,...`）；8 路并行、单应用 300 ms 预算、进程内 `Mutex<HashMap>` + 30 min TTL；单条失败不放进 map、不整体报错。
- `src/lib/useAppIcons.ts`（新）：一次批量取、家族→dataURL、失败记 null、`isTauri()` 门控。
- `src/components/AppIcon.tsx`（新）：`size=28|64`；dataURL 有则 `<img>` 交叉淡入，无则字母徽标（`fnv1a(family)%6` 六组低饱和渐变，HSL 160–280 / S45%）；`inset ring 1px rgba(255,255,255,.08)`。

**验收**
1. `cargo check` + `cargo test -p prune-core appicon`（纯函数：路径反推、哈希稳定）。
2. 浏览器 dev（无线程）：全部字母徽标、无 console 错误。
3. Tauri 实机：Cursor / Chrome / 微信 三个图标正确显示；`/usr/libexec/*` 系统进程落徽标。
4. 首屏 12 卡批量耗时 < 400 ms（打日志）。

**风险**：`sips` 缺失 → 空 map 降级；无 icns 的 app → 徽标；Windows 全徽标（`cfg` 门控，不写死 `navigator.platform`）。

---

### D2 卡片改版（整卡进详情）

**目标**：点击语义唯一化，图标替代折叠箭头。

**改动**（`src/components/AppResourceGrid.tsx`）
- 左侧 `›` → `<AppIcon size={28} />`；右侧追加 `›` 尾部箭头（hover `translateX(2px)`）。
- 删除展开态：`expanded` state / `aria-expanded` / `ResourceDetail` 挂载点全部移除。
- 整卡 `<button aria-label={`打开 ${family} 详情`}` → `onOpenApp(family)`。
- hover：边框 `rgba(255,255,255,.10)` + 图标 `scale(1.03)`；`prune-anim` 守卫。
- 键盘：Enter/Space 进详情；`⌘↵` 保留原展开语义不宣传（兜底）。
- 列表状态（搜索/排序/滚动）留在 `AppResourceBoard`，详情不改它。

**验收**：`tsc` 0；点击任意卡进入占位详情；返回后搜索词、排序、滚动位置完全一致。

---

### D3 历史数据层（7 天）

**目标**：全家族小时级历史可查，缺数据如实为空。

**改动**
- `crates/prune-core/src/db.rs`
  - `SCHEMA` 增 `app_hourly`（§5.5.2）+ 索引。
  - `app_hourly_add(rows: &[HourlyDelta])`：单事务批量 UPSERT（增量相加）。
  - `app_hourly_get(family, from_bucket) -> Vec<HourlyRow>`：只返回有行的小时。
  - `prune_before` 同步清 `bucket < cutoff`（8 天）。
- `src-tauri/src/sampler.rs`
  - `AppAgg`：`Mutex<HashMap<String, Acc>>`，fast 轮累加 rss / cpu（`cpu_covered` 才计），60 s 或跨小时 flush（`spawn_blocking` 落库，不阻塞采样）。
  - 提供 `pid -> family` 只读映射给网速采样（复用本轮 `merged`）。
- `src-tauri/src/net_sampler.rs`
  - 新增后台低频轮（60 s）：`nettop` 一轮 → 按 pid 映射累加 `net_down/net_up`；仅历史用途，不推事件。
  - 设置项 `appHistoryNet`（默认 true）关闭时跳过 spawn。
- `src-tauri/src/commands.rs`：`app_history(family, hours)`。
- `src/stores/diskStore.ts` 之外的 settings 读取沿用 `settings.rs`。

**验收**
1. `cargo test -p prune-core db`：UPSERT 增量正确、重复 flush 不重复计、8 天裁剪生效、缺小时不补 0。
2. 跑 10 min 后 `app_history("Cursor", 24)` 返回 ≥ 9 个小时桶，`samples` 合理。
3. 关掉 `appHistoryNet` 后流量增量为 0 且 `updated_at` 仍推进（可区分「关采样」与「真没流量」）。
4. 内存占用：`app_hourly` 7 天 < 10 MB。

**风险**：60 s `nettop` 在低电量模式可能变慢 → 单轮 2 s 超时直接跳过，不重试堆积。

---

### D4 详情页骨架

**目标**：从卡片进入独立二级页，Hero + 四联卡可用。

**改动**
- `src/windows/MainWindow.tsx`：新增 `appDetail: string | null` 状态；`tab === "resources"` 且 `appDetail` 有值时渲染 `<AppDetail family={...} onBack={...} />`；`backTo` 机制复用。
- `components/appdetail/AppDetail.tsx`：吸顶栏（返回 / 刷新 / ⋯）+ Hero + 四联卡 + 预留历史区 / 明细区槽位。
- `components/appdetail/ResourceTile.tsx`：外壳（标题 + 大数字 + 图表槽 + 脚注），四张实例化。
- 数据：`useMemStore` 取家族 `FamilyMem` + members；`appDisk` 按需测；`netSpeedOfPids`；60 点 / 10 min 环形缓冲自持，卸载释放。
- `Esc` 返回（输入框聚焦不拦截）。

**验收**：四张卡都显示真实读数或诚实空态；刷新按钮同步更新；Esc / 返回按钮都回到列表且状态保留。

---

### D5 历史区 UI

**目标**：三轨历史可读、缺口诚实、区间可切。

**改动**
- `components/charts/BarSeries.tsx`：`bars: {label, value, hasData}[]`、`tone`、`highlightPeak`、`height`；缺失柱画 1px 灰虚线底；hover tooltip；`role="img"` + `aria-label` 概述。
- `components/appdetail/HistoryTrack.tsx`：三轨（CPU / 内存 / 流量）+ 右侧主读数 + 右上 `24 h | 7 天` chip + 覆盖率角标。
- `lib/useAppHistory.ts`：拉 192 h、5 min 刷新（`document.visibilityState === "visible"` 才刷）、24h/7d 聚合、`coverage = 有样本小时 / 应有时长`。
- 空态：`EmptyArt` 复用，文案「还没有历史 · 运行满一小时后出现」。

**验收**
1. 7 天视图 7 根柱、24h 视图 24 根柱；切换不重新请求。
2. 人为删掉某小时行 → 该柱为灰、tooltip「无记录」，不显示 0。
3. 覆盖率 < 60% → 角标 warn 态 + tooltip 原因。
4. `prefers-reduced-motion` 下柱高不做过渡。

---

### D6 明细区

**目标**：一个应用的全部细节在本页看完，且能下钻。

**改动**
- `components/appdetail/ProcTable.tsx`：`<table>` + 排序（内存/CPU/换出）+ 行 hover + 右键 `ProcKillMenu` + 点行进内存页 `ProcessInspector`（`backTo="resources"`）。
- `ResourceDetail` 从 `AppResourceGrid` 迁移：磁盘三层按钮、端口 chips（点击 → 端口页并聚焦）、工作区归属、泄漏证据。
- 脚注：口径一句话（footprint / 历史 RSS / 网速 1s 差分）。

**验收**：45 进程表排序正确；右键结束走两步确认；进程行 / 端口 chip 下钻后「← 返回应用」可回原详情页；泄漏未命中时整块不渲染。

---

### D7 复制诊断摘要

**目标**：一键把本页读数变成可粘贴的 Markdown。

**改动**
- `src-tauri/src/net_diag.rs`：把 `write_clipboard` 提为 `pub(crate)`；新增 `clipboard_write(text) -> Result<usize,String>` 并注册。
- `lib/clipboard.ts`（新）：从 `netDiagStore.copyText` 抽出三段兜底（IPC → `navigator.clipboard` → `execCommand`）。
- `lib/appSummary.ts`（新）：按 §5.7.2 模板拼装；缺失项写「不可读（原因）」；泄漏未命中写「无（未发现持续增长）」；Top 3 进程名。
- `AppDetail` Hero：`[复制摘要]` 按钮，成功 1.6 s「✓ 已复制」、失败「✗ 复制失败」+ tooltip。

**验收**
1. 真机点复制 → `pbpaste` 内容与模板结构一致（字段齐全、无 `undefined` / `NaN`）。
2. 断网速采样 / 未测磁盘时，对应行写「不可读（原因）」而非 0。
3. 剪贴板命令失败（模拟 `pbcopy` 不可用）→ UI 显示失败态。

---

### D8 收尾验收

- 动效：进入 180ms / 图标淡入 / CountUp / 柱切换 160ms，全部 `prune-anim`。
- 无障碍：卡片 `aria-label`、表格 `<th scope>`、历史区 `aria-label` 概述、焦点环可见。
- 全链路：`tsc --noEmit` 0 + `vite build` + `cargo test`；浏览器走字母徽标与空态；Tauri 实机 6 屏 + 详情页 + tray 不回归。
- 文档回填：本文件 §5.5–§5.7 与实现一致；`docs/ui-style-v3.html` 第 11 节补 7 天历史条与复制按钮。

**总风险表**

| 风险 | 影响 | 缓解 |
|---|---|---|
| `sips` 不可用 | 图标全落徽标 | 空 map 降级，UI 无感；日志可查 |
| 348 应用首次批量转换 | 首屏卡顿 | 首批 12，8 路并行 + 300 ms 预算，异步不阻塞 |
| 60 s `nettop` 增加常驻成本 | 约 0.2% CPU | 默认开、可在设置关闭；单轮 2 s 超时跳过 |
| `app_hourly` 行数超预期 | DB 膨胀 | 348 家族上限、8 天裁剪、实测 < 10 MB |
| 历史口径被误读为 footprint | 用户对比不同数字 | 脚注 + tooltip 明确「历史 RSS / 当前 footprint」 |
| 详情页自持缓冲 | 内存占用 | 仅挂载时采，卸载释放 |

---

## 10. 决策记录（v1.1 已确认）

| # | 问题 | 决策 | 落地位置 |
|---|---|---|---|
| 1 | 卡片点击语义 | **整卡进详情**，取消卡内展开；`⌘↵` 保留展开兜底不宣传 | D2 |
| 2 | 详情页 7 天历史 | **首版就做**：新增 `app_hourly` 小时表 + 后台 60 s 网速采样 + 三轨历史区（24h / 7d） | D3 / D5 / §5.5 |
| 3 | Windows 图标 | **首版全部字母徽标**，`cfg` 门控；icns 提取仅 macOS | D1 |
| 4 | 复制诊断摘要 | **首版就做**：复用 `pbcopy` / `Set-Clipboard` 通道的 `clipboard_write` + 前端模板拼装 | D7 / §5.7 |

> 后续可选项（不在首版）：7 天数据导出 CSV、跨应用历史对比视图、Windows 图标提取（`SHGetFileInfo`）。
