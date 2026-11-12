# Prune UI v5 · 前端实现规格书

> **设计源**：`docs/ui-v5-prototype.html`（2891 行，功能对齐版，6 board + 系统壳全量）
> **核对依据**：`docs/ui-v5-feature-checklist.md`（112 项，源码行号级）
> **适用代码**：`src/`（18k 行 React + Tailwind v4）
> **版本**：spec 1.0 · 2026-11-12
> **一句话**：把 v5 原型 1:1 落成 `src/` 的 UI，**IPC 契约与 Rust 侧零改动**；唯一新增的前端能力是浅色主题（纯 CSS + localStorage）。

---

## 0. 边界：什么改、什么绝对不改

### 0.1 改动范围（写权限）

| 层 | 路径 | 允许动作 |
|---|---|---|
| 设计 token | `src/index.css` | 追加/重排 token，**不删旧名**（向后兼容组件） |
| 基础组件 | `src/components/primitives.tsx`、`icons.tsx`、`EmptyArt.tsx` | 扩展变体，保持旧 props 可用 |
| 版式组件 | `src/components/*.tsx`、`appdetail/*`、`stats/*`、`charts/*` | 结构重排、样式替换 |
| 壳与面板 | `src/windows/MainWindow.tsx`、`src/panels/*.tsx` | 布局/交互呈现层 |
| 文案 | `src/i18n/*` | 只加 key，不改既有 key 语义 |

### 0.2 冻结项（碰了就是返工）

| 冻结对象 | 原因 |
|---|---|
| `src-tauri/src/**`（19 个 rs 文件） | 本次零后端改动。所有需求都能在前端消化 |
| `src/lib/ipc.ts` 的函数签名与返回结构 | 它是 Rust 命令的唯一出口；改签名会连带 7.9k 行 Rust |
| `src/types/*.ts` 的字段名 | 与 serde 结构体一一对应。**只允许新增可选字段的前端派生类型，不改源类型** |
| store 的 state 形状（`stores/*.ts`） | 后端数据 → store → UI 是单向链；spec 只消费，不重构。**唯一允许的动作签名扩展**：`execute(confirmed, items?)`（可选参数，旧调用不受影响） |
| `tauri.conf.json` 窗口尺寸 | 主窗口 960×640 / min 720×480 / panel 380×432 是产品既定值 |

### 0.3 需要「前端补偿」而非后端改动的 3 处

| 问题 | 后端方案（不采用） | 前端方案（采用） |
|---|---|---|
| 浅色主题 | 新增 settings 命令 + 持久化 | CSS `:root[data-theme]` + `localStorage`，启动时 `matchMedia` 兜底 |
| 右键「在 Finder 中打开」 | 新增 reveal 命令 | 复用现有打开路径命令；**若不存在则降级为「复制路径」单项**（见 §7.4） |
| 应用图标缺失时的占位 | 新增图标抽取接口 | 沿用 `useAppIcons` 的现有回退：首字母 + 域色底 |

> ⚠️ 硬约束：**任何需要在 Rust 侧新增 command 的设计，本 spec 一律降级**。第 4 章每个 board 都标了「降级实现」一栏，照它做就不会开天窗。

---

## 1. 落地画布：1160 原型 ≠ 960 实机

原型画布 **1160×720**（便于审视），实机主窗口 **960×640**（min 720×480）。**不能等比缩放**——等比缩放会让 11px 正文变成 9px，可读性崩。

### 1.1 缩放规则（唯一正确做法）

| 维度 | 规则 |
|---|---|
| 字号 / 行高 / 图标 / 圆角 / 边框 | **1:1 保持不变**（原型 `font-size:12px` 基准 = 实机基准） |
| 水平内边距 | board `22px → 18px`；card `16px → 14px` |
| 垂直间距 | 保持（原型本就是 960 宽下测过的密度） |
| 栅格列数 | 按**可用内容宽**断点降列，不缩卡片 |
| 表格列宽 | 固定列用 `minmax()` 收敛，弹性列吃剩余空间 |

### 1.2 断点定义（内容宽 = 窗口宽 − 58 rail − 2 边框）

| 断点 | 窗口宽 | 内容宽 | 域卡网格 | 应用表 | 端口表 | 侧栏 |
|---|---|---|---|---|---|---|
| `xl` | ≥1120 | ≥1060 | 3 列 | 全 7 列 | 全列 | 272px |
| `lg` | 960–1119 | 900–1059 | 3 列 | 全 7 列 | 全列 | 260px |
| `md` | 780–959 | 720–899 | 2 列 | 隐藏「磁盘 IO」列 | 隐藏「项目」列 | 228px |
| `sm` | 720–779 | 660–719 | 2 列 | 隐藏 2 列 | 隐藏 2 列 | 抽屉化 |

> 落地时 **960 是主场景**：`lg` 为默认设计态，`md/sm` 只保证「不破版」，不做精修。

### 1.3 全局骨骼

```
┌────────────────────────────────────────────────┐
│ rail 58px │ topbar 48px                        │
│           ├────────────────────────────────────┤
│  logo     │ board  padding 16/18/14            │
│  6 nav    │   ┌──────────────────────────┐     │
│           │   │ 域卡 / 表 / 详情          │     │
│  spacer   │   └──────────────────────────┘     │
│  gear     │                                    │
└────────────────────────────────────────────────┘
```

| 区域 | 规格 |
|---|---|
| rail | `58px` 宽，`background: raised`，`border-right: 1px hairline-soft`，`padding: 44px 0 8px`（44px 给 macOS 红绿灯让位） |
| rail 按钮 | `40×40`，`radius 10`，`gap 3px`，图标 `19×19` |
| 选中态 | `bg chip-hover` + 图标染域色 + 左侧 `2.5×22px` 竖条，`left:-9px`，`box-shadow 0 0 10px 域色` |
| topbar | `48px`，`padding 0 18px`，`border-bottom 1px hairline-soft`，`gap 10px` |
| board | `flex:1`，`overflow-y:auto`，`padding 16px 18px 14px` |

**底部安全区**：任何 sticky 操作栏必须给 board 补 `padding-bottom: 52px`，否则会裁掉最后一张卡（原型踩过）。

---

## 2. 设计 Token

### 2.1 命名策略

原型用短名（`--surface`），实机用 Tailwind v4 `@theme` 前缀（`--color-surface`）。**保留双向映射，只增不删**，让 44 个组件渐进迁移：

```css
@theme {
  /* 既有：不动 */
  --color-surface: #0e1114;
  /* 新增：语义层别名，v5 组件优先用这层 */
  --color-ink-mute: var(--color-ink-faint);   /* 原型 --faint 的语义名 */
}
```

### 2.2 深色（默认，已存在 → 只补 4 个）

| Token | 值 | 用途 | 状态 |
|---|---|---|---|
| `--color-surface` | `#0e1114` | 窗口底 | 已有 |
| `--color-surface-raised` | `#14181d` | 卡片 | 已有 |
| `--color-surface-overlay` | `#191e24` | 弹层/抽屉/菜单 | 已有 |
| `--color-surface-sunk` | `#0b0e11` | 凹陷区（表头、代码块） | **新增** |
| `--color-hairline` | `#242a32` | 分隔线/描边 | 已有 |
| `--color-hairline-soft` | `#1c2128` | 卡片边（比 hairline 更轻） | 已有 |
| `--color-ink` | `#e8ebee` | 主文字 | 已有 |
| `--color-ink-dim` | `#99a1aa` | 次文字 | 已有 |
| `--color-ink-faint` | `#656d77` | 三级/注释 | 已有 |
| `--color-accent` | `#34d399` | 品牌绿 = 总览/存储域 | 已有 |
| `--color-warn` | `#f5b74a` | 警告 | 已有 |
| `--color-danger` | `#f0705f` | 危险（结束进程） | 已有 |
| `--color-info` | `#6aa9ff` | 信息 = 内存域 | 已有 |
| `--color-domain-cpu` | `#a78bfa` | 应用域 | 已有 |
| `--color-domain-net` | `#22d3ee` | 网络域 | 已有 |
| `--color-domain-port` | `#f2a35c` | 端口域 | **新增** |
| `--color-accent-soft` | `rgba(52,211,153,.12)` | 徽标底 | **新增** |
| `--color-warn-soft` | `rgba(245,183,74,.12)` | 同上 | **新增** |
| `--color-cpu-soft` | `rgba(167,139,250,.14)` | 同上 | **新增** |
| `--color-net-soft` | `rgba(34,211,238,.12)` | 同上 | **新增** |
| `--color-port-soft` | `rgba(242,163,92,.14)` | 同上 | **新增** |
| `--chip` | `rgba(255,255,255,.045)` | 胶囊/次级按钮底 | **新增** |
| `--chip-hover` | `rgba(255,255,255,.08)` | hover | **新增** |
| `--hatch` | `rgba(255,255,255,.10)` | 斜纹（L4 不可回收） | **新增** |

> `.12/.14` 的 soft 值是**位移补偿**：域色本身在中性底上偏亮，纯 `color-mix` 会算得过浓，故用固定 alpha 而非 20% 混合。

### 2.3 浅色（新增能力，P3 交付）

```css
:root[data-theme="light"] {
  --color-surface: #f4f6f8;      --color-surface-raised: #ffffff;
  --color-surface-overlay: #ffffff; --color-surface-sunk: #eef1f4;
  --color-hairline: #dfe5ea;     --color-hairline-soft: #e8edf1;
  --color-ink: #171c21;          --color-ink-dim: #5b656f;  --color-ink-faint: #8b949e;
  --color-accent: #0e9e6e;       /* 4.5:1 于 #fff，见下 */
  --color-warn: #b97a12;         --color-danger: #c94f40;   --color-info: #2273cc;
  --color-domain-cpu: #7c5cd6;   --color-domain-net: #0e9cba; --color-domain-port: #b87333;
  --chip: rgba(0,0,0,.045);      --chip-hover: rgba(0,0,0,.075); --hatch: rgba(0,0,0,.14);
}
```

**对比度实测**（WCAG AA 正文 4.5:1）：

| 组合 | 比值 | 判定 |
|---|---|---|
| `#0e9e6e` on `#ffffff` | 4.52:1 | ✅ AA |
| `#34d399` on `#ffffff` | 1.98:1 | ❌ 故浅色必须换深绿 |
| `#5b656f` on `#f4f6f8` | 5.61:1 | ✅ |
| `#8b949e` on `#f4f6f8` | 3.06:1 | ⚠️ 仅限 ≥12px 且非承载语义（注释级） |

> 浅色下 `--ink-faint` 只用于 `footnote / 极简标签`，**不得**用于数值与状态。深色无此限制（`#656d77` on `#14181d` = 3.9:1，同为注释级限定）。

### 2.4 字体与数字

| Token | 值 |
|---|---|
| `--font-sans` | `-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif` |
| `--font-mono` | `ui-monospace, "SF Mono", Menlo, monospace` |

**所有数值列强制** `font-variant-numeric: tabular-nums`（否则 CPU/内存数字每帧跳动）。

### 2.5 字号阶梯（v5 定稿，勿增删档位）

| 角色 | size | weight | color | 用例 |
|---|---|---|---|---|
| `title` | 13.5 | 600 | ink | topbar 页名 |
| `sub` | 10.5 | 400 | faint | topbar 副标 |
| `card-title` | 11.5 | 500 | dim | 卡片标题 |
| `card-label` | 10.5 | 400 | faint | 卡片内标签 |
| `strong` | 12 | 500 | ink | 表内主值、行标题 |
| `body` | 11.5 | 400 | dim | 正文 |
| `mono` | 10.5 | 400 | dim | 路径 / PID / 端口 / IP |
| `number-lg` | 11.5 | 500 | ink | 表内数值（tabular） |
| `badge` | 9.5 | 500 | 域色 | 徽标 |
| `footnote` | 9.5 | 400 | faint | 口径说明、页脚 |

> **9.5px 是下限**，不得再小。原型中所有「看不清」的抱怨都出现在 9px 以下。

### 2.6 间距与圆角

| Token | 值 | 用途 |
|---|---|---|
| `--radius-card` | 13px | 卡片 |
| `--radius-hero` | 18px | Hero 主卡（已有） |
| `--radius-ctl` | 7px | 按钮 / 输入 |
| `--radius-pop` | 8px | 菜单 / popover 内块 |
| `--space-unit` | 4px | 基准 |
| 卡内 | 14 / 16 | padding |
| 卡间 | 12 | gap |
| 区块间 | 16 | margin-top |
| 行高 | 表行 28px / 密行 24px | |

### 2.7 阴影与层级（z-index 是硬约定）

| 层 | z | 阴影 |
|---|---|---|
| board 内容 | 0 | — |
| sticky 操作栏 | 30 | `0 -8px 24px rgba(0,0,0,.35)` |
| 下拉/筛选 popover | 70 | `0 12px 32px rgba(0,0,0,.45)` |
| 右键菜单 | 92 | `0 16px 40px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.02)` |
| 抽屉遮罩 | 80 | — |
| 抽屉 | 82 | `0 -12px 40px rgba(0,0,0,.5)` |
| 确认浮层（全局） | 95 | `0 20px 60px rgba(0,0,0,.6)` |
| toast | 100 | `0 10px 30px rgba(0,0,0,.5)` |

> **教训**：结束进程确认层曾嵌在卡片内，其他入口触发时不可见。**所有确认类浮层一律挂 body，z=95**。

---

## 3. 基础组件库（先建这层，再铺页面）

> 一次性在 `primitives.tsx` 补齐，后续 6 个 board 只做组合。**不允许**在页面里写一次性样式。

### 3.1 `Badge`（扩展 `tone`）

```ts
type Tone = "accent" | "warn" | "danger" | "info" | "neutral" | "cpu" | "port";
```

| 属性 | 值 |
|---|---|
| 尺寸 | `font 9.5/500`，`padding 2px 7px`，`radius 999` |
| 布局 | `inline-flex; align-items:center; gap:3px; white-space:nowrap; flex-shrink:0` |
| 底色 | `--{tone}-soft` |
| 前景 | `--{tone}` |

**必须 `flex-shrink:0`**：否则在窄列里会被压成两行（原型泄漏卡踩过）。

### 3.2 `ScopeBadge`（口径徽标，新组件）

用于「这个数字是怎么算的」。**不是装饰**——Prune 的可信度全靠它。

| 项 | 规格 |
|---|---|
| 结构 | `[icon 9px] 文案` |
| 样式 | `font 9.5`，前景 `--info`，底 `--info-soft`，边 `1px color-mix(info 25%, transparent)`，`radius 999`，`padding 1px 7px` |
| 交互 | `title` 承载完整解释（不止 abbreviation）；可点击时带 `cursor:help` |
| 文案库 | 见 §8.1「口径文案白名单」 |

### 3.3 `ProposalTag`（提案标记，新组件）

只用于「设计稿有、实现没有」的新增功能：

| 项 | 规格 |
|---|---|
| 样式 | `font 9`，`color --faint`，`border 1px dashed --faint`，`radius 999`，`padding 1px 6px` |
| 文案 | `新增提案` |
| 约束 | **仅出现在可整体删除的独立卡上**，不得贴在既有功能上模糊边界 |

### 3.4 `Card`

```css
background: var(--raised);
border: 1px solid var(--hairline-soft);
border-radius: 13px;
padding: 14px 16px;
```

| 变体 | 差异 |
|---|---|
| `Card` | 基础 |
| `Card interactive` | `hover: border-hairline` + `translateY(-1px)`，`transition 140ms` |
| `Card hero` | `radius 18px`，`padding 18px 20px`，内底 `linear-gradient(180deg, rgba(52,211,153,.06), transparent 60%)` |
| `Card insets`（卡中卡） | `background: --sunk`，`border: 1px hairline`，`radius 9px` |

### 3.5 按钮族

| 类型 | 高 | padding | 字号 | 样式 |
|---|---|---|---|---|
| `btn-primary` | 28 | `0 12px` | 11.5/500 | 底 `--accent`，字 `--accent-ink`，`radius 7`，`hover: brightness(1.06)` |
| `btn-ghost`（= 原型 icon-btn） | 26 | `0 7px` | 11 | 底 `--chip`，边 `1px hairline`，字 `--dim`，`hover: 字 ink + 边 faint` |
| `btn-danger` | 28 | `0 12px` | 11.5/500 | 底 `--danger`，字 `#fff` |
| `btn-text`（= backlink） | 24 | `0 9px` | 10.5 | 无底/无边，字 `--dim`，`hover: bg chip-hover + 字 ink` |
| `icon-only` | 26×26 | 0 | — | 同 ghost，无文字 |

**统一约束**：

- `white-space: nowrap`（防「去清理」被挤成两行）
- `:focus-visible` → `outline: 2px solid var(--accent); outline-offset: 2px`
- `disabled` → `opacity .45; cursor: not-allowed; pointer-events: none`
- `active` → `transform: scale(.98)`

### 3.6 `Meter` / `TrackProgress` / `CellBar`

| 组件 | 用途 | 规格 |
|---|---|---|
| `Meter` | 单值占比（磁盘总量、swap 阈值） | 高 4px，`radius 999`，底 `--chip`，填充色由 `tone` 决定。**组件内不做阈值升档**：本项目的条有两种语义 —— 占用率（如 swap 89%）和相对长度（`bytes/max`，最长那条比值恒为 1）。后者一旦自动升档，最长的分类会被误标成危险红（实测踩过：存储页 L2 24.5 GB 变红） |
| `TrackProgress` | 分段（L1–L4 构成） | 高 6px，段间 `2px` 间隙，段色 = L 级色，`radius 999` |
| `CellBar` | 表格内迷你条 | 高 3px，宽 42px，同域色，`opacity .8` |

**分段条必须 `gap` 而非 border 分隔**，否则总长会因边框累加而溢出容器（原型 L1 满条事故根因）。

### 3.7 `EmptyState`（4 处空态共用）

```tsx
<EmptyState art="scan" | "search" | "no-record" | "no-device"
            title description action={{label, onClick}} footnote />
```

| 项 | 规格 |
|---|---|
| 容器 | `padding 36px 20px`，`text-align:center`，`min-height 180px` |
| 插图 | `EmptyArt`，`64×64`，`opacity .55`，`filter: grayscale(.3)` |
| 标题 | `12.5/500 ink`，`margin-top 12px` |
| 描述 | `11 faint`，`margin-top 4px`，`max-width 320px`，`margin-inline:auto` |
| 动作 | `btn-primary`，`margin-top 14px` |
| 脚注 | `9.5 faint`，`margin-top 10px` |

**两态必须区分**（这是产品语义，不是样式）：

| 空态 | 场景 | 主文案 | 动作 |
|---|---|---|---|
| 「没扫到」 | 扫描过，结果为空 | `扫描完成，没有发现可回收空间` | `重新扫描` |
| 「没有数据」 | 从未扫描 / 功能未启用 | `还没有扫描过` | `立即扫描` |

> 反例：把两者写成同一句「暂无数据」= 用户无法判断该等还是该点。

### 3.8 `Toast`

| 项 | 规格 |
|---|---|
| 位置 | 底部居中，`bottom 18px`，`z 100` |
| 样式 | `bg overlay`，`border 1px hairline`，`radius 9`，`padding 8px 14px`，`font 11`，`shadow 0 10px 30px` |
| 时长 | 2.4s 自动消失；同一时刻只允许 1 条（新替旧） |
| 语义色 | 左侧 3px 竖条：成功 `--accent` / 失败 `--danger` / 提示 `--info` |

### 3.9 `Tooltip`（纯 CSS，不引库）

| 项 | 规格 |
|---|---|
| 触发 | `title` 属性优先（可访问性 + 系统原生）；自定义 tooltip 仅用于需要富文本/多行的口径说明 |
| 自定义样式 | `bg --overlay`，`border hairline`，`radius 7`，`padding 6px 9px`，`font 10`，`max-width 260px`，`line-height 1.5` |
| 延迟 | 400ms 出现，`150ms` 淡入 |
| 位置 | 上方优先，距目标 6px；顶部空间不足则下方 |

### 3.10 `PopMenu`（右键菜单，命名空间独立）

> **必须用独立类名 `pmenu`**。原型曾误用 `.ctx` 撞上「结束进程」菜单的 `flex-direction:column`，导致高度从 59 变 102。

```
.pmenu { position:fixed; z-index:92; width:184px; padding:3px 0;
         background:var(--overlay); border:1px solid var(--hairline);
         border-radius:8px; box-shadow:0 16px 40px rgba(0,0,0,.55); }
.pmenu-item { display:flex; flex-direction:row; align-items:center; gap:8px;
              width:100%; padding:6px 10px; font-size:11.5px; color:var(--ink);
              background:transparent; border:0; cursor:default; text-align:left; }
.pmenu-item:hover { background:var(--chip-hover); }
.pmenu-ico { width:13px; height:13px; color:var(--dim); flex:0 0 13px; }
.pmenu-err { padding:5px 10px; font-size:10px; color:var(--danger); border-top:1px solid var(--hairline-soft); }
```

**行为契约**：

1. 打开即 `focus()` 第一项；`↑/↓` 循环，`Enter` 执行，`Esc` / 点击外部 / `blur` 关闭
2. 关闭动画 80ms 淡出，不做位移
3. 靠近右/下边缘时翻转，保持距窗口边 8px
4. 单项菜单（降级态）只渲染「复制路径」+ 一行脚注「此系统不支持直接定位」

---

## 4. 六个 Board 的实现规格

> 每节结构：**结构树 → 关键尺寸 → 状态 → 交互 → 降级实现**。
> 尺寸单位 px，颜色写 token 名（不写 hex）。

### 4.1 总览 `stats`（⌘1）

#### 结构树

```
board-ov
├─ Hero 可回收卡（Card hero，grid 2 列：260px 1fr）
│  ├─ 左：RingGauge 148 + 中央 [数字 28/600] + [「可回收」9.5 faint]
│  └─ 右：标题行 [「可回收空间」12.5/500] + [ScopeBadge「4 级构成」]
│         ├─ 2×2 图例（L1 系统缓存 / L2 应用残留 / L3 大文件 / L4 需确认）
│         └─ 脚注「L4 为斜纹段，不计入总量」
├─ 六域卡网格（grid 3 列，gap 12）
│  ├─ 内存卡：值 + Meter(swap 阈值) + 脚注「swap 5.2/8 GB」
│  ├─ CPU 卡：值 + AreaChart(10min) + 脚注「整机每核平均」
│  ├─ 硬盘卡：值 + Meter(占比) + 脚注「可用 / 总量」
│  ├─ 端口卡：值 + DotMatrix + 脚注「监听 N · 连接 M」
│  ├─ 下载卡：值 + Sparkline
│  └─ 上传卡：值 + Sparkline
├─ Next Best Action 卡（ProposalTag「新增提案」）
└─ 剪除成绩单卡（CountUp + 换算 + Top3 + 「N 项已剪除 · 自 {date} 起」）
```

#### 关键尺寸

| 元素 | 规格 |
|---|---|
| Hero 高 | 172（含 padding） |
| RingGauge | 外径 148，环宽 9，段间 `gap 2°`，起始角 −90°，顺时针 L1→L4 |
| 中央数字 | `28/600`，tabular，单位 `13/500 dim` 同行基线 |
| 图例格 | 2 列 × 2 行，`gap 10px 18px`；色块 `8×8 radius 2` |
| 图例数值 | `11.5/500 ink` tabular + 单位 `9.5 faint` |
| 域卡 | 高 96（固定），`padding 12px 14px` |
| 域卡值 | `19/600` tabular + 单位 `10 dim` |
| 域卡 caption | **移入 `title`**，卡内只留 1 行 `9.5 faint` 关键口径 |

**图例数字右对齐**：`display:grid; grid-template-columns: 8px 1fr auto`，值列 `text-align:right`。

#### 状态

| 状态 | 表现 |
|---|---|
| 未扫描 | Hero 替换为「未扫描」形态：环画成 4 段 8% 淡色占位 + 中央「—」+ 说明 + `btn-primary 立即扫描`（`stopPropagation`，不下钻） |
| 扫描中 | Hero 数字位显示 `扫描中…`（`9.5 dim`），环按已扫进度着色，域卡保持上次快照并加 `opacity .6` |
| 无剪除记录 | 成绩单卡换成空态：「还没有剪除记录」+ `去存储看看`（切 tab） |
| 有数据 | 正常 |

#### 交互

- Hero 点击 → 下钻存储页（`disk`），并设 `backTo = stats`
- 六域卡点击 → 下钻对应 tab
- 端口卡点击 → 端口页并**预选「监听」子 tab**
- 全部 `hover` → `Card interactive` 抬升 1px

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 四段弧数据缺失（只有总量） | 退化为**单色整环** + 图例四项数值全显示 `—` + 脚注「当前版本未分级」，**不伪造比例** |
| 端口点阵所需数据缺失 | 点阵画 24 个空格 + 「—」+ 脚注「端口扫描未启用」 |

---

### 4.2 应用 `resources`（⌘2）

> **P0 实测纠偏**：本节初稿写成「7 列表格」，与实现不符 —— `AppResourceGrid.tsx`
> 是**卡片网格**（每应用一张卡，六字段），`nav.resourcesHint` 的原话也是
> 「每个应用一张卡」。**不要改造成表格**：卡片能同时表达「CPU/内存/磁盘/网速」
> 四条不同量纲的条形，表格只能放数字，反而丢掉 v4 最有价值的信息层次。
> 真正的缺口是「卡片内文字过多」，v5 只做信息分层，不改形态。

#### 结构树

```
board-app
├─ 工具行
│  ├─ 搜索框（⌘F，220 宽）
│  ├─ 排序切换：按 内存/CPU/进程/磁盘/端口/下载（域色小圆点 + 文案）
│  └─ 结果计数：「显示 12 / 17」
├─ 摘要行（10.5 faint，单行不换行）
│  └─「17 个应用 · 159 进程」+ 聚合提示（网络尚未采样 / 内存条形基准 9.0 GB）
└─ 卡片网格（grid，gap 12）
   └─ 应用卡 ×N（Card interactive）
      ├─ 头部：[AppIcon 20×20] [家族名 12.5/500] … [「21 进程」徽标]
      │        [副行：「— 端口 · 0 连接」9.5 faint]
      ├─ 四行字段（CPU / 内存 / 磁盘 / 网速）
      │  └─ 每行 = [label 9.5 faint，宽 34] [Meter/CellBar 弹性] [值 11.5/500 tabular 右对齐]
      └─ 右下角 hover 才出现的 `›` 下钻指示
```

#### 关键尺寸

| 元素 | 规格 |
|---|---|
| 网格列数 | `xl` 3 列 / `lg` 2 列 / `md` 2 列（960 实机是 2 列） |
| 卡片 | 高 ~142（四行字段时），`padding 13px 15px`，`radius 13` |
| 图标 | 20×20，`radius 6`，缺图时首字母 + 域色底 |
| 字段行高 | 22；标签列 34px 固定，值列 `min-width 62px` 右对齐 |
| 条形 | 高 3（`CellBar`），底 `--chip`，填充按字段域色 |
| 数值 | `tabular-nums`；无值显示 `—`（不是 `0`） |

#### 状态

| 状态 | 表现 |
|---|---|
| 默认 | 卡面 `bg raised` + `border hairline-soft` |
| hover | `-translate-y-px` + `border hairline`，右上角出现 `›` |
| 点击 | 打开 `AppDetail` 全屏浮层（不是展开行） |
| 搜索无结果 | `EmptyState art="search"`：「没有匹配「{q}」的应用」+ `清除搜索` |
| 无进程数据 | `EmptyState art="no-record"`：「进程列表为空」+ 脚注「可能需要更高权限」 |
| 网络未采样 | 网速行显示 `尚未采样`（9.5 faint）+ `title` 说明；不画 0 条 |

#### 交互

1. **搜索匹配范围 = 标题 + 整卡文本**（`(dataset.name + ' ' + card.textContent).toLowerCase()`）。曾只匹配中文标题，导致搜 `chrome` 无结果。
2. 排序是**全局单选**（域色圆点标记当前维度），不是逐列点击。
3. 点卡片任意位置 → `AppDetail`；卡内无「展开行」概念，进程明细在详情里。
4. 进程级「结束进程」在 `AppDetail` 的进程表上右键（§7.3）；应用卡不直接提供破坏性动作。

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 磁盘 IO / 网速无数据 | 显示 `—` 或 `尚未采样` + `title="该平台不提供"`，**隐藏而非显示 0** |
| 图标未取到 | `AppIcon` 首字母 + `--cpu-soft` 底 |

---

### 4.3 内存 `memory`（⌘3）

#### 结构树

```
board-mem（stretch: flex 横向）
├─ aside 260px（bg raised，border-right hairline-soft）
│  ├─ 头部：口径切换 [footprint | RSS]（Tabs sm）+ 进程数
│  ├─ 搜索框（⌘F）
│  └─ 进程列表（虚拟滚动）：[图标 14] [名 11.5] [内存 11.5 tabular] [PressureDot]
└─ main（flex 1，overflow-y auto）
   ├─ 标题行：[进程名 15/600] [口径徽标] [结束进程 btn-ghost]
   ├─ 概览格（3 列）：常驻 / 换出 / 峰值（各 值 18/600 + label 10.5 faint）
   ├─ 历史三轨（30天 / 7天 / 24h，默认 7天；Tabs sm 切换）
   │  └─ AreaChart ×3（内存 / CPU / 线程），共用 x 轴
   ├─ 泄漏卡 LeakCard（见下）
   └─ 工作区卡：路径（mono，可右键）+ 归属置信度 Badge
```

#### 关键尺寸

| 元素 | 规格 |
|---|---|
| aside | 宽 260（md 228），`padding 12px 10px` |
| 进程行 | 高 26，`padding 0 8px`，`radius 6` |
| main padding | `16px 18px` |
| 概览格 | `grid 3 列`，`gap 12`，`Card insets`，`padding 10px 12px` |
| 历史图 | 高 78，`gap 4`，x 轴标签 `9 faint` |
| 泄漏卡 | 高 92（有证据时 118） |

#### 三轨历史的硬约束

| 约束 | 说明 |
|---|---|
| 同一时间窗 | 三轨共用 x 轴范围，**不可各自 auto-scale**，否则会误读「CPU 尖峰与内存同时上升」 |
| 窗口切换 = 重取 | 切 30天/7天/24h 触发一次拉取；切回命中缓存（内存态，不持久化） |
| 数据不足 | 该窗数据点 < 3 时，图区显示「样本不足（{n}/3）」+ 不画线，**不插值** |

#### 泄漏卡状态矩阵

| 判定 | 徽标 | 文案 | 证据区 |
|---|---|---|---|
| 疑似泄漏 | `b-warn 疑似泄漏` | 「连续 {n} 个采样窗口只增不减」+ 「+{x} MB / {t}」 | 展开显示「证物」：采样点表 |
| 正常 | `b-ok 正常` | 「内存曲线平稳」 | 隐藏 |
| 已释放 | `b-neutral 已释放` | 「峰值后已回落 {x}」 | 隐藏 |
| **不可用** | `b-neutral 判定不可用` | 「采样窗口不足（{n}/6）」 | 隐藏 + 脚注「需连续观察约 30 分钟」 |

> 第四态最容易被漏画。**必须给**，否则用户会以为「没提示 = 没泄漏」。

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 换出值不可得 | 概览第 2 格显示 `—` + `title="该进程已退出，快照非实时"` |
| 工作区路径不可得 | 卡片替换为「未识别工作区」+ `9.5 faint` 说明三种来源判定失败 |
| 历史接口失败 | 三轨区整体替换为 `EmptyState no-record`：`历史读取失败` + `重试` |

---

### 4.4 存储 `disk`（⌘4）

#### 结构树

```
board-disk
├─ 卷卡行（grid 2 列，gap 12）
│  └─ 卷卡：[卷名 12/500] [Badge 文件系统] / Meter 占比 / 「可用 {x} · 共 {y}」/ 容量条
├─ 可回收主卡
│  ├─ 头部：[可回收 {size} 26/600] [ProposalTag?] … [btn-primary 去清理]
│  ├─ 分级构成条（TrackProgress，L1–L4）
│  ├─ 分级列表（4 行）
│  │  └─ 行：[勾选 14] [L 徽标] [级名 11.5] [说明 9.5 faint] [size 11.5 tabular] [展开箭头]
│  └─ 底部 sticky 操作栏（选中 N 项 · 共 {size} → 去清理）
├─ 分组预览（点击某级展开）：条目列表
│  └─ 条目：[图标] [名 11.5] [路径 mono 10 faint，可右键] [size]
└─ 大文件卡 / 项目健康卡 ProjectHealthCard
```

#### **交互链路（v5 唯一正确形态，逐条实现）**

原型的旧版画成「L1–L4 四个勾选框 → 直接清理」，**是错的**。真实链路是三段：

| 阶段 | UI | 关键约束 |
|---|---|---|
| ① 分级构成 | 顶部 TrackProgress + 4 行分级概览 | 只读，**不设级勾选框**（见下方 P1 纠偏） |
| ② 分组预览 | 点击行展开分组清单，逐组勾选 | 勾选单位是「分组」（`CacheGroup.key`），不是级；聚合项只统计 |
| ③ 底部抽屉确认 | 点「去清理」→ 抽屉从底升起（`max-height 70%`，`z 82`） | 抽屉内：总计 + 逐条取消勾选 + 需二次确认项（L4）单列 + 主按钮 |

> **P1 落地纠偏（原文写错，已按真实数据模型修正）**：
>
> 初稿假设「级 = 分组集合」，可以勾选级来批量选组。实测不成立 —— **L1「跨平台二进制」
> 不是一个目录**，而是缓存目录内部被标记为死重的条目切片，寄居在 Yarn 缓存
> （`level=l2`）里；`report.totals` 也按条目而非按组记账
> （L1 6.94 + L2 26.27 = Yarn 整组的 15.34 被拆到两级）。因此：
> - 级行改为**只读构成**（`report.totals` 与顶部总量同源，三处数字一致）；
> - 展开某级列出**该级的分组**：L1 = 带 `deadweight` 的组（显示死重切片大小），
>   L2/L3/L4 = 本级且无死重切片的组（避免同一个组出现两次）；
> - 勾选与清理的单位统一为分组 → `reclaim_preview(keys)` / `reclaim_execute(items)`。


**抽屉规格**：

| 元素 | 规格 |
|---|---|
| 遮罩 | `rgba(0,0,0,.45)`，`backdrop-filter: blur(2px)`，点击关闭 |
| 面板 | `bg overlay`，`border-top 1px hairline`，`radius 16px 16px 0 0`，`max-height 60vh` |
| 标题 | `13/600`「确认清理」 |
| 汇总行 | `11.5 dim`「共 {n} 项 · {size}」 |
| L4 警告块 | `bg warn-soft`，`border 1px color-mix(warn 25%)`，`padding 8px 10px`，`9.5 warn` |
| 二次确认 | `toggleConfirm`：勾选框 + 「我确认删除需确认项」，未勾选则主按钮 `disabled` |
| 主按钮 | `btn-primary`，文案「清理 {size}」；危险级存在时改 `btn-danger` |
| 次按钮 | `btn-ghost`「取消」 |

**禁止**：不得出现「7 天可撤销」这类文案——真实能力只有「走废纸篓可恢复」。正确文案：

> `删除后进入废纸篓，可自行恢复。清空废纸篓才真正释放空间。`

#### 右键路径菜单（全局复用组件）

| 项 | 行为 |
|---|---|
| 在 Finder 中打开 | 调用现有打开命令；成功后 toast「已在 Finder 中显示」 |
| 复制路径 | `navigator.clipboard.writeText`；成功 → toast「已复制」；失败 → 菜单内 `pmenu-err`「复制失败：剪贴板不可用」**不关菜单** |
| 目录判定 | `data-dir="1"` 时文案「在 Finder 中打开」，否则「在访达中显示所在文件夹」 |

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 无 reveal 命令 | 菜单只留「复制路径」+ 脚注「此系统不支持直接定位」 |
| 分级数据缺失 | TrackProgress 退化为单色条 + 4 行数值全 `—` + 脚注「未分级」 |
| 扫描中 | 主按钮 `disabled` + 文案「扫描中…」，分级列表骨架屏（3 行 40px 灰块） |

---

### 4.5 网络 `net`（⌘5）

#### 结构树

```
board-net
├─ 速率卡行（grid 3 列）
│  ├─ 下载卡：值 22/600 + 单位 + Sparkline(60s) + 「峰值 {x}」
│  ├─ 上传卡：同上
│  └─ 7 天流量卡：值 + BarSeries(7 天) + 「今日 {x} / 均 {y}」
├─ 网卡表（本机接口）
│  └─ 行：[名 11.5] [IP mono] [状态 Badge] [↓ 速率] [↑ 速率] [累计流量]
├─ 分层诊断卡（**三层，不是四层**）
│  ├─ L1 链路：网卡是否 up + IP 获取
│  ├─ L2 网关：ping 网关 / ARP
│  └─ L3 外网：DNS 解析 + HTTPS 探测
│     └─ 附加提示（非第四层）：captive portal 检测命中时，在 L3 行下方加一条 `9.5 warn` 提示
└─ 局域网卡 LanBoard
   ├─ 头部：[「局域网」12.5] [设备数] [btn-ghost 扫描]
   ├─ 扫描进度（5 阶段，见下）
   └─ 设备网格（LanDeviceCard ×N，grid 2 列）
```

#### 速率采样四态（顶栏与卡内共用）

| 态 | 表现 |
|---|---|
| `ok` | 数值 + 曲线 |
| `sampling` | 数值保留上次 + 曲线右端加 `9.5 dim`「采样中」+ 骨架点闪烁 |
| `unreadable` | 数值 `—` + **橙色横幅**（见下） |
| `disabled` | 整卡 `opacity .5` + 「网络监测未启用」+ `启用` 按钮 |

**不可读橙色横幅**：

```
高度 30，bg warn-soft，border-bottom 1px color-mix(warn 25%)，
文案 10.5 warn：「当前网卡不提供速率统计（{reason}），数值暂不可用」，
右侧 btn-ghost「重试」
```

#### 局域网扫描 5 阶段（进度必须可见）

| 阶段 | 文案 | 进度 |
|---|---|---|
| 1 | 获取网段 | 0–15% |
| 2 | ARP 缓存 | 15–35% |
| 3 | 主动探测 | 35–75% |
| 4 | 反查主机名 | 75–92% |
| 5 | 归并去重 | 92–100% |

进度条：高 3px，`--net` 填充，右侧 `9.5 faint` 显示当前阶段文案 + `{done}/{total}`。**不允许**只转圈不给阶段——用户会以为卡死。

#### 网卡表列宽

`grid-template-columns: 1fr 132px 74px 80px 80px 92px`
（速率列 `80px` 是踩坑值：66px 时「12.4 MB/s」会换行）

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 无局域网能力 | 局域网卡整体替换为 `EmptyState no-device`：「局域网扫描不可用」+ 脚注原因 |
| 诊断只到 L2 | L3 行显示 `b-neutral 未执行` + 「需要外网连通性权限」 |
| 流量统计无历史 | 7 天卡显示「需要累计 1 天数据」+ 单柱占位 |

---

### 4.6 端口 `port`（⌘6）

#### 结构树

```
board-port
├─ 子 tab 行：[监听 | 连接]（Tabs sm）+ 搜索框（端口/进程/项目）+ 排序
├─ 端口表（sticky 表头）
│  └─ 行：[端口 mono 11.5] [协议 Badge] [暴露面 Badge] [进程 11.5] [PID mono] [项目 mono] [CMD]
└─ 空态 / 无权限提示
```

#### 列宽（content 900）

`grid-template-columns: 70px 74px 216px 1fr 56px`

> 原型的列宽给错（进程/项目列大片空白）。**正确分配**：端口 70、协议+暴露 74、进程+项目 216、地址（弹性 1fr）、PID 56。

#### 暴露面语义（`exposureMeta`）

| 值 | 徽标 | 含义 |
|---|---|---|
| `public` | `b-danger 公网` | 绑定 `0.0.0.0` 且非本机回环 |
| `lan` | `b-warn 局域网` | 非回环但仅内网段 |
| `local` | `b-neutral 本机` | `127.0.0.1` / `::1` |

#### 状态

| 状态 | 表现 |
|---|---|
| 默认 | 行高 28 |
| hover | `bg chip` + 行尾出现 `定位` `btn-text` |
| 选中 | 左 2px `--port` 条 |
| 无监听 | `EmptyState search`「当前没有监听端口」+ `重新扫描` |
| 无权限 | 横幅 `bg info-soft`：「部分端口需要更高权限才能读取」+ `了解` |

#### 降级实现

| 依赖 | 降级 |
|---|---|
| 项目归属不可判定 | 项目列 `—` + `title="未识别到所属项目"` |
| PID 无法映射进程 | 进程列显示 `PID {n}` + `b-neutral 未知进程` |

---

## 5. 系统壳（主窗口之外）

### 5.1 菜单栏 Popover（`MenuBarPanel.tsx`，380×432）

macOS 上最高频入口，**信息优先级从上到下严格固定**：

| 序 | 区块 | 高 | 规格 |
|---|---|---|---|
| 1 | 头部 | 44 | 左：`Prune` 12.5/600 + 状态 dot；右：`打开主窗口` icon-only |
| 2 | 可回收 CTA | 56 | 大数字 `22/600` + 「可回收」+ 右侧 `btn-primary 去清理`（打开主窗口 disk 页） |
| 3 | 泄漏条（条件） | 28 | 仅当有疑似泄漏时出现：`b-warn` + 「{name} 疑似泄漏 +{x}」+ 箭头，点击进内存页 |
| 4 | 六行统计 | 6×26 | 图标 13 + 名 11.5 + 值 11.5 tabular；行 hover `bg chip`，点击进对应 tab |
| 5 | 按应用 Top3 | 3×24 | 图标 14 + 名 + 内存值；`显示全部` 文本按钮 |
| 6 | 更新卡（条件） | 44 | `UpdateCard` 紧凑态 |
| 7 | 页脚 | 30 | 左：`⌘1 打开主窗口` kbd；右：`退出` btn-text |

**约束**：

- 面板 `radius 12`，`border 1px hairline`，`bg raised`，外层 `shadow 0 18px 50px rgba(0,0,0,.55)`
- 内边距 `10px 12px`，区块间 `border-top 1px hairline-soft`（除首块）
- **纵向不得滚动**：内容超出时只允许裁掉「按应用 Top3」（有 `显示全部` 兜底）
- 打开时 80ms 淡入 + `translateY(-4px) → 0`

### 5.2 灵动岛（`Island.tsx`）

4 类事件 × 2 态，**所有事件共用一套几何**：

| 态 | 尺寸 | 圆角 | 内容 |
|---|---|---|---|
| collapsed | `168×32` | 16 | 图标 14 + 主文案 11.5 + 值 11.5 tabular |
| expanded | `320×96` | 18 | 图标 + 标题 + 2 行说明 + 行内操作（`查看` / `忽略`） |

| 事件 | 图标色 | collapsed 文案 | expanded 标题 / 说明 |
|---|---|---|---|
| `cpu` | `--cpu` | `CPU {p}%` | 高负载 / `{topApp} 占用 {p}%，已持续 {t}` |
| `mem` | `--info` | `内存 {p}%` | 内存压力 / `可用 {x}，swap 使用 {y}` |
| `port` | `--port` | `端口 {n} 冲突` | 端口冲突 / `{port} 被 {app} 占用` |
| `lan` | `--net` | `新设备 {ip}` | 局域网 / `{host} 加入网络` |

**行为**：

- 出现：`translateY(-8px) + opacity 0 → 1`，200ms `cubic-bezier(.2,.8,.2,1)`
- 6s 自动收起（expanded → collapsed → 淡出）；hover 暂停计时并展开
- 点击 collapsed → expanded；点击 expanded 主体 → 跳主窗口对应 tab
- 连续同类事件：**替换内容并重置计时**，不排队堆叠
- `prefers-reduced-motion: reduce` → 只淡入淡出，不做位移

### 5.3 设置弹层（`SettingsBoard.tsx`）

| 项 | 规格 |
|---|---|
| 形态 | 居中浮层 `520×420`，`bg overlay`，`radius 14`，遮罩 `rgba(0,0,0,.4)` |
| 结构 | 左侧 124px 分区（通用 / 外观 / 通知 / 关于）+ 右侧内容 |
| 通用 | 开机启动、菜单栏图标、刷新间隔（下拉 2/5/10s） |
| 外观 | **主题：跟随系统 / 浅色 / 深色**（三态分段控件，v5 新增） |
| 通知 | 4 类灵动岛事件各一个开关（cpu/mem/port/lan）+ 阈值滑块 |
| 关于 | 版本、检查更新、开源许可 |
| 关闭 | `Esc` / 点遮罩 / 右上 `×` |

> 主题选择的落盘：`localStorage["prune.theme"]`，值 `system|light|dark`。`system` 时监听 `matchMedia('(prefers-color-scheme: dark)')` 变化并实时切换。**不新增 Rust command。**

---

## 6. 全局反馈与空态矩阵

### 6.1 错误横幅（顶栏下方，全局唯一）

```
高度 32，bg danger-soft，border-bottom 1px color-mix(danger 25%)，
左：警告图标 12 + 文案 10.5 danger（最多 1 行，超出省略 + title 全文）
右：btn-text「重试」 + btn-text「忽略」
```

| 触发 | 文案 |
|---|---|
| 内存快照失败 | `读取进程列表失败：{msg}` |
| 磁盘扫描失败 | `扫描失败：{msg}` |
| 端口读取失败 | `端口读取失败：{msg}` |
| 网络采样失败 | `网络采样不可用：{msg}` |

同一时刻只显示**最高优先级**一条（danger 顺序：内存 > 磁盘 > 端口 > 网络）。「忽略」仅本次会话生效。

### 6.2 四处空态统一

| 位置 | art | 标题 | 描述 | 动作 |
|---|---|---|---|---|
| 总览·未扫描 | `scan` | 还没有扫描过 | 扫描后可以看到本机可回收空间与六个资源域概况 | 立即扫描 |
| 应用·搜索无果 | `search` | 没有匹配「{q}」的应用 | 试试应用名的一部分，或清除搜索条件 | 清除搜索 |
| 存储·无回收项 | `no-record` | 扫描完成，没有发现可回收空间 | 当前已很干净；可以查看大文件与项目健康度 | 查看大文件 |
| 网络·无设备 | `no-device` | 局域网内没有发现其他设备 | 确认与本机处于同一网络，或重新扫描 | 重新扫描 |

### 6.3 空态演练开关（仅 dev）

顶栏放 `空态` 切换按钮（`import.meta.env.DEV` 才渲染），用于验收四种空态而不必构造真实条件。生产构建必须被 tree-shake 掉。

---

## 7. 浮层与状态机

### 7.1 层叠与焦点

| 规则 | 说明 |
|---|---|
| 一次只允许一个模态层 | 抽屉打开时不得弹确认层（确认层放抽屉内部） |
| 确认层永远挂 body | z=95，避免「嵌在卡片里别人看不见」 |
| 焦点陷阱 | 模态层内 `Tab` 循环；关闭后焦点回到触发元素 |
| `Esc` 优先级 | 菜单 > 确认层 > 抽屉 > 弹层；一次只关一层 |

### 7.2 清理抽屉（三段链路）

```
点「去清理」
  → 收集已勾选条目（级 × 条目双向同步后的最终集合）
  → 空集合：主按钮 disabled，文案「请选择要清理的项」
  → 打开抽屉：遮罩 160ms 淡入 + 面板 translateY(100%)→0 220ms
  → 存在 L4 项：显示警告块 + 二次确认勾选框（默认未勾选）
  → 点主按钮：按钮进入 loading（文案「清理中…」+ 禁点）
  → 完成：抽屉关闭 → toast「已清理 {size}，进入废纸篓」→ 触发重扫
  → 失败：抽屉不关，主按钮下方出现 9.5 danger 错误行 + 按钮恢复
```

**清理中禁止关闭抽屉**（遮罩点击与 `Esc` 均失效），避免「以为取消了其实还在删」的错觉。

### 7.3 结束进程（**两步**，不可简化）

```
右键进程行 → 上下文菜单（181px，z 92）
├─ 结束进程        SIGTERM
├─ 强制结束        SIGKILL（danger 色文字 + 右侧 kbd ⌥⌘K）
└─ 复制 PID
   ↓ 点击任一结束项
确认浮层（全局，z 95，340px 宽）
├─ 标题：结束「{name}」？
├─ 正文：PID {pid} · 占用 {mem}
├─ 说明（按类型）：
│    SIGTERM → 「先请求退出，未保存的更改可能丢失。」
│    SIGKILL → 「立即终止，不会给应用保存机会。」
├─ 行：次按钮「取消」 + 主按钮「结束进程 / 强制结束」（danger）
└─ 执行中：主按钮 loading，两步都不可重复点
```

**约束**：

1. 菜单项与确认按钮的动词必须一致（「强制结束」不能变成「结束进程」）
2. 成功后 toast「已结束 {name}」，并在 1500ms 后刷新进程列表（给系统回收时间）
3. 失败（`EPERM`）→ 确认层内联错误「权限不足，可能需要在系统设置中授权」+ 追加 `btn-text 用管理员身份重试`（若已有该能力；无则只提示）
4. **不允许**在菜单里一步执行——SIGTERM/SIGKILL 语义不同，合并即事故

### 7.4 右键路径菜单

| 场景 | 触发 | 菜单项 |
|---|---|---|
| 存储条目路径 | `contextmenu` on `[data-path]` | 在访达中显示 / 复制路径 |
| 应用工作区路径 | 同上 | 同上 |
| 进程表行 | `contextmenu` on `tr[data-pid]` | §7.3 菜单（优先级高于路径） |
| 聚合项（无单一路径） | 聚合行 | **不弹菜单**，`toast` 提示「聚合项没有单一路径：展开明细后右键单条可定位」 |

**判定顺序必须是**：`[data-path]` → `tr[data-pid]` → 聚合行。反了会导致进程行被路径菜单截胡。

### 7.5 键盘

| 快捷键 | 作用 | 作用域 |
|---|---|---|
| `⌘1`–`⌘6` | 切 tab | 全局（输入框聚焦时仍生效） |
| `⌘R` | 刷新当前页（按 tab 分流文案与 loading） | 全局 |
| `⌘F` | 聚焦当前页搜索框 | 有搜索的页 |
| `⌘,` | 打开设置 | 全局 |
| `Esc` | 关层 / 清空搜索 | 见 §7.1 优先级 |
| `↑/↓` | 列表/菜单移动 | 列表与菜单聚焦时 |
| `Enter` | 下钻 / 执行 | 同上 |
| `⌥⌘K` | 强制结束选中进程 | 进程列表聚焦时 |

刷新语义分流（顶栏按钮文案与 loading 必须与 tab 对应）：

| tab | 按钮文案 | loading 文案 |
|---|---|---|
| 总览 | 重新扫描 | 扫描中… |
| 应用 | 刷新 | 刷新中… |
| 内存 | 刷新 | 刷新中… |
| 存储 | 重新扫描 | 扫描中… |
| 网络 | 采样 | 采样中… |
| 端口 | 扫端口 | 扫描中… |

---

## 8. 文案规范

### 8.1 口径文案白名单（`ScopeBadge` / tooltip 唯一来源）

| key | 展示短文案 | tooltip 全文 |
|---|---|---|
| `scope.footprint` | footprint | 包含常驻内存与已换出部分，macOS 活动监视器的「内存」列同口径 |
| `scope.rss` | RSS | 仅常驻物理内存，不含换出；进程刚启动时数值偏低属正常 |
| `scope.cpu.total` | 整机均值 | 整机每核平均 0–100%，与进程单核口径不同，**不能相减** |
| `scope.cpu.proc` | 单核 | 单进程占单核 100% 即 100%，多线程可超过 100% |
| `scope.net.system` | 系统口径 | 网卡层统计，含所有进程与系统服务 |
| `scope.disk.volume` | 卷容量 | 按卷统计，含快照与系统保留空间 |

> 原则：**任何会被用户拿来相减/相加的两个数字，必须能点开看到口径差异**。

### 8.2 禁用词与禁用承诺

| 禁止 | 原因 | 替换 |
|---|---|---|
| 「7 天可撤销」 | 实现没有 | 「删除后进入废纸篓，可自行恢复」 |
| 「一键释放 XX GB」 | 夸大（L4 需确认） | 「可回收 {size}（其中 {l4} 需确认）」 |
| 「暂无数据」 | 混淆「没扫到」与「没有」 | 分两态写（§6.2） |
| 「立即优化」 | 无对应动作 | 指向具体 board 的具体动作 |
| 「深度清理」「极致加速」 | 营销腔，与 Quiet Cockpit 调性冲突 | 用数量与位置描述 |

### 8.3 i18n 约束

- 新增文案一律进 `src/i18n/`，**不写死在组件里**（`RailNav` 的 `ITEMS` 是模块级常量，只能存 key）
- 英文长度按中文 ×1.6 预留：`nowrap` 元素（按钮、徽标、表头）必须实测英文不溢出
- 数字与单位之间留 `1px` 空隙，不做空格（中英混排下空格会破坏 tabular 对齐）

---

## 9. 文件级改造清单

> 规模为「净增/改写行」估算，用于排期与 review 分片。**按 P0→P3 顺序改**，每阶段结束跑 `pnpm exec tsc --noEmit`。

### 9.1 P0 · 地基（必须先完成，否则后续全部返工）

| 文件 | 改动 | 规模 |
|---|---|---|
| `src/index.css` | 补 §2.2 的 12 个 token；加 `:root[data-theme="light"]` 块；加 `@media (prefers-reduced-motion)` 兜底；滚动条 token 化 | +70 |
| `src/components/primitives.tsx` | `Badge` 扩 `cpu/port`；新增 `ScopeBadge`、`ProposalTag`、`Card`（4 变体）、`Btn`（4 变体）、`EmptyState`、`Toast`、`PopMenu` | +320 |
| `src/components/EmptyArt.tsx` | 4 个 art 变体（scan/search/no-record/no-device），统一 64×64 viewBox | +80 |
| `src/lib/theme.ts` | **新增**：`getTheme/setTheme/applyTheme`，localStorage + matchMedia 订阅 | +45 |
| `src/components/RailNav.tsx` | 域色映射补 `--color-domain-port`；竖条改 `2.5px` + 发光；hover 恢复 `title` 双段文案；选中态用域色 | ~40 改写 |
| `src/windows/MainWindow.tsx` | 顶栏重排（标题/副标/pill/按钮组）；错误横幅；刷新语义分流；`空态` dev 开关 | ~120 改写 |

**P0 完成判据**：主窗口 6 tab 都能切换，每页首屏结构与原型一致（内容可临时沿用旧实现），无 TS 错误，无控制台报错。

### 9.2 P1 · 主路径三页（覆盖 80% 使用时长）

| 文件 | 改动 | 规模 |
|---|---|---|
| `src/components/StatsBoard.tsx` | Hero 四段弧 + 2×2 图例 + 未扫描态；六域卡重排（拆下载/上传 + 端口点阵）；成绩单空态 | ~260 改写 |
| `src/components/stats/RingGauge.tsx` | 单色 → 四段弧（`segments: {value,color}[]`），中央 slot | +60 |
| `src/components/stats/DomainCard.tsx` | 值 19/600 + caption 移入 title + 单行脚注 | ~35 改写 |
| `src/components/stats/StatsRow.tsx` | 统一 6 卡网格（3/2 列断点） | ~40 改写 |
| `src/components/AppResourceGrid.tsx` | **保持卡片网格形态**（§4.2 纠偏：不是表格）；卡内四行字段分层、副行降噪、搜索匹配 `textContent`、hover 下钻指示 | ~120 改写 |
| `src/components/ReclaimBoard.tsx` | 三段链路（分级构成 → 分组预览 → 抽屉）；分级条 `gap` 修溢出；右键菜单改 `PopMenu`；L4 二次确认；抽屉内逐条取消勾选 | ~300 改写 |
| `src/stores/diskStore.ts` | **唯一例外**：`execute(confirmed)` → `execute(confirmed, items?)`。抽屉里取消勾选后要能只清理子集；不传时行为不变（向后兼容），state 形状未变 | +6 |
| `src/components/appdetail/AppDetail.tsx` | 概览格 3 列 + 三轨历史联动 + 泄漏卡四态 | ~140 改写 |

**P1 完成判据**：总览→存储→清理抽屉→完成 toast 全链路可走通；应用搜索「chrome」有结果；存储分级条不溢出；抽屉在 720×480 下不遮挡主按钮。

### 9.3 P2 · 剩余三页 + 系统壳

| 文件 | 改动 | 规模 |
|---|---|---|
| `src/components/PortList.tsx` | 5 列 grid 重排（70/74/216/1fr/56）；暴露面徽标；无权限横幅；定位按钮 | ~150 改写 |
| `src/components/NetBoard.tsx` | 速率三卡 + 网卡表 6 列 + 三层诊断 + 四态采样 + 橙条 | ~280 改写 |
| `src/components/LanBoard.tsx` | 5 阶段进度条；设备网格 2 列；空态 | ~120 改写 |
| `src/components/LanDeviceCard.tsx` | 卡片密度统一（名/ip/厂商/最后出现） | ~60 改写 |
| `src/components/LeakCard.tsx` | 四态（含「判定不可用」）；证据区折叠；徽标 nowrap | ~80 改写 |
| `src/components/ProjectHealthCard.tsx` | 版式对齐（hairline + insets） | ~35 改写 |
| `src/panels/MenuBarPanel.tsx` | 7 段结构重排；不滚动约束；泄漏条条件渲染 | ~180 改写 |
| `src/panels/Island.tsx` | 4 类事件 × 2 态几何统一；替换式更新；reduced-motion | ~200 改写 |
| `src/components/SettingsBoard.tsx` | 左分区右内容；新增「外观·主题三态」 | ~90 改写 |

**P2 完成判据**：菜单栏面板 380×432 内 7 段不滚动；灵动岛 4 类事件各触发一次，6s 自收；端口表 900 宽无空白列。

### 9.4 P3 · 收尾

| 文件 | 改动 | 规模 |
|---|---|---|
| `src/i18n/*` | 新增 key（约 60 条）+ 英文长度回归 | +140 |
| `src/index.css` | 浅色主题对比度逐项实测、暗色残留硬编码色清理 | +60 |
| 全量 | 键盘可达性、focus ring、`aria-*` 补齐 | 分散 |

### 9.5 明确不动

```
src-tauri/**
src/lib/ipc.ts            （签名不变；仅在调用处做兜底 try/catch）
src/types/**              （字段不改）
src/stores/**             （state 形状不改）
vite.config.ts / package.json
```

---

## 10. 分阶段交付与门禁

| 阶段 | 内容 | 门禁（缺一不可） |
|---|---|---|
| **P0** | token + 基础组件 + 壳骨架 | `tsc --noEmit` 0 error；`pnpm build` 通过；6 tab 可切换 |
| **P1** | 总览 / 应用 / 存储 / 应用详情 | 上述 + 清理全链路走通 + 960×640 截图比对通过 |
| **P2** | 网络 / 端口 / 面板 / 岛 / 设置 | 上述 + 380×432 与岛两态截图通过 |
| **P3** | i18n / 浅色 / a11y | 上述 + 浅色对比度表实测 + 键盘全流程无死点 |

**每阶段提交粒度**：一个阶段拆 2–4 个 commit（`feat(ui): ...`），每个 commit 自身可编译可运行。**不允许**「一个 commit 改 15 个文件」——UI 回归定位成本会爆炸。

**视觉验收方式**（不靠肉眼记忆）：

```bash
pnpm dev                                   # vite 1430
# 浏览器打开 http://localhost:1430?window=main
# 与 docs/ui-v5-prototype.html 同屏对比，或按 §11.2 逐项过
```

---

## 11. 验收标准

### 11.1 反例清单（照做即失败）

| # | 反例 | 为什么致命 |
|---|---|---|
| 1 | 把 1160 原型等比缩到 960 | 正文变 9px，可读性崩溃 |
| 2 | 给 L1–L4 画勾选框并「一键清理」 | 真实是「分级构成 → 分组预览 → 抽屉确认」三段，勾选框只用于选预览范围 |
| 3 | 写「7 天可撤销」 | 实现只有废纸篓恢复，属于虚假承诺 |
| 4 | 结束进程一步执行 | SIGTERM/SIGKILL 语义不同 |
| 5 | 网络诊断画四层 | 真实三层，captive portal 只是 L3 的附加提示 |
| 6 | 空态统一写「暂无数据」 | 用户无法判断该等还是该点 |
| 7 | 聚合项伪造单一路径 | 数据是聚合的，右键应提示展开明细 |
| 8 | 分段条用 border 分隔 | 总长溢出容器（L1 满条事故） |
| 9 | 徽标不加 `flex-shrink:0` | 窄列换行，卡片高度跳变 |
| 10 | 确认层嵌在卡片内 | 其他入口触发时不可见 |
| 11 | 搜索只匹配标题 | 搜 `chrome` 无结果 |
| 12 | 泄漏卡只画「疑似/正常」两态 | 缺「判定不可用」，用户误以为「没提示=没泄漏」 |
| 13 | 新菜单复用 `.ctx` 类名 | 继承 `column` 布局，高度翻倍 |
| 14 | 历史三轨各自 auto-scale | 会读出「内存与 CPU 同时上涨」的错误结论 |
| 15 | 数字不加 `tabular-nums` | 每秒刷新的数值左右抖动 |

### 11.2 逐项验收表

**壳与导航**

- [ ] 6 个 rail 图标，hover 有 `页面名 · 说明` tooltip
- [ ] 选中态：域色图标 + 左侧 2.5px 发光竖条（五域色各不相同）
- [ ] `⌘1`–`⌘6` 切换，顺序为 总览/应用/内存/存储/网络/端口
- [ ] 顶栏刷新按钮文案随 tab 变化（6 套），loading 文案同步
- [ ] 错误横幅只显示最高优先级一条，可「重试」「忽略」
- [ ] 齿轮打开设置，`Esc` 关闭

**总览**

- [ ] 未扫描：环为 4 段淡色占位 + 中央 `—` + 「立即扫描」，点按钮不下钻
- [ ] 有数据：环四段角度之和 = 360，L4 段为斜纹且不计入总量
- [ ] 图例 4 项数值右对齐、tabular
- [ ] 六域卡齐备：内存/CPU/硬盘/端口/下载/上传
- [ ] 无剪除记录时空态可点「去存储看看」并切 tab

**应用**

- [ ] 搜 `chrome` / `Chrome` 均命中（匹配整卡文本，不是只匹配标题）
- [ ] 排序切换为全局单选，当前维度有域色圆点
- [ ] 卡片四行字段（CPU/内存/磁盘/网速）对齐，值列右对齐 tabular
- [ ] 网速未采样显示「尚未采样」，不画 0 长度条
- [ ] 点卡片进详情，卡内无展开行

**内存**

- [ ] 口径切换 footprint/RSS，选中的口径同步到概览格与列表
- [ ] 三轨共用 x 轴；切 30天/7天/24h 重新拉取
- [ ] 数据点 <3 显示「样本不足（n/3）」，不插值
- [ ] 泄漏卡四态均可复现（含「判定不可用」）

**存储**

- [ ] 卷卡 Meter 与「可用/共」数值一致
- [ ] 分级条 4 段有 2px 间隙，总宽 = 容器宽（不溢出）
- [ ] 点级展开条目；取消条目后级的总数同步减少
- [ ] 抽屉：空选主按钮 disabled；L4 存在时需二次勾选
- [ ] 清理中不可关闭抽屉；完成 toast 后自动重扫
- [ ] 右键路径：两项齐备；复制失败在菜单内报错且不关闭

**网络**

- [ ] 三卡：下载/上传/7 天流量
- [ ] 网卡表速率列「12.4 MB/s」不换行
- [ ] 诊断三层，captive portal 为 L3 附加提示
- [ ] 局域网扫描显示 5 阶段文案与百分比
- [ ] 不可读时出现橙色横幅 + 重试

**端口**

- [ ] 监听/连接子 tab
- [ ] 暴露面三色徽标（公网/局域网/本机）
- [ ] 端口列 70px 不换行；进程/项目列无大片空白
- [ ] 无权限时 info 横幅

**系统壳**

- [ ] 菜单栏面板 7 段齐全，432 高内不出现滚动条
- [ ] 灵动岛 4 类事件各触发一次，几何一致，6s 自收，hover 暂停
- [ ] 设置内可切三态主题；重启后保持（localStorage）

**无障碍与动效**

- [ ] 所有交互元素 `:focus-visible` 有 2px accent 环
- [ ] 模态层焦点陷阱生效，关闭后焦点回到触发元素
- [ ] `prefers-reduced-motion: reduce` 下无位移动画
- [ ] 浅色主题：正文对比度 ≥4.5:1（`#0e9e6e`/`#5b656f` 达标，`#8b949e` 仅注释级）

### 11.3 三档尺寸回归

| 尺寸 | 必测项 |
|---|---|
| 960×640（默认） | 全部验收项 |
| 720×480（min） | 无横向滚动条；抽屉主按钮可见；表格不塌列 |
| 380×432（面板） | 7 段不滚动；CTA 不被裁 |

---

## 12. 风险与回滚

| 风险 | 触发条件 | 缓解 |
|---|---|---|
| 组件改写引入回归 | P1 一次改 `ReclaimBoard` 300 行 | 先补最小快照测试（渲染不崩 + 关键文案存在），再改 |
| store 耦合 | 版式改动顺手「优化」了 selector | 禁止在 P0–P2 动 `stores/*`；如需新增派生值，写在组件内 `useMemo` |
| i18n 键膨胀 | 新文案硬编码 | code review 卡「组件内出现中文字符串」 |
| 浅色主题残留硬编码 | 组件里写 `#fff` / `rgba(255,255,255,...)` | P3 用 `grep -rn "rgba(255" src` 清零；`bg-white/` 改为 `--chip` |
| 性能 | 进程列表 500+ 行全量渲染 | 沿用现有虚拟滚动；新表格不得引入「每行一个 effect」 |
| 后端被顺手改 | 「这里加个 command 更干净」 | §0.2 冻结项；PR 描述必须写「Rust 未改动」 |

**回滚策略**：P0–P3 各自独立 commit 段，任一阶段出问题 `git revert` 该阶段全部 commit 即可回到上一可用态；token 采用「只增不删」，故旧组件在回滚后仍可正常渲染。

---

## 13. 交付物清单

| 文件 | 状态 | 说明 |
|---|---|---|
| `docs/ui-v5-prototype.html` | 已有 | 视觉基准，浏览器直接打开 |
| `docs/ui-v5-feature-checklist.md` | 已有 | 112 项功能核对（源码行号级） |
| `docs/ui-v5-implementation-spec.md` | **本文** | 实现规格：token / 组件 / 六页 / 壳 / 状态机 / 验收 |
| `docs/ui-v5-feature-gap-review.md` | 已有 | 第一轮覆盖度评审 |

---

**规格结束**。执行顺序：§9.1 → §9.2 → §9.3 → §9.4，每段过 §10 门禁；验收按 §11.2 逐条勾，任一项未勾不得进入下一阶段。




---

## 12. 落地纠偏记录（P0 / P1 / P2 实测）

原型到实现之间必有落差。以下每条都在真机/浏览器实测后修正，**以本表为准**，勿照抄前文表格：

| 章 | 原文 | 实测修正 | 依据 |
|---|---|---|---|
| §4.2 应用 | 7 列表格 | **卡片网格**（每应用一张卡，含四条不同量纲条形） | `AppResourceGrid` 现有实现；表格装不下四量纲 |
| §4.4 存储 | 勾选分级 → 清理 | 级行**只读**，勾选单位是**分组**；L1 是 Yarn 缓存内的死重切片，不是目录 | `report.totals` 按条目记账 |
| §3.6 Meter | 超阈值自动升档 | **撤销**：`bytes/max` 的相对长度会被误判成占用率（L2 满条转红） | 条有两种语义，组件猜不出 |
| §3.6 TrackProgress | 分段条用 border 分隔 | 用 `gap` + 圆角段，border 会在 3px 高度上吃掉 1px | 视觉实测 |
| §5.1 面板 | 六行 6×26 | **6×40**：CTA 提到第 2 位后 432 装不下 6×48，第 6 行被挤出可视区 | 380×432 实测溢出 |
| §5.1 面板 | 按应用 Top3 | **不做**（432 高度预算已满）；保留「应用 N ›」入口 | 同 spec 允许「内容超出只裁 Top3」 |
| §5.2 灵动岛 | 168×32 / 320×96 | **229×50 / 380×180**：岛是 Rust 侧真实窗口，尺寸受刘海几何约束（冻结区） | `island_set_expanded` 由后端改窗口 |
| §4.5 网卡表 | 名 / IP / 状态 / ↓ / ↑ / 累计 | **无 IP 与状态列**：`IfaceRate` 不透出，按降级原则不造假列 | `types/net.ts: IfaceRate` |
| §4.6 端口表 | 端口 / 协议+暴露 / 进程+项目 / 地址 / PID | **端口 / 暴露 / 进程 216 / 项目 1fr / 连接 56**；无地址与 CMD 字段 | `PortEntry` 无地址 |
| §5.3 设置 | 左 124px 分区导航 | **单列滚动卡**（480→520 居中）：分区只有语言/外观/灵动岛三类，左侧导航为空转 | 分区数不足 |

另：`--color-overlay` 在 P0 漏建，P2 使用 `bg-overlay` 时补入 index.css（深 `#191e24` / 浅 `#ffffff`）。

---

## 13. 落地纠偏记录（P3 实测）

| 章 | 原文 | 实测修正 | 依据 |
|---|---|---|---|
| §9.4 硬编码色 | `bg-white/` 清零 | **三档中性面**：卡面 0.02–0.035 → `chip-soft`，静态填充 0.04–0.06 → `chip`，hover/focus 与 0.08–0.1 → `chip-hover` | 原值有 8 档 alpha，两级装不下；新增 `--color-chip-soft` 属「只增不删」 |
| §2.3 浅色 | `--chip` 两种 | **三档同步翻转**（`--color-chip-soft` 浅色 = `rgba(0,0,0,.03)`） | 浅色下卡面需要比 hover 更淡的一层 |
| §9.4 硬编码色 | 组件内 grep `rgba(255` 清零 | 灵动岛 **17 处白色系**不参与主题翻转（岛是恒定深玻璃），提取为 `--island-hairline / ink / ink-dim / ink-mute / ink-faint` 五个主题无关变量 | 岛悬浮于壁纸，浅色主题下也得是深玻璃 |
| §2.1 强调色 | `bg-accent` + 白字 | **`text-accent-ink`**：深色 `#34d399` 上白字仅 1.98:1；浅色 `#0e9e6e` 上白字 4.52:1 才反过来需要白 | 对比度实测 |
| §2.1 危险色 | `bg-danger` + 白字 | **`--color-danger-ink`**：深色 `#f0705f` 配暖黑 `#2a1512`（≈6:1），浅色 `#c94f40` 配白（4.9:1） | 单靠白字在深色下达不到 AA |
| §11 a11y | 分散补 focus ring | **全局 `:focus-visible` 一条**（`@layer base`，accent 70% 2px）：输入框的 `focus:outline-none` 仍可覆盖，避免环与描边双重指示 | 逐组件补会漏；放 base 层让工具类优先 |
| §11 a11y | — | RailNav 6 个 tab 原先只有 `title`（读屏不读），补 `role=tablist` 父级 + 逐项 `aria-label` | `document.activeElement` 扫描出无名按钮 |
| §9.4 i18n | 新增 60 条 + 英文回归 | 实际词条已在 P0–P2 随手补齐；**英文回归结论：380×432 面板 scrollHeight = clientHeight = 432，无溢出**；960 主窗无裁切元素 | 实测 |

**P3 门禁结果**：`tsc --noEmit` 0 error；`pnpm build` 6.2 s 通过；`check:i18n` 无硬编码中文；浅色 960×640 与 380×432 截图通过；键盘 Tab 全程有焦点环、6 页无名称按钮 0 个。
