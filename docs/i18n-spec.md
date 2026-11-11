# Prune i18n 技术规格（Implementation Spec）

> 上游方案：`docs/i18n-plan.md`（决策与分阶段）。
> 本文是**可执行规格**：契约、签名、key 清单、逐文件迁移表、测试与验收。
> 实现时以本文为准；与方案冲突时以本文为准并同步改方案。
> 基线版本 v0.6.0。

---

## 0. 规格边界

| 本文规定 | 本文不规定 |
|---|---|
| 数据结构、函数签名、IPC/事件协议 | 具体英文措辞的最终润色（P5 走查可微调，但 key 与参数不改） |
| key 命名、namespace 划分、降级行为 | 新增第三个语言的适配层 |
| Rust 侧翻译表的组织方式 | CLI（`prune-cli`）输出——**冻结为现状中文** |
| 每文件的改动范围与阶段归属 | 发布物料（README 双版在 P5） |

---

## 1. 冻结契约

以下五项一旦实现，改动需评审。前端与 Rust 各自依赖它们，任何一侧单方面修改都会静默断链。

### 1.1 Locale

**唯一的规范形态是两值字符串**：`"zh"` | `"en"`。

```ts
// src/i18n/types.ts
export type Locale = "zh" | "en";
```

```rust
// crates/prune-core/src/locale.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    Zh,
    #[default]
    En,
}

impl Locale {
    /// BCP-47 标签 → Locale。只做前缀匹配，不引入 ICU。
    /// zh-Hans / zh-CN / zh-Hant → Zh（繁体用户看到简体文案，已知取舍）
    /// 其余一律 En（含 ja / de / 无法识别）
    pub fn from_tag(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("zh") { Self::Zh } else { Self::En }
    }
    pub fn tag(self) -> &'static str {
        match self { Self::Zh => "zh", Self::En => "en" }
    }
}
```

**约束**：Rust 侧不得出现第三个变体；前端不得接受 `"auto"` 作为 `Locale` 值（`auto` 只存在于设置层，见 1.2）。

### 1.2 settings.json

```jsonc
{
  "cpuAnim": true,
  "menuNumbers": true,
  "iconTheme": "scissors",
  "island": { "enabled": true, "cpuPct": 90, "...": "..." },
  "locale": "auto"          // 新增。缺省 = "auto"
}
```

| 属性 | 规格 |
|---|---|
| 键名 | `locale`（与现有 camelCase 习惯一致；`island` 是例外，沿用旧键） |
| 取值域 | `"auto"` \| `"zh"` \| `"en"`；**其它值视为 `"auto"`** |
| 缺省 | `"auto"`，老用户不迁移、不写盘 |
| 写入时机 | 仅在用户显式切换时写（同 `set_many` 一次写） |
| 读取 | `settings::get_str(app, "locale")` |

### 1.3 IPC 命令

| 命令 | 入参 | 成功返回 | 失败 | 线程/幂等 |
|---|---|---|---|---|
| `locale_get` | — | `{ current: Locale, source: "auto" \| "manual" }` | 不失败（读不到系统语言时 `current="en", source="auto"`） | 主线程只读，幂等 |
| `locale_set` | `{ locale: "auto" \| "zh" \| "en" }` | 同上结构（解析后的结果） | `Result<_, String>`：写盘失败 / 重建菜单失败 | 幂等（同值重复调用只做一次重建） |
| `net_diag_get` | `{ locale: Locale }` | `NetDiag \| null`（按该 locale 重渲染 verdict） | 不失败 | 只读，不改存储 |
| `net_diag_copy_report` | —（**签名不变**） | `usize` 字符数 | `Result<_, String>` | 内部从 `AppHandle` 取当前 locale |

命令注册：`lib.rs` 的 `generate_handler!` 列表追加三项（`net_diag_copy_report` 已存在，只改实现）。

**为什么不给 `net_diag_copy_report` 加参数**：它是「点了就复制」的无参命令；加参数会让前端在点击瞬间还要多读一次 store。Rust 侧读 managed state 更短、更不会不同步。

### 1.4 事件

| 事件名 | 载荷 | 发送方 | 订阅方 |
|---|---|---|---|
| `locale://changed` | `{ current: Locale, source: "auto" \| "manual" }` | `locale_set` 成功后 `app.emit(...)`（**全局，不是 emit_to**） | 主窗口 / 面板 / 灵动岛三份 store |

规格：
- 必须在托盘菜单重建**之后**发送（否则前端立刻点托盘会看到旧语言）。
- 事件是「通知」，不是「命令」：收到事件的窗口只更新 store，不回写、不再 invoke。
- 发送失败（无监听者）不算错误。

### 1.5 降级链

| 场景 | 行为 | 用户可见 |
|---|---|---|
| 缺 key | dev：`console.error` + 返回 key 本身；prod：同上但不打日志 | 屏上出现 `disk.reclaim.title` 这种串 —— **故意难看，便于发现** |
| 缺参数 | 原样保留 `{name}` | `12 {count}` 这种串 |
| 未知系统语言 | `En` | 英文界面 |
| `settings.json` 读失败 / 解析失败 | 视为 `auto` | 跟随系统 |
| `locale_set` 写盘失败 | 前端回滚到旧 locale + toast | 「语言切换失败：{err}」 |
| `locale_get` IPC 不可用（浏览器 dev） | 用 `navigator.language` 推断 | 正常 |

> 降级原则沿用项目既有纪律：**不静默、不假装成功**。缺 key 显示 key，缺参数显示占位符。

---

## 2. 前端规格

### 2.1 文件与职责

| 文件 | 职责 | 约行数 |
|---|---|---|
| `src/i18n/types.ts` | `Locale`、`Params`、`MessageKey`、`ParamsOf<K>`、`Messages` 结构类型 | 40 |
| `src/i18n/index.ts` | `flatten` / `createI18n` / `i18nFor`（按 locale 记忆化） | 70 |
| `src/i18n/store.ts` | zustand：`locale` / `source` / `ready` / `init()` / `setLocale()` / `useI18n()` | 80 |
| `src/i18n/detect.ts` | `fetchLocale()`（IPC + 浏览器兜底） | 25 |
| `src/i18n/messages/zh-CN.ts` | **唯一 source of truth**：聚合 13 个 namespace 并导出结构 | 25 |
| `src/i18n/messages/en-US.ts` | `satisfies Messages`，漏/多 key 编译失败 | 25 |
| `src/i18n/messages/namespaces/*.ts` | 每文件导出 `{ zh, en }` 并置 | 13 个 |

### 2.2 导出 API（精确签名）

```ts
// src/i18n/index.ts
export function flatten(tree: object, prefix?: string): Record<string, string>;
export function createI18n(locale: Locale): I18n;
export function i18nFor(locale: Locale): I18n;   // 记忆化，最多两个实例

export interface I18n {
  locale: Locale;
  t(key: MessageKey, params?: Params): string;
  /** 复数：英文按 Intl.PluralRules 选 .one/.other；中文恒 .other */
  plural(key: MessageKey, n: number, params?: Params): string;
}

// src/i18n/store.ts
export interface LocaleState {
  locale: Locale;
  source: "auto" | "manual";
  ready: boolean;
  init(): Promise<void>;                       // 启动时 await，只跑一次
  setLocale(next: "auto" | "zh" | "en"): Promise<void>;
}
export function useI18n(): I18n;               // 组件入口
export function tGlobal(key: MessageKey, params?: Params): string;  // 见 2.5 使用限制
```

`useI18n()` 实现（必须用 memo，不能每次 render 新建）：

```ts
export function useI18n(): I18n {
  const locale = useLocaleStore((s) => s.locale);
  return useMemo(() => i18nFor(locale), [locale]);
}
```

### 2.3 类型层（L2 严格）

```ts
// src/i18n/types.ts
import type { zh } from "./messages/zh-CN";

export type Locale = "zh" | "en";
export type Params = Record<string, string | number>;

/** 嵌套 → 点分联合：{ disk: { title: "…" } } → "disk.title" */
type FlatKeys<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${P}${K}`
    : FlatKeys<T[K], `${P}${K}.`>;
}[keyof T & string];

export type MessageKey = FlatKeys<typeof zh>;

/** 从模板串抽参数名：`{count} 项` → "count" */
type ExtractParams<S> = S extends `${string}{${infer P}}${infer R}`
  ? P | ExtractParams<R>
  : never;

type FlatOf<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? { [Q in `${P}${K}`]: T[K] }
    : FlatOf<T[K], `${P}${K}.`>;
}[keyof T & string];

type FlatMessages = FlatOf<typeof zh>;
type ParamNames<K extends MessageKey> = ExtractParams<FlatMessages[K]>;

export type Args<K extends MessageKey> =
  [ParamNames<K>] extends [never] ? [] : [Record<ParamNames<K>, string | number>];
```

`t` 的签名改为泛型：

```ts
t<K extends MessageKey>(key: K, ...args: Args<K>): string;
```

效果与代价：

| 错误 | 是否编译失败 |
|---|---|
| `t("disk.titl")` 拼错 | ✅ |
| `t("disk.title", { n: 1 })` 该 key 无 `{n}` | ✅ |
| `t("disk.found")` 漏传 `{count}` | ✅ |
| en-US 少一个 key / 多一个 key / 值是函数 | ✅（`satisfies Messages`） |

> 若 `tsc` 增量耗时超过 300 ms（用 `tsc --extendedDiagnostics` 量），降级为 L1：只保留 `MessageKey` 校验，`t(key, params?: Params)`。降级只需删 `Args`/`FlatMessages`，不动调用点。

### 2.4 启动时序（防闪语言）

```tsx
// src/main.tsx
async function boot() {
  await useLocaleStore.getState().init();          // isTauri? invoke locale_get : navigator 兜底
  const { locale } = useLocaleStore.getState();
  document.documentElement.lang = locale === "zh" ? "zh-Hans" : "en";
  createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
}
void boot();
```

规格要点：
- **先 resolve 再 render**，避免「中文闪一下变英文」。
- `init()` 内部幂等：`if (get().ready) return;`（StrictMode 双调用无害）。
- `init()` 里注册 `listen("locale://changed")`，回调只 `set({locale, source})`，不 await、不 IPC 回环。
- `index.html` 的 `lang` 改为 `en`（静态兜底），运行时被覆盖。
- 三个窗口各自执行一次 `boot`，互不依赖（岛的 `init` 与内存快照无关）。

### 2.5 使用限制（硬规则）

| 规则 | 理由 | 检查方式 |
|---|---|---|
| 组件里只用 `useI18n()`，不 import `i18nFor` | 直接 import 会绕过 locale 订阅，切语言不重渲染 | code review |
| `lib/` 下的纯函数**禁止** import `tGlobal` | 纯函数读全局 → 测试语言不可控、切语言后旧串不更新 | `check-i18n.mjs` 加一条：`src/lib/**` 里出现 `tGlobal` 即失败 |
| `tGlobal` 只允许在 store 内造 toast 文案时用 | 无 React 上下文的最小出口 | code review |
| 传给纯函数的参数是 `locale`，不是 `t` | 纯函数还要调 `format.ts` 的格式化，光有 `t` 不够 | 见 2.6 |

### 2.6 纯函数签名变更

| 现状 | 改为 |
|---|---|
| `memoryReadout(snapshot)` | `memoryReadout(snapshot, locale)` |
| `cpuReadout(cpu)` | `cpuReadout(cpu, locale)` |
| `diskReadout(volume, reclaimable, known)` | `diskReadout(volume, reclaimable, known, locale)` |
| `portReadout(ports)` | `portReadout(ports, locale)` |
| `netReadout(sample, history)` | `netReadout(sample, history, locale)` |
| `netDirectionReadout(sample, history, dir)` | `netDirectionReadout(sample, history, dir, locale)` |
| `buildAppSummary(input)` | `buildAppSummary(input, locale)` |
| `formatClock(tsSec)` | `formatClock(tsSec, locale?)` |
| `formatRelative(tsSec)` | `formatRelative(tsSec, locale?)` |
| `formatCpu(v)` | `formatCpu(v, locale?)` |

`locale?` 缺省读 `useLocaleStore.getState().locale` —— 这样 40+ 个调用点不必全部改，只有跨语言渲染的场景显式传。

调用点（4 处，必须同步改）：`StatsBoard.tsx:81-88`、`MenuBarPanel.tsx:128-134`、`AppDetail.tsx:109`、`AppDetail.tsx` 其余 `formatRelative` 调用。

### 2.7 格式化规则（逐函数）

| 函数 | 中文 | 英文 | 注意 |
|---|---|---|---|
| `formatClock` | `zh-CN`，`hour12: false` | `en-GB`，`hour12: false` | **不要用 `en-US`**：`en-US` + `hour12:false` 在部分 WebKit 版本会出 `24:05`；`en-GB` 稳定 24 小时制 |
| `formatRelative` | `Intl.RelativeTimeFormat("zh")` | `Intl.RelativeTimeFormat("en", { numeric: "auto" })` | 分档不变：<60s → now / <1h → 分钟 / <24h → 小时 / 否则时钟。`numeric:"auto"` 才给「昨天」而不是「1 天前」 |
| `formatCpu` | `100%（约 1.2 核）` | `100% (≈1.2 cores)` | `plural` 处理 cores |
| `formatBytes` | `7.74 GB` | 同 | 单位不译，空格不变 |
| `formatPercent` | `93%` | 同 | 不空格（英文习惯也接受） |
| `dayString` / `recentDays` | `YYYY-MM-DD` | 同 | **必须与 Rust `localtime::day_string` 一致**，不随 locale 变 |
| 数字千分位 | 不做 | 不做 | 明确非目标 |

---

## 3. 文案目录规格

### 3.1 规模与 namespace

| namespace | 覆盖来源 | 预算 key | 阶段 |
|---|---|---|---|
| `common` | 三态、空态、动作、单位、时间、脚注、错误前缀 | 55 | P0 |
| `nav` | `RailNav` / `MainWindow` tab / 返回 | 18 | P1 |
| `stats` | `StatsBoard` / 成绩单 | 55 | P2-1 |
| `resources` | `AppResourceGrid` / `AppDetail` / `ProcTable` / `appSummary` | 110 | P2-2 |
| `memory` | 内存概况 + 进程家族 + `LeakCard` | 75 | P2-3 |
| `disk` | `ReclaimBoard` / `ProjectHealthCard` / 分级 / 49 条缓存名 | 95 | P2-4 |
| `net` | `NetBoard` / 诊断 / 测速 / `types/net.ts` 标签 | 120 | P2-5 |
| `ports` | `PortList` / 暴露面 | 32 | P2-6 |
| `lan` | `LanBoard` / `LanDeviceCard` / `types/lan.ts` | 38 | P2-6 |
| `island` | `Island.tsx` | 28 | P3 |
| `panel` | `MenuBarPanel` | 26 | P3 |
| `settings` | `SettingsBoard` | 42 | P1 |
| `update` | `UpdateCard` | 16 | P1 |
| **合计** | 521 处字面量 | **~470** | |

差额说明：521 处字面量里有约 60 处是同一句话在多个组件重复（如「尚未采样」出现 6 次），以及 30+ 处是被合并进同一个 key 的模板片段（`${days} 天未改动` 与 `${days} 个月未改动` 合成一条带参数的 key 不成立，见 3.3 的 aging 规则）。

### 3.2 `common`（完整清单，P0 冻结）

```ts
export const common = {
  state: {
    notScanned: "尚未扫描",            notScannedHint: "进入本页会自动扫一轮",
    notSampled: "尚未采样",
    notMeasured: "尚未测量",
    unavailable: "不可读",             unavailableWith: "不可读（{why}）",
    unknown: "未知",
    samplingWarmup: "采样中…（首拍只建基线，约 2 s）",
  },
  action: {
    scanNow: "立即扫描",  rescan: "重新扫描",  refresh: "刷新",  refreshing: "刷新中",
    cancel: "取消", confirm: "确认", close: "关闭",
    copy: "复制", copied: "已复制", copyFailed: "复制失败",
    revealInFinder: "在 Finder 中打开",  revealAndSelect: "在 Finder 中显示",
    executing: "执行中…",
  },
  progress: {
    scanning: "扫描中…",  sampling: "采样中…",  measuring: "测量中…",
    diagnosing: "诊断中…",  speedTesting: "测速中…",
  },
  unit: {
    coresApprox: "约 {n} 核",  items: "{n} 项",  processes: "{n} 个进程",
    daysAgo: "{n} 天前",  monthsAgo: "{n} 个月前",  yearsAgo: "{n} 年前",
    unchanged: "{n} 天未改动",  unchangedMonths: "{n} 个月未改动",  unchangedYears: "{n} 年未改动",
  },
  time: { justNow: "刚刚", minutesAgo: "{n} 分钟前", hoursAgo: "{n} 小时前" },
  empty: {
    reclaim: "没有发现可回收项",
    swapped: "当前没有明显被换出的进程。",
    listenPorts: "最近一次扫描里没有该进程的监听端口。",
    listenPortsUnscanned: "还没扫过端口。这里为空不代表该进程没有监听——打开「端口」页扫一轮再看。",
  },
  hint: {
    firstScan: "首次扫描在启动 30 秒后自动开始，也可以手动触发。",
    statsCadence: "内存 / CPU 每 10 s · 网速每 2 s · 端口仅进入端口页时扫描",
    statsPrinciple: "统计只呈现事实读数与口径。清理入口在「存储」，结束进程在「内存」与「端口」的实体行上——总览不放破坏性按钮。",
  },
  err: { prefix: "不可读：", saveFailed: "保存失败：{err}" },
  nav: { backToOverview: "← 返回概况", mainAria: "主导航" },
} as const;
```

英文侧（`satisfies` 同结构）：

```ts
export const commonEn = {
  state: {
    notScanned: "Not scanned yet", notScannedHint: "Entering this page scans once automatically",
    notSampled: "Not sampled yet",
    notMeasured: "Not measured yet",
    unavailable: "Unavailable",  unavailableWith: "Unavailable ({why})",
    unknown: "Unknown",
    samplingWarmup: "Sampling… (first sample only builds the baseline, ~2 s)",
  },
  action: {
    scanNow: "Scan now", rescan: "Rescan", refresh: "Refresh", refreshing: "Refreshing",
    cancel: "Cancel", confirm: "Confirm", close: "Close",
    copy: "Copy", copied: "Copied", copyFailed: "Copy failed",
    revealInFinder: "Reveal in Finder", revealAndSelect: "Show in Finder",
    executing: "Working…",
  },
  progress: {
    scanning: "Scanning…", sampling: "Sampling…", measuring: "Measuring…",
    diagnosing: "Diagnosing…", speedTesting: "Testing speed…",
  },
  unit: {
    coresApprox: "≈{n} cores", items: "{n} items", processes: "{n} processes",
    daysAgo: "{n} days ago", monthsAgo: "{n} months ago", yearsAgo: "{n} years ago",
    unchanged: "unchanged for {n} days", unchangedMonths: "unchanged for {n} months", unchangedYears: "unchanged for {n} years",
  },
  time: { justNow: "just now", minutesAgo: "{n} min ago", hoursAgo: "{n} h ago" },
  empty: {
    reclaim: "Nothing reclaimable found",
    swapped: "No process is being swapped out right now.",
    listenPorts: "No listening ports for this process in the last scan.",
    listenPortsUnscanned: "Ports haven't been scanned yet. Empty here doesn't mean nothing is listening — open the Ports tab and scan once.",
  },
  hint: {
    firstScan: "The first scan starts 30 s after launch — or trigger it manually.",
    statsCadence: "Memory / CPU every 10 s · network every 2 s · ports only when the Ports tab is open",
    statsPrinciple: "Stats show facts and their measurement basis only. Cleanup lives in Storage; killing processes happens on the rows in Memory and Ports — no destructive buttons on the overview.",
  },
  err: { prefix: "Unavailable: ", saveFailed: "Save failed: {err}" },
  nav: { backToOverview: "← Back to overview", mainAria: "Main navigation" },
} as const;
```

> **aging 三档不合并**：中文「天/个月/年」与英文「days/months/years」都要按数值切换，合并成一条需要运行时拼单位，反而更难译。三档 × 两套（未改动 / 装过 / 动过）= 9 条，可接受。

### 3.3 `nav`（完整清单）

| key | zh | en |
|---|---|---|
| `nav.stats` | 总览 | Overview |
| `nav.resources` | 应用 | Apps |
| `nav.memory` | 内存 | Memory |
| `nav.disk` | 存储 | Storage |
| `nav.net` | 网络 | Network |
| `nav.ports` | 端口 | Ports |
| `nav.statsHint` | 可回收 + 内存 / CPU / 硬盘 / 端口 / 网速：本机资源总览 | Reclaimable + memory / CPU / disk / ports / speed: this machine at a glance |
| `nav.resourcesHint` | 每个应用一张卡：进程 / CPU / 内存 / 磁盘 / 端口 / 网速 | One card per app: processes / CPU / memory / disk / ports / speed |
| `nav.memoryHint` | 内存概况、swap 与换出受害者 · 进程家族与明细 | Memory overview, swap and victims · process families and detail |
| `nav.diskHint` | 卷容量、可回收空间与清理 | Volume capacity, reclaimable space and cleanup |
| `nav.netHint` | 本机下载 / 上传速率、走势、网卡分布与今日流量 | Download / upload rate, trend, interface split and today's traffic |
| `nav.portsHint` | 谁占着 3000：按端口反查持有进程 | Who owns 3000: map ports back to processes |
| `nav.settings` | 设置 | Settings |
| `nav.settingsHint` | 设置 · 灵动岛开关与阈值 | Settings · Dynamic Island switches and thresholds |
| `nav.tabAria` | 主标签 | Main tabs |
| `nav.cmdHint` | ⌘{n} 切换 | ⌘{n} to switch |

### 3.4 `island`（完整清单，折叠态有 4 字符硬限制）

| key | zh | en | 长度约束 |
|---|---|---|---|
| `island.tag.cpu` | CPU | CPU | ≤4 |
| `island.tag.swap` | 换出 | Swap | ≤4 |
| `island.tag.port` | 端口 | Port | ≤4 |
| `island.tag.lan` | 新面孔 | New | ≤4（中文 3，英文 3） |
| `island.tag.conflict` | 冲突 | Clash | ≤5 |
| `island.pressure.normal` | 正常 | Normal | |
| `island.pressure.warning` | 警告 | Warning | |
| `island.pressure.critical` | 严重 | Critical | |
| `island.mem.trigger.both` | 换出破阈值 · 内核压力：{level} | Swap over threshold · kernel pressure: {level} | 展开态，可换行 |
| `island.mem.trigger.pressure` | 内核内存压力：{level} | Kernel memory pressure: {level} | |
| `island.mem.trigger.swap` | 换出破阈值 | Swap over threshold | |
| `island.mem.swapLine` | 换出 {gb} | Swap {gb} | |
| `island.port.conflict` | 同端口多个持有者 | Held by multiple processes | |
| `island.port.occupiedBy` | 被 {comm} 占用 | Held by {comm} | |
| `island.lan.extra` | 还有 {n} 台新设备 | +{n} more new devices | 折叠态极窄 |
| `island.devKind.router` | 路由器 | Router | |
| `island.devKind.computer` | 电脑 | Computer | |
| `island.devKind.phone` | 手机 | Phone | |
| `island.devKind.tablet` | 平板 | Tablet | |
| `island.devKind.tv` | 电视 | TV | |
| `island.devKind.speaker` | 音箱 | Speaker | |
| `island.devKind.printer` | 打印机 | Printer | |
| `island.devKind.camera` | 摄像头 | Camera | |
| `island.devKind.iot` | IoT 设备 | IoT device | |
| `island.devKind.unknown` | 未知设备 | Unknown device | |
| `island.ratio` | 占比 {pct} | {pct} of window | |

> `devKind` 与 `types/lan.ts` 的 `DEV_KIND_LABEL` **合并为一份**（当前两处各有一份映射，纯重复）。

### 3.5 `panel`（完整清单）

| key | zh | en |
|---|---|---|
| `panel.footprintNote` | footprint 口径：含被换出到磁盘的部分 | footprint basis: includes what's swapped to disk |
| `panel.footprintUnavailable` | footprint 不可用，当前全部为 RSS 口径，不含换出量 | footprint unavailable — all readings fall back to RSS, excludes swap |
| `panel.refresh` | 立即刷新 | Refresh now |
| `panel.openMain` | 打开主窗口 | Open main window |
| `panel.reclaim.scanning` | 正在深探… / 看进度 | Deep scan running… / View progress |
| `panel.reclaim.notScanned` | 尚未深探 / 去扫描 | Not scanned yet / Scan |
| `panel.reclaim.none` | 暂无可回收 / 看明细 | Nothing to reclaim / Details |
| `panel.reclaim.cta` | 可回收 {size} / 去清理 | Reclaimable {size} / Clean up |
| `panel.reclaim.ctaHint` | {label} · 打开存储页 | {label} · Open storage |
| `panel.row.memory` | 内存 | Memory |
| `panel.row.disk` | 存储 | Storage |
| `panel.row.ports` | 端口 | Ports |
| `panel.appsHint` | 应用资源：按应用看进程 / CPU / 内存 / 磁盘 / 端口 / 网速 | App resources: processes / CPU / memory / disk / ports / speed per app |
| `panel.loading` | 正在采样… | Sampling… |
| `panel.noData` | 无数据 | No data |

> 面板宽 380，行标签最多 2 词；`panel.row.*` 不得超 8 字符。

### 3.6 `settings`（完整清单，与 `SettingsBoard.tsx` 一一对应）

| key | zh | en |
|---|---|---|
| `settings.title` | 设置 | Settings |
| `settings.language.title` | 语言 | Language |
| `settings.language.auto` | 跟随系统 | Follow system |
| `settings.language.zh` | 中文 | 中文 |
| `settings.language.en` | English | English |
| `settings.language.hint` | 当前：{current}（{source}） | Current: {current} ({source}) |
| `settings.language.sourceAuto` | 跟随系统语言 | from system |
| `settings.language.sourceManual` | 手动选择 | manually set |
| `settings.island.title` | 灵动岛 | Dynamic Island |
| `settings.island.noNotch` | 本机无刘海 | No notch on this display |
| `settings.island.noNotchHint` | 本机屏幕没有物理刘海（或只接了外接屏） | This display has no physical notch (or only an external display is attached) |
| `settings.island.ready` | 已就位 | Ready |
| `settings.island.desc` | CPU 持续偏高、内存紧张或端口被占用时从刘海里长出一条提报，6 秒后自行收回。悬停展开看是谁在烧。 | When CPU stays high, memory gets tight, or a port is taken, a report grows out of the notch and retracts after 6 s. Hover to see who's burning. |
| `settings.island.notchSize` | 刘海 {w} × {h} pt | Notch {w} × {h} pt |
| `settings.island.notchHint` | 接上带刘海的屏幕后会自动启用；外接屏不会造假刘海 | Turns on automatically when a notched display is attached; external displays never get a fake notch |
| `settings.island.enable` | 启用灵动岛 | Enable Dynamic Island |
| `settings.portGuard.title` | 端口冲突保镖 | Port conflict guard |
| `settings.portGuard.tag` | 关注端口新占用 / 多持有者 | Watched ports newly taken / multi-holder |
| `settings.portGuard.desc` | 谁占了 3000、5173 这类开发端口，从刘海里直接告诉你，不用切到端口页。同一端口 10 分钟内不重复提醒。 | Tells you straight from the notch who took 3000 or 5173 — no tab switching. Same port is not re-reported within 10 min. |
| `settings.portGuard.enable` | 启用端口冲突保镖 | Enable port conflict guard |
| `settings.lanWatch.title` | 局域网新面孔 | New device on LAN |
| `settings.lanWatch.tag` | 只看邻居表，零主动探测 | Neighbor table only, zero active probing |
| `settings.lanWatch.desc` | 有新设备连上 Wi-Fi 时从刘海里说一声。同一设备只提醒一次；换了网络会重新记住，不刷屏。 | A word from the notch when a new device joins Wi-Fi. Reported once per device; a new network re-baselines. |
| `settings.lanWatch.enable` | 启用局域网新面孔 | Enable new-device alerts |
| `settings.cpu.title` | CPU 暴走 | CPU runaway |
| `settings.cpu.tag` | 判定逻辑在 prune-core，可单测 | Logic lives in prune-core and is unit-tested |
| `settings.cpu.threshold` | 阈值 | Threshold |
| `settings.cpu.thresholdHint` | 采样值 ≥ 这个数才算「高」 | A sample counts as high at or above this |
| `settings.cpu.window` | 判定窗口 | Window |
| `settings.cpu.windowHint` | 看最近多久的采样，太短会被尖峰骗，太长反应慢 | How far back samples count — too short is spike-fooled, too long reacts late |
| `settings.cpu.tolerance` | 达标率 | Required share |
| `settings.cpu.toleranceHint` | 窗口里多少比例的采样要超阈值；90% 表示容忍短暂回落 | Share of samples that must exceed the threshold; 90% tolerates brief dips |
| `settings.cpu.maxPerHour` | 每小时上限 | Per-hour cap |
| `settings.cpu.maxPerHourHint` | 所有灵动岛事件的总量闸门 | Overall gate for all Dynamic Island events |
| `settings.cpu.rule` | 当前规则：最近 {window} 秒里 {tolerance}% 的采样 ≥ {threshold}% 才提报；触发后要掉到 {reset}% 以下才会再次触发。 | Current rule: {tolerance}% of samples in the last {window} s must be ≥ {threshold}% to report; it re-arms only after dropping below {reset}%. |
| `settings.mem.title` | 内存异常 | Memory anomaly |
| `settings.mem.tag` | 内核压力 + swap 占用，双通道 | Kernel pressure + swap usage, two channels |
| `settings.mem.swapThreshold` | Swap 阈值 | Swap threshold |
| `settings.mem.swapHint` | swap 占用 ≥ 此值且持续才算紧张；拉到 95% 视为关闭该通道 | Tight only when swap stays at or above this; 95% effectively disables the channel |
| `settings.mem.window` | 持续窗口 | Sustain window |
| `settings.mem.windowHint` | 内核压力警告 / swap 超阈值要连续保持多久才提报 | How long a kernel warning or swap breach must hold before reporting |
| `settings.mem.rule` | 当前规则：内核压力严重立即提报；警告或 swap ≥ {swap}% 持续 {window} 秒才提报；swap 掉到 {reset}% 以下才会再次触发。 | Current rule: critical pressure reports immediately; warning or swap ≥ {swap}% must hold {window} s; re-arms below {reset}%. |
| `settings.saved` | 已保存 | Saved |
| `settings.autoSave` | 改动即时生效并保存 | Changes apply and save immediately |
| `settings.loading` | 读取设置… | Loading settings… |
| `settings.saveFailed` | 保存失败：{err} | Save failed: {err} |

> `settings.language.zh` 在两种语言下都是「中文」；`settings.language.en` 都是「English」。语言名不翻译是通行做法。

### 3.7 大 namespace 的登记规则（不逐条列在本文）

`stats` / `resources` / `memory` / `disk` / `net` / `ports` / `lan` 共约 520 key，不在此重复罗列，登记时按以下规则，P2 每批 PR 附上该批的 key 清单 diff：

| 规则 | 示例 |
|---|---|
| 组件内的说明性长句保留**整句**为一个 key，不做片段拼接 | `disk.reclaim.deadweightHint` = "零后悔 · 你的 Mac 永远加载不了" |
| 带数字的句子只保留一个 key + 参数 | `disk.project.installedDaysAgo` = "{n} 天前装过" |
| 缓存名按后端稳定 key 生成，key 名与 `WhitelistEntry.key` 同名 | `disk.cache.yarn` = "Yarn 缓存" / "Yarn cache" |
| 平台名不译，不进 catalog | `darwin-arm64`、`linux-x64` 原样显示 |
| `k`/`ids` 之类机器串不译 | `SIGTERM`、`nettop`、`statvfs`、`footprint` |
| 图标类装饰（emoji、`✂️`、`🟢`）留在代码里，不进 catalog | — |

**缓存名映射表**（`disk.cache.<key>`，**49 条**，key 与 `whitelist.rs::ENTRIES[].key` 逐字相同，kebab-case，P2-4 一次性补齐）：

| key | zh | en |
|---|---|---|
| `yarn` | Yarn 缓存 | Yarn cache |
| `npm` | npm 缓存 | npm cache |
| `pnpm` | pnpm store | pnpm store |
| `pnpm-legacy` | pnpm store（旧路径） | pnpm store (legacy path) |
| `bun` | Bun 安装缓存 | Bun install cache |
| `playwright` | Playwright 浏览器 | Playwright browsers |
| `cypress` | Cypress 二进制 | Cypress binaries |
| `electron` | Electron 下载缓存 | Electron download cache |
| `cargo` | Cargo registry | Cargo registry |
| `homebrew` | Homebrew 下载缓存 | Homebrew download cache |
| `cocoapods` | CocoaPods 缓存 | CocoaPods cache |
| `composer` | Composer 缓存 | Composer cache |
| `gradle` | Gradle 缓存 | Gradle cache |
| `maven` | Maven 本地仓库 | Maven local repository |
| `gomod` | Go module 缓存 | Go module cache |
| `deriveddata` | DerivedData（编译产物） | DerivedData (build output) |
| `xcode-modulecache` | ModuleCache（模块缓存） | ModuleCache (module cache) |
| `xcode-previews` | SwiftUI Previews 缓存 | SwiftUI Previews cache |
| `xcode-cache` | Xcode 总缓存 | Xcode caches |
| `simulator-caches` | Simulator 运行时缓存 | Simulator runtime cache |
| `simulator` | Simulator 设备 | Simulator devices |
| `xcode-devicesupport` | iOS DeviceSupport | iOS DeviceSupport |
| `xcode-archives` | Archives（归档包） | Archives |
| `cursor-cacheddata` | Cursor V8 代码缓存 | Cursor V8 code cache |
| `cursor-cache` | Cursor HTTP 缓存 | Cursor HTTP cache |
| `cursor-gpu` | Cursor GPU 缓存 | Cursor GPU cache |
| `cursor-vsix` | Cursor 扩展安装包缓存 | Cursor extension packages |
| `cursor-logs` | Cursor 日志 | Cursor logs |
| `cursor-caches` | Cursor 系统缓存 | Cursor system caches |
| `claude-app-cache` | Claude HTTP 缓存 | Claude HTTP cache |
| `claude-desktop-caches` | Claude 系统缓存 | Claude system caches |
| `claude-shell-snapshots` | Claude Code shell 快照 | Claude Code shell snapshots |
| `ollama-models` | Ollama 模型权重（可重新 pull） | Ollama model weights (re-pullable) |
| `ollama-logs` | Ollama 日志 | Ollama logs |
| `xcode-ibsupport` | Interface Builder 设备支持 | Interface Builder device support |
| `simulator-runtimes` | Simulator 旧平台运行时（可重下） | Simulator legacy runtimes (re-downloadable) |
| `docker-vm` | Docker VM 磁盘 | Docker VM disk |
| `orbstack` | OrbStack 数据 | OrbStack data |
| `colima` | Colima 数据 | Colima data |
| `win-temp` | 临时文件（Temp） | Temp files |
| `win-npm` | npm 缓存 | npm cache |
| `win-pnpm` | pnpm store | pnpm store |
| `win-yarn` | Yarn 缓存 | Yarn cache |
| `win-pip` | pip 缓存 | pip cache |
| `win-nuget` | NuGet 下载缓存 | NuGet download cache |
| `win-electron` | Electron 下载缓存 | Electron download cache |
| `win-crashdumps` | 崩溃转储 | Crash dumps |
| `win-playwright` | Playwright 浏览器 | Playwright browsers |
| `win-cypress` | Cypress 二进制 | Cypress binaries |

> 两条纪律：**catalog 里缺某个 key 时回退显示后端下发的 `label`**（中文环境永远正确，英文环境会漏中文，因此 P2-4 的验收要求「49 条逐条核对为空」）；Windows 专属条目（`win-*`）在 macOS 上不出现，但 catalog 必须都有——否则跨平台发版即漏。

### 3.8 术语表（冻结，改词需改全仓库）

| 中文 | English |
|---|---|
| 可回收 | reclaimable |
| 移入废纸篓 | Move to Trash |
| 跨平台二进制 | cross-platform binaries |
| 包管理器缓存 | package manager caches |
| 换出 / 换出受害者 | swapped out / swap victims |
| 真实占用 | true footprint |
| 悬案卷宗 / 立案 / 结案 | leak case / open / clear |
| 灵动岛 | Dynamic Island |
| 新面孔 | new device |
| 关注层端口 | watched ports |
| 对外暴露 | exposed |
| 尚未扫描 | Not scanned yet（**不写 N/A**） |
| 口径 | measurement basis |
| 阈值 | threshold |
| 提报 | report（动作用 report，不用 push） |
| 深探 | deep scan |
| 剪除 | reclaim |

### 3.9 英文写作的可检查规则

| 规则 | 反例 → 正例 |
|---|---|
| 只用 ASCII 标点 | `「端口」` → `"Ports"`；`·` 保持（分隔符，非标点） |
| 不用中文量词 | `12 项` → `12 items` |
| 句首大写、其余小写 | `Reclaimable Space` → `Reclaimable space` |
| 数字与单位空格 | `90%` 不空格；`7.74 GB` 空格 |
| 不用 `!`、不用「请」的直译 | `请稍候` → `Just a moment` |
| 缩写首次出现给全称 | `RSS` → `RSS (resident set size)` 首次出现处 |

---

## 4. Rust 规格

### 4.1 模块布局

```
crates/prune-core/src/locale.rs            Locale 枚举（无 Tauri 依赖）
crates/prune-core/src/i18n/mod.rs          诊断结论与报告的 zh/en 文本表
crates/prune-core/src/i18n/diag_text.rs    VerdictKind 渲染
crates/prune-core/src/i18n/report.rs       report_markdown 模板
src-tauri/src/i18n.rs                      resolve / system_locale / TrayText / ErrText
```

`Locale` 放 core 的理由：`diag::verdict` 在 core 里生成文案，core 若从 tauri 取 Locale 会反向依赖。

### 4.2 `src-tauri/src/i18n.rs`

```rust
pub enum Source { Auto, Manual }

/// settings 覆盖 > 系统首选语言 > En
pub fn resolve(app: &AppHandle) -> (Locale, Source);

/// 系统首选语言（BCP-47 原始串）。失败返回 None —— 调用方兜底 En
fn system_locale() -> Option<String>;

/// 当前进程内缓存的 locale（避免每个命令都读盘 + 读系统）
pub struct AppLocale(pub AtomicU8);   // 1 = Zh, 2 = En
impl AppLocale {
    pub fn get(&self) -> Locale;
    pub fn set(&self, l: Locale);
}
```

依赖：`sys-locale = "0.3"`（仅 `src-tauri`，不进 core）。
**兜底实现**（若离线无法拉 crate）：`#[cfg(target_os="macos")]` 用已引入的 `objc2-foundation` 读 `NSLocale::preferredLanguages().firstObject()`；Windows 用 `GetUserDefaultUILanguage` 映射 `LANGID` 高位 0x04 = 中文。**接口不变**，只是 `system_locale()` 内部实现替换。

规格要点：
- `resolve` 每次调用都读 settings（不缓存 settings，只缓存解析结果到 `AppLocale`）。
- `locale_set` 写盘成功后**立即** `AppLocale::set`，再重建托盘，最后 emit。
- 读系统语言失败不报错、不写盘。

### 4.3 托盘重建

```rust
// src-tauri/src/tray.rs
pub struct TrayMenuState {
    themes: RwLock<Vec<(IconTheme, CheckMenuItem<tauri::Wry>)>>,  // 原 Vec 改为 RwLock
}

/// 重建菜单（切语言时调用）。必须在主线程执行。
pub fn rebuild_menu(app: &AppHandle) -> tauri::Result<()>;
```

实现规格（逐条都是踩过的坑）：

| # | 要求 |
|---|---|
| 1 | 通过 `app.run_on_main_thread(...)` 调用；不能在命令线程直接建菜单 |
| 2 | `TrayMenuState` **只在 `create()` 里 `manage` 一次**；重建只替换内部 `Vec`（重复 `manage` 同类型在 Tauri 2 会覆盖，旧句柄悬空） |
| 3 | 菜单项 id 必须与现状完全一致（`tray.rs:31-38`）：`open-main` / `check-update`（`MENU_CHECK_UPDATE`）/ `toggle-cpu-anim` / `toggle-menubar-numbers` / `tray-icon-theme` / `tray-icon:<theme-id>` / `quit` |
| 4 | 勾选态从当前原子量读：`CPU_ANIM_ENABLED` / `MENU_NUMBERS_ENABLED` / `TRAY_THEME`，不要从 settings 重读（可能已被本次会话改过但未落盘） |
| 5 | 用 `app.tray_by_id(TRAY_ID)?.set_menu(Some(menu))` 换菜单；`on_menu_event` 绑在 tray 上，无需重绑（已确认：`tray.rs:205`） |
| 6 | 失败只 log，不 panic、不回滚语言（菜单错语言比语言切换失败轻） |
| 7 | `tooltip` 保持 `"Prune"`（品牌名，不译） |

```rust
pub struct TrayText {
    pub open_main: &'static str,
    pub check_update: &'static str,
    pub cpu_anim: &'static str,
    pub menu_numbers: &'static str,
    pub icon_theme: &'static str,
    pub quit: &'static str,
}
pub const TRAY_ZH: TrayText = TrayText {
    open_main: "打开主窗口", check_update: "检查更新", cpu_anim: "CPU 动画",
    menu_numbers: "菜单栏网速", icon_theme: "顶部图标", quit: "退出 Prune",
};
pub const TRAY_EN: TrayText = TrayText {
    open_main: "Open Main Window", check_update: "Check for Updates", cpu_anim: "CPU Animation",
    menu_numbers: "Menu Bar Speed", icon_theme: "Tray Icon", quit: "Quit Prune",
};
pub fn tray_text(l: Locale) -> &'static TrayText;
```

主题名（`tray_icon.rs::IconTheme::label()`）改为 `label(locale)`：

| theme id | zh | en |
|---|---|---|
| `scissors` | 剪刀（默认） | Scissors (default) |
| `cat` | 小猫 | Cat |

> 用结构体而非 `t("tray.open_main")` 字符串查表：Rust 没有 TS 那种模板类型，字段名拼错只能运行时发现；结构体能保证漏字段编译失败。

### 4.4 `locale_get` / `locale_set` 实现要点

```rust
#[tauri::command]
pub fn locale_get(app: AppHandle) -> serde_json::Value;   // { current, source }，不返回 Result

#[tauri::command]
pub fn locale_set(app: AppHandle, locale: String) -> Result<serde_json::Value, String>;
```

| 步骤 | `locale_set` 顺序 | 失败处理 |
|---|---|---|
| 1 | `raw` 校验：非 `auto|zh|en` → 按 `auto` 处理（不报错） | — |
| 2 | `settings::set(&app, "locale", json!(raw))` | 失败 → `Err("保存语言设置失败: {e}")`，**不改内存态、不发事件** |
| 3 | 解析出 `(Locale, Source)` 并 `AppLocale::set` | — |
| 4 | `tray::rebuild_menu(&app)` | 失败 → log，继续 |
| 5 | `app.emit("locale://changed", payload)` | 失败 → log，继续 |
| 6 | 返回 payload | — |

幂等：若 `raw` 与当前 settings 值相同且解析结果相同，仍执行 4/5（保证「跟随系统」在系统语言变化后能刷新），但不写盘。

### 4.5 `diag` 改造（core）

```rust
// crates/prune-core/src/i18n/diag_text.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerdictKind {
    LanDown, DnsFail, Captive, LanSlow, WanGap, Jitter, DnsSlow, AllGood,
}

/// 渲染所需数字。缺项按 0 处理（不 panic）
#[derive(Debug, Clone, Copy, Default)]
pub struct VerdictParams {
    pub ms: f64,        // LanSlow / Jitter / DnsSlow 的实测值
    pub gap_ms: f64,    // WanGap 的差值
}

/// 纯函数：8 个分支 × 2 语言，全覆盖单测
pub fn render(kind: VerdictKind, params: VerdictParams, locale: Locale) -> Verdict;
```

`Verdict` 结构**保持不变**（`label` / `advice` / `rule`），新增两个字段：

```rust
pub struct Verdict {
    pub label: String,
    pub advice: String,
    pub rule: String,
    /// 命中的规则类型，供单测断言与日志排查（前端不消费）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<VerdictKind>,
}
```

> 加 `kind` 的目的：现有单测断言中文整句，改文案必挂。有了 `kind`，断言 `verdict(...).kind == Some(LanSlow)`，文案怎么改都不影响测试。

`verdict()` 签名变为：

```rust
pub fn verdict(
    lan: Option<&LayerProbe>, dns: Option<&LayerProbe>, wan: Option<&LayerProbe>,
    jitter_ms: Option<f64>, captive: CaptiveState, locale: Locale,
) -> Option<Verdict>;
```

**8 条结论的中英对照（冻结）**：

| kind | zh label / advice / rule | en label / advice / rule |
|---|---|---|
| `LanDown` | 连不上路由器 / 检查 Wi-Fi / 网线是否连接 / 网关连接探测全部失败 | Can't reach the router / Check the Wi-Fi or Ethernet connection / All gateway probes failed |
| `DnsFail` | DNS 解析失败 / 网关通但域名解析不了，多为 DNS 配置或运营商 DNS 故障 / DNS 探测全部失败 | DNS resolution failed / The gateway responds but names don't resolve — usually DNS config or the ISP resolver / All DNS probes failed |
| `Captive` | 需要网页登录（酒店/机场 Wi-Fi）/ 打开浏览器完成认证 / portal 探测未返回 Success | Sign-in required (hotel or airport Wi-Fi) / Open a browser and complete sign-in / Portal probe did not return Success |
| `LanSlow` | 路由器响应慢（{ms} ms）/ 重启路由器试试 / 连接耗时 > {LAN_SLOW_MS} ms | Router responds slowly ({ms} ms) / Try restarting the router / Connect time > {LAN_SLOW_MS} ms |
| `WanGap` | 出口/运营商延迟高（差值 {gap} ms）/ 你的宽带问题，Prune 无能为力，找 ISP / WAN − LAN > {WAN_LAN_GAP_MS} ms | High WAN/ISP latency (gap {gap} ms) / This is your broadband, not Prune — contact your ISP / WAN − LAN > {WAN_LAN_GAP_MS} ms |
| `Jitter` | 链路抖动大（±{ms} ms）/ 信号弱 / 拥挤信道 / 运营商波动，多为 Wi-Fi 环境问题 / 抖动（MAD）> {JITTER_HIGH_MS} ms | Link jitter is high (±{ms} ms) / Weak signal, congested channel, or ISP fluctuation — usually the Wi-Fi environment / Jitter (MAD) > {JITTER_HIGH_MS} ms |
| `DnsSlow` | DNS 偏慢（{ms} ms）/ 可在系统设置换 DNS 试试 / 解析耗时 > {DNS_SLOW_MS} ms | DNS is slow ({ms} ms) / Try a different DNS in system settings / Resolve time > {DNS_SLOW_MS} ms |
| `AllGood` | 各层正常 / 慢感可能来自对端服务本身 / 所有层都在阈值内 | All layers normal / The slowness may be the remote service itself / All layers within thresholds |

阈值数字用 `format!("{:.0}", LAN_SLOW_MS)` 之类动态拼，避免文案与常量漂移（现状即如此，保留）。

### 4.6 报告渲染

```rust
pub fn report_markdown(diag: &NetDiag, speed: Option<&SpeedResult>, locale: Locale) -> String;
```

模板逐行规格（左中右英）：

| 行 | zh | en |
|---|---|---|
| H1 | `# Prune 网络诊断报告` | `# Prune network diagnostic report` |
| 时间 | `- 时间：<t:{ts}>（Unix 秒）` | `- Time: <t:{ts}> (Unix seconds)` |
| Wi-Fi | `- Wi-Fi：{ssid}` | `- Wi-Fi: {ssid}` |
| 平台 | `- 平台：macOS` | `- Platform: macOS` |
| H2 | `## 分层探测` | `## Layer probes` |
| 表头 | `\| 层 \| 连接耗时 \| 说明 \| 原始样本 \|` | `\| Layer \| Connect time \| Notes \| Raw samples \|` |
| DNS 行名 | `DNS（解析 www.apple.com）` | `DNS (resolve www.apple.com)` |
| LAN 行名 | `网关 LAN（{gw}:80）` | `Gateway LAN ({gw}:80)` |
| WAN 行名 | `出口 WAN（首选 1.1.1.1:443，被拦回退 223.5.5.5:443）` | `WAN (1.1.1.1:443 first, falls back to 223.5.5.5:443 if blocked)` |
| 探测失败 | `探测失败` | `probe failed` |
| 未探测 | `未探测` | `not probed` |
| captive 行 | `- captive portal：{state}` | `- Captive portal: {state}` |
| captive 值 | 正常公网 / 疑似需要网页登录（酒店/机场 Wi-Fi）/ 探测失败，不下结论 | Public internet / Suspected sign-in page (hotel or airport Wi-Fi) / Probe failed — no conclusion |
| 结论 | `## 结论` + label/advice/rule | `## Verdict` + 同上结构 |
| 失败整轮 | `整轮失败：{note}` | `Round failed: {note}` |
| 测速 | `## 测速` … | `## Speed test` … |

**约束**：报告里不得出现「（Unix 秒）」以外的中文残留；P4 验收用正则 `[一-龥]` 断言英文报告为空。

### 4.7 错误串

`src-tauri` 与 `commands.rs` 的用户可见错误共约 20 条，用结构体常量：

```rust
pub struct ErrText {
    pub pid_critical: &'static str,      // "pid {pid} 是系统关键进程，不允许结束"
    pub self_kill: &'static str,         // "不能结束 Prune 自身"
    pub path_missing: &'static str,      // "路径不存在：{path}"
    pub reveal_failed: &'static str,
    pub disk_measure_failed: &'static str,
    pub not_a_dir: &'static str,
    pub scan_pending_note: &'static str, // 塞进 ReclaimReport.notes
    pub diag_state_busy: &'static str,
    pub diag_none: &'static str,         // "还没有诊断结果，请先运行诊断"
    pub clipboard_denied: &'static str,
    pub speed_macos_only: &'static str,
    pub settings_write_failed: &'static str,
}
pub fn err_text(l: Locale) -> &'static ErrText;
```

调用点模式：`return Err(err_text(cur(app)).self_kill.into())`，需要占位的用 `format!`。

**`prune-cli` 不改**：CLI 是机器接口，冻结为现状中文（`Locale::Zh` 硬编码）。这样 core 的 `err` 类文本不必为 CLI 再建一套。

### 4.8 其余 core 文案

| 位置 | 改法 |
|---|---|
| `reclaim/mod.rs` `notes` | 结构体化：`NoteCode { DeletedStale, SkippedMissing, Failed }` + `render(code, params, locale)` |
| `DupSummary.advice` | 同上，`AdviceCode::DuplicateManagers` + 参数（各管理器体积） |
| `disk/whitelist.rs` `label` | **不改**，前端按 `key` 查表，`label` 作为兜底 |

---

## 5. 逐文件迁移表

### P0–P1

| 文件 | 字面量 | namespace | 特殊处理 |
|---|---|---|---|
| `src/lib/uiText.ts` | 19 | `common` | 文件**删除**，`UI_TEXT.x` → `t("common.…")`；14 个引用点同步改 |
| `src/main.tsx` | 0 | — | 加 `boot()` 与 `lang` 同步 |
| `index.html` | 0 | — | `lang="zh-CN"` → `lang="en"` |
| `src/components/RailNav.tsx` | 15 | `nav` | `ITEMS` 数组的 `label`/`hint` 改为 key，渲染时 `t()` |
| `src/components/Tabs.tsx` | 2 | `nav` | |
| `src/components/primitives.tsx` | 3 | `common` | `PressureDot` 三档 |
| `src/components/SettingsBoard.tsx` | 53 | `settings` | 新增「语言」段（3 胶囊）；滑杆 `suffix` 里「 秒」→ `settings.unit.seconds` |
| `src/components/UpdateCard.tsx` | 24 | `update` | |
| `src/lib/format.ts` | 18 | `common` | 见 2.7；`formatRelative` 换 `Intl.RelativeTimeFormat` |

### P2

| 文件 | 字面量 | namespace |
|---|---|---|
| `src/components/StatsBoard.tsx` | 17 | `stats` |
| `src/lib/statsReadout.ts` | 23 | `stats`（+ 签名加 `locale`） |
| `src/components/stats/*.tsx`（5 个） | 49 | `stats` |
| `src/components/AppResourceGrid.tsx` | 25 | `resources` |
| `src/components/appdetail/AppDetail.tsx` | 28 | `resources` |
| `src/components/appdetail/{ProcTable,ResourceTile,HistoryTrack}.tsx` | 18 | `resources` |
| `src/lib/appSummary.ts` | 25 | `resources`（Markdown 摘要，签名加 `locale`） |
| `src/windows/MainWindow.tsx` | 48 | `memory` + `nav` + `common`（一个文件三个 namespace，按段落切） |
| `src/components/LeakCard.tsx` | 12 | `memory` |
| `src/components/ReclaimBoard.tsx` | 45 | `disk` |
| `src/components/ProjectHealthCard.tsx` | 23 | `disk` |
| `src/types/disk.ts` | 4 | `disk`（分级名） |
| `src/types/appdisk.ts` | 3 | `resources` |
| `src/components/NetBoard.tsx` | 44 | `net` |
| `src/components/stats/{DomainCard,NetReading}.tsx` | 13 | `net` |
| `src/types/net.ts` | 10 | `net`（暴露面 / 网卡类型 / RPM 分级） |
| `src/stores/netDiagStore.ts` | 4 | `net`（失败文案 + dev 报告） |
| `src/components/PortList.tsx` | 17 | `ports` |
| `src/components/LanBoard.tsx` + `LanDeviceCard.tsx` | 38 | `lan` |
| `src/types/lan.ts` | 13 | `lan`（与岛合并一份） |

### P3

| 文件 | 字面量 | namespace |
|---|---|---|
| `src/panels/MenuBarPanel.tsx` | 19 | `panel` |
| `src/panels/Island.tsx` | 25 | `island` |
| `src-tauri/src/tray.rs` | 6 项 | `TRAY_*` 结构体 |
| `src-tauri/src/tray_icon.rs` | 2 项 | `IconTheme::label(locale)` |
| `src-tauri/Info.plist` | 1 段 | `CFBundleLocalizations` |

### P4

| 文件 | 改动 |
|---|---|
| `crates/prune-core/src/locale.rs` | 新增 |
| `crates/prune-core/src/i18n/{mod,diag_text,report}.rs` | 新增 |
| `crates/prune-core/src/net/diag.rs` | `verdict`/`run_diag`/`report_markdown` 加 locale；`Verdict.kind` |
| `crates/prune-core/src/reclaim/mod.rs` | notes / advice 结构化 |
| `src-tauri/src/net_diag.rs` | `net_diag_get`；`net_diag_run` 传 locale；`copy_report` 读 AppHandle |
| `src-tauri/src/commands.rs` | 错误串走 `err_text` |
| `src-tauri/src/updater.rs` | 错误串同上 |

### 不迁移（明确保留中文）

`src/lib/devFixture.ts`（dev 假数据）、`src/components/icons.tsx`（无文案）、所有注释、`prune-cli` 全部输出。

---

## 6. 行为规格（场景表）

| # | 场景 | 期望 | 可测 |
|---|---|---|---|
| B1 | 首次启动，系统中文，无 `locale` 键 | 全中文，托盘中文，`settings.json` **不**被写入 | 手测 + 读文件 |
| B2 | 首次启动，系统英文 | 全英文 | 手测 |
| B3 | 设置里点 English | 主窗口/设置页立即英文；托盘菜单立即英文；面板与岛（若已打开）同步 | 手测 |
| B4 | 切到 English 后重启 | 仍英文（`locale:"en"` 已落盘） | 手测 |
| B5 | 选「跟随系统」+ 系统切英文后重启 | 全英文 | 手测 |
| B6 | 扫描进行中切语言 | 进度条文案切语言；**不中断扫描**；完成后条目名为新语言 | 手测 |
| B7 | 磁盘报告已存在时切语言 | 30 秒内**不触发**新扫描（`diskStore.report` 不变） | 手测 + 看日志 |
| B8 | 诊断页已出结论时切语言 | 结论、建议、阈值行、复制出的报告全部切语言 | 手测 |
| B9 | 岛显示中切语言 | 岛不闪烁、不重播动画；文本切换 | 手测 |
| B10 | 托盘菜单打开状态下切语言 | 菜单可能保持旧语言直到下次打开（macOS 行为），可接受 | 手测记录 |
| B11 | `locale_set` 写盘失败（模拟 chmod 只读） | toast「语言切换失败：…」；界面**回滚**到旧语言 | 手工造错 |
| B12 | 浏览器 dev（无 Tauri） | 按 `navigator.language` 渲染；设置页语言段仍可用（写 localStorage 兜底） | 手测 |
| B13 | 三个窗口同时存在 | 任一窗口切换后三处一致 | 手测 |
| B14 | 英文下 380 面板 | 无横向滚动、无省略号截断（标题类允许 1 行 truncate） | 截图走查 |
| B15 | 英文下折叠岛 | 标签不换行、不裁字 | 截图走查 |
| B16 | 英文下菜单栏标题 | 仍只含数字与单位，长度 ≤ 现状 | 手测 |

> B7 是「切语言零重扫」的硬指标：验收时在切语言前后各取一次 `diskStore.report.scannedAt`，必须相同。

---

## 7. 测试规格

### 7.1 Rust 单测

| 模块 | 用例 | 断言方式 |
|---|---|---|
| `Locale::from_tag` | `zh` / `zh-Hans` / `zh-Hant` / `ZH` / `en-US` / `ja` / `""` | `assert_eq!(…, Locale::Zh/En)` |
| `i18n::resolve` | 手动 zh / 手动 en / auto + 系统 zh / auto + 系统 en / auto + 语言读不到 | `(Locale, Source)` |
| `VerdictKind` 8 分支 | 每种输入 → `verdict(...).kind` | 断言 `kind` 与 `params`，**不断言整句** |
| `render` 覆盖 | 8 kind × 2 locale = 16 条 | 断言 `label` 非空、含期望数字（如 `"210"`）、`rule` 含阈值常量 |
| `report_markdown` | zh / en 各一条 | 英文版 `assert!(!report.contains('（'))`，且 `regex [一-龥]` 为空（用 `char::is_ascii` 不行，手写 `c >= '\u{4e00}' && c <= '\u{9fff}'`） |
| `TrayText` | 字段非空、无中英混杂（en 版断言 `is_ascii()` 为真） | — |

现有会被影响的测试（需同步改）：

| 测试 | 影响 |
|---|---|
| `net/diag.rs` 的 `verdict` 系列断言中文 | 改为断言 `kind` |
| `report_markdown` 相关快照（若有） | 传 `Locale::Zh` 保持原断言，另加 en 用例 |
| `tray.rs` 内 `assert!(…)` 用中文 panic 消息 | panic 消息不是 UI，**不改** |

### 7.2 前端验证

无前端测试框架（`package.json` 无 `test`），因此用三道闸：

| 闸 | 命令 | 通过标准 |
|---|---|---|
| 类型 | `pnpm build`（内含 `tsc`） | 0 错误；**en-US 漏 key 必须在这里失败**（P0 就要验证一次「故意删 key → 确认报错」） |
| 文案门禁 | `node scripts/check-i18n.mjs` | 退出码 0 |
| 手测 | 场景表 B1–B16 | 逐项打勾 |

### 7.3 `scripts/check-i18n.mjs` 规格

```
输入：src/**/*.{ts,tsx}
流程：
 1. 逐文件读入，剥离 // 行注释与 /* */ 块注释与 JSX 注释 {/* */}
 2. 匹配含 [\u4e00-\u9fff] 的字符串字面量（"…" / '…' / `…`）
 3. 排除白名单：
      - src/lib/devFixture.ts
      - src/i18n/messages/**        （catalog 本体）
      - 文件首行含 // i18n-ignore 的文件
 4. 允许的残留：注释、aria-label 已迁移后不应有残留 → 一律视为违规
 5. 输出：按文件分组的 "文件:行 文案" 清单
 6. 退出码：0 = 无违规；1 = 有违规
附加检查：
 7. src/lib/** 中出现 tGlobal( → 违规（见 2.5）
 8. 出现 "zh-CN" 硬编码（format.ts 白名单除外）→ 违规
```

`package.json`：

```json
"check:i18n": "node scripts/check-i18n.mjs",
"build": "tsc && node scripts/check-i18n.mjs && vite build"
```

> 把门禁放进 `build`：`tauri build` 会跑 `beforeBuildCommand`，因此**发版路径天然被拦**。

### 7.4 双语渲染验收脚本（可选，P5）

```
跑 `pnpm dev` → Playwright 打开 ?window=main 与 ?window=panel
  · 设 locale=en，逐 tab 截图，页面文本经 evaluate 取出 body.innerText
  · 断言 innerText 不含 [\u4e00-\u9fff]（白名单：中文语言选项文字「中文」、缓存条目若后端回退中文需先修）
```

---

## 8. 实施顺序（commit 切分）

| # | commit | 内容 | 门禁 |
|---|---|---|---|
| 1 | `feat(i18n): 语言解析与设置持久化` | `locale.rs`、`i18n.rs`、`locale_get/set`、settings `locale` 键 | `cargo test` 绿；命令手测可用 |
| 2 | `feat(i18n): 前端 i18n 内核与语言切换 UI` | `src/i18n/*`、`main.tsx` boot、SettingsBoard 语言段、`check-i18n.mjs`（此时白名单全仓库） | `tsc` 0；切语言设置页变英文 |
| 3 | `refactor(i18n): 导航与外壳文案迁移` | P1 全部文件 | 门禁脚本对 P1 范围零违规 |
| 4 | `refactor(i18n): 总览与应用面板文案迁移` | P2-1 / P2-2 | 逐批 |
| 5 | `refactor(i18n): 内存与存储面板文案迁移` | P2-3 / P2-4（含 49 条缓存名） | 缓存名英文核对表 |
| 6 | `refactor(i18n): 网络与端口面板文案迁移` | P2-5 / P2-6 | |
| 7 | `feat(i18n): 托盘菜单与系统层本地化` | P3（含 `rebuild_menu`、Info.plist） | 切语言后托盘立即更新 |
| 8 | `feat(i18n): 诊断结论与报告本地化` | P4 core + 命令 | 英文报告正则断言 |
| 9 | `chore(i18n): 门禁收紧与文档` | 移除白名单、README 双版、验收清单 | 全仓库零违规 |

每条 commit 前跑：`cargo fmt --check` → `cargo test` → `pnpm build` → `node scripts/check-i18n.mjs`。

---

## 9. 验收清单

**契约层**
- [ ] `settings.json` 中 `locale` 三值与缺省行为符合 1.2
- [ ] `locale_get` 在系统语言读不到时返回 `{current:"en", source:"auto"}`，不抛错
- [ ] `locale_set` 非法值按 `auto` 处理，不报错
- [ ] `locale://changed` 在托盘重建之后发出（时序可通过在事件回调里读托盘菜单文案验证）
- [ ] `net_diag_copy_report` 无参数，报告语言 = 当前界面语言

**覆盖层**（见 §10 的逐屏清单）

**质量层**
- [ ] `pnpm build` 0 错误（含 `check:i18n`）
- [ ] `cargo test` 全绿（含 16 条 render 用例）
- [ ] `cargo fmt --check` 干净
- [ ] 英文界面全局 `[\u4e00-\u9fff]` 为空（除「中文」语言名）
- [ ] `src/lib/**` 无 `tGlobal`
- [ ] 无 `toLocaleTimeString("zh-CN")` 硬编码残留

---

## 10. 逐屏走查清单（每项中英各一遍）

| 屏 | 检查点 |
|---|---|
| 总览 | 6 张卡标题/说明/单位；成绩单卡（电影/照片换算）；进度条图例 |
| 应用 | 排序 tooltip、卡片指标、展开/收起、空态、搜索框 placeholder |
| 应用详情 | 标题、口径说明、磁盘三层、复制摘要 toast、空态（应用已退出） |
| 内存 | 物理内存四行、swap 轨道刻度（75 / 90）、压力三档、进程表表头、右键菜单、结束进程确认弹窗 |
| 存储 | 分级四行 + 说明、扫描进度、可回收构成、项目体检卡（四结论 + 相对时间）、明细表、确认弹窗、清理结果 toast |
| 网络 | 下载/上传卡、7 天柱状 tooltip、网卡分布、按应用 Top 8、诊断三层、结论卡、复制报告、测速卡、RPM 说明 |
| 端口 | 三档筛选、点阵图例、局域网设备卡（类型词）、空态、右键菜单 |
| 设置 | 语言段、四段灵动岛设置、滑杆 hint、「当前规则」动态句、保存状态 |
| 菜单栏面板 | 6 行标签、CTA 三态（未扫/0/有值）、footprint 提示、更新卡 |
| 灵动岛 | 4 类事件折叠+展开、设备类型词、占比、冲突文案 |
| 托盘 | 6 项 + 主题子菜单 2 项 + 勾选态 |
| 报告 | 诊断 Markdown、应用摘要 Markdown（粘贴到编辑器看） |

---

## 11. 未决项（实现前需确认）

| # | 问题 | 建议 | 影响 |
|---|---|---|---|
| S1 | `sys-locale` 能否拉取 | 先试 `cargo add sys-locale`；失败走 objc2 兜底 | P0 第 1 天 |
| S2 | L2 类型档位的 `tsc` 增量耗时 | 实测 > 300 ms 则降 L1 | P0 |
| S3 | 繁体用户显示简体是否可接受 | 可接受（已知取舍，写进 README） | 无 |
| S4 | `en-GB` vs `en-US` 时间格式 | 用 `en-GB`（稳定 24h） | P1 |
| S5 | 是否给面板加「语言」快捷入口 | 不加，只在设置里 | — |
| S6 | 缓存名 70 条英文由谁定稿 | 我出初稿，你在 P2-4 PR 里逐条核 | P2-4 |
