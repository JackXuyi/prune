# 组件契约（真实 API，照抄可用）

**先查这里有没现成的**。重画一份「看起来差不多」的组件是返工的最常见原因。

## 1. `Ribbon` — 顶部状态带

`src/components/Ribbon.tsx`

```tsx
import { RibbonShell, RibbonSeg, RibbonChip, RibbonValue } from "./Ribbon";
import { MiniGauge } from "./stats/MiniGauge";

<RibbonShell ariaLabel={t("stats.ribbonAria")}>
  {/* 百分比位：环嵌图标 */}
  <RibbonSeg title={t("stats.rim.cpuTip", { pct: 12 })} onClick={() => onDrill("resources")}>
    <MiniGauge ratio={0.12} color="var(--color-domain-cpu)" size={26} icon={<ChipIcon size={12} />} />
    <RibbonValue value="12" unit="%" />
  </RibbonSeg>

  {/* 非百分比位：图标芯片 */}
  <RibbonSeg title={t("stats.rim.downTip")}>
    <RibbonChip color="var(--color-domain-net)" icon={<DownloadIcon size={13} />} />
    <RibbonValue value="3.2" unit="MB/s" />
  </RibbonSeg>
</RibbonShell>
```

| Props | 说明 |
|---|---|
| `RibbonShell.ariaLabel` | 必填（整条带的读屏名） |
| `RibbonSeg.title` | 必填，完整口径（换算、时间窗、阈值） |
| `RibbonSeg.onClick` | 给了才渲染 button（导航 / 切排序） |
| `RibbonChip.color` | 传 `var(--color-*)`，不要传裸色值 |
| `RibbonChip.glow` | 域色微光晕，只在「排序透镜」段用 |
| `RibbonValue.tone` | `warn` / `danger` 只改颜色，tooltip 里必须有明文 |

**别名**：应用页封装成 `AppSummaryRibbon`（`src/components/resources/`），内存页可照抄一份 `MemSummaryRibbon`。

## 2. `MiniGauge` — 环形仪表

`src/components/stats/MiniGauge.tsx`

```tsx
<MiniGauge ratio={0.62} color="var(--color-info)" size={24} stroke={2.4} icon={<MemoryCloudIcon size={12} />} />
```

- `ratio: number | null` —— **null 只画空轨道**（读不到时绝不画 0 弧）。
- `size` 24（磁贴）/ 26（状态带）；`stroke` 2.2–2.4；图标约 `size × 0.45`。
- 三通道分工：环=量感、环心图标=语义、旁侧 mono 数字=精确值。**三者缺一就别用环**。

**什么时候不用**：速率（MB/s）、计数（端口数）、温度（℃）、字节数（GB）——这些没有「占满」语义。

## 3. `InfoTile` — 磁贴

`src/components/stats/InfoTile.tsx`

```tsx
<InfoTile
  title={t("stats.tile.mem")}
  caption={t("stats.tile.memTip", { swap: "1.8/6.0 GB" })}   // 完整口径
  color="var(--color-info)"
  icon={<MemoryCloudIcon size={12} />}
  gaugeRatio={0.62}                                            // 传了才有环
  main={{ value: "62", unit: "%", tail: "20.5 / 32 GB" }}
  sub={t("stats.tile.memSub", { swap: "1.8/6.0 GB" })}         // 一行口径
  viz={<CellBar ratio={0.62} tone="mem" />}                    // 卡底微图表
  onOpen={() => onDrill("memory")}                             // 只导航，不做清理
/>
```

| 规则 | 说明 |
|---|---|
| 高度 | `h-full min-h-[104px]`，由网格单元决定，**不写死** `h-[104px]` |
| 文字层级 | 标题 11px dim → 读数 23px mono → 口径 10px faint；第四层进 tooltip |
| `main` 缺省 | 显示 `note`（原因），绝不显示 0 |
| 装饰 | 只有顶缘洗光 + 底缘发丝线，别加阴影堆叠 |
| 空卡禁令 | 标题与口径行不能同时省（用户反馈过「卡片太空」） |

## 4. `CellBar` — 比例条

`src/components/CellBar.tsx`

```tsx
<CellBar ratio={mem / denom} tone="mem" show={denom > 0} />
```

- `tone`：`mem`（蓝）/ `cpu`（紫）/ `net`（青）/ `warn` / `danger` / `neutral`。
- `show=false` 时不画填充（分母未知 / 读数缺失），**不画 0 条**。

## 5. `AppIcon` — 应用图标

```tsx
<AppIcon family={family.name} icon={icons[family.name]} size={22} />
```

- 三层兜底：真实图标 → 字母徽标（家族哈希渐变）→ 仍是徽标。**永不显示占位灰图**。
- `size` 只有 `28 | 64` 两个合法值（踩过坑：传 22/30 会 TS 报错，需要扩展类型再改）。
- 字母前景走 `var(--color-badge-ink)`（徽标渐变是固定材质，不随主题翻转）。

## 6. `Sparkline` — 趋势线

```tsx
<Sparkline values={history} tone="warn" height={22} baselineZero />
```

- 颜色走 CSS 变量（`var(--color-warn)`），不要传裸 hex——浅色主题会失真。

## 7. 行式组件（表格 / 列表）

### 列头

```tsx
<div className="grid grid-cols-[minmax(0,1fr)_58px_62px_66px_46px] gap-2 px-1.5 text-[9px] text-ink-faint text-right">
  <span className="text-left">{t("memory.victims.col.proc")}</span>
  <span>{t("memory.victims.col.ratio")}</span>
  ...
</div>
```

### 行

```tsx
<div className="grid grid-cols-[...] items-center gap-2 rounded-[7px] px-1.5 py-[5.5px] text-right hover:bg-chip">
  <span className="flex min-w-0 items-center gap-1.5 text-left">
    <b className="truncate text-[11.5px] font-medium text-ink">{name}</b>
    <span className="shrink-0 text-[9px] text-ink-faint">{family}</span>
  </span>
  <span className="h-[3px] overflow-hidden rounded-full bg-chip"><i className="block h-full bg-warn" style={{ width }} /></span>
  <span className="font-mono text-[10.5px] tabular-nums text-ink-dim">{bytes}</span>
</div>
```

- 行高：表行 28px / 密行 24px；hover `bg-chip`；选中 `bg-accent/[0.10]`。
- 名称 `truncate`、数值右对齐 + `tabular-nums`、单位 9–10px faint。

## 8. 徽标 / 状态点

```tsx
<Badge tone="accent" title={...}>{t("memory.badgeFootprint")}</Badge>
```

- `Badge` 在 `src/components/primitives.tsx`；口径徽标必须带 `title` 说明含义。
- 状态点用 9px 方块（`rounded-[3px]`）+ 域色，不用 emoji。

## 9. 新增组件前自检

1. `grep -rn "RibbonShell\|MiniGauge\|InfoTile\|CellBar" src/components` 有没有现成形态？
2. 只是换个域色 / 换个数值 → **传 props，不要新组件**。
3. 确实是新形态（如构成堆叠条、电平条）→ 放 `src/components/stats/`，并在本文补一节。
