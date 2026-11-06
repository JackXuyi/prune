# M16 菜单栏面板「统计」化 + 双端网速方案

> 状态：**已实施（S0–S2 + v4 单页化 + v5 贴高，2026-11-06）**；S3 文档同步已做；真机走查待做
> 前置：M9 统计 tab（四卡总览）、M13 菜单栏总网速（`sysinfo::Networks` 2 s 常驻）、M14 网络 tab、M15 灵动岛
> 需求：① 菜单栏面板里的「应用」页改成**统计数据**，与主窗口统计 tab **口径一致**；② 两边统计都**增加网络速度**
> 约束：面板固定宽 380；高度**贴合内容**（v5 起 400，原 560），宽度不允许改

---

## 0. 已拍板（2026-11-06）

| # | 决定 | 落地含义 |
|---|---|---|
| P1 | 面板**删掉「应用」tab** | `PanelTab = "stats" \| "overview"`，默认 `stats`；应用列表退化为底部一行入口 |
| P2 | 面板**行高 48** | 每行 = 标签 + 主读数一行、Meter 4 px + 副读数一行；网络行例外为 64（含合并走势），详见 §4.2 |
| P3 | 主窗口网络卡**放第 3 行横跨两列** | `col-span-2`；为了一屏放得下，三行卡的 `min-h` 从 152 收到 140（§4.1 有算式） |
| P4 | 面板**只保留一个 tab**，去掉切换器（v4） | 面板没有 `PanelTab`、不渲染 `<Tabs>`；原「概况」内容（内存压力 / swap / 换出受害者）移除，主窗口「内存」tab 有完整版 |
| P5 | 面板**高度贴合内容**（v5） | `PANEL_H` 560 → **400**（macOS `tray.rs` + Windows `tauri.conf.json` 同改）；内容自然高 394，余 6；底部不再留 154 px 空白 |

> 上一版遗留的「三 tab 备选」与「128 px 卡高备选」作废，不再讨论。

---

## 1. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 面板 tab 从「应用 / 概况」改为「统计 / 概况」，默认统计 | 见 P1 | 面板是「瞄一眼整机状态」，应用级明细属于主窗口 |
| 面板**移除 `DomainHero`** | 内存 / 存储两行并入统计列表的第 1、2 行 | 否则同一屏里「13.4 / 16 GB」出现两次，纯浪费 78 px 垂直空间 |
| 「应用」页整体移除，只留**一行入口** | 底行「查看全部 343 个应用（CPU / 磁盘 / 网速对比）→」跳 `resources` tab | 入口不丢、版面不占；面板本来也只显示内存前 3 个 |
| 双端**共用一层纯函数读数层**，不共用组件树 | `src/lib/statsReadout.ts` 输出 `{ main, unit, ratio, sub[], status, tone }`；主窗口渲染成「卡」，面板渲染成「行」 | 一致性靠代码约束，不靠人眼比对两份 JSX |
| 主窗口网络卡横跨两列，第 3 行 | 见 P3 | 网络是「流动」指标，宽卡才能放下双读数和双走势 |
| 面板网络行带**一条 18 px 合并走势** | 用同一 `history` 的 `down+up` 序列，不做双线 | 380 px 宽塞两条线会挤成噪声；主数字仍分 ↓/↑ |
| 统计页 / 面板**不提速采样**，只调 `init()` | 保持 `net_total` 2 s 节拍；`net_total_set_active(true)` 仍只属于 `net` tab | 统计页不需要 1 s 精度；避免与 `net` tab 的门控互相抢开关 |
| 网络读数的空值语义**三态**，绝不填 0 | `warmup` →「采样中…」；`unavailable` →「—」+ reason；`ok && 0` →「0 B/s」 | 读不到 ≠ 没有流量，这条是 M14 已定的红线 |
| 端口在统计页只做**一次性扫描** | 复用 `initPorts()`（有快照只读，无快照扫一轮 39.6 ms），不挂 30 s 定时轮 | 沿用 M9 验收项「统计页常驻开销为 0」 |

---

## 2. 需求理解与边界

**用户问题**（每个 UI 决策都要能对上一条）：

1. 「我这台机器现在什么状态？」——面板点开即答，不用切 4 个 tab 拼答案
2. 「网现在还跑不跑得动？」——↓/↑ 实时速率要在统计页第一梯队，和内存同级
3. 「面板看到的数，和主窗口对得上吗？」——同一时刻两边差值 ≤ 1 个采样周期
4. 「应用列表去哪了？」——一行入口直达，不埋在菜单里

**边界**：

- ✅ 做：面板统计页（5 个域 + 网速）、主窗口统计页加网络卡、共享读数层、tab 与入口改造、文案与口径同步
- ❌ 不做：新增 Rust 命令、新增定时轮、改面板窗口尺寸、把「统计」与「网络」tab 合并、往统计页放任何清理/限速按钮

---

## 3. 现状诊断（代码级）

| 项 | 现状 | 缺口 |
|---|---|---|
| 面板 tab | ~~`PanelTab = "apps" \| "overview"`~~ → ~~`stats` / `overview`~~ → **无 tab（v4）** | 面板只留统计一屏，`<Tabs>` 与 `PanelTab` 一并删除 |
| 面板 hero | `DomainHero`（`MenuBarPanel.tsx:301`）固定渲染内存 + 存储两行，在 tab 之上 | 与统计页第 1、2 行完全重复，需移除 |
| 面板应用区 | `AppsSection` → `TopAppCards`（内存前 3）+ `LeakBanner` | 移除卡片，保留泄漏横幅（安全信号）+ 一行入口 |
| 主窗口统计 | `StatsBoard.tsx` 2×2 四卡（内存 / CPU / 硬盘 / 端口），`DrillTab = "overview" \| "disk" \| "ports"` | 缺网络卡；`DrillTab` 需加 `"net"` |
| 卷容量 | `StatsBoard.tsx:46` 私有 `useEffect` 取 `diskVolume()`，含 dev fallback 与 `refreshNonce` | 面板也要这份读数 → 必须抽成 `useVolumeInfo(refreshNonce)` |
| 网速数据 | `netTotalStore`：`init()` 订阅 `net://total` + 拉快照/历史；**调用点只有 `MainWindow.tsx:190` 且门控在 `tab === "net"`** | **P0**：统计页不补 `init()`，`sample` 恒为 null → 永远「采样中…」 |
| 采样开销 | `net_sampler.rs`：`TOTAL_INTERVAL_MS = 2000` 常驻轮，`set_active` 只改节拍 2 s ↔ 1 s | 无缺口，统计页读它零新增开销 |
| 网络读数口径 | `types/net.ts`：`splitIfaces`（物理口合计）、`weightedAverage(history)`、`formatSpeed` | 直接复用，不重写 |
| 迷你走势 | `Sparkline` 以自身 `min/max` 归一化 | 0 流量附近的噪声会被放大成「满屏尖峰」，需加 `baselineZero` 选项 |

---

## 4. 共享读数层（一致性靠这个，不靠人眼）

### 4.1 `src/lib/statsReadout.ts`（纯函数，无 React、无 DOM）

```ts
export type ReadoutStatus = "ok" | "warmup" | "unavailable";

export interface Readout {
  /** 主读数数值（已格式化，不含单位） */
  main: string;
  unit: string;
  /** 0–1，无占比概念的域为 null */
  ratio: number | null;
  /** 次级读数：按「短 → 长」排序，面板只取前 1–2 条 */
  sub: string[];
  status: ReadoutStatus;
  /** 状态原因，status 非 ok 时必有值 */
  note?: string;
  tone: "accent" | "warn" | "danger" | "info" | "neutral";
}

memoryReadout(snapshot: MemSnapshot | null): Readout
cpuReadout(cpu: CpuInfo | null): Readout
diskReadout(volume: VolumeInfo | null, reclaimable: number | null): Readout
portReadout(ports: PortSnapshot | null): Readout
netReadout(sample: NetTotalSample | null, history: NetTotalPoint[]): NetReadout
```

`NetReadout` 在主读数之外多带网络专属字段：

```ts
interface NetReadout extends Readout {
  downBps: number;
  upBps: number;
  /** 面板用：down+up 合并序列；主窗口用 downSeries / upSeries */
  totalSeries: number[];
  downSeries: number[];
  upSeries: number[];
  /** 「近 5 min 平均」两个方向 */
  avgDownText: string;
  avgUpText: string;
}
```

**为什么是纯函数而不是 hook**：口径要能被单测直接盯住（`statsReadout.test.ts` 断言三态与文案），并且面板与主窗口的差异只剩「布局密度」一个变量。

### 4.2 `src/lib/useVolumeInfo.ts`

把 `StatsBoard.tsx:46-64` 的 effect 原样抽出，签名 `useVolumeInfo(refreshNonce: number)` → `{ volume, known }`。主窗口与面板共用，`statvfs` 微秒级，各自持有一份可接受。

### 4.3 展示组件（两份，薄壳）

| 组件 | 文件 | 用在哪 | 形态 |
|---|---|---|---|
| `StatsCard` | `components/stats/StatsCard.tsx` | 主窗口 960 | 卡：标题 + caption + 24 px 主读数 + Meter + 副读数 + 可选 slot（sparkline） |
| `StatsRow` | `components/stats/StatsRow.tsx` | 面板 380 | 行：**固定 48 px**，标签 + 主读数一行、Meter 4 px + 副读数一行（§5.2 有像素分解） |
| `NetReading` | `components/stats/NetReading.tsx` | 两者共用 | `density="regular" \| "compact"`：regular 出双 sparkline，compact 出单条合并走势 |

`StatsCard` / `StatsRow` 都只吃 `Readout`，不碰 store —— 数据注入在容器层（`StatsBoard` / `MenuStatsSection`）。

---

## 5. 布局

### 5.1 主窗口统计页（960×640，内容宽约 920）

```
本机资源总览                                  每 10 s 随内存常采更新
┌─────────────┬─────────────┐
│ 内存        │ CPU         │
└─────────────┴─────────────┘
┌─────────────┬─────────────┐
│ 硬盘        │ 端口        │
└─────────────┴─────────────┘
┌───────────────────────────┐
│ 网络  ↓ 12.3 MB/s  ↑ 820 KB/s  │  ← 第 3 行，col-span-2
│ 近 5 min 双走势（各 32 px）      │
└───────────────────────────┘
```

**卡高收到 140 px 的算式**（P3 的直接后果，必须一次算清）：

| 项 | 高度 |
|---|---|
| 内容区（`h-12` header 之后） | 592 |
| 容器 `py-4` | 32 |
| 标题块（h1 + p） | 36 |
| 网格上边距 `mt-4` | 16 |
| 3 行卡 `140×3` + 2 个 `gap-3` | 420 + 24 |
| 页脚说明 2 行 + `mt-4` | 46 |
| **合计** | **574 < 592 ✅ 一屏放下，不滚动** |

若沿用 152：610 > 592 → 多出 18 px 滚动。所以 `StatsBoard` 里的网格卡统一 `min-h-[140px]`（网络卡同高），主窗口其它区域不动。

- 网络卡点击 → `onDrill("net")`，与其它卡一致
- 页脚那句「清理入口在存储 tab」保留

### 5.2 面板统计页（380×432，v5 贴高 + v6 拆方向行）

```
Prune  footprint                          ⟳  ⤢
● 内存  警告                    13.4 / 16.0 GB   ┐
  ▓▓▓▓▓▓▓░░░░░  84% · 可用 2.1 GB · swap 12%    ┘ 48
⚡ 存储    可回收 46.0 GB          剩余 14.9 GB    ┐
  ▓▓▓▓▓▓▓░░░░░  77%                              ┘ 48
⚡ CPU                     23%                       ┐
  7 核 · load 1.20 / 1.50 / 1.80                   ┘ 48
↓ 下载                1.2 MB/s                     ┐
  ▁▂▃▅▆▅▃▂      近 5 min 1.0 MB/s                 ┘ 48
↑ 上传                240 KB/s                     ┐
  ▁▂▃▁▂▁▃▂      近 5 min 210 KB/s                 ┘ 48
◉ 端口              12 个监听 · 对外 3 · 连接 47    ┘ 48
──────────────────────────────────────────────
查看全部 343 个应用（CPU / 磁盘 / 网速对比） →
上次深探 1 分钟前                     每次 10 s · 网速 2 s
```

> v4：原草图中间的 `[统计 | 概况]` 切换行已删除——面板只有这一屏，没有可切的东西。
> **v6：网络行（64 px 例外）已删除**，拆成「↓ 下载」「↑ 上传」两条 48 px 标准行，算式见 `m16v6-net-direction-split-plan.md` §5.2。

**48 px 行的像素分解（P2 定稿，实施照此对齐）**：

| 段 | 高 | 内容 |
|---|---|---|
| `py-1.5` 上下内边距 | 12 | — |
| 第一行 | 18 | 左：标签（11 px / medium）+ 状态点；右：主读数（13.5 px mono）+ 小一号单位 |
| 间隔 | 2 | — |
| 第二行 | 14 | 内存/存储：Meter 4 px（`flex-1`）+ 右侧 10 px 副读数；CPU/端口：无 Meter，副读数占满整行；下载/上传（v6）：左 44% 迷你走势 14 px + 右侧近 5 min 均值 |
| 合计 | **48** | — |

**两条与 48 直接相关的取舍**：

1. **Meter 只保留在「内存」「存储」两行**。CPU / 端口 / 网络 / 下载 / 上传没有「占比」语义，塞一条空槽只会让第二行变窄；去掉后副读数能整行用（CPU 的 `7 核 · load 1.20/1.50/1.80` 约 165 px，可用 352 px，放得下）。
2. ~~**网络行 64 px 是唯一例外**~~（**v6 起作废**）：原 18 + 4 + 18 + 2 + 10 + 12 = 64 的放宽不再需要——网络拆成两条 48 px 方向行，48 px 规则重新无例外。

**空间预算（v5 贴高 + v6：窗口高 432，内容自然高 426，余 6）**：

| 项 | 高度 |
|---|---|
| `p-2` 上下 | 16 |
| 卡片 1px 边框 ×2 | 2 |
| header `h-11` | 44 |
| 内容区 `pt-1`（补 tab 行留白） | 4 |
| 6 行列表（48×6，`divide-y` 的 1px 因 `border-box` 含在 h-12 内） | 288 |
| 「查看全部」入口（`mt-1` 4 + `py-1.5` 12 + 文本行 15） | 31 |
| 内容区 `pb-3` | 12 |
| footer（`pt-1` 4 + `pb-2.5` 10 + 文本行 15） | 29 |
| **内容自然高** | **426** |
| **窗口高** | **432**（余 6 吸收子像素舍入） |

> `UpdateCard` 默认状态整块 `return null`（无待更新、无提示时 0 高），所以不进预算；一旦发现新版会长出约 40 px，此时内容区走 `overflow-y-auto` 滚动——这是低频事件，不为它常年留白。

- 泄漏横幅 `LeakBanner` 保留在列表第 1 行之前，仅命中时出现
- 列表区仍包 `min-h-0 overflow-y-auto` 兜底：极端情况（横幅 + 更新卡同时出现）允许滚动，但**默认状态必须不滚动**

---

## 6. 口径定义（正确性核心）

| 域 | 主读数 | 副读数（面板取前 1–2 条） | 空值语义 |
|---|---|---|---|
| 内存 | `physicalUsedBytes / physicalTotal` + 百分比 | 可用 `freeBytes`、swap 已用 %、压力点 | 旧快照无 `physicalUsedBytes` → `warmup`「采样中…」 |
| CPU | `cpu.usage`（整机每核平均 0–100） | 核数、load 1/5/15（无 load 平台不显示） | `usage == null` → `warmup`「采样中…（约 10 s）」 |
| 硬盘 | 已用 = `totalBytes − userAvailableBytes`（statvfs 粗略口径）+ 百分比 | 可用 `userAvailableBytes`、可回收 `report.totals.reclaimable`（未扫描则**整条不显示**） | `volume == null` → 读失败「容量不可读」/ 读取中 |
| 端口 | `entries.length` 个监听端口 | 关注层数、对外暴露数（>0 用 warn 色）、活跃连接 | `ports == null` → 「尚未扫描」（**不是 0**） |
| 网络 | ↓ `downBps` / ↑ `upBps`（物理口合计） | 近 5 min 平均（时间加权）、今日 ↓/↑（null → 不显示）、会话峰值 | `warmup`「采样中…」；`unavailable`「—」+ note；`ok && 0` →「0 B/s」 |

**三条硬口径（沿用 M9/M14，不在本方案改动）**：

1. 网速主数字**只算物理网卡**，虚拟口不计入 —— VPN 会与物理口重复计一次
2. 「今日」= **Prune 运行期间累计**，未运行时段无法回填 —— 文案必须带这个限定
3. 端口「连接多 ≠ 流量大」的提示保留在端口卡，网络卡不重复解释

---

## 7. 分期与改动清单

### S0 共享读数层（纯新增，不动 UI）

| 文件 | 动作 |
|---|---|
| `src/lib/statsReadout.ts` | 新增：5 个 readout 函数 + `Readout` / `NetReadout` 类型 + `loadTone` / `splitSpeed` |
| `src/lib/useVolumeInfo.ts` | 新增：从 `StatsBoard` 抽出的卷容量 effect |
| `src/lib/statsReadout.test.ts` | **未做**：项目没有测试框架（无 vitest），引入依赖超出「无新依赖」的边界；口径收敛在纯函数里、由 `tsc --noEmit` 守门，见 §10 偏差 1 |
| `src/components/Sparkline.tsx` | 加可选 `baselineZero`（默认 false，零破坏），网络迷你线用 true |

### S1 主窗口统计页加网络卡

| 文件 | 动作 |
|---|---|
| `src/components/StatsBoard.tsx` | 改用 readout；`DrillTab` 加 `"net"`；新增 `NetCard`（第 3 行 `col-span-2`）；网格卡 `min-h` 152 → 140 |
| `src/components/stats/NetReading.tsx` | 新增：regular / compact 双密度网络读数 |
| `src/windows/MainWindow.tsx` | `tab === "stats"` 时补 `void initNetTotal()`（**P0 坑**）；stats 分支的 `onRefresh` 加 `refreshNetTotal()`；`busy` 纳入 `netTotalRefreshing`；tab hint 改为「内存 / CPU / 硬盘 / 端口 / 网速」 |

### S2 面板统计化

| 文件 | 动作 |
|---|---|
| `src/panels/MenuBarPanel.tsx` | v4：**无 tab**，直接渲染统计区；删除 `DomainHero` / `AppsSection` / `MemSection`；`StatsSection`（4 行 48 + 网络行 64 + `LeakBanner` + 底行入口）；`useEffect` 补 `initNetTotal()` / `initPorts()`；刷新按钮加 `refreshNetTotal()` |
| `src/components/stats/StatsRow.tsx` | 新增：紧凑行，**高度锁定 48**（`h-12`，不用 `min-h`，避免副读数换行把行撑开） |

### S3 文档同步

README 的功能列表、`prune-feature-spec-v2.md` 的 tab 表；本文状态改「已实施（S0–S2）」；v5 同步 `tray.rs: PANEL_H`、`tauri.conf.json` 的 panel height 与本文 §5.2 空间预算。

---

## 8. 假事实规避清单

| 风险 | 处置 |
|---|---|
| 统计页网速恒「采样中…」 | S1/S2 都补 `initNetTotal()`；实施后必须真机确认 3 s 内出数 |
| 端口未扫描显示 0 | `ports == null` → 「尚未扫描」，与 M9 一致 |
| 可回收为 null 时显示 0 GB | 整条副读数不渲染 |
| 断网被当成「不可读」 | `status === "ok"` 且 `0 B/s` → 正常显示 0；只有 `unavailable` 才显示「—」 |
| 迷你走势把 0 附近噪声画成尖峰 | `Sparkline` 用 `baselineZero`（y 轴从 0 起） |
| 面板与主窗口数字不一致 | 同源 Rust 状态，误差上限 1 个采样周期（2 s）；验收按「差值 ≤ 1 拍」判定，不追求逐位相同 |
| 两个 tab 抢 `net_total_set_active` | 统计页**不调** `setActive`，只 `init()`；开关仍唯一属于 `net` tab |
| 48 px 行被副读数换行撑破 | `StatsRow` 用固定 `h-12` + 副读数 `truncate`，长文案省略而不是折行 |

---

## 9. 验收清单

- [ ] 面板窗口实测 **380×400**（v5），打开即统计一屏，**没有任何 tab 切换器**，5 行读数齐全
- [ ] 面板默认状态**整窗不滚动**、底部无空白：footer 紧贴卡片底边（内容自然高 394 vs 窗口 400）
- [ ] 面板每行实测高度 48 px（DevTools 量），网络行 64 px；默认状态**整窗不滚动**
- [ ] 面板与主窗口同时可见，内存 / 硬盘 / 端口 / 网速四项读数差值 ≤ 1 个采样周期
- [ ] 主窗口统计页三行卡（第 3 行网络卡横跨两列）**一屏放下不滚动**，窗口 960×640 实测
- [ ] 统计页进入 3 s 内网速出数（不是「采样中…」）
- [ ] 切到「网络」tab 采样变 1 s，切回「统计」仍显示数字（不归零、不停采样）
- [ ] 断网：网络行显示 `0 B/s`；伪造 `unavailable` 场景显示「—」+ 原因，**不显示 0**
- [ ] 面板内不启动 `nettop` 子进程（统计页只用系统口径；`ps` 里看不到新增 nettop）
- [ ] 端口未扫描时显示「尚未扫描」；点刷新后出数字
- [ ] 「查看全部 N 个应用」跳转到主窗口 `resources` tab 且 N 与实际一致
- [ ] 泄漏命中时 `LeakBanner` 仍在统计页顶部出现，未命中不占版面
- [ ] `tsc --noEmit` 0 错误；`cargo check` 无改动（本方案不动 Rust）

---

## 10. 工作量与风险

| 阶段 | 估时 | 主要风险 |
|---|---|---|
| S0 读数层 + 单测 | 0.5 d | readout 粒度切太细会变成第二套口径，需严守「一个域一个函数」 |
| S1 主窗口网络卡 | 0.5 d | 卡高从 152 收到 140 后，四张旧卡的信息密度需真机确认不显拥挤 |
| S2 面板统计化 | 0.5 d | 48 px 行里「主读数 + 单位 + 状态点」在 352 px 可用宽度下的排布，需按最长文案（`13.4 / 16.0 GB` + 「警告」）实测 |
| S3 文档 | 0.1 d | — |
| **合计** | **约 1.6 d** | 无 Rust 改动，无新依赖 |

**回滚**：S2 单独成 commit，面板若观感不佳可只回退该 commit，主窗口网络卡独立生效。

---

## 11. 实施记录（2026-11-06）

| # | 偏差 | 原因与处置 |
|---|---|---|
| 1 | 未加 `statsReadout.test.ts` | 项目无测试框架，引入 vitest 违反「无新依赖」。口径集中在 5 个纯函数（可机械验证），当前用 `tsc --noEmit` + `vite build` 守门；后续若引入测试框架，这批函数是第一批该被覆盖的 |
| 2 | 主窗口 CPU 卡不再带 10 min 走势 | 三行卡必须等高 140 px 才能一屏放下；CPU 走势在「应用资源」与灵动岛已有，统计页只留读数 |
| 3 | 面板「存储」行主读数用「已用 / 总量」而非「可回收 GB」 | 与主窗口统计卡保持同一主读数（一致性优先）；可回收降为副读数第一条，仍在一屏内可见 |
| 4 | 删除 `TopAppCards`（`AppResourceGrid.tsx`） | 面板「应用」tab 撤除后它变成零消费导出（约 110 行）。`ResourceCard` 的 `dense` 档位保留，未动 |
| 5 | ~~面板「概况」tab 的 `MemSection` 原样保留~~ → **v4 已删除** | 用户决定面板只留单一 tab，`MemSection` / `VictimRow` / `leakFootnote` 随之零消费，一并清理（约 130 行）。内存压力 / swap / 换出受害者在主窗口「内存」tab 有完整版，面板不再重复 |

**实际改动文件**

| 类型 | 文件 |
|---|---|
| 新增 | `src/lib/statsReadout.ts`、`src/lib/useVolumeInfo.ts`、`src/components/stats/StatsCard.tsx`、`src/components/stats/StatsRow.tsx`、`src/components/stats/NetReading.tsx` |
| 改写 | `src/components/StatsBoard.tsx`（5 卡 + readout）、`src/panels/MenuBarPanel.tsx`（v4 后为单页统计，无 tab / 无 hero / 无 apps） |
| 修改 | `src/windows/MainWindow.tsx`（init net total / 刷新分流 / busy / hint）、`src/components/Sparkline.tsx`（baselineZero）、`src/components/AppResourceGrid.tsx`（删 TopAppCards）、`README.md` |
| 修改（v5） | `src-tauri/src/tray.rs`（`PANEL_H` 560→400 + 算式注释）、`src-tauri/tauri.conf.json`（panel `height` 560→400）、`src/components/stats/StatsRow.tsx` 与 `src/panels/MenuBarPanel.tsx` 的尺寸注释 |
| 未动 | `net_sampler.rs`、窗口宽度 `PANEL_W`、采样节拍、其它 Rust 逻辑 |

**验证结果**：`tsc --noEmit` 0 错误；`vite build` 通过。v4 单页化后为 73 modules，373.38 kB / gzip 112.13 kB（v3 为 378.21 kB，删掉概况区块约 4.8 kB）；未做真机截图（见 §9 验收清单）。

---

## 12. 变更记录

| 版本 | 日期 | 变更 |
|---|---|---|
| v1 | 2026-11-06 | 初稿：双端统计统一 + 网速，含三 tab / 128 px 卡高等备选 |
| v2 | 2026-11-06 | 用户拍板 P1–P3：删「应用」tab、行高锁 48、网络卡第 3 行横跨；卡高 152→140（一屏放下算式见 §5.1）；48 px 行结构定稿（§5.2）；作废三 tab 与 128 px 备选 |
| v3 | 2026-11-06 | S0–S3 实施完成；补 §11 实施记录（5 条偏差 + 改动文件清单 + 验证结果） |
| v4 | 2026-11-06 | 用户拍板 P4：面板**只保留一个 tab**，去掉切换器。删 `PanelTab` / `<Tabs>` / `MemSection` / `VictimRow` / `leakFootnote`；省 36 px（空间预算 408→372）；构建体积 378.21→373.38 kB |
| v5 | 2026-11-06 | 用户拍板 P5：面板**高度贴合内容**，560 → **400**（`tray.rs: PANEL_H` + `tauri.conf.json` panel `height` 同改）。内容自然高 394（逐项算式见 §5.2），余 6；去掉 154 px 底部空白，默认态不滚动。`UpdateCard` 默认 0 高不进预算，出现时走内容区滚动 |
| v6 | 2026-11-06 | 网络读数按**下载 / 上传**拆分（主窗口总览 3×2 六卡 + 面板 6 行），48 px 规则重新无例外；`PANEL_H` 400 → **432**。完整方案与改动清单见 **`m16v6-net-direction-split-plan.md`** |
