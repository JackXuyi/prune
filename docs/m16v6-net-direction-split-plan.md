# M16 v6 统计读数「下载 / 上传」拆分方案

> 状态：**已实施（2026-11-06）**；门禁全绿（`tsc --noEmit` 0 / `cargo check` 通过 / `vite build` 378.05 kB），真机走查待做
> 前置：M16 共享读数层（`src/lib/statsReadout.ts`）、M14 网络 tab、`net_sampler` 2 s 常驻轮
> 需求：① 主窗口「本机资源总览」里的**网络卡**拆成「下载」「上传」两张卡；② 菜单栏面板里的**网络行**拆成「下载」「上传」两行
> 约束：面板宽度 380 不变；主窗口三行卡不得溢出 592 px 内容区；口径与空值语义沿用 M16 §8 红线（读不到 ≠ 0）

---

## 0. 已拍板

| # | 决定 | 落地含义 |
|---|---|---|
| P1 | 总览页变 **3 行 × 2 列共 6 卡** | 网络卡去掉 `col-span-2`，下载 / 上传各占一格；行数不变 → 垂直高度预算不变 |
| P2 | 面板网络行 **64 → 两行 48** | 六行总和 288（原 48×4+64=256，+32）；`PANEL_H` 400 → **432**，`tauri.conf.json` 同改 |
| P3 | 上下行**各自一条走势**，不做合并线 | 量级常差 10 倍，共享纵轴会把上传压成直线；沿用各自归一化 + `baselineZero` |
| P4 | 每个方向的读数由 `netDirectionReadout()` 派生 | 口径仍然只在 readout 层定义一次，两处界面只负责排版（M16 §4 的纪律） |
| P5 | 卡片脚注**单行两个字段**（近 5 min 均值 · 今日），峰值进 `title` | 140 px 卡高只够一行脚注；且 `peakAt` 是**双方向共用**的单一时戳，逐方向标会误读成「该方向在该时刻达到峰值」 |

---

## 1. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 总览页网络卡拆两张 | 下载卡（accent 绿）/ 上传卡（info 蓝），各带大数字 + 独立走势 + 方向专属脚注 | 拆开后每张卡有自己的刻度、峰值、今日累计，不用在两个方向之间来回找小数点 |
| 面板网络行拆两行 | 每行 48 px：标签 + 速度 / 迷你走势 + 近 5 min 均值 | 面板是「瞄一眼」，方向必须在对齐的位置各占一行，而不是挤在一行里比大小 |
| 卡高不变（140） | 新卡内部预算重排：读数 24 + 走势 26 + 脚注 10 = 68 | 三行卡 574 px 的算式（M16 §5.1）原样成立，不重算页面 |
| 面板高度 +32 | 400 → 432（内容自然高 426 + 6 余量） | 面板高度是**贴内容**算出来的（M16 P5），多一行就必须把它一起改掉，否则底行被压进滚动区 |
| 读数层新增派生函数，不新增数据源 | 输入仍是 `NetTotalSample` + `history`，零新增 IPC、零新增采样 | 面板「不轮询、不提速」的前提（M16 §5.2）不能破 |
| 删掉 `totalSeries` / `NetReading` / `NetStatsRow` | 三者唯一消费者本次全部消失 | 留着就是「已实现但零消费」的死代码，下次改口径时还会误导 |

---

## 2. 需求理解与边界

**用户问题**（每条 UI 决策都要能对上一条）：

1. 「现在下载多少？」「现在上传多少？」——两个问题要能**分别**回答，而不是一行里两个数字抢位置
2. 「上传是不是卡住了？」——上传需要自己的走势，否则被下载的大数值淹没，看不出它其实在动
3. 「两个界面看到的一样吗？」——同一时刻两处同方向数字差值 ≤ 1 个采样周期（2 s）
4. 「拆开后还能一屏看完吗？」——总览页仍是一屏，面板仍是贴高不滚动

**边界**：

- ✅ 做：`StatsBoard` 布局、`NetReading` 拆方向、`StatsRow` 网络行拆方向、`statsReadout` 新增派生函数、`tray.rs` / `tauri.conf.json` 面板高度、相关注释与文档同步
- ❌ 不做：新增 Rust 命令、改采样节拍、改 `netStore` / `netTotalStore`、动「网络 tab」（`NetBoard`）本身、动菜单栏标题（`TITLE_NET` 仍是合并数值）

---

## 3. 现状诊断（代码级）

| 项 | 现状 | 缺口 |
|---|---|---|
| 总览页布局 | `StatsBoard.tsx:65-98`：`grid-cols-2`，四卡 + 第 3 行 `<div className="col-span-2"><StatsCard 网络 /></div>` | 网络卡内左右并排两个方向（`NetReading.tsx:16-33`），方向之间只靠颜色区分 |
| 网络卡主体 | `NetReading.tsx`：`Direction`（19 px 读数 + 24 px 走势）+ 共享一行脚注（只取 `sub.slice(1)`） | 读数比其它卡（24 px）小一号；脚注两个方向混排，`今日 / 峰值` 被挤成一行 |
| 面板网络行 | `StatsRow.tsx:97-155` `NetStatsRow`：**64 px**（48 px 规则的唯一例外），一行放 ↓/↑ 两个读数 + 一条 `totalSeries` 合并走势 | 合并走势看不出方向；两读数都在第一行右侧，宽度紧张（`max-w-[62%]`） |
| 面板高度 | `tray.rs:79` `PANEL_H = 400`（内容 394 + 6）；`tauri.conf.json:29` panel `height: 400` | 加一行（+32）后必须同步，否则底行被裁 |
| 读数层 | `netReadout()` 已返回 `downBps / upBps / downSeries / upSeries / totalSeries / avgDownText / avgUpText` | 缺「单个方向」的完整读数（今日 / 峰值 / 状态），展示层只能自己拼 |
| 采样 | `netTotalStore`：`init()` 幂等，2 s 常驻；总览页与面板都只 `init()` 不提速 | 无缺口，本次不碰 |

---

## 4. 读数层改造（`src/lib/statsReadout.ts`）

### 4.1 新增 `NetDirectionReadout`

```ts
export type NetDirection = "down" | "up";

export interface NetDirectionReadout {
  dir: NetDirection;
  /** 原始 bps（0 在 ok 下是「真的没有」，warmup/unavailable 下不可信） */
  bps: number;
  /** 数字与单位拆开，给大号读数排版用 */
  main: string;
  unit: string;
  /** 该方向的走势序列（主窗口与面板共用，各自归一化） */
  series: number[];
  /** 近 5 min 时间加权均值，已格式化，如 "90.7 KB/s" */
  avgText: string;
  /** 今日累计，已格式化，如 "738 MB"；today 为 null 时是 null */
  todayText: string | null;
  /** 本次会话单拍峰值，已格式化；无读数时为 null */
  peakText: string | null;
  /** 会话累计（自启动起），已格式化 */
  sessionText: string | null;
  status: ReadoutStatus;      // ok / warmup / unavailable
  note?: string;              // 非 ok 时的原因
}
```

### 4.2 `netDirectionReadout()` 实现要点

```ts
export function netDirectionReadout(
  sample: NetTotalSample | null,
  history: NetTotalPoint[],
  dir: NetDirection,
): NetDirectionReadout
```

- **序列**：直接取 `history.map(p => dir === "down" ? p.downBps : p.upBps)`——不重算、不重采样。
- **均值**：`weightedAverage(history)` 一次算两边（同一函数，M16 已定），只取对应方向字段。
- **今日 / 峰值 / 会话**：分别取 `sample.today?.downBytes / upBytes`、`sample.peakDownBps / peakUpBps`、`sample.sessionDownBytes / sessionUpBytes`。
- **三态**（与 M16 §8 完全一致）：
  - `sample == null` → `warmup`，`note = "采样中…"`
  - `status === "warmup"` → `warmup`，`note = "采样中…（首拍只建基线，约 2 s）"`
  - `status === "unavailable"` → `unavailable`，`main = "—"`、`unit = ""`、`note` 取 `sample.note`——**不显示 0**
  - `ok` 且 `bps === 0` → 正常显示 `0 B/s`（真的没有流量）
- 数值格式化只调 `splitSpeed()` / `formatSpeed()` / `formatBytes()`，不新增格式化函数。

### 4.3 移除 `totalSeries`

`NetReadout.totalSeries` 的唯一消费者是 `NetStatsRow`，拆行后归零 → 一并删除（含 `netReadout()` 里的一行 map）。`netReadout()` 本身保留：面板行的 `title` 与状态说明仍用它，避免两处各自拼「不可读原因」。

---

## 5. 布局

### 5.1 主窗口「本机资源总览」（960×640，内容区宽约 920 / 高 592）

```
┌─────────────┬─────────────┐
│ 内存        │ CPU         │   min-h-[140px]
├─────────────┼─────────────┤
│ 硬盘        │ 端口        │   min-h-[140px]
├─────────────┼─────────────┤
│ 下载 ↓      │ 上传 ↑      │   min-h-[140px]
└─────────────┴─────────────┘
```

**高度算式（不变）**：3 行 × 140 + 行距 12×2 + 标题块 ≈ 74 + 页脚 ≈ 24 = 574 px ≤ 592 px。行数没变，所以算式原样成立。

**单卡内部预算（140 px 怎么装下三块）**：

| 块 | 高度 | 说明 |
|---|---|---|
| `px-4 py-3.5` | 28 | 上下内边距（`StatsCard` 既有） |
| 标题 11 px | 15 | 「下载」/「上传」 |
| caption 10 px | 14 | 「物理网卡合计 · VPN 不会双计」 |
| `mt-3` | 12 | 标题块与读数之间 |
| 主读数 | 24 | `font-mono text-[24px]`，与其它卡一致（原网络卡是 19 px） |
| `mt-1` + 走势 26 | 30 | `Sparkline height={26}`，各自归一化 |
| `mt-1` + 脚注 10 | 14 | 单行 10 px |
| 合计 | **137 / 140** | 留 3 px 余量，不撑高 |

**脚注文案**：`近 5 min ↓ 90.7 KB/s · 今日 ↓ 738 MB`；`title` 补 `本次会话峰值 ↓ 12.4 MB/s · 会话累计 1.2 GB`。

### 5.2 菜单栏面板（380 宽，高度贴内容）

```
内存            12.6 GB / 16.0 GB     48
存储            451.4 GB / 460.4 GB   48
CPU             48 %                  48
↓ 下载              99.1 KB/s         48   ← 新
↑ 上传               8.2 KB/s         48   ← 新
端口            25 个监听             48
```

**行内排版（48 px = 内容 36 + 上下内边距 12）**：

| 行段 | 内容 |
|---|---|
| 第一行 18 px | `↓` 箭头（accent）+ 「下载」标签 · 右侧 `13.5 px` mono 数值 + `10 px` 单位（复用 `Reading` 尺寸） |
| 第二行 14 px | 左侧 44% 宽迷你走势（`Sparkline height={14}`）+ 右侧 9.5 px `近 5 min 90.7 KB/s`（truncate） |
| 交互 | 整行可点 → `openMain(undefined, "net")`；`title` 给全量脚注（均值 / 今日 / 峰值 / 会话） |

**高度算式**：

| 项 | 旧 | 新 |
|---|---|---|
| 内容区 | pt-1 4 + (48×4 + **64**) + 入口 31 + pb-3 12 = **303** | 4 + (48×6 = **288**) + 31 + 12 = **335** |
| 面板自然高 | p-2 16 + 边框 2 + header 44 + 303 + footer 29 = **394** | 16 + 2 + 44 + 335 + 29 = **426** |
| `PANEL_H` | 400（+6 余量） | **432**（+6 余量） |

---

## 6. 展示组件

### 6.1 `NetDirectionReading`（`src/components/stats/NetReading.tsx`，替换 `NetReading`）

```tsx
export function NetDirectionReading({ readout }: { readout: NetDirectionReadout }) {
  const ok = readout.status === "ok";
  const text = readout.dir === "down" ? "text-accent" : "text-info";
  // 三态：warmup → 「采样中…」；unavailable → 「—」+ 原因；ok → 数值 + 走势 + 脚注
}
```

- 走势：`series.length >= 2` 才画，否则保留「走势积累中（每秒一个点）」占位（沿用现有文案）。
- 脚注：`ok` 时单行；非 `ok` 时改为一行原因文本（不画走势、不写 0）。

### 6.2 `NetDirectionRow`（`src/components/stats/StatsRow.tsx`，替换 `NetStatsRow`）

- 结构对齐 `StatsRow`（`h-12 flex-col justify-center gap-0.5`），复用 `Reading`（`size="sm"`）渲染数值 + 单位。
- 箭头颜色区分方向，标签只写「下载」/「上传」（面板宽度有限，不重复写 `↓` 两次）。
- 非 `ok` 时第二行不画走势，整行显示原因（`max-w-[62%] truncate`，与现有面板行一致）。

### 6.3 `StatsBoard` 接线

```tsx
const netDown = netDirectionReadout(netSample, netHistory, "down");
const netUp   = netDirectionReadout(netSample, netHistory, "up");

<StatsCard title="下载" caption="物理网卡合计 · 虚拟接口不计入，VPN 不会双计"
  readout={net} body={<NetDirectionReading readout={netDown} />}
  onOpen={() => onDrill("net")} />
<StatsCard title="上传" caption="物理网卡合计 · 与下载各自刻度，量级常差 10 倍"
  readout={net} body={<NetDirectionReading readout={netUp} />}
  onOpen={() => onDrill("net")} />
```

> `readout={net}` 只用于 `StatsCard` 的内部判定（`status !== "ok" && !body` 的分支在传 `body` 时不生效），主读数渲染完全由 `body` 决定。

---

## 7. 空值 / 边界语义（两处一致）

| 状态 | 主读数 | 走势 | 脚注 / 原因 |
|---|---|---|---|
| `warmup`（首拍未到） | 「采样中…」 | 不画，占位文案 | 「采样中…（首拍只建基线，约 2 s）」 |
| `unavailable`（计数器读不到） | 「—」 | 不画 | 「不可读：<sample.note>。这里不显示 0」 |
| `ok`，本拍 0 | 「0 B/s」 | 平线（真实数据） | 正常显示今日累计等 |
| `ok`，`today === null` | 正常 | 正常 | 脚注只留近 5 min 均值，**不写「今日 0」** |
| 物理网卡为空（断网） | 0 B/s | 平线 | 沿用 M14：「断网时是 0 B/s，不是不可读」 |

---

## 8. 改动清单（文件级）

| 文件 | 改动 | 类型 |
|---|---|---|
| `src/lib/statsReadout.ts` | 新增 `NetDirection` / `NetDirectionReadout` / `netDirectionReadout()`；删除 `NetReadout.totalSeries` 及其赋值 | 改 |
| `src/components/stats/NetReading.tsx` | 删 `NetReading` + 私有 `Direction`，改为导出 `NetDirectionReading` | 重写（约 70 行） |
| `src/components/stats/StatsRow.tsx` | 删 `NetStatsRow`，新增 `NetDirectionRow` | 重写（约 55 行） |
| `src/components/StatsBoard.tsx` | 网络卡去掉 `col-span-2`，拆成下载 / 上传两卡；头部说明文案微调 | 改 |
| `src/panels/MenuBarPanel.tsx` | 网络行 → 两行；顶部注释「五行」→「六行」 | 改 |
| `src-tauri/src/tray.rs` | `PANEL_H` 400 → 432，注释算式同步（303 → 335） | 改 |
| `src-tauri/tauri.conf.json` | panel 窗口 `height` 400 → 432 | 改 |
| `docs/m16-stats-unify-plan.md` | §0 的 P2/P3 加「v6 变更」标注；§5.1 / §5.2 布局图更新；§12 变更记录追加一行指向本文档 | 改 |
| `docs/m16v6-net-direction-split-plan.md` | 本文档 | 新增 |

**不动的文件**：`stores/netTotalStore.ts`、`stores/netStore.ts`、`types/net.ts`、`components/NetBoard.tsx`（网络 tab 本身）、`lib/ipc.ts`、任何 `src-tauri/src/net_*.rs`。

---

## 9. 验收清单

- [ ] `pnpm tsc --noEmit` 0 错误
- [ ] `cargo check`（`src-tauri`）通过，无「未使用」告警
- [ ] 总览页：6 张卡 3×2 对齐，页面**不出现竖向滚动条**（574 ≤ 592）
- [ ] 总览页：下载卡数字为 accent、上传卡为 info；两条走势形状不同且各自铺满卡片高度
- [ ] 总览页：点击下载卡 / 上传卡都跳到「网络」tab
- [ ] 面板：六行读数等高对齐，**不出现内部滚动条**（426 ≤ 432）
- [ ] 面板：两行各自走势可见，上传空闲时是一条低平线而不是空白
- [ ] 同一时刻面板与总览页同方向数值差 ≤ 1 个采样周期（2 s）
- [ ] 断网 / 拔掉网线：两处都显示 `0 B/s` 平线，而不是「不可读」
- [ ] 杀掉网卡读数（devFixture 模拟 `unavailable`）：两处都显示原因文本，不显示 0
- [ ] 冷启动 1–2 s 内：两处都显示「采样中…」，不闪 0
- [ ] 菜单栏标题（图标旁数字）仍为合并数值，未受影响

---

## 10. 工作量与风险

| 项 | 评估 |
|---|---|
| 改动规模 | 6 个源码文件（约 +150 / −120 行）+ 1 个原生常量 + 2 处文档 |
| 风险 R1 | **面板高度算错** → 底行被裁或出现滚动条。缓解：`PANEL_H` 与 `tauri.conf.json` 同改，实测截图核对；万一溢出，把入口行 `31` 收到 27 可再省 4 px |
| 风险 R2 | 卡片脚注换行 → 卡片被撑到 142 px，三行合计 580 仍 < 592，仍不滚动（可接受），但视觉不齐。缓解：脚注 `truncate` + 单位固定 10 px |
| 风险 R3 | 删 `totalSeries` 后有遗漏引用 → 类型检查会直接报错，属于「编译期可发现」，低风险 |
| 风险 R4 | 面板行内 14 px 走势在 Retina 上糊成一团。缓解：`Sparkline` 已有 18 px 版本，14 px 下只保留线宽 1 px、不画填充 |
| 回滚 | 纯展示层 + 一个窗口高度常量，`git revert` 单提交即可，无数据/存储迁移 |

---

## 11. 变更记录

| 日期 | 版本 | 变更 |
|---|---|---|
| 2026-11-06 | v1 | 初版：仅主窗口总览页拆卡（用户第一轮反馈） |
| 2026-11-06 | v2 | 补菜单栏面板网络行拆分 + `PANEL_H` 连带改动（用户第二轮反馈截图） |
| 2026-11-06 | v3 | 成文：补读数层派生函数、卡片高度预算算式、面板高度算式、删除清单、验收清单 |
| 2026-11-06 | v4 | 实施完成。三处实施偏差：① 面板不再调用 `netReadout()`（合并读数在面板归零），但函数保留——总览页仍用它做 `StatsCard` 状态判定；② 方向行非 ok 时第二行留空（第一行已显示原因，重复渲染会顶高行内容）；③ 卡片峰值/会话 tooltip 落在 `NetDirectionReading` 根元素 `title` 上（`StatsCard` 不接受 `title` prop，不改它的接口）。验证：`tsc --noEmit` 0、`cargo check` 通过、`vite build` 378.05 kB / gzip 113.40 kB |
