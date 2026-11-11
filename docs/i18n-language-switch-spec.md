# 语言切换 spec：设置页下拉菜单 + 系统语言可见 + 面板入口

> 状态：**已实施**（2026-11-11，P1-0 → P2-3 全部落地）　|　日期：2026-11-11（v2：确认繁体落英文）
> 方案：`docs/i18n-language-entry-plan.md`（v4）
> 上游规范：`docs/i18n-spec.md`（§1.1 标签规则、§1.5 降级链、§4.3 入口、§11 S3 / S5）
> 影响面：前端 8 文件（1 新增）、Rust 4 文件、文案 2 namespace、上游 spec 3 处修订
> 验证：`tsc --noEmit` / `check-i18n.mjs` / `vite build` / `rustfmt --check` 全通过；
> **Rust 未过编译器**（本机 cargo 拉不齐 registry），需在有网环境跑
> `cargo test -p prune-core`（含 `tag_prefix_matching` 新断言）与 `cargo check -p prune-app`。

---

## 1. 目标与非目标

### 目标

| # | 目标 | 判定方式 |
|---|---|---|
| G1 | 设置页的语言切换从「三颗胶囊」变为下拉菜单，含「跟随系统」 | 目视 + 验收 #2 |
| G2 | 选「跟随系统」时能看到系统语言被识别成什么 | 副标题显示「系统语言：简体中文」 |
| G3 | 系统语言在 macOS 里改过之后，重选「跟随系统」立即生效（不需重启） | 验收 #5 |
| G4 | ⌘, 打开设置 | 验收 #1 |
| G5 | 只开面板的用户也能到设置 | 验收 #11 |
| G6 | 切换失败回滚 + 就地报错（保持现有行为） | 验收 #7 |
| G7 | 繁体系统落到英文，不再显示简体文案 | 验收 #13 |

### 非目标

- 不新增语言（catalog 仍只有 zh / en）
- 不做繁体中文 catalog：繁体系统按**英文**处理（见 §4.5）
- 不做系统语言热监听（改系统语言后不自动切，需用户重选「跟随系统」）
- 不改 `prune-cli`（其文案冻结中文）
- 不动托盘菜单（v1 方案已降级，见 plan 附录）

---

## 2. 背景：三个缺陷

| 编号 | 缺陷 | 代码位置 | 后果 |
|---|---|---|---|
| D1 | `macos_locale()` 用 `OnceLock` 缓存系统语言 | `src-tauri/src/i18n.rs:143-162` | 进程内首次读到的值跟一辈子；`locale_set` 注释（`:201`）声称「重复点一次应当刷新」，实际不成立 |
| D2 | 载荷只有 `{current, source}` | `i18n.rs:74-79` | 用户不知道「跟随系统」解析成了什么；系统是日语时静默回落英文 |
| D3 | 设置只有一个入口（主窗口齿轮） | `MainWindow.tsx:95`；`MenuBarPanel.tsx:99-106` 页脚无设置 | 只开面板的用户无路可走 |

现状能力（不需要重做）：`locale_set` 的「写盘 → 重建托盘 → 广播」链路完整（`i18n.rs:193-229`），`setLocale` 失败回滚完整（`store.ts:58-78`），跨窗口同步已通（`store.ts:48-51`）。

---

## 3. 需求（用户视角，可判定）

| ID | 需求 | 判据 |
|---|---|---|
| R1 | 打开设置就能看见当前语言与来源 | 触发器显示 `跟随系统（简体中文）` 或 `English` |
| R2 | 「跟随系统」必须说明系统语言是什么 | auto 项下有副标题；系统语言读不到时给「无法读取」 |
| R3 | 系统语言不被支持时要显式说明回落 | 日语系统显示「系统语言：ja-JP · 暂用 English」 |
| R4 | 切完立刻生效，所有窗口 + 托盘一起变 | 主窗口 / 面板 / 灵动岛 / 托盘菜单 |
| R5 | 写盘失败不能假装成功 | 界面停在旧语言 + 卡片内红字 |
| R6 | 键盘可达 | Tab / Enter / ↑↓ / Esc 全通 |
| R7 | 繁体系统不被当成简体 | `zh-Hant` / `zh-TW` / `zh-HK` / `zh-MO` → 英文界面，副标题写明「繁體中文 · 暂用 English」 |

---

## 4. 架构设计

### 4.1 分层与数据流

```
┌─────────────────────────── Rust（src-tauri） ───────────────────────────┐
│  settings.json ──┐                                                       │
│                  ├─ resolve() ──> Resolved { locale, source, system }    │
│  system_tag() ───┘        │                                              │
│  (defaults read / Win32)  ├─> LocalePayload { current, source,           │
│                           │                   system, autoResolved }     │
│                           ├─> AppLocale（原子缓存，热路径只读这里）        │
│                           └─> emit locale://changed（三窗口）            │
└──────────────────────────────────┬───────────────────────────────────────┘
                                   │ IPC
┌──────────────────────────────────▼───────────────────────────────────────┐
│ 前端（src）                                                               │
│  localeStore { locale, source, system, autoResolved }                    │
│      │                                                                   │
│      ├─> useI18n()  → 所有文案                                            │
│      └─> <LanguageSection />  → 触发器 + 菜单（读 source/system 决定文案） │
└──────────────────────────────────────────────────────────────────────────┘
```

关键点：**判定与展示分离**。`Resolved.locale` 是最终结论（进 `AppLocale`，供所有后端文案用）；`system` / `autoResolved` 只用于设置页的解释性文案，不参与任何判定。

### 4.2 数据契约

Rust（`src-tauri/src/i18n.rs`）：

```rust
/// 解析结果：最终语言 + 来源 + 系统原始 tag
pub struct Resolved {
    pub locale: Locale,
    pub source: Source,
    /// 系统首选语言原始串（如 "zh-Hans-CN"）；读不到为 None
    pub system: Option<String>,
}

impl Resolved {
    /// 若此刻选 auto 会解析成哪个（与 resolve 的 auto 分支同一规则）
    /// 繁体 tag 也走这里：`from_tag` 判 `En`（§4.5），所以 autoResolved 是 En
    pub fn auto_resolved(&self) -> Locale {
        self.system
            .as_deref()
            .map(Locale::from_tag)
            .unwrap_or(Locale::En)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalePayload {
    pub current: Locale,
    pub source: &'static str,
    pub system: Option<String>,
    pub auto_resolved: Locale,
}

impl LocalePayload {
    fn from_resolved(r: Resolved) -> Self {
        let auto = r.auto_resolved(); // 先算再 move
        Self {
            current: r.locale,
            source: r.source.as_str(),
            system: r.system,
            auto_resolved: auto,
        }
    }
}
```

线上 JSON（`serde(rename_all = "camelCase")` 只影响下划线字段）：

```json
{ "current": "en", "source": "auto", "system": "zh-Hans-CN", "autoResolved": "zh" }
```

`Locale` 已有 `#[serde(rename_all = "lowercase")]`（`prune-core/src/locale.rs:13`），序列化为 `"zh"` / `"en"`，无需改。

TypeScript（`src/i18n/detect.ts`）：

```ts
export interface LocaleState {
  current: Locale;
  source: LocaleSource;
  /** 系统首选语言原始 tag；浏览器 dev 或后端读不到时为 null */
  system?: string | null;
  /** 若此刻选 auto 会解析成哪个 */
  autoResolved?: Locale;
}
```

字段设为可选：浏览器 dev（`fetchLocale` 走 navigator 分支）不产出这两个值，且旧后端返回时前端不能炸。

### 4.3 系统语言读取（修 D1）

`macos_locale()` 去掉 `OnceLock`，改名为 `macos_tag()`；`windows_locale()` → `windows_tag()`；`system_locale()` → `system_tag()`。

```rust
#[cfg(target_os = "macos")]
fn macos_tag() -> Option<String> {
    // 不缓存：用户可能刚在系统设置里改了语言，选「跟随系统」必须立刻拿到新值。
    // `defaults read` 一次约 8–15 ms，只在启动 / 切语言 / 生成报告时触发，
    // 不在任何采样循环或渲染路径上。
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split('"')
        .nth(1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

调用频率核算：`resolve()` 的调用点是启动一次（`lib.rs:110`）、`locale_get`（前端 boot 一次）、`locale_set`（用户点击时）、`current()` 未初始化时的 fallback。`AppLocale` 初始化后，`current()` 走原子读，不进 `resolve`。因此每次切语言最多一次子进程，可接受。

> 备选（不采用）：`Mutex<Option<(String, Instant)>>` + 5 s TTL。多一层状态，且 TTL 内的旧值仍会让用户以为没生效。

### 4.4 入口拓扑与事件通道

| 入口 | 触发 | 通道 |
|---|---|---|
| 设置页下拉 | 点击 | 前端 `setLocale` → `locale_set` |
| ⌘, | 主窗口 keydown | 本地 `setSettingsOpen(true)` |
| 面板齿轮 | 点击 | `open_main(settings=true)` → emit `ui://settings` → 主窗口 `setSettingsOpen(true)` |

`ui://settings` 是新通道，不复用 `ui://tab`：`mapExternalTab`（`MainWindow.tsx:69-72`）的职责是「页面 id → tab」，把设置弹层塞进去会让这张兼容映射表（含 `port`/`apps`/`overview` 旧 id）承担两种语义。

### 4.5 繁体系统的归属规则（决策 6）

现状：`Locale::from_tag` 只做 `starts_with("zh")` 判断，`zh-Hant-TW` 也落 `Zh`（`crates/prune-core/src/locale.rs:25-31`），繁体用户看到的是简体文案。**本 spec 改为落 `En`。**

判定表：

| 系统 tag | `from_tag` 结果 | 依据 |
|---|---|---|
| `zh` / `zh-CN` / `zh-SG` / `zh-Hans*` | `Zh` | 明确简体 |
| `zh-Hant` / `zh-Hant-HK` / `zh-TW` / `zh-HK` / `zh-MO` | **`En`** | 繁体，catalog 不支持 |
| `en*` / `ja` / `de` / 空串 | `En` | 非中文 |
| `zh-Hans-HK`（罕见混写） | `Zh` | `hans` 优先，视为明确简体 |

规则（Rust 与 TS 必须一字不差）：

```rust
/// 繁体标签：hant / -TW / -HK / -MO。`hans` 优先，避免 zh-Hans-HK 被地区规则误判。
fn is_traditional(low: &str) -> bool {
    if low.contains("hans") {
        return false;
    }
    low.contains("hant") || low.contains("-tw") || low.contains("-hk") || low.contains("-mo")
}

pub fn from_tag(tag: &str) -> Self {
    let low = tag.trim().to_ascii_lowercase();
    if !low.starts_with("zh") {
        return Self::En;
    }
    if is_traditional(&low) {
        return Self::En;
    }
    Self::Zh
}
```

TS 侧（`src/i18n/detect.ts`，浏览器 dev 用，规则必须与 Rust 一致）：

```ts
/** 繁体判定：与 Rust `is_traditional` 同规则（hans 优先） */
export function isTraditionalTag(tag: string): boolean {
  const low = tag.toLowerCase();
  if (low.includes("hans")) return false;
  return low.includes("hant") || low.includes("-tw") || low.includes("-hk") || low.includes("-mo");
}

export function fromTag(tag: string): Locale {
  const low = tag.toLowerCase();
  return low.startsWith("zh") && !isTraditionalTag(low) ? "zh" : "en";
}
```

**为什么两处都要改**：Tauri 环境走 Rust（`i18n.rs:102`），浏览器 dev 走 `fromTag`（`store.ts:64`）。只改一边，dev 与真机的 auto 结果就不一致，而这类差异最容易在「我以为已经修了」时漏掉。

**测试用例（两边共用同一组，10 条）**：

| tag | 期望 |
|---|---|
| `zh` | Zh / "zh" |
| `zh-Hans-CN` | Zh |
| `zh-Hant` | **En** |
| `zh-Hant-TW` | **En** |
| `zh-TW` | **En** |
| `zh-HK` | **En** |
| `zh-Hans-HK` | Zh |
| `en-US` / `ja` / `` | En |

Rust 侧落在 `locale.rs::tests::tag_prefix_matching`；TS 侧无单测框架，靠验收 #13 手工覆盖，并在两处注释里写明「改这条规则必须同步另一处」。

---

## 5. 模块设计

### 5.1 `systemLangName`（`src/i18n/index.ts`）

```ts
/**
 * 系统语言原始 tag → 可读名。未收录的语言原样返回 tag（够用户判断）。
 * 名字跟 UI 语言走：这是对「系统设置里那个语言」的解释，不是可点击的选项；
 * 可点击的选项名（中文 / English）恒为原文，理由见 localeName。
 *
 * 繁简必须复用 isTraditionalTag，不能在这里另写一套正则 —— 否则
 * `zh-Hans-HK` 会被显示成「繁體中文」、解析却是 Zh，名与实相反。
 */
export function systemLangName(tag: string | null | undefined, t: I18n["t"]): string | null {
  if (!tag) return null;
  const low = tag.toLowerCase();
  if (low.startsWith("zh")) {
    return isTraditionalTag(low)
      ? t("settings.language.languageName.zhHant")
      : t("settings.language.languageName.zh");
  }
  if (low.startsWith("en")) return t("settings.language.languageName.en");
  return tag;
}
```

| tag 示例 | 显示（zh UI） | 显示（en UI） | 解析结果 |
|---|---|---|---|
| `zh-Hans-CN` / `zh-CN` | 简体中文 | Chinese (Simplified) | Zh |
| `zh-Hant-TW` / `zh-HK` | 繁體中文 | Chinese (Traditional) | **En**（见 §4.5） |
| `en-US` | English | English | En |
| `ja-JP` | ja-JP（原样） | ja-JP | En |
| `null` | ——（走「无法读取」文案） | —— | En |

注意繁体这一行：**名字如实显示「繁體中文 / Chinese (Traditional)」，但解析结果是 En**。名字与结果不一致是刻意的——名字告诉用户「系统认到了什么」，副标题再补一句「暂用 English」解释为什么界面是英文。

### 5.2 store 与 detect

`src/i18n/store.ts`：

```ts
interface LocaleStore {
  locale: Locale;
  source: LocaleSource;
  system: string | null;
  autoResolved: Locale;
  ready: boolean;
  error: string | null;
  init: () => Promise<void>;
  setLocale: (next: LocalePref) => Promise<void>;
}
```

三处写入点（`init` / 事件回调 / `setLocale`）必须都带上新字段，否则切完语言副标题会停在旧值：

```ts
// init
const st = await fetchLocale();
set({
  locale: st.current,
  source: st.source,
  system: st.system ?? null,
  autoResolved: st.autoResolved ?? st.current,
  ready: true,
});

// 事件
listen<LocaleState>("locale://changed", (e) =>
  set({
    locale: e.payload.current,
    source: e.payload.source,
    system: e.payload.system ?? null,
    autoResolved: e.payload.autoResolved ?? e.payload.current,
  }),
);

// setLocale（Tauri 分支）
const st = await invoke<LocaleState>("locale_set", { locale: next });
set({ locale: st.current, source: st.source, system: st.system ?? null, autoResolved: st.autoResolved ?? st.current });
```

> 顺手修：现在 `init` 里是 `set({ ...st, ready: true })`，会把 `current` 这个前端不用的字段混进 store。改显式映射后消失。

`src/i18n/detect.ts`：

- `LocaleState` 加 `system?: string | null; autoResolved?: Locale`（可选：浏览器 dev 与旧后端都不产出这两个字段，store 侧用 `?? null` / `?? st.current` 兜底）
- 新增导出 `isTraditionalTag()`，`fromTag` 改为「zh 且非繁体 → zh，其余 en」（规则与 Rust 一字不差，见 §4.5）
- `index.ts` 的 `systemLangName` 从这里 import `isTraditionalTag`，不要另写一套正则（`detect.ts` 不反向依赖 `index.ts`，无循环）

浏览器 dev 分支（`setLocale` 里 localStorage 那条）保持现状，`system` 设 `null`、`autoResolved` 用解析结果。

### 5.3 `LanguageSection` 组件（新增 `src/components/LanguageSection.tsx`）

导出一个渲染整张卡片的组件，`SettingsBoard` 只写 `<LanguageSection />`，与设置页其他卡片结构一致。

```tsx
const OPTIONS: LocalePref[] = ["auto", "zh", "en"];

export function LanguageSection() {
  const { t, locale } = useI18n();
  const source = useLocaleStore((s) => s.source);
  const system = useLocaleStore((s) => s.system);
  const autoResolved = useLocaleStore((s) => s.autoResolved);
  const setLocale = useLocaleStore((s) => s.setLocale);

  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);     // 键盘高亮
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const activePref: LocalePref = source === "manual" ? locale : "auto";
  const systemName = systemLangName(system, t);
  // 繁体不算 supported：它解析落 En（§4.5），副标题要说「暂用 English」
  const supported = system != null && /^(zh|en)/i.test(system) && !isTraditionalTag(system);
  ...
}
```

**触发器文案**

| 状态 | 文案 key | 结果（zh UI） |
|---|---|---|
| 手动选了语言 | — | `中文` / `English` |
| auto，系统语言读得到 | `settings.language.autoWith` | `跟随系统（简体中文）` |
| auto，系统语言读不到 | `settings.language.auto` | `跟随系统` |

模板必须进 catalog（英文用半角括号）：`autoWith: "跟随系统（{lang}）"` / `"Follow system ({lang})"`。不要用代码里的 `（${name}）` 拼——中文标点会漏进英文界面。

**auto 项的副标题**

```ts
function autoHint(): string {
  const fallback = localeName(autoResolved);
  // systemName 与 system 同生共死：system 非空时必然有可显示名（未知语言退化为原始 tag）
  if (!system || !systemName) return t("settings.language.systemUnknown", { fallback });
  if (!supported) return t("settings.language.systemFallback", { lang: systemName, fallback });
  return t("settings.language.systemLang", { lang: systemName });
}
```

**渲染结构**

```
<section className="rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5">
  <div className="flex items-center justify-between gap-4">
    <span className="text-[13px] font-medium">{t("settings.language.title")}</span>
    <div ref={rootRef} className="relative">
      <button role="button" aria-haspopup="menu" aria-expanded={open} ...>
        {triggerLabel}
        <ChevronRightIcon size={11} className="rotate-90" />
      </button>
      {open ? (
        <div role="menu" className="absolute right-0 top-[calc(100%+4px)] z-50 w-[260px] rounded-lg border border-hairline bg-surface-raised p-1 shadow-xl shadow-black/40">
          {OPTIONS.map(...)}
        </div>
      ) : null}
    </div>
  </div>
  <p className="mt-1.5 text-[10px] leading-4 text-ink-faint">
    {t("settings.language.current", { current: localeName(locale) })}
  </p>
  {error ? <p className="mt-1 text-[10px] text-danger">{error}</p> : null}
</section>
```

**菜单项**

```
<button role="menuitemradio" aria-checked={active}
        className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left …">
  <span className="w-3 shrink-0 text-[10px] text-accent">{active ? "✓" : ""}</span>
  <span className="min-w-0">
    <span className="block text-[11.5px] …">{label}</span>
    {sub ? <span className="mt-0.5 block text-[10px] leading-4 text-ink-faint">{sub}</span> : null}
  </span>
</button>
```

- 选中项文字 `text-ink`，未选中 `text-ink-dim`，hover `bg-white/[0.06]`
- 只有 `auto` 项带副标题；`zh` / `en` 不带，避免菜单变胖
- 勾选态看**偏好**不看当前语言：`activePref === opt`

**交互行为**

| 事件 | 处理 |
|---|---|
| 点击触发器 | `setOpen(v => !v)`；打开时 `setCursor(OPTIONS.indexOf(activePref))` |
| 点击项 | **先 `setOpen(false)`** 再 await：切完语言整棵树重渲染，菜单若还开着会以新语言闪一下 |
| 点项 == 当前偏好 | **不短路**，照样调 `setLocale`：`locale_set` 值没变不写盘但会重读系统语言 + 广播，这正是 D1 修好后「刷新跟随系统」的路径 |
| 点外部 | `document` 上挂 `pointerdown`，`!rootRef.current?.contains(e.target)` → 关闭 |
| Esc | 关闭并把焦点还给触发器 |
| ↑ / ↓ | 在 3 项间循环移动 `cursor`（`preventDefault`，避免滚动页面） |
| Enter / Space | 选中 `OPTIONS[cursor]` |
| 切换中 | 触发器 `disabled` + 文字降到 `opacity-60`；菜单项同样禁用 |
| 失败 | `setError(t("settings.language.switchFailed", { err: String(e) }))`；store 未变，界面天然停旧语言 |

`useEffect` 清理：`pointerdown` 与 `keydown` 监听都挂在 `open` 为 true 时才注册，卸载时移除。

### 5.4 `SettingsBoard` 接入

删除：`LOCALE_OPTIONS`（`:55-56`）、`pickLocale`（`:119-130`）、`switching` / `localeError` 状态、语言段的整段 JSX（`:146-179`）、`activePref` / `sourceLabel`（`:137-141`），以及不再使用的 import（`localeName`、`LocalePref`、`useLocaleStore`）。

新增：`<LanguageSection />` 放在第一张卡片的位置。

文件头注释「3. 语言不防抖」保留，改为指向新组件（防抖逻辑本来也不在语言上）。

### 5.5 `MainWindow`：⌘, 与 `ui://settings`

键盘分支（`MainWindow.tsx:295-321`）加一条，位置在 ⌘F 之后：

```ts
} else if (meta && e.key === ",") {
  e.preventDefault();
  setSettingsOpen(true);
}
```

监听（与 `ui://tab` 并列）：

```ts
useEffect(() => {
  if (!isTauri()) return;
  const un = listen("ui://settings", () => setSettingsOpen(true));
  return () => { void un.then((fn) => fn()); };
}, []);
```

### 5.6 Rust `i18n.rs` 与 `locale.rs`

**`crates/prune-core/src/locale.rs`（决策 6 的落点，先做）**

| 改动 | 位置 |
|---|---|
| 新增 `is_traditional()`，`from_tag` 判繁体落 `En` | `:20-31` |
| 文档注释改写（原文「繁体看到简体，已知取舍」要删） | `:1-9`、`:21-24` |
| `tag_prefix_matching` 测试改 3 条断言（`zh-Hant` / `zh-Hant-TW` / `zh-TW` → `En`），补 `zh-Hans-HK` → `Zh` | `:77-88` |

**`src-tauri/src/i18n.rs`**

| 改动 | 位置 |
|---|---|
| 新增 `Resolved` 结构 + `auto_resolved()` | 替换 `resolve` 的返回类型 |
| `resolve()` 改为读系统 tag 并返回 `Resolved` | `:93-107` |
| `LocalePayload` 加 `system` / `auto_resolved` + `rename_all` | `:74-88` |
| `LocalePayload::new` → `from_resolved` | 三处调用 |
| `system_locale()` → `system_tag()`，去 `OnceLock` | `:116-178` |
| `current()` 用 `resolve(app).locale` | `:110-114` |
| 新增单测 `auto_resolved_falls_back_for_unsupported_tags` | tests 模块 |

`resolve()` 新实现：

```rust
pub fn resolve(app: &AppHandle) -> Resolved {
    let pref = crate::settings::get_str(app, SETTING_LOCALE)
        .map(|raw| Locale::pref_from_str(&raw))
        .unwrap_or(LocalePref::Auto);
    let system = system_tag();
    let (locale, source) = match pref {
        LocalePref::Zh => (Locale::Zh, Source::Manual),
        LocalePref::En => (Locale::En, Source::Manual),
        LocalePref::Auto => (
            system.as_deref().map(Locale::from_tag).unwrap_or(Locale::En),
            Source::Auto,
        ),
    };
    Resolved { locale, source, system }
}
```

注意 manual 分支现在也读系统语言（为了副标题），这是新增的一次子进程 —— 只在 `locale_get` / `locale_set` 时发生。

调用点改动：

| 文件:行 | 旧 | 新 |
|---|---|---|
| `i18n.rs:113` | `resolve(app).0` | `resolve(app).locale` |
| `i18n.rs:183` | `let (l, s) = resolve(&app);` | `LocalePayload::from_resolved(resolve(&app))` |
| `i18n.rs:208-212` | `let (resolved, source) = resolve(&app);` | `let r = resolve(&app); st.set(r.locale); let payload = LocalePayload::from_resolved(r);` |
| `lib.rs:110` | `let (locale, source) = i18n::resolve(...)` | `let r = i18n::resolve(...); r.locale / r.source.as_str()` |

单测（不依赖真实系统语言，直接构造 `Resolved`）：

```rust
#[test]
fn auto_resolved_falls_back_for_unsupported_tags() {
    let mk = |tag: Option<&str>| Resolved {
        locale: Locale::En,
        source: Source::Auto,
        system: tag.map(str::to_string),
    };
    assert_eq!(mk(Some("zh-Hans-CN")).auto_resolved(), Locale::Zh);
    assert_eq!(mk(Some("zh-Hans-HK")).auto_resolved(), Locale::Zh); // hans 优先
    assert_eq!(mk(Some("zh-Hant-TW")).auto_resolved(), Locale::En); // 繁体落英文
    assert_eq!(mk(Some("zh-TW")).auto_resolved(), Locale::En);
    assert_eq!(mk(Some("en-GB")).auto_resolved(), Locale::En);
    assert_eq!(mk(Some("ja-JP")).auto_resolved(), Locale::En);
    assert_eq!(mk(None).auto_resolved(), Locale::En);
}
```

### 5.7 面板入口（P2）

`src-tauri/src/commands.rs:77`：

```rust
pub fn open_main(
    app: AppHandle,
    pid: Option<i32>,
    tab: Option<String>,
    settings: Option<bool>,
) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    crate::tray::show_main(&app);
    if settings.unwrap_or(false) {
        let _ = app.emit("ui://settings", ());
    }
    // pid / tab 逻辑不变
}
```

顺序：`show_main`（show + focus）**之后**再 emit，避免事件发到还没显示的 webview。

`src/lib/ipc.ts:59-64`：

```ts
export function openMain(
  pid?: number,
  tab?: "stats" | "resources" | "memory" | "disk" | "ports" | "net",
  settings?: boolean,
): Promise<void> {
  return invoke<void>("open_main", { pid: pid ?? null, tab: tab ?? null, settings: settings ?? null });
}
```

`src/panels/MenuBarPanel.tsx` header（`:72-80` 两个按钮之后）加第三个：

```tsx
<IconButton label={t("panel.action.settings")} onClick={() => fireAndForget(openMain(undefined, undefined, true), "open_settings")}>
  <GearIcon size={13} />
</IconButton>
```

- `GearIcon` 已存在（`components/icons.tsx`），`RailNav` 用它
- 尺寸 `size-6` 与现有两个 `IconButton` 一致，header 高 44 px 不变，`PANEL_H = 432` 不用改
- 面板宽度 380：3 × 24 + badge + 标题，实测有余量；若溢出，优先缩短 badge 而不是加高

### 5.8 文案（`src/i18n/messages/namespaces/settings.ts`）

| key | 中文 | 英文 | 英文长度 |
|---|---|---|---|
| `language.autoWith` | 跟随系统（{lang}） | Follow system ({lang}) | 20 + lang |
| `language.systemLang` | 系统语言：{lang} | System language: {lang} | 17 + lang |
| `language.systemFallback` | 系统语言：{lang} · 暂用 {fallback} | System language: {lang} · using {fallback} | 33 + lang |
| `language.systemUnknown` | 系统语言无法读取 · 暂用 {fallback} | System language unavailable · using {fallback} | 44 |
| `language.current` | 当前生效：{current} | Active: {current} | 8 + current |
| `language.languageName.zh` | 简体中文 | Chinese (Simplified) | 20 |
| `language.languageName.zhHant` | 繁體中文 | Chinese (Traditional) | 21 |
| `language.languageName.en` | English | English | 7 |

删除（旧 UI 专用，无其他引用）：`language.hint`、`language.sourceAuto`、`language.sourceManual`。
保留：`language.title` / `auto` / `zh` / `en` / `switchFailed`。

`panel.action.settings`：中文「设置」/ 英文「Settings」。

**宽度校验**：最长组合 = `Follow system (Chinese, Simplified)` = 35 字符 ≈ 200 px（11.5 px 字，字宽约 5.7 px）；触发器可用宽度 = 480（弹层）− 32（padding）− 26（标题）− 16（gap）≈ 400 px，安全。
菜单副标题最长 `System language: Chinese (Simplified)` = 38 字符 ≈ 218 px，菜单宽 260 − 16（padding）− 12（勾选列）− 8（gap）= 224 px，刚好贴边。**英文副标题不允许再加长**，需要时改短为 `System: …`。

---

## 6. 边界与异常

| # | 场景 | 期望行为 | 实现要点 |
|---|---|---|---|
| 1 | 系统语言 = 日语 | `跟随系统（ja-JP）`；auto 项副标题「系统语言：ja-JP · 暂用 English」 | `supported = /^(zh\|en)/i` 判定 |
| 2 | 系统语言读不到（Linux 容器 / `defaults` 失败） | 触发器只显示「跟随系统」；副标题「系统语言无法读取 · 暂用 English」 | `system === null` 分支 |
| 3 | 系统语言 = 繁体（`zh-Hant-TW` / `zh-HK`） | 界面**英文**，不再显示简体；触发器 `Follow system (Chinese, Traditional)`，副标题「System language: Chinese, Traditional · using English」 | `from_tag` 判繁体落 `En`（§4.5）；`supported` 排除繁体，走 `systemFallback` 分支 |
| 4 | macOS 改语言后重选「跟随系统」 | 立即生效 | D1 修复 |
| 5 | 重选当前项 | 不写盘、不闪、语言不变；但会重读系统语言并广播 | `locale_set` 的 `next_raw != prev_raw` 分支 |
| 6 | `settings.json` 只读 | 界面停旧语言 + 卡片红字；勾选态回实际生效值 | 现有 `setLocale` catch 链路 |
| 7 | 浏览器 dev（无 Tauri） | localStorage 兜底；`system = null`，副标题走「无法读取」，不崩 | `isTauri()` 分支 |
| 8 | 面板点齿轮时主窗口已销毁/未创建 | 静默无反应（与现有 `open_main` 一致），不报错 | `show_main` 内部已有 `eprintln` 兜底 |
| 9 | 菜单展开时切语言 | 菜单已关闭，不出现「新语言 + 旧菜单」 | 先 `setOpen(false)` 再 await |
| 10 | 设置弹层滚动到中部时展开菜单 | 向下展开 130 px 在可视区内，不被 `overflow-y-auto` 裁切 | 语言卡在首位；后续若移到底部需改向上展开 |

---

## 7. 风险矩阵

| 风险 | 级别 | 影响 | 缓解 |
|---|---|---|---|
| 去 `OnceLock` 后 `resolve()` 被高频调用 | P1 | 每次 10 ms 子进程，堆积会卡 UI 线程 | 已核算调用点（§4.3）；`AppLocale` 原子缓存挡住热路径；如实测有问题，退回「可失效缓存」方案 |
| 新载荷字段导致旧前端在读 `st.system` 时出错 | P2 | 不会：字段可选 + `?? null` | 前端全部用可选访问 |
| `open_main` 参数增加，其他调用点漏改 | P1 | 面板「可回收」按钮失效 | 参数用 `Option<bool>`，Rust 侧旧调用无需改；前端 `ipc.ts` 只有一处封装 |
| `ui://settings` 在 webview 未 ready 时丢失 | P2 | 首次点齿轮无反应 | 与现有 `ui://tab` 同性质；`show_main` 先执行，通常 webview 已创建 |
| 英文副标题贴边溢出 | P1 | 菜单文字被截 | 字符数已校验（§5.8），留 224 px 预算；超长时缩写而非加宽 |
| 面板 header 多一个按钮显拥挤 | P2 | 观感 | 与现有按钮同尺寸；实测溢出则缩 badge |
| 改 `from_tag` 影响全局，不只是设置页 | **P0** | 繁体系统上托盘、灵动岛、报告、CLI 文案全部改英文；若判断有误会误伤简体用户 | 规则窄化到 `hant` / `-tw` / `-hk` / `-mo`，且 `hans` 优先；8 条用例两边同测（§4.5）；`zh-CN` / `zh-SG` 明确保持 Zh |
| Rust 与 TS 的繁体判定漂移 | P1 | dev 与真机 auto 结果不一致 | 两处注释互指；用例表同一份写进 §4.5；验收 #13 覆盖真机 |
| Rust 改动在无 cargo 环境无法编译 | P1 | 语法对但类型错 | 本机 `cargo` 拉不齐 registry，只能 `rustfmt --check`；需在有网环境跑 `cargo test -p prune-core` 与 `cargo check -p prune-app` |

---

## 8. 分级选项

| 级别 | 范围 | 适用 |
|---|---|---|
| **S** | 只做 §5.3 + §5.4 + §5.8（设置页菜单、系统语言副标题）+ §5.6 的 payload 与去缓存 | 想尽快看到效果 |
| **M（本次）** | S + ⌘, + 面板齿轮（§5.5 + §5.7） | 已确认 |
| L | M + 托盘「设置…」单项 + 系统语言变更监听 | 不做 |

---

## 9. 实施顺序与门禁

每步结束都要过门禁，失败不进入下一步。

| 步骤 | 内容 | 门禁 |
|---|---|---|
| **P1-0** | 繁体规则：`locale.rs::from_tag` + `is_traditional` + 改 3 条测试断言；`detect.ts` 的 `fromTag` / `isTraditionalTag` 同步；`docs/i18n-spec.md` §1.1 / §971 / §11 S3 三处修订 | `rustfmt --check crates/prune-core/src/locale.rs`；`tsc --noEmit`；有网环境 `cargo test -p prune-core`（`tag_prefix_matching`） |
| P1-1 | Rust：`Resolved` / payload / 去 `OnceLock` / 4 个调用点 / 单测 | `rustfmt --edition 2021 --check src-tauri/src/i18n.rs src-tauri/src/lib.rs`；有网环境 `cargo test -p prune-app` |
| P1-2 | TS：`LocaleState` / store 三处写入 / `systemLangName` | `npx tsc --noEmit` |
| P1-3 | `LanguageSection.tsx` 新增 + `SettingsBoard` 接入 + 文案（含删 3 个旧 key） | `tsc --noEmit` + `node scripts/check-i18n.mjs` |
| P1-4 | ⌘, 打开设置 | `tsc --noEmit`；手测 ⌘, 与 Esc |
| P2-1 | `open_main` 加 `settings` + `ui://settings` + `ipc.ts` | `rustfmt --check` + `tsc --noEmit` |
| P2-2 | 面板 header 齿轮 + `panel.action.settings` 文案 | `tsc --noEmit` + `check-i18n` |
| P2-3 | 收尾：`vite build`、验收清单逐条过 | 退出码 0 |

提交建议拆两个：`feat(i18n): 设置页语言切换改为下拉菜单并显示系统语言`（P1）、`feat(i18n): 面板与 ⌘, 直达设置`（P2）。

---

## 10. 验收清单

| # | 操作 | 期望 |
|---|---|---|
| 1 | 主窗口按 ⌘, | 设置弹层打开，语言卡在第一屏；Esc 关闭 |
| 2 | 点语言触发器 | 菜单展开，勾选落在当前偏好；auto 项有副标题 |
| 3 | 选 English | 设置页 / 主窗口 / 面板 / 灵动岛 / 托盘菜单全英文；勾选落在 English；菜单已关闭 |
| 4 | 选「跟随系统」 | 回到系统语言；勾选落在 auto；触发器显示 `Follow system (Chinese, Simplified)` 类文案 |
| 5 | macOS 系统语言改 English → 回 Prune 点「跟随系统」 | 界面变英文，不需重启（D1） |
| 6 | 系统语言设日语 → 重开设置页 | 副标题「系统语言：ja-JP · 暂用 English」 |
| 7 | `settings.json` 改只读后切语言 | 语言不变 + 卡片红字；勾选回实际值 |
| 8 | 连点同一项两次 | 不写盘、语言不闪 |
| 9 | 键盘：Tab → Enter → ↑↓ → Enter；Esc | 全通，焦点不丢 |
| 10 | 浏览器 dev | 走 localStorage；副标题显示「无法读取」；无报错 |
| 11 | 只开面板 → header 齿轮 | 主窗口打开并停在设置弹层 |
| 12 | 英文界面下逐屏检查设置页 | 无中文残留（`check-i18n` 已静态保证） |
| 13 | macOS 系统语言设为**繁體中文（台灣）** → 重开 Prune，选「跟随系统」 | 界面**英文**（不是简体）；托盘 / 灵动岛 / 报告同样英文；副标题 `System language: Chinese, Traditional · using English` |
| 14 | 繁体系统上手动选「中文」 | 界面简体（手动选择优先于系统判定，不受 §4.5 影响） |

---

## 11. 已确认（原待确定项，2026-11-11 拍板）

| 问题 | 结论 |
|---|---|
| 繁体系统怎么办 | **落到英文**（推翻 `docs/i18n-spec.md` §11 的 S3），见 §4.5 |
| 系统语言名是否收录日语 / 韩语 / 德语 | 不收录，只映射 zh / en，其余原样显示 tag |
| 「当前生效」措辞 | `当前生效：中文`（强调与「偏好」的区别） |

---

## 12. 受影响文件清单

| 文件 | 类型 | 约行数 |
|---|---|---|
| `crates/prune-core/src/locale.rs` | 改（P1-0） | +18 / -8 |
| `docs/i18n-spec.md` | 改（P1-0） | +8 / -6 |
| `src-tauri/src/i18n.rs` | 改 | +45 / -30 |
| `src-tauri/src/lib.rs` | 改 | +3 / -3 |
| `src-tauri/src/commands.rs` | 改（P2） | +6 / -1 |
| `src/components/LanguageSection.tsx` | **新增** | ~180 |
| `src/components/SettingsBoard.tsx` | 改 | +3 / -50 |
| `src/panels/MenuBarPanel.tsx` | 改（P2） | +6 / -0 |
| `src/windows/MainWindow.tsx` | 改 | +10 / -0 |
| `src/i18n/index.ts` | 改 | +16 / -0 |
| `src/i18n/detect.ts` | 改 | +12 / -3 |
| `src/i18n/store.ts` | 改 | +18 / -6 |
| `src/lib/ipc.ts` | 改（P2） | +3 / -2 |
| `src/i18n/messages/namespaces/settings.ts` | 改 | +16 / -6 |
| `src/i18n/messages/namespaces/panel.ts` | 改（P2） | +1 / -0 |
| `docs/i18n-spec.md` | 改 | +3 / -2 |
