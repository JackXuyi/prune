# Prune 中英文双语方案（i18n Plan）

> 目标：**默认跟随系统语言**，设置里可手动覆盖（跟随系统 / 中文 / English），切换即时生效、不重启、不重扫。
> 版本基线：v0.6.0。本文是决策与阶段划分；**可执行规格（契约/签名/key 清单/测试）见 [`docs/i18n-spec.md`](./i18n-spec.md)**。

---

## 0. 结论摘要

| 项 | 决策 |
|---|---|
| 运行时 | **自研极简 i18n**（约 120 行 TS，零依赖），不引 i18next |
| 系统语言来源 | **Rust 侧 `sys-locale` 读系统首选语言**，不用 `navigator.language`（WKWebView 不可信） |
| 持久化 | `settings.json` 新增 `locale: "auto" \| "zh" \| "en"`，与现有 `settings.rs` 同源 |
| 跨进程同步 | 前端 `invoke` → Rust 写盘 + 重建托盘菜单 + `emit` 广播 → 三个窗口各自更新 store |
| 文案归属 | **一句话只有一个 owner**：静态标签归前端 catalog，动态长文本归生成它的 Rust，系统层归 Rust/bundle |
| 规模 | 约 450 个 key，30 个前端文件、13 个 Rust 文件涉及 |
| 工期 | 约 9.5 人日，分 P0–P5 六个阶段，每阶段独立可验收 |

三个必须提前知道的约束（详见 §4）：

1. **WKWebView 的 `navigator.language` 靠不住**——App bundle 未声明中文本地化时，系统是中文也会返回 `en-US`。语言判定必须下沉到 Rust。
2. **托盘菜单文案在 Rust 进程里**，切语言要重建 `MenuItem` 句柄，不能靠前端重渲染。
3. **菜单栏标题字符预算是 6**（`tray.rs: TITLE_BUDGET_CHARS`），英文变长会直接截断/溢出。

---

## 1. 现状盘点（量化）

用脚本统计（去注释后的字符串字面量）：

| 层 | 含中文字面量 | Top 文件 |
|---|---|---|
| 前端 `src/` | **521** | `MainWindow.tsx` 48、`ReclaimBoard.tsx` 45、`NetBoard.tsx` 44、`AppDetail.tsx` 28、`Island.tsx` 25、`appSummary.ts` 25、`AppResourceGrid.tsx` 25 |
| Tauri 壳 `src-tauri/` | **约 90** | `tray.rs` 33、`commands.rs` 26、`tray_icon.rs` 22、`net_diag.rs` 20、`updater.rs` 敏感 |
| 核心 `crates/prune-core` | **约 400** | `net/diag.rs` 74、`disk/whitelist.rs` 66、`reclaim/mod.rs` 44、`disk/mod.rs` 28 |

### 1.1 文案分布的真实结构（决定架构）

不是「所有文案都在前端」，而是**三处各有一摊，且互相渗透**：

```
┌─ 前端 TSX ──────────────────────────────────────────────┐
│  纯 UI 框架文案：导航、按钮、表头、空态、说明段落            │
│  但大量「业务语义」来自后端：                            │
│    · reclaim 条目 label ← whitelist.rs（key 稳定，可前端查表）│
│    · 诊断结论 label/advice/rule ← net/diag.rs（带参数，动态） │
│    · 平台名 darwin-arm64 / linux-x64（不译）              │
└─────────────────────────────────────────────────────────┘
┌─ Rust 壳（src-tauri）───────────────────────────────────┐
│  托盘菜单 6 项 + 主题名 2 项  ← 必须在 Rust 侧本地化        │
│  IPC 错误串 ~20 条          ← 直接 toast 给用户看          │
│  「尚未完成扫描，请稍候」notes ← 塞进报告结构体              │
└─────────────────────────────────────────────────────────┘
┌─ 核心（prune-core）─────────────────────────────────────┐
│  白名单 label 49 条          ← 有稳定 key，前端可查表兜底    │
│  诊断结论 + Markdown 报告     ← 一段话 + 阈值数字，需整句渲染 │
│  reclaim notes / dup advice  ← 短句 + 数字                │
└─────────────────────────────────────────────────────────┘
```

### 1.2 已经做对的部分（方案要复用的地基）

| 现状 | 为什么有利 |
|---|---|
| `src/lib/uiText.ts` 三态文案常量库（19 条） | 已有「文案收敛到单一来源」的先例，直接改造成 catalog 的一个 namespace |
| `WhitelistEntry.key` 稳定标识（`yarn`/`npm`/…） | 前端可 `t('reclaim.cache.' + key)`，**切语言不必重扫磁盘** |
| 灵动岛 payload 全是枚举 + 数字（`kind`/`pressure`/`trigger`） | 岛内文案 100% 前端渲染，零 Rust 改动 |
| `statsReadout.ts` 把口径与文案集中一处，面板与主窗口共用 | 只改一处，两块屏同时双语 |
| `settings.rs` 已有 `get_str/set_many` 通用键值读写 | 加一个 `locale` 键不需要新机制 |
| `src-tauri/Info.plist` 已存在并被 Tauri 合并 | 可加 `CFBundleLocalizations` |

### 1.3 硬编码语言的地方（必须一起改）

| 位置 | 问题 |
|---|---|
| `src/lib/format.ts:55` | `toLocaleTimeString("zh-CN")` 写死 |
| `src/lib/format.ts` `formatRelative` | 「刚刚 / N 分钟前 / N 小时前」写死，应换 `Intl.RelativeTimeFormat` |
| `index.html` | `<html lang="zh-CN">` 写死 |
| `README.md` / 应用描述 | 中英双版（发布物料，P5 处理） |

---

## 2. 目标与非目标

### 目标

| # | 目标 | 验收方式 |
|---|---|---|
| G1 | 首次启动按系统语言自动选中文或英文 | 系统切英文重启 → 全英文；系统中文 → 全中文 |
| G2 | 设置里可手动覆盖，切完**不重启**即时生效（含托盘菜单） | 切一次看三个窗口 + 托盘 |
| G3 | 覆盖全部可见文本：主窗口 6 tab、设置弹层、菜单栏面板、灵动岛、托盘菜单、复制出的 Markdown 报告 | 逐屏双语言走查清单 |
| G4 | 英文下**不溢出、不截断、不出现中文标点** | 380pt 面板 / 229pt 岛 / 6 字符标题三处专项 |
| G5 | 新增文案漏翻**在 CI 拦得住** | `scripts/check-i18n.mjs` 退出码非 0 |

### 非目标（本期不做）

- 第三种语言、RTL 布局
- CLI（`prune-cli`）输出本地化——面向开发者的机器接口，保持英文/中文现状
- 数字千分位、货币、时区等完整 CLDR 本地化
- 文案 A/B、翻译平台接入、`.po`/`.xliff` 工作流
- 应用名/包名本地化（`Prune` 是品牌名，不译）

---

## 3. 方案选型

### 3.1 运行时：自研 vs i18next

| 维度 | A. 自研（推荐） | B. i18next + react-i18next |
|---|---|---|
| 依赖体积 | 0 | ~45 KB (gzip) + 两个包 |
| 编译期 key 检查 | ✅ 可做到**穷尽性检查**（少 key/多 key 都编译失败） | ⚠️ 需额外 `i18next-typescript` 之类，key 仍是 string |
| 参数插值 | 自写 30 行 | 成熟 |
| 复数 | `Intl.PluralRules` 10 行 | 成熟（i18next v21+ 内置） |
| 命名空间/懒加载 | 桌面应用本地 bundle，**不需要懒加载** | 主要优势用不上 |
| 与 Rust 交叉进程同步 | 自己写更直接 | 无帮助 |
| 与项目风格匹配 | 高（重纪律、少依赖、编译期保障） | 中 |
| 风险 | 复数/嵌套等边缘要自己写 | 引入概念负担（backend/detector/plugin） |

**决策：A 自研。** 理由：全量 450 key 一次打包进桌面应用，i18next 的懒加载与插件生态用不上；而本项目最看重的「漏翻必须编译失败」这件事，自研用 `satisfies` + 派生类型最省事。

### 3.2 Rust 侧文案：三选一

| 方案 | 做法 | 优点 | 缺点 |
|---|---|---|---|
| R1 code 化 | 后端只返 `code + params`，前端渲染 | 单一来源在前端 | 49 条 whitelist + 8 条结论都要前端建表；Markdown 报告仍需 Rust 侧文本 → **双份** |
| R2 后端渲染 | Rust 内置 zh/en 字典，按 locale 出成品文本 | 单一来源；报告天然对 | 切语言需重新拉数据 |
| R3 混合（推荐） | **按「谁来生成」切分**：静态标签前端查表（key 已存在）；动态长文本 Rust 渲染 | 磁盘切语言不重扫；报告不重复维护 | 需要一条清晰的归属规则 |

**R3 的归属规则（写进 README，后续新增文案照此判断）：**

> 1. **静态、名词性、有稳定 key** → 前端 catalog（`reclaim.cache.<key>`）。Rust 原来的 `label` 降级为**未知 key 的兜底**。
> 2. **动态、整句、含阈值/数字/需要拼 Markdown** → 生成方渲染（`net/diag.rs` 收 `Locale` 参数）。
> 3. **系统层可见**（托盘菜单、宿主权限弹窗）→ Rust / bundle 本地化。
> 4. **纯 UI 框架**（按钮/表头/空态/帮助段落）→ 前端 catalog。

---

## 4. 关键约束与技术坑

| # | 约束 | 后果 | 对策 |
|---|---|---|---|
| C1 | WKWebView `navigator.language` 受 `CFBundleLocalizations` 影响，未声明中文时恒为 `en-US` | 中文系统被判成英文 | 语言判定只在 Rust 做；同时给 Info.plist 补 `CFBundleLocalizations = [en, zh-Hans]` 双保险 |
| C2 | 托盘菜单 `MenuItem` 句柄存在 `TrayMenuState`，文案在建菜单时写死 | 切语言不重建 = 托盘还是旧语言 | `tray::rebuild_menu(app)`，主线程执行，重建后重新 `app.manage` 状态 |
| C3 | 菜单栏标题预算 6 字符（`TITLE_BUDGET_CHARS`），`compose_numbers` 已有降档链 | 英文单位/文案变长 → 标题被系统截成省略号 | 标题只放数字与单位（现状已如此），**翻译不得进入标题**；`format_speed` 单位短写（`M`/`K`）保持 |
| C4 | 菜单栏面板 380×432、灵动岛折叠 229×50 是硬尺寸 | 英文文案溢出换行 → 布局跳动 | ①面板行标签限 1 行 + `truncate`；②岛标签用固定短词（`CPU`/`Swap`/`Port`/`New`）；③P5 专项走查 |
| C5 | 磁盘扫描一次约 30 s，结果落 store | 切语言若触发重扫 → 用户等 30 s | whitelist 标签走前端 key 查表，**切语言零 IPC** |
| C6 | 诊断快照只存内存（`last result`），不落库 | 切语言要重新渲染结论 | `net_diag_last(locale)` 命令，前端切语言后重新取一次（几十毫秒） |
| C7 | 复制出的 Markdown 报告（诊断报告、应用摘要）给 AI/同事看 | 中英混排或英文环境拿到中文报告 | 报告由生成方按当前 locale 渲染；`appSummary.ts` 在前端 → 前端 catalog |
| C8 | 零遥测、零埋点 | 不能上报语言分布做「优化」 | 不做；语言从本地读，不写日志 |
| C9 | 现有测试有中文断言（`tray.rs`、`net/diag.rs` 单测） | 改文案会挂测试 | 断言改断言「结构/枚举/数字」而非整句；P4 一并处理 |

---

## 5. 架构设计

### 5.1 语言数据流

```
                    ┌──────────────── 系统首选语言（NSLocale / GetUserDefaultUILanguage）
                    │
settings.json ──►  Rust i18n::resolve() ──►  AppState.locale: Locale
  locale: "auto"        │                        │
  | "zh" | "en"         │                        ├──► tray::rebuild_menu(locale)   托盘菜单
                        │                        └──► prune_core::diag::render(locale) 诊断结论 / 报告
                        │
                        ▼  invoke("locale_get") → { current: "en", source: "auto" }
                 ┌──────────────────────────────────────────────┐
                 │ 前端 i18nStore (zustand)                      │
                 │  locale = "en"                                │
                 │  t = createT(enMessages)                      │
                 └───────┬──────────────┬──────────────┬────────┘
                         │              │              │
                    MainWindow     MenuBarPanel     Island
                     （三个窗口各自一份 store，通过 locale://changed 事件同步）
```

### 5.2 切语言时序（不重启）

```
用户点「English」
  → 前端 i18nStore.set("en")           // 乐观更新，立即重渲染，无白屏
  → invoke("locale_set", {locale:"en"})
      → settings::set("locale","en")
      → AppState.locale = En
      → tray::rebuild_menu()           // 主线程重建菜单
      → emit("locale://changed", {current:"en"})   // 广播到三个窗口 + 岛
      → net_diag 若有 last result：按新 locale 重渲染一份缓存（供下次读取）
  → 其它窗口收到事件 → 各自 store.set → 重渲染
  → document.documentElement.lang = "en"
  → 若当前在「网络诊断」页：重新 invoke("net_diag_last") 刷新结论文本
```

失败处理：`locale_set` reject → 回滚 store 到旧值 + toast「语言切换失败：{err}」（走 catalog，双语）。

### 5.3 前端模块结构

```
src/i18n/
├── index.ts            // createI18n(locale) → { t, locale, plural }
├── store.ts            // zustand: locale / setLocale / syncFromRust / listen events
├── types.ts            // Locale、MessageTree、ParamsOf<K> 模板参数类型
├── detect.ts           // invoke locale_get、兜底 navigator.language（仅浏览器 dev）
└── messages/
    ├── zh-CN.ts        // ← 唯一 source of truth（结构定义在此）
    ├── en-US.ts        // satisfies MessageTree，漏 key / 多 key 编译失败
    └── namespaces/
        ├── common.ts   // 三态、单位、时间、通用动词（原 uiText.ts 并入）
        ├── nav.ts      // rail / tab / 快捷键提示
        ├── stats.ts
        ├── resources.ts
        ├── memory.ts
        ├── disk.ts
        ├── net.ts      // 含诊断
        ├── ports.ts
        ├── lan.ts
        ├── island.ts
        ├── panel.ts    // 菜单栏面板
        ├── settings.ts
        └── update.ts
```

> **为什么按 namespace 分文件、文件内 zh/en 并置**：450 key × 2 语言放两个大文件，翻译时来回跳，早晚漂移。并置后改一句能看到另一语言的对应句，漏改肉眼可见，类型检查兜底。

### 5.4 运行时核心（约 120 行）

```ts
// src/i18n/index.ts
export type Locale = "zh" | "en";
type Params = Record<string, string | number>;

/** 扁平化：{ disk: { reclaim: { title: "…" } } } → "disk.reclaim.title" */
function flatten(tree: object, prefix = "", out: Record<string, string> = {}) {
  for (const [k, v] of Object.entries(tree)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out[key] = v;
    else if (v && typeof v === "object") flatten(v, key, out);
  }
  return out;
}

export function createI18n(locale: Locale) {
  const dict = flatten(locale === "en" ? enMessages : zhMessages);

  /** 占位替换：缺参数原样保留 `{name}`，便于一眼看出漏传 */
  const fill = (tpl: string, params?: Params) =>
    params
      ? tpl.replace(/\{(\w+)\}/g, (m, name) =>
          name in params ? String(params[name]) : m,
        )
      : tpl;

  const t = (key: MessageKey, params?: Params): string => {
    const tpl = dict[key] ?? fallbackDict[key];
    if (tpl == null) {
      // 开发期必须炸出来，生产期降级为 key（绝不显示空白）
      if (import.meta.env.DEV) console.error(`[i18n] missing key: ${key}`);
      return key;
    }
    return fill(tpl, params);
  };

  const rules = new Intl.PluralRules(locale === "en" ? "en" : "zh");

  /** 复数：英文按 Intl.PluralRules 选 .one/.other；中文恒 .other（无复数形态） */
  const plural = (key: string, n: number, params?: Params): string => {
    const cat = locale === "en" ? rules.select(n) : "other";
    return fill(dict[`${key}.${cat}`] ?? dict[`${key}.other`] ?? key, { n, ...params });
  };

  return { locale, t, plural };
}
```

**类型安全的两种档位（选其一，P0 定）：**

| 档位 | 做法 | 收益 | 成本 |
|---|---|---|---|
| L1 简单 | `MessageKey = keyof typeof flatZh`（编译期算一次） | 拼错 key 编译失败 | 不校验参数 |
| L2 严格（推荐） | 追加 `ParamsOf<K>`：从模板字符串里抽 `{name}` 生成联合类型，`t(key, params)` 参数缺一个就编译失败 | 参数也编译期校验 | 约 30 行类型体操，`tsc` 慢几十毫秒 |

L2 实现要点：

```ts
type ExtractParams<S extends string> =
  S extends `${string}{${infer P}}${infer Rest}` ? P | ExtractParams<Rest> : never;
type ParamsOf<K extends MessageKey> =
  ExtractParams<(typeof zhFlat)[K]> extends never
    ? [params?: undefined]
    : [params: Record<ExtractParams<(typeof zhFlat)[K]>, string | number>];
```

### 5.5 组件接入方式

```tsx
// 组件里（推荐：一个 hook，依赖 locale 自动重渲染）
const { t } = useI18n();
<h2>{t("disk.reclaim.title")}</h2>
<p>{t("disk.reclaim.deadweightHint", { ratio: "8.3 : 1" })}</p>

// 纯函数 / store / 报告生成器里（非组件上下文）
import { tGlobal } from "../i18n/store";
buildAppSummary({ t: tGlobal, ... })
```

> `lib/statsReadout.ts`、`lib/appSummary.ts` 是纯函数且被多处调用 —— 统一改为**接受 `t` 参数**（依赖注入），不在模块顶层读全局，避免「测试里语言不可控」和「切语言时已生成的字符串不更新」两个坑。

---

## 6. Key 命名规范

| 规则 | 示例 |
|---|---|
| `域.区域.用途`，全小写点分，2–4 段 | `disk.reclaim.deadweight.title` |
| 域与 rail tab 对齐：`stats` / `resources` / `memory` / `disk` / `net` / `ports` / `lan` / `island` / `panel` / `settings` / `update` / `common` | `ports.empty.notScanned` |
| 动作用动词原形，状态用形容词 | `common.action.scanNow`、`common.state.notScanned` |
| 参数用 `{snake}` 或 `{camel}` 单一风格（选 camel），不写位置参数 | `t("disk.reclaim.found", { count: 12, size: "7.74 GB" })` |
| 复数 key 后缀 `.one` / `.other`，中文只写 `.other` | `ports.count.one` = "{n} port"，`.other` = "{n} ports" |
| 不把数字/单位拼进 key | ❌ `common.last5min` → ✅ `net.window.minutes` + `{n: 5}` |
| 品牌名/术语不译 | `Prune`、`swap`、`RSS`、`darwin-arm64`、`pnpm` |

### 6.1 术语表（先定，再翻）

| 中文 | English | 备注 |
|---|---|---|
| 可回收 | reclaimable | 主数字用 `Reclaimable`；动作 `Reclaim` |
| 清理 / 剪除 | reclaim / prune | 移动废纸篓 = `Move to Trash` |
| 跨平台二进制 | cross-platform binaries | |
| 包管理器缓存 | package manager caches | |
| 换出 / 换出受害者 | swapped out / swap victims | |
| 真实占用 | true footprint | 口径名保留 `phys_footprint` |
| 悬案卷宗 | leak case | 「疑似内存泄漏」= `suspected leak` |
| 灵动岛 | Dynamic Island | 保持 Apple 术语 |
| 新面孔 | new device | 局域网语境 |
| 端口冲突保镖 | port conflict guard | |
| 关注端口 | watched ports | |
| 不可读 | unavailable | 不写 `unreadable`（读起来像 bug） |
| 尚未扫描 / 尚未采样 / 尚未测量 | Not scanned yet / Not sampled yet / Not measured yet | 三态必须区分，不得合并成 `N/A` |
| 口径 | measurement basis | 说明性文字里用 `based on …` |
| 阈值 | threshold | |

### 6.2 英文文案风格

- 句首大写、其余小写（sentence case），不用 Title Case
- 标点用 ASCII：`·` 换成 ` · `；`「」` 换成 `"…"`；`→` 保留（技术箭头通用）
- 不用中文量词思维：`12 项` → `12 items`；`3.2 秒` → `3.2 s`
- 数字与单位之间加空格：`7.74 GB`、`90 %` → 统一 `90%`（百分比不空格）
- 长句拆短：中文逗号堆叠的段落，英文用两个短句

---

## 7. 语言解析与持久化

### 7.1 优先级链

```
1. settings.json: locale == "zh" | "en"      → 用它（手动覆盖）
2. settings.json: locale == "auto" 或缺失     → 读系统首选语言（Rust sys-locale）
3. 系统语言匹配：
     zh / zh-Hans / zh-CN / zh-Hant …        → zh
     en / en-US / en-GB …                    → en
     其他（ja / de / fr …）                   → en（英文兜底）
4. 系统语言读取失败                            → en
```

> 为什么其他语言兜底英文而非中文：Prune 面向开发者，英文是安全默认；中文用户量最大但仍应由系统显式告知。

### 7.2 settings.json 变更

```jsonc
{
  "cpuAnim": true,
  "menuNumbers": true,
  "island": { "...": "..." },
  "locale": "auto"        // 新增：auto | zh | en，缺省视为 auto（老用户零迁移）
}
```

兼容：`settings.rs` 用 `get_str("locale")`，`None` 即 `auto`，**不需要迁移脚本**。

### 7.3 Rust 侧接口

```rust
// src-tauri/src/i18n.rs（新增）
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale { Zh, En }

pub enum Source { Auto, Manual }

/// 解析：settings 覆盖 > 系统语言 > 英文
pub fn resolve(app: &AppHandle) -> (Locale, Source);

/// 系统首选语言（sys-locale 主选；macOS 可用 objc2 NSLocale 兜底）
fn system_locale() -> Option<String>;

/// 托盘菜单文案（结构体比字符串 key 更早暴露漏字段）
pub struct TrayText {
    pub open_main: &'static str,
    pub check_update: &'static str,
    pub cpu_anim: &'static str,
    pub menu_numbers: &'static str,
    pub icon_theme: &'static str,
    pub quit: &'static str,
    pub theme_scissors: &'static str,
    pub theme_cat: &'static str,
}
pub const TRAY_ZH: TrayText = TrayText { open_main: "打开主窗口", /* … */ };
pub const TRAY_EN: TrayText = TrayText { open_main: "Open Main Window", /* … */ };
```

新命令（`lib.rs` 注册）：

| 命令 | 入参 | 出参 |
|---|---|---|
| `locale_get` | — | `{ current: "zh"\|"en", source: "auto"\|"manual" }` |
| `locale_set` | `{ locale: "auto"\|"zh"\|"en" }` | 新 `{current, source}`；失败返回 String 错误 |
| `net_diag_last` | `{ locale }` | 按 locale 重渲染的最近一次诊断（无则 `null`） |

依赖：`sys-locale = "0.3"`（纯 Rust、跨平台、体积可忽略）。
**离线兜底**：若无法拉新 crate，macOS 用已引入的 `objc2-foundation` 读 `NSLocale::preferredLanguages`，Windows 用 `GetUserDefaultUILanguage`，接口不变。

### 7.4 core 侧改造（`prune-core`）

| 项 | 现状 | 改法 |
|---|---|---|
| `disk/whitelist.rs` label 49 条 | `&'static str` 中文 | **保留不动**（前端按 `key` 查表）；`label` 只作未知 key 的兜底。若做 CLI 双语再议 |
| `net/diag.rs` 结论 | `label/advice/rule` 中文字符串 | 新增 `VerdictKind` 枚举 + `params`，`render(kind, params, Locale)` 出文本；结构体保持 `label/advice/rule` 字段（IPC 兼容），由 render 填充 |
| `net/diag.rs` Markdown 报告 | 硬编码中文表头 | `render_report(diag, Locale)`，表头/单位随 locale |
| `reclaim/mod.rs` notes | 中文短句 | 同上：`NoteCode + params` 或直接按 locale 渲染 |
| `DupSummary.advice` | 中文 | 同上 |

签名变更示例：

```rust
pub fn render(kind: VerdictKind, params: &VerdictParams, locale: Locale) -> VerdictText
```

`Locale` 定义在 `prune-core`（不依赖 tauri），`src-tauri` 与 core 共用同一枚举，避免两套。

---

## 8. 分阶段任务

> 每阶段结束都要 `pnpm build`（含 `tsc`）零错误 + `cargo test` 绿 + 双语手测一次。

### P0 地基（0.5 人日）

| # | 任务 | 文件 |
|---|---|---|
| P0-1 | `Locale` 枚举 + `i18n::resolve()` + `sys-locale` 依赖 | `src-tauri/src/i18n.rs`（新）、`Cargo.toml` |
| P0-2 | `locale_get` / `locale_set` 命令 + 注册 | `src-tauri/src/commands.rs`、`lib.rs` |
| P0-3 | 前端 i18n 内核（flatten / t / plural / L2 类型） | `src/i18n/index.ts`、`types.ts` |
| P0-4 | zustand store + `locale://changed` 监听 + `useI18n` hook | `src/i18n/store.ts`、`detect.ts` |
| P0-5 | 设置弹层加「语言」段（跟随系统 / 中文 / English，三选一胶囊） | `SettingsBoard.tsx` |
| P0-6 | `document.documentElement.lang` 同步 + `index.html` 去写死 | `main.tsx`、`index.html` |
| P0-7 | 迁 `lib/uiText.ts` → `messages/namespaces/common.ts`（19 条） | 全仓库引用点 |

验收：设置里切 English → 「设置」页自身变成英文（不重启）；重启后保持；系统切英文 + locale=auto → 英文。

### P1 主窗口外壳（1.5 人日）

`MainWindow.tsx`(48)、`RailNav.tsx`(15)、`Tabs.tsx`、`primitives.tsx`、`SettingsBoard.tsx`(53)、`UpdateCard.tsx`(24)、`lib/format.ts`、`lib/uiText.ts` 的调用点。

| # | 任务 |
|---|---|
| P1-1 | 导航 6 tab + tooltip + ⌘1–6 提示 |
| P1-2 | 标题栏（刷新/返回/Badge/口径说明） |
| P1-3 | 设置弹层全部文案（灵动岛 4 段 + 滑杆 label/hint + 「当前规则」动态句） |
| P1-4 | 更新卡（检查/下载/进度/已是最新） |
| P1-5 | `format.ts`：`toLocaleTimeString(locale)`、`formatRelative` → `Intl.RelativeTimeFormat`、`约 N 核` 本地化 |
| P1-6 | `EmptyArt.tsx`、`primitives.tsx` 共享文案 |

验收：主窗口切 tab 全英文无残留中文；`formatRelative` 显示 `just now` / `3 min ago`；时间列 `14:32` 格式不因 locale 变（`hour12: false` 保持）。

### P2 业务面板（3 人日，按文件推进）

| 批次 | 文件 | 字面量 |
|---|---|---|
| P2-1 总览 | `components/StatsBoard.tsx`、`stats/*`、`lib/statsReadout.ts`(23)、`MenuBarPanel` 共用层 | ~60 |
| P2-2 应用 | `AppResourceGrid.tsx`(25)、`appdetail/AppDetail.tsx`(28)、`ProcTable.tsx`、`ResourceTile.tsx`、`HistoryTrack.tsx`(10)、`appSummary.ts`(25)、`LeakCard.tsx`(12) | ~110 |
| P2-3 内存 | `windows/MainWindow.tsx` 内存段、`types/mem.ts` label 表、`MeterBar`/`StackBar` | ~50 |
| P2-4 存储 | `ReclaimBoard.tsx`(45)、`types/disk.ts`(4)、whitelist key 映射表（49 条 key → 英文名） | ~90 |
| P2-5 网络 | `NetBoard.tsx`(44)、`netDiagStore.ts`、`types/net.ts`、`DomainCard`/`NetReading` | ~80 |
| P2-6 端口 | `PortList.tsx`(17)、`types/net.ts` 的 `exposureMeta` | ~30 |

验收：每批单独 PR，双语截图对照；英文下「存储」页长条目名允许换行但不撑破卡片。

### P3 小窗与系统层（1.5 人日）

| # | 任务 | 关键点 |
|---|---|---|
| P3-1 | `MenuBarPanel.tsx`(19) 六行紧凑版 | 380 宽，英文行标签要能放下（`Memory`/`Storage`/`CPU`/`Down`/`Up`/`Ports`） |
| P3-2 | `Island.tsx`(25) 折叠/展开态 | 折叠态标签必须 ≤ 4 字符：`CPU` / `Swap` / `Port` / `New` |
| P3-3 | `tray.rs` 双字典 + `rebuild_menu` | 主线程重建，`TrayMenuState` 重新 manage |
| P3-4 | `Info.plist`：`CFBundleLocalizations` + `CFBundleDevelopmentRegion` | 双保险 C1 |
| P3-5 | `NSLocalNetworkUsageDescription` 双语（`zh-Hans.lproj` / `en.lproj` 的 `InfoPlist.strings`） | 可选 P3-5b，需手动核对 bundle；做不了就仅在中文系统显示中文（现状） |

验收：切语言后**立刻**点托盘 → 菜单是新语言；岛在两种语言下都不裁字。

### P4 Rust 业务文案（2 人日）

| # | 任务 |
|---|---|
| P4-1 | `net/diag.rs` 结论 → `VerdictKind` + params + `render(locale)`；单测断言 kind 与数字，不断言整句 |
| P4-2 | `render_report(locale)`：Markdown 表头/字段名双语，内容段落走同一结论文案 |
| P4-3 | `reclaim/mod.rs` notes + `DupSummary.advice` 同上 |
| P4-4 | `commands.rs` / `net_diag.rs` / `updater.rs` 错误串双语（12+ 条） |
| P4-5 | `net_diag_last(locale)` 命令 + 前端切语言后重取 |
| P4-6 | 诊断页「阈值透明表」随 locale 换单位（`ms` 不变，说明文字变） |

验收：英文环境跑一轮网络诊断 → 结论、阈值说明、复制出的 Markdown 报告全英文，无中文字符（正则断言 `[一-龥]` 为空）。

### P5 打磨与验收（1 人日）

| # | 任务 |
|---|---|
| P5-1 | 英文排版专项走查：380 面板 / 229 岛 / 432 面板 / 6 字符标题 / rail tooltip |
| P5-2 | 复数与单位统一（`Intl.PluralRules` 覆盖 `items` / `dependencies` / `processes` / `ports` / `entries`） |
| P5-3 | `scripts/check-i18n.mjs`：扫 `src/**/*.tsx?` 去注释后的中文字面量，白名单 `devFixture.ts`，超阈值退出非 0 |
| P5-4 | `package.json` 加 `check:i18n`，并入 `build` 前置 |
| P5-5 | README 双语（`README.md` / `README.en.md`），术语表入 `docs/` |
| P5-6 | 验收清单逐项打勾（§10） |

### 工作量汇总

| 阶段 | 人日 | 可独立交付 |
|---|---|---|
| P0 地基 | 0.5 | ✅ 设置里能切，外壳部分双语 |
| P1 主窗口外壳 | 1.5 | ✅ |
| P2 业务面板 | 3.0 | ✅ 按批次 |
| P3 小窗与系统层 | 1.5 | ✅ |
| P4 Rust 业务文案 | 2.0 | ✅ |
| P5 打磨验收 | 1.0 | ✅ |
| **合计** | **9.5** | |

---

## 9. 风险矩阵

| 级别 | 风险 | 触发条件 | 缓解 | 兜底 |
|---|---|---|---|---|
| **P0** | 系统语言判错（中文系统显示英文） | 仅依赖 `navigator.language` | 语言判定只在 Rust；`CFBundleLocalizations` 补声明 | 设置里手动切，且 `locale` 落盘 |
| **P0** | 切语言后托盘仍是旧语言 | 未重建菜单句柄 | `rebuild_menu` 显式调用 + 手测清单 | 提示「重启后生效」（下策，不采用） |
| **P0** | 漏翻导致中英混排 | 450 key 无法一次翻全 | 编译期穷尽检查 + `check-i18n.mjs` CI 门禁 | 缺失降级显示 key（dev 报错） |
| **P1** | 英文溢出破版（面板/岛/标题） | 英文长 30–60% | 短词表 + `truncate` + P5 专项走查 | 允许换行但禁撑破容器 |
| **P1** | 切语言触发重扫，用户等 30 s | 磁盘标签走后端渲染 | whitelist 走前端 key 查表（C5） | 诊断结果走轻量重取（C6） |
| **P1** | 纯函数里读全局 t，切语言后旧串不更新 | `statsReadout` / `appSummary` 顶部 import 全局 | 强制依赖注入 `t` 参数 | — |
| **P2** | 单测断言中文整句，改文案挂测试 | `tray.rs` / `diag.rs` 现有断言 | P4 改为断言枚举/数字 | — |
| **P2** | `InfoPlist.strings` 打包路径踩坑 | Tauri bundle 资源映射 | 标为 P3-5b，可延后 | 权限弹窗单语言可接受 |
| **P2** | 文案 owner 混乱，同一句两处维护 | 新增文案时判断错归属 | §3.2 归属规则写进 README | 代码评审把关 |

---

## 10. 验收清单

**语言解析**
- [ ] 系统中文 + 无 `locale` 键 → 全中文
- [ ] 系统英文 + 无 `locale` 键 → 全英文
- [ ] 手动选中文 + 系统英文 → 全中文，重启保持
- [ ] 手动选「跟随系统」→ 立即按系统语言渲染
- [ ] 老用户 `settings.json` 无 `locale` → 不报错、不迁移、按 auto 处理

**覆盖范围（每项双语言各走一遍）**
- [ ] 主窗口 6 tab 全部文案
- [ ] 设置弹层（含滑杆 hint 与「当前规则」动态句）
- [ ] 菜单栏面板 6 行 + 更新卡
- [ ] 灵动岛 4 类事件（CPU / 内存 / 端口 / 新面孔）折叠 + 展开
- [ ] 托盘菜单 6 项 + 图标主题子菜单 2 项（**切换后立即更新**）
- [ ] 磁盘可回收条目名（49 条 key 全覆盖，无中文残留）
- [ ] 网络诊断结论 + 阈值说明 + 复制出的 Markdown 报告
- [ ] 应用诊断摘要（复制出的 Markdown）
- [ ] IPC 错误提示（结束进程失败、路径不存在、清理失败）

**技术健康**
- [ ] `pnpm build` 零错误；`cargo test` 全绿
- [ ] 英文环境全局搜 `[一-龥]` 于渲染结果为空（诊断报告正则断言）
- [ ] 无 `toLocale*` 硬编码 `zh-CN`
- [ ] `check-i18n.mjs` 在新增裸中文时退出非 0
- [ ] 三窗口语言一致（无「主窗口英文、面板中文」）

---

## 11. 待拍板问题

| # | 问题 | 建议 |
|---|---|---|
| Q1 | 英文是否也做「中文原文对照」注释（便于日后校对）？ | 不做。catalog 并置已能对照，注释会腐烂 |
| Q2 | `prune-cli` / MCP 输出是否也本地化？ | 本期不做，保持机器友好；若要，走 `--lang` 显式参数 |
| Q3 | 是否给应用名加本地化后缀（如中文系统显示「Prune 剪」）？ | 不做，品牌名不译 |
| Q4 | 类型档位选 L1 还是 L2？ | **L2 严格**（参数也编译期校验），一次性成本 30 行 |
| Q5 | `sys-locale` 拉不下来时的兜底实现要不要现在就写？ | 先按主选实现，`i18n::system_locale()` 留单函数边界，兜底可后续替换不动上层 |

---

## 附：改动文件总览

```
新增
  src-tauri/src/i18n.rs
  src/i18n/{index,types,store,detect}.ts
  src/i18n/messages/{zh-CN,en-US}.ts
  src/i18n/messages/namespaces/*.ts        (13 个)
  scripts/check-i18n.mjs
  docs/i18n-plan.md                        (本文)

修改（前端 ~30）
  src/main.tsx  index.html
  src/windows/MainWindow.tsx
  src/panels/{MenuBarPanel,Island}.tsx
  src/components/**                        (约 22 个)
  src/lib/{uiText,format,statsReadout,appSummary,clipboard}.ts
  src/stores/netDiagStore.ts

修改（Rust ~13）
  src-tauri/{Cargo.toml,Info.plist}
  src-tauri/src/{lib,commands,tray,tray_icon,net_diag,i18n,updater}.rs
  crates/prune-core/src/{net/diag,reclaim/mod,disk/mod}.rs
```
