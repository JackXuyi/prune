---
name: prune-ui-design
description: Prune 桌面端 UI 的设计语言（eul 式图标优先）工作流。当需要新增或改造 Prune 页面（总览/应用/内存/存储/网络/端口/设备）、产出 HTML 设计稿、按稿落码、复用组件（Ribbon/MiniGauge/InfoTile）、或审查 UI 是否符合设计约束时使用。触发词：设计稿、重设计、按首页风格、图标优先、视觉统一、UI 规范、设计约束、组件复用。
---

# Prune UI 设计（eul 式图标优先）

> **先看图形，再看数字，最后才读字。**
> 去掉所有文字还说不出这是什么 → 图形位没设计好。

权威约束在仓库里：`docs/ui-eul-design-language.md`。本 skill 是它的**操作版**——怎么做，不是为什么。

## 三条工作流

| 触发 | 工作流 | 产出 |
|---|---|---|
| 「重新设计 X 页」「参考首页风格」 | **W1 出设计稿** | `output/design/<page>-eul-mockup.html`（双主题）+ 截图验证 |
| 「按稿落码」「按上述方案优化」 | **W2 落码** | 复用既有基元的组件实现 + `npm run build` 绿 |
| 「检查设计」「视觉回归」 | **W3 自查** | DoD 八条逐项 + 门禁输出 |

---

## W1 出设计稿（mockup first，不许直接改代码）

用户说「重新设计」时**先出稿**，不要动 `src/`。步骤：

1. **读现有实现与数据面**：该页组件、`src/types/*.ts`（能拿到哪些字段）、i18n namespace。
   —— 稿子上出现的每个数字都必须有真实字段对应，否则就是画饼。
2. **算高度总账**（1000×700 硬约束）：

   ```
   660 可用 = Ribbon 62 + Hero 118–160 + 内容区 + gaps(9×n)
   ```

   预算超了就先砍区块，不要指望滚动。
3. **复制模板**：`assets/mockup-template.html` → `output/design/<page>-eul-mockup.html`，改标题与注释区。
4. **套骨架**：Ribbon（3–7 段）→ Hero（环/构成条/主表）→ 磁贴或列表 → 提醒带（可选）。
5. **双主题截图验证**（必须做，不能靠肉眼想象）：

   ```bash
   python3 -m http.server 8641 --directory output/design &
   playwright-cli open && playwright-cli goto "http://localhost:8641/<page>-eul-mockup.html"
   playwright-cli resize 1060 880 && playwright-cli screenshot --filename=/tmp/mock-dark.png
   playwright-cli eval "document.getElementById('btnLight').click()"
   playwright-cli screenshot --filename=/tmp/mock-light.png
   ```

6. 判定三条：**无重叠/溢出/截断**、**扫一眼能读出主值**、**浅色仍清晰**。
7. 交付时讲清「结构 / 取舍 / 诚实口径」，等用户确认再进 W2。

细节见 `references/mockup-workflow.md`。

---

## W2 落码

**复用顺序**（不要重画「看起来差不多」的东西）：

| 需求 | 用哪个 | 别做什么 |
|---|---|---|
| 顶部速读条 | `components/Ribbon.tsx`（Shell/Seg/Chip/Value） | 自己写一排 div |
| 百分比图标位 | `components/stats/MiniGauge.tsx` | 用 canvas / 自算 dasharray |
| 磁贴 | `components/stats/InfoTile.tsx` | 手搭三层文字 |
| 比例条 | `components/CellBar.tsx` | 手写 div 宽度 |
| 应用图标 | `components/AppIcon.tsx` | 占位灰图 |
| 趋势线 | `components/Sparkline.tsx` | 内联 polyline |

落码节奏：

1. 先改 `src/types/` 或 `src/lib/` 的数据映射（展示值 → 读数），避免组件里算业务。
2. 组件按「结构 → 文字 → 装饰」顺序写；装饰只允许顶缘 6% 洗光 + 底缘 2px 发丝线。
3. i18n 双语同时补（`src/i18n/messages/namespaces/*.ts` 的 zh + en），中文不进组件。
4. 跑 `npm run build`（tsc + i18n + docs + design + vite 五道）。
5. dev server（1430）截图核对，与设计稿逐区块对照。

组件契约与真实代码片段见 `references/components.md`。

---

## W3 自查（DoD 八条）

对着**截图**判定，不对着代码想象：

1. [ ] 有设计稿且双主题无重叠溢出
2. [ ] 骨架 Ribbon → Hero → 内容，总高 ≤ 660 不滚动
3. [ ] 每个区块有图形锚点；**只有百分比套环**
4. [ ] 文字 ≤ 三层（标题 / 读数 / 一行口径），其余进 tooltip
5. [ ] 只用 §1.2 的域色，浅色主题有覆盖且 ≥ 4.5:1
6. [ ] 诚实四检：`— ≠ 0`、无口径整段隐藏、推导值斜纹 + 说明、口径进 tooltip
7. [ ] 复用基元（未复用需说明理由）
8. [ ] `npm run build` 全绿

快查命令：

```bash
npm run check:design   # R1 图标方向 hack / R2 硬编码主题色
npm run check:i18n     # 中文只许活在 catalog
npm run build          # 全量门禁
```

---

## 硬规则速查（违反即返工）

- **环只表达占比**：速率、计数、温度、字节数套环 = 语义稀释。
- **`h-[NNNpx]` + `overflow-hidden` 的卡片会压扁口径行**——用 `h-full min-h-[104px]` 撑满网格单元。
- **`—` 不是 `0`**：读不到显示 `—` 或原因，绝不画 0 弧 / 写 0 B。
- **图标不靠 transform 翻方向**：成对图标（`UploadIcon` / `DownloadIcon`）。
- **域色不能跨界借用**：颜色即语义，端口不得借 warn。
- **卡里不能只剩图标 + 数字**（空卡反馈过）：至少留标题 + 一行口径。
- **没有语义的按钮不要放**（假刷新按钮）：点了没变化比没有更糟。
- **设计稿 class 名要带页面前缀**：`.fam` 这类通用名会与实现类撞车（真实事故）。

## 文件地图

```
docs/ui-eul-design-language.md          权威约束（本文的母文档）
docs/ui-v5-implementation-spec.md       token 底座（色彩/字号/圆角/z-index）
output/design/*-eul-mockup.html         已验收设计稿（真值样例）
src/components/Ribbon.tsx               带子基元
src/components/stats/MiniGauge.tsx      环形仪表
src/components/stats/InfoTile.tsx       磁贴
scripts/check-design.mjs                设计门禁
references/tokens.md                    令牌与字号阶梯
references/layouts.md                   页面骨架与高度预算
references/components.md                组件契约与代码片段
references/honesty.md                   假事实红线
references/mockup-workflow.md           设计稿规范与验证流程
assets/mockup-template.html             可直接复制的骨架
```
