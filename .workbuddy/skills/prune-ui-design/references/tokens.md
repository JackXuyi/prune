# 令牌与字号（引用自 src/index.css + v5 §2）

**不要在组件里写色值。** 一律 `var(--color-*)` 或 Tailwind 语义类（`bg-surface-raised`、`text-ink-dim`）。
门禁 R2 会拦截硬编码的主题令牌十六进制。

## 1. 面与文字

| 令牌 | 深色 | 浅色 | 用途 |
|---|---|---|---|
| `--color-surface` | `#0e1114` | `#f4f6f8` | 窗口底 |
| `--color-surface-raised` | `#14181d` | `#ffffff` | 卡片 / 带子 |
| `--color-surface-sunk` | `#0b0e11` | `#eef1f4` | 侧栏（左栏列表） |
| `--color-overlay` | `#191e24` | `#ffffff` | 弹层 |
| `--color-hairline` | `#242a32` | `#dfe5ea` | 主分隔 |
| `--color-hairline-soft` | `#1c2128` | `#e8edf1` | 发丝线（卡片边） |
| `--color-ink` | `#e8ebee` | `#171c21` | 主文字 |
| `--color-ink-dim` | `#99a1aa` | `#5b656f` | 次文字 / 标题 |
| `--color-ink-faint` | `#656d77` | `#8b949e` | 口径 / 脚注 |
| `--color-chip` | `rgba(255,255,255,.045)` | `rgba(0,0,0,.045)` | 芯片底 / 轨道 |
| `--color-chip-soft` | `.028` | `.028` | 卡片顶缘渐变起点 |

## 2. 语义色

| 令牌 | 深色 | 浅色 | 语义 |
|---|---|---|---|
| `--color-accent` | `#34d399` | `#0e9e6e` | 品牌 / 可回收 / 安全 |
| `--color-warn` | `#f5b74a` | `#b97a12` | 需注意（阈值 75%） |
| `--color-danger` | `#f0705f` | `#c94f40` | 危险（阈值 90%） |
| `--color-info` | `#6aa9ff` | `#2273cc` | 内存域 |
| `--color-domain-cpu` | `#a78bfa` | `#7c5cd6` | CPU |
| `--color-domain-net` | `#22d3ee` | `#0e9cba` | 网络 |
| `--color-domain-port` | `#f2a35c` | `#b87333` | 端口 |
| `--color-domain-device` | `#f072a8` | `#bf4f87` | 设备 |

**浅色主题的 accent 不是深色值**：`#34d399` 在白底只有 1.98:1，所以浅色换成 `#0e9e6e`（4.52:1）。
新增任何颜色，必须同时给 `:root[data-theme="light"]` 覆盖。

## 3. 字号阶梯

| 角色 | size | weight | color | 用例 |
|---|---|---|---|---|
| 页名 | 13.5 | 600 | ink | topbar |
| Hero 数字 | 25–28 | 600 | ink / 域色 | 环心 |
| 卡内大读数 | 23 | 600 | ink | `InfoTile` main |
| 带内读数 | 17 | 600 | ink | `RibbonValue` |
| 表内主值 | 12 | 500 | ink | 行标题 |
| 卡片标题 | 11 / 11.5 | 500–600 | dim | `InfoTile` title |
| 正文 | 11.5 | 400 | dim | 说明 |
| 字段标签 | 10.5 | 400 | faint | 列头 |
| 口径行 | 10 | 400 | faint | 读数下方 |
| 徽标 / 脚注 | 9.5 | 400–500 | faint | **下限** |

> 9.5px 是下限（v5 教训：所有「看不清」的抱怨都在 9px 以下）。
> 现有 7–9px 属历史遗留（灵动岛 / 图标内），新代码禁止。

## 4. 圆角与间距

| 令牌 | 值 | 用途 |
|---|---|---|
| `--radius-hero` | 18px | Hero 主卡 |
| `--radius-card` | 13px | 卡片 / 磁贴 |
| `--radius-pop` | 8px | 菜单内块 / 带内芯片 |
| `--radius-ctl` | 7px | 按钮 / 输入 |

- 带子外壳实测 14px（`RibbonShell` 写死 `rounded-[14px]`，比卡片大一号）。
- 区块间距 9–10px；卡内 padding 10–14px；网格 gap 10px。
- 段间分隔一律 `divide-x divide-hairline-soft` / `border-hairline-soft`，不要用背景色块硬切。

## 5. 阴影与 z-index（v5 §2.7 硬约定）

| 层 | z | 备注 |
|---|---|---|
| 内容 | 0 | |
| sticky 操作栏 | 30 | |
| 下拉 | 70 | |
| 右键菜单 | 92 | |
| 抽屉 | 82 | 遮罩 80 |
| 确认浮层 | 95 | **一律挂 body** |
| toast | 100 | |

阴影只用于浮层与卡片 hover（`hover:shadow-lg hover:shadow-black/10`）。
**不做**多层投影、发光边框堆叠、玻璃拟态——「安静驾驶舱」不允许视觉噪音。

## 6. 数字排版

- 数值：`font-mono` + `tabular-nums`（缺 tabular 每帧会跳）。
- 单位：`<small>` / `text-[10px] text-ink-faint`，不进等宽串。
- 缺失：`—`（em dash）；负号：`−`（U+2212）；增速前缀 `+`。
- 字节/速率一律走 `formatBytes` / `formatSpeed`，组件里不拼单位。
