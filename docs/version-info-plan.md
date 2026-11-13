# 应用内版本信息展示 · Spec

> 状态：待确认 → 实现
> 关联：`scripts/bump-version.mjs`（版本号四处同步机制，本 spec 不改动它）、`docs/updater-plan.md`（自动更新，本次不动）

## 1. 背景与目标

- **背景**：Prune 当前版本 0.8.4，由 `pnpm bump` 统一维护 package.json / tauri.conf.json / Cargo.toml / Cargo.lock 四处，但 UI 上没有任何地方展示当前版本。用户无法在应用内确认自己装的是哪个版本。
- **目标**：在设置页底部新增「关于」卡片，展示应用名 + 当前版本号。
- **非目标**：
  - 不新增 Rust command（版本号走前端官方 API）
  - 不改版本号维护机制
  - 不移动 / 收敛 `UpdateCard` 的「检查更新」入口（后续单独迭代）
  - 不做彩蛋（连点版本号弹 debug 等一律不做，保持极简）

## 2. 版本数据来源

选 **`getVersion()`（`@tauri-apps/api/app`）**：运行时读 `tauri.conf.json` 的 `version`，与 bump 脚本天然同步，零 Rust 改动。

| 备选 | 结论 |
|---|---|
| Rust command 返回 `app.package_info().version` | 多余，不选 |
| 复用 `update_pending` 的 `currentVersion` | 仅更新场景可用，非通用来源，不选 |

**异常与降级**（关键边界）：

| 环境 | 行为 |
|---|---|
| Tauri 运行时（正常） | `getVersion()` → `v0.8.4` |
| 浏览器 dev（`isTauri() === false`） | 不调 API，直接显示 `dev`，不发请求不报错 |
| Tauri 下 `getVersion()` 抛错（理论不会发生） | catch 后显示 `dev`，不渲染错误卡片 |

版本号只在组件挂载时取一次（应用生命周期内版本不变），不订阅、不轮询。

## 3. UI Wireframe

### 3.1 设置页（主窗口 · SettingsBoard）

新「关于」卡片插在 **`LanguageSection` 下方、外观卡片上方**？——否。**放在整列最底部**（所有功能卡片之后、保存状态行之前），符合 macOS「关于信息沉底」惯例：

```
┌─ 主窗口 · 设置页（max-w 560，居中）────────────┐
│                                                │
│  ┌─ 卡片：语言 ────────────────────────────┐  │
│  │ 语言                        [跟随系统 ▾] │  │
│  └──────────────────────────────────────────┘  │
│  ┌─ 卡片：外观 ────────────────────────────┐  │
│  │ 外观                  (跟随系统|浅|深)   │  │
│  └──────────────────────────────────────────┘  │
│  ┌─ 卡片：灵动岛 ──────────────────────────┐  │
│  │ 灵动岛 [已就位]                  [on ○] │  │
│  └──────────────────────────────────────────┘  │
│  ┌─ 卡片：端口冲突保镖 ────────────────────┐  │
│  │ …                                        │  │
│  └──────────────────────────────────────────┘  │
│  ┌─ 卡片：局域网新面孔 ────────────────────┐  │
│  │ …                                        │  │
│  └──────────────────────────────────────────┘  │
│  ┌─ 卡片：CPU 暴走 / 内存异常 ─────────────┐  │
│  │ …                                        │  │
│  └──────────────────────────────────────────┘  │
│                                                │
│  ┌─ 卡片：关于  ← 新增（本 spec）──────────┐  │
│  │  Prune                            v0.8.4 │  │
│  └──────────────────────────────────────────┘  │
│                                                │
│  改动即时生效并保存                             │
└────────────────────────────────────────────────┘
```

### 3.2 「关于」卡片内部（单行，左名右版本）

```
┌─ AboutSection ─────────────────────────────────┐
│                                                │
│   Prune                              v0.8.4    │
│   ↑ text-[13px] font-medium        ↑ text-[11px]
│                                     font-mono tabular-nums
│                                     text-ink-faint
│                                                │
└────────────────────────────────────────────────┘
```

- 容器复用现有卡片样式：`rounded-card border border-hairline bg-surface-raised px-4 py-3.5`，与相邻卡片间距走外层 `gap-4`。
- 左侧固定文案 `Prune`（产品名不进 i18n，与 `tauri.conf.json productName` 一致）。
- 右侧版本号 `v{version}`：`font-mono tabular-nums text-ink-faint`，与 `UpdateCard` 中版本号的展示风格（mono + tabular）一致。
- 浏览器 dev 下右侧显示 `dev`（无 `v` 前缀）。

### 3.3 双语文案

| key | zh | en |
|---|---|---|
| `settings.about.title` | 关于 | About |

仅标题一个 key；`Prune` 与版本号是字面量，不翻译。

## 4. 改动清单

### T1 · 新增 `src/components/AboutSection.tsx`

```
职责：单卡片组件，风格对齐 LanguageSection（section 级、自包含）。
接口：export function AboutSection()，无 props。
逻辑：
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (!isTauri()) return;            // 浏览器 dev → 保持 null → 显示 "dev"
    getVersion().then(setVersion).catch(() => {});  // 失败静默降级为 "dev"
  }, []);
渲染：卡片 + 左 "Prune" + 右 version ? `v${version}` : "dev"
```

### T2 · i18n：`settings.about.title`

- `src/i18n/messages/namespaces/settings.ts`：zh 树新增 `about: { title: "关于" }`（en 树同文件新增 `about: { title: "About" }`）。
- `MessageTree<typeof zh>` + en-US.ts `satisfies Messages` 双闸自动覆盖，无聚合层改动。
- 门禁：`pnpm check:i18n` 通过。

### T3 · 挂载：`src/components/SettingsBoard.tsx`

- `import { AboutSection } from "./AboutSection";`
- 在「内存异常」卡片之后、保存状态行（`savedAt` / `error` 那个 flex 行）**之前**插入 `<AboutSection />`。

### 验证

- [ ] `tsc && pnpm check:i18n` 0 错误
- [ ] `pnpm tauri dev`：设置页底部看到 `Prune  v0.8.4`
- [ ] 浏览器 `pnpm dev`：版本位置显示 `dev`，console 无报错
- [ ] 卡片样式与相邻卡片（圆角/边框/内边距/间距）视觉一致，浅色 / 深色 / 跟随系统三主题下均可读
- [ ] `pnpm bump` 后 UI 版本号随新构建变化（机制层面确认即可，不必真 bump）

## 5. 风险

| 风险 | 等级 | 对策 |
|---|---|---|
| `getVersion` 在非 Tauri 环境抛错 | 低 | `isTauri()` 前置 + catch 降级 `dev` |
| i18n 漏 key | 低 | 双重类型闸 + check:i18n 门禁兜底 |
| 版本展示与实际安装版本不一致 | 极低 | 数据源即 tauri.conf.json，与打包元信息同源 |
