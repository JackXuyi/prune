# 设计稿规范（mockup workflow）

设计稿是**可验收的静态 HTML**，不是图片、不是口头描述。
它的作用：先把版式和信息层级定死，再动代码——避免「边写边试」把组件试成四不像。

## 1. 文件约定

```
output/design/<page>-eul-mockup.html     例：stats-eul-mockup.html / memory-eul-mockup.html
```

- 单文件、零依赖（内联 CSS/JS，不引 CDN）。
- 顶部工具栏：深色 / 浅色切换 + 一句说明（页面名 · 1000×700 · 值为示意数据）。
- 窗口下方 `notes` 区：2–4 张卡片写「设计要点 / 纪律不变 / 与实现的映射」。
- **所有数值都是示意数据**，但每个数字都必须有真实字段对应（在 notes 里注明）。

## 2. 必守的写稿纪律

### 2.1 类名带页面前缀

```css
/* ❌ 通用名会与实现里的同名类撞车（真实事故：.fam 撑到 584px 把带子顶出窗口） */
.fam { ... }
/* ✅ 带页面/区块前缀 */
.leak-famtag { ... }
```

### 2.2 高度写死，宽度自适应

设计稿里给窗口内每个区块**写死高度**（`height: 62px`），这样才能验证「一屏装得下」。
宽度用 `flex: 1` / `grid`，不要写死像素宽（除了 rail 58 / 侧栏 300–350）。

### 2.3 主题用变量，不写两套

```css
:root, [data-theme="dark"] { --info: #6aa9ff; --surface: #0e1114; ... }
[data-theme="light"]       { --info: #2273cc; --surface: #f4f6f8; ... }
```

切主题只改 `document.body.dataset.theme`，其余全部引用 `var(--*)`。

### 2.4 环的 dasharray 手算

```
C = 2πr；段长 = C × 占比；gap = C − 段长
r = 10.5 → C ≈ 65.97   （26px 环）
r = 48   → C ≈ 301.6   （104px 压力环）
r = 62   → C ≈ 389.6   （134px Hero 环）
```

写法：`stroke-dasharray="段长 余长" transform="rotate(-90 cx cy)"`。

## 3. 验证流程（必须做完再交付）

```bash
# 1) 起静态服务
python3 -m http.server 8641 --directory output/design &

# 2) 打开 + 定尺寸（比窗口多一点，留注解区）
playwright-cli open
playwright-cli goto "http://localhost:8641/<page>-eul-mockup.html"
playwright-cli resize 1060 880

# 3) 深色截图
playwright-cli screenshot --filename=/tmp/mock-dark.png

# 4) 浅色截图
playwright-cli eval "document.getElementById('btnLight').click()"
playwright-cli screenshot --filename=/tmp/mock-light.png

# 5) 收尾
playwright-cli close
```

用 `ViewImage` 读两张图，逐项问：

1. 有没有元素重叠 / 溢出窗口 / 文字被裁？
2. 每块卡片是不是都有图形锚点 + 读数 + 一行口径（不是空卡）？
3. 环的弧长与数值是否大致吻合（12% 一小段、87% 接近满）？
4. 浅色主题下灰字 / 彩色数值还读得清吗？
5. 列表最后一行有没有被窗口底边切掉？

> 量宽高别靠眼睛：`playwright-cli eval` 里 `getBoundingClientRect()` 比截图判断准。

## 4. 常见坑（都踩过）

| 现象 | 原因 | 修法 |
|---|---|---|
| 带子溢出到窗口外 | 类名撞车（`.fam`）或子元素 `flex:1` 没收 min-width | 前缀命名 + `min-width:0` |
| 卡片底部小字被裁 | 定高不够 / 子项没 `shrink-0` | 加高 4–8px，`shrink-0` |
| 环弧比例不对 | dasharray 用错半径 | 按 §2.4 重算 |
| 浅色下看不见 | 色值写死在深色主题 | 全部走变量 |
| 截图空白/尺寸不对 | `open` 后未 `resize`，或 reload 后视口被重置 | 重新 resize 再截 |

## 5. 交付话术

```
设计稿已打开（右上角切深/浅）。结构：①Ribbon … ②Hero … ③磁贴 …
取舍：删了什么、为什么；新增了什么、数据从哪来。
纪律：假事实红线怎么守（— ≠ 0 / 推导值斜纹 / 无口径隐藏）。
下一步：确认后按稿落码，复用 Ribbon/MiniGauge/InfoTile。
```

## 6. 稿 → 码的映射表（交付时一起给）

| 稿上区块 | 落到哪个组件 |
|---|---|
| 顶部状态带 | `RibbonShell` + `RibbonSeg` + `MiniGauge`/`RibbonChip` |
| Hero 环 | `RingGauge`（`components/charts/`）或 `MiniGauge` 放大 |
| 磁贴 | `InfoTile` |
| 比例条 | `CellBar` |
| 列表行 | 页内 `*Row` 组件（grid 列定义照抄稿） |
| 徽标 | `Badge` |
