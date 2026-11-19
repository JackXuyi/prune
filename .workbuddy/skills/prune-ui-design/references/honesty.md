# 假事实红线（No fake facts）

Prune 的立身之本是「读数可信」。**任何一个漂亮的 0 都在透支这个信任。**
以下四条没有例外，也不需要「这次先这样」。

## 1. 读不到 ≠ 0

读数层 `src/lib/statsReadout.ts` 的 `Readout.status` 已区分 `ok` / `stale` / `error` / `unsupported`。
展示层必须消费 `status`：

```tsx
// ✅ 读不到就写原因
{readout.status !== "ok" ? (
  <p className="truncate text-[11px] text-ink-faint" title={readout.note}>
    {readout.note ?? "—"}
  </p>
) : (
  <Reading size="md" readout={readout} />
)}

// ❌ 把 undefined 当 0 渲染
<span>{formatBytes(readout.value ?? 0)}</span>
```

`MiniGauge` 已内建：`ratio = null` 只画空轨道，不画 0 弧。

## 2. 失败即隐藏（不是占位）

平台没有这个口径（Windows 无压缩内存 / 无换页计数 / 无 SMC 温度），**整段不渲染**：

```tsx
{swap.total > 0 ? <SwapSection /> : null}
{hasSwapMetrics ? <ThrashSection /> : null}     // thrash == null → 整块消失
{compressorBytes != null ? <Stat … /> : null}
```

反面：显示「不可用 / — / N/A」占位——用户会以为功能坏了，或以为值为 0。

## 3. 推导值必须标注

算出来的数字不能和实测长得一样：

| 类型 | 例子 | 画法 |
|---|---|---|
| 实测 | 物理内存、swap 已用、进程 footprint | 实色 |
| 推导 | 「缓存·可回收」= 总量 − 已用 − 可用 | 斜纹 `repeating-linear-gradient(45deg, var(--color-hatch) 0 2px, transparent 2px 4px)` + tooltip 写明「推导而非实测」 |
| 下界 | `deepCovered < processCount` 的家族总量 | 前缀 `≈` 或 `≥` |

同理：共享内存的「内存合计」是 footprint 求和，会在 tooltip 里注明「共享页重复计入」。

## 4. 口径进 tooltip

版面只放**发生了什么**，`title` 放**怎么算的**：

```tsx
<RibbonSeg title={t("memory.rim.usedTip", { used: "20.5 GB", total: "32 GB" })}>
```

必须进 tooltip 的信息：

- 换算（kernel 1/4 档 vs occupancy %）
- 时间窗（近 10 min / 24 h / 窗口 30 min）
- 分母（比例条的分母是「全表最大值」还是「总量」）
- 推导公式（缓存 = 总量 − 已用 − 可用）
- 平台差异（macOS Pages free vs Windows AvailPhys）
- 阈值（swap 75% 黄 / 90% 红）

## 5. 空态与未就绪

| 状态 | 文案 | 禁止 |
|---|---|---|
| 首次采样中 | `common.progress.sampling`（「采样中…」） | 显示 0 |
| 未扫描 | 「尚未扫描」 | 显示「无端口」 |
| 确实没有 | 「无」/ 空态插画 | 显示「—」（— 的含义是读不到） |
| 无法判定 | 原样显示 `note` | 当成「正常」 |

`LeakReport.note` 非空 = 无法判定，**不能**当成「没有泄漏」。

## 6. 动作类谎言

- 按钮没有真实语义就别放（设备页曾挂了一个「刷新」但什么都不刷）。
- 「结束进程」类操作必须走确认浮层（z=95，挂 body），不允许卡片内联展开。
- 清理动作只允许从对应域页面发起，总览/磁贴只导航——**下钻不做动作**。
