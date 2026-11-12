# M20 局域网拓扑视图 · 详细 Spec

> 上游方案：docs/m20-lan-topology-plan.md（已评审，默认裁决：离线设备直显、切换按钮放筛选条右侧、无 idle 浮动）。
> 本 spec 是唯一实施依据，含组件契约 / 算法 / 事件流 / i18n / 验收。状态：待实施。
> 对应代码：新增 `src/components/LanTopology.tsx`；改动 `src/components/LanBoard.tsx`、`src/i18n/messages/namespaces/lan.ts`。

## 0. 裁决记录（wireframe 已确认）

| 项 | 裁决 |
|---|---|
| 离线设备 | 画布直接显示：节点 45% 置灰 + 虚线边，不折叠 |
| 视图切换 | 筛选条右侧，与重扫按钮同排的二态 segmented（卡片 / 拓扑） |
| idle 浮动 | 不做。拖拽 + 脉冲 + 悬停高亮即为「好玩」边界，避免每帧重算 SVG 边 |

## 1. 组件契约

### 1.1 LanTopology

```ts
// src/components/LanTopology.tsx
export function LanTopology({
  devices,          // LanDevice[]（LanBoard 透传 snapshot.devices，不做二次过滤）
  onSelect,         // (d: LanDevice) => void，LanBoard 的 setSelected
}: {
  devices: LanDevice[];
  onSelect: (d: LanDevice) => void;
}): JSX.Element
```

- **不内嵌详情弹窗**：弹窗仍由 LanBoard 持有（`selected` state），拓扑只回调 `onSelect`——切换视图时弹窗状态不丢。弹窗的时间锚 `at` 由 LanBoard 直接传（snapshot.at 就在它手上），拓扑组件不重复接。
- 自身不含数据获取，纯展示组件；快照刷新由 LanBoard → store 驱动。

### 1.2 LanBoard 接线

```ts
const [view, setView] = useState<"cards" | "topo">("cards");
```

| 规则 | 细节 |
|---|---|
| 切换按钮位置 | 筛选条右侧：`重扫按钮 ← [卡片\|拓扑]`，segmented 复用 FilterChip 视觉语言（active = `bg-accent/[0.14] text-accent`） |
| 可见性 | 仅在「有快照且非扫描中」渲染切换（沿用现有 early-return 分支之后，自然满足） |
| topo 分支渲染 | 替换 `filter === "inbound" ? … : <>…</>` 的整个设备区块（筛选 chips 在 topo 模式下**隐藏**—— chips 语义只属于卡片网格，避免「点了没反应」）；指标卡 / 随机 MAC 提示 / footer 保留 |
| 选中设备 | `selected` 状态两视图共享；从拓扑打开的弹窗关掉后回拓扑，不强制切回卡片 |
| 重扫完成 | `snapshot` 更新 → 拓扑用新设备集重算缺失位置（§3.4），视图不重置 |

## 2. 状态模型（LanTopology 内部）

| 状态 | 类型 | 生命周期 |
|---|---|---|
| `size` | `{ w: number; h: number }` | `ResizeObserver` 实测容器；初始 `{ w: 760, h: 460 }` 兜底（首帧测量前） |
| `pos` | `Record<string, { x: number; y: number }>` | 节点中心坐标（画布 px）。**只增不改已拖过的 key**：重算布局时跳过 pos 中已存在的 ip |
| `hovered` | `string \| null` | 悬停节点 ip，驱动边高亮与节点放大 |
| `dragRef` | `{ ip, dx, dy, moved } \| null` | useRef，不触发渲染 |
| `suppressClick` | ref boolean | pointerup 时若 moved 则置 true，click 消费后复位——拖拽结束不误开弹窗 |

**签名与重算**：`const sig = devices.map(d => d.ip).join(",")`；`useEffect([sig, size])` 时为 pos 中**缺失的 ip** 计算初始位（§3），已有 key 不动。组件卸载即全部释放，不落盘。

## 3. 布局算法（确定性星形）

```
function layout(devices, w, h) → Record<ip, {x,y}>
  cx = w / 2          cy = h * 0.46
  gw  = devices.find(d => d.isGateway)
  me  = devices.find(d => d.isSelf)

  // 1. 中心锚
  if gw   → pos[gw.ip] = { cx, cy }
  anchor  = gw ?? me
  if !gw && me → pos[me.ip] = { cx, cy }        // 无网关：本机顶替中心
  if gw && me  → pos[me.ip] = { cx, cy + 84 }   // 本机内圈：网关正下方

  // 2. 其余设备单环
  rest = devices.filter(d => d !== gw && d !== me)
  R    = clamp(130, min(w, h) / 2 - 100, 260)
  rest.sort(by ip)                               // 稳定排序：跨渲染不换位
  rest.forEach((d, i) =>
    angle = (i / rest.length) * 2π - π/2 + jitter(d.ip)
    pos[d.ip] = { cx + R·cos(angle), cy + R·sin(angle) })

jitter(ip) = (hash31(ip) % 21) - 10  →  度，转弧度
hash31(s)  = fold: h = (h·31 + code) | 0             // 确定性，非加密
```

| 约束 | 值 | 依据 |
|---|---|---|
| 画布高度 | 460px 固定 | 方案 §3；宽度 100% 自适应 |
| 环半径 | clamp(130, min(w,h)/2 − 100, 260) | 20 台设备周长 2π·200 ≈ 1256px，均摊 ~63px/节点 ≥ 节点直径 46 + 标签宽 96 的安全下限 |
| 拖拽 clamp | x ∈ [30, w−30]，y ∈ [26, h−26] | 节点半径 + 标签可视余量 |
| 边界冲突 | rest.length === 0 | 只画中心锚，无环；me 与 gw 重叠风险不存在（me 偏移 84px） |

## 4. 渲染规格

### 4.1 画布

```
<div ref>  ← relative, h-[460px], rounded-[12px], border-hairline-soft
  <svg>    ← absolute inset-0 w/h-full, pointer-events-none（边层）
  {nodes}  ← absolute, transform: translate(-50%,-50%) + left/top px
```

- 画布底色：`bg-[#0b0e11]`（与 MainWindow aside 同源的深色面），点阵纹理用 inline `backgroundImage: radial-gradient(rgba(255,255,255,0.05) 1px, transparent 1px)` + `backgroundSize: 16px 16px`——不引图片资源。

### 4.2 节点（HTML div，非 SVG——复用 icon 与 tailwind token）

| 元素 | 规格 |
|---|---|
| 圆形 tile | 46px（`size-[46px]`）rounded-full flex 居中；网关 54px |
| 图标 | `<DeviceKindIcon kind size={17}/>`（网关 19） |
| 网关/本机配色 | `bg-accent/[0.14] border border-accent/40 text-accent`；本机描边改虚线 `border-dashed` 区分 |
| 普通设备 | `bg-white/[0.05] border-hairline text-ink-dim` |
| 离线 | 整节点组 `opacity-45`，描边 `border-dashed` |
| 新设备 | tile 外层套 `<span class="absolute inset-0 rounded-full bg-accent/30 animate-ping -z-10">` + 右上角「新」badge（`bg-accent/[0.12] text-accent text-[9.5px] px-1.5 rounded`，与设备卡 badgeSelf 同 token） |
| 标签 | tile 下方 4px，宽 96px 居中两行：主标题 11px text-ink（`truncate`，hostname ?? kindLabel(kind)）；副行 10px text-ink-faint mono（ip）。网关/本机主标题用 `lan.card.gateway` / `lan.card.self`（与卡片口径一致） |
| 悬停 | `scale-[1.08]` transition-transform + `z-10`；连线高亮见 §4.3 |
| 点击区 | 整个节点 wrapper（tile + 标签）可点，`cursor-grab` / 拖拽中 `cursor-grabbing` |

### 4.3 边（SVG line，画布坐标即 px，无需换算）

| 边类型 | 样式 |
|---|---|
| 网关 → 设备（在线） | `stroke="rgba(255,255,255,0.15)" strokeWidth 1.2` |
| 网关 → 设备（离线） | 同上 + `strokeDasharray="4 3"` + opacity 0.5 |
| 本机 → 设备（inbound 非空） | `stroke` accent 实色 `strokeDasharray="5 3"` strokeWidth 1.4——**盖在网关边上层渲染** |
| 悬停高亮 | 被悬停节点的所有关联边：accent/60 + strokeWidth 1.8；其余边 opacity 0.35 |

### 4.4 图例 + 提示行（画布下方）

```
拖拽节点重排 · 点击节点看详情     ● 本机 ● 网关 ◉ 新设备 ┄ 离线 ┄ 连了我
```

- 左：`topo.dragHint`（text-[10px] text-ink-faint）；右：图例（10.5px，色点用真实 token 色）。
- 图例五项仅在对应元素存在时显示（如无新设备则不渲染「新设备」项）——避免图例撒谎。

## 5. 拖拽事件流（实现细节）

```
onPointerDown(node)  dragRef = { ip, dx = e.clientX − nodeScreenX, dy = …, moved = false }
                     node.setPointerCapture 可省（window 监听已覆盖）
window pointermove   dragRef 存在 → moved = true（位移 > 4px 才置，防微抖误判）
                     计算画布相对坐标 → clamp（§3）→ setPos
window pointerup     moved ? suppressClick = true : null；dragRef = null
onClick(node)        suppressClick ? (suppressClick = false) : onSelect(d)
```

- window 级监听在组件挂载时注册一次（`useEffect` 空依赖 + ref 读写），卸载时移除——不为每次拖拽 add/remove。
- 触屏：pointer events 天然覆盖 touch，无需单独处理；`touch-action: none` 加在节点上防画布滚动。

## 6. i18n 契约（lan.topo.*）

| key | zh | en |
|---|---|---|
| `lan.topo.view` | 拓扑 | Topology |
| `lan.topo.cards` | 卡片 | Cards |
| `lan.topo.dragHint` | 拖拽节点重排 · 点击节点看详情 | Drag nodes to rearrange · click a node for details |
| `lan.topo.legendSelf` | 本机 | This machine |
| `lan.topo.legendGateway` | 网关 | Gateway |
| `lan.topo.legendNew` | 新设备 | New |
| `lan.topo.legendOffline` | 离线 | Offline |
| `lan.topo.legendInbound` | 连了我 | Connected to me |

- 加在 `namespaces/lan.ts` 的 zh / en 两处顶层（与 `footer` 平级）；`MessageTree<typeof zh>` 保证 en 漏 key 编译失败。
- 节点主标题不新增 key：复用 `lan.card.self` / `lan.card.gateway` / `kindLabel()`。

## 7. 边界与兜底

| 场景 | 行为 |
|---|---|
| devices 为空 | LanBoard 现有空态分支先拦（`snapshot.devices.length === 0`），拓扑组件不感知 |
| 只有网关没有本机 | 网关居中，无 inbound 边（me 不存在自然跳过） |
| 只有本机没有网关 | 本机居中，其余设备环绕 |
| 两者都无 | 全员单环，anchor 缺席时 cx/cy 仍有效 |
| 重扫后设备集变化 | 新 ip 补初始位；消失 ip 的 pos 残留不渲染（无害） |
| 容器极窄（<320px） | R 被 clamp 到 130，节点可能溢出画布边——clamp 拖拽限制兜底，属已知可接受 |
| 浏览器 dev（非 Tauri） | store 给 null 快照 → 空态，拓扑不可达，无假数据 |
| reduced-motion | Tailwind `animate-ping` 自带 media query 降级，无额外处理 |

## 8. 实施拆分与验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| T1 | `LanTopology.tsx` 全量：布局 / 渲染 / 拖拽 / 悬停 / 图例 | 走查五态：正常混合、全离线、只有网关、只有本机、单设备 |
| T2 | LanBoard 接线：view state + segmented + topo 分支 + chips 隐藏 | 卡片↔拓扑来回切换状态不丢；扫描中不出现切换；弹窗跨视图共享 |
| T3 | i18n `lan.topo.*` zh/en | `tsc --noEmit` 0 错 + `check:i18n` 通过 |
| T4 | 门禁 + CHANGELOG + `npm run bump`（patch） | `tsc` / `check:i18n` / `npm run build` 全绿；CHANGELOG 补 M20 条目 |

提交分组（沿用 M19 惯例，2 个 commit）：
1. `feat(lan): 局域网拓扑视图——星形布局 + 可拖拽节点`（T1+T2+T3）
2. `docs: M20 拓扑视图 plan/spec + CHANGELOG + bump`（T4）

## 9. 明确不做

- 不做力导仿真 / 碰撞检测（环形均匀分布已保证间距）
- 不做节点框选、缩放、平移（设备规模用不上）
- 不做拓扑布局持久化（组件卸载即重置，重扫不清布局）
- 不做边动画（流动虚线等），保留 `animate-ping` 一处动效

## 10. build vs buy 裁决（2026-11-12 已确认）

**维持零依赖自绘，不引 react-flow。** 对比结论：

| 候选 | 判决 |
|---|---|
| `@xyflow/react` v12 | React 19 兼容、MIT，但 +59 KB gzipped JS + 2.6 KB CSS；依赖 `zustand@^4` 造成 zustand 5/4 双拷贝；自带缩放/平移/minimap/多选均属本项目明确不做；且其**不含布局算法**，spec §3 星形布局一行省不掉。能替代的只有「拖拽 + 边随节点动」（自绘 ~40 行） |
| `d3-force` | 力导仿真与「确定性布局、无仿真」裁决冲突 |
| `dagre` | 层级布局，星形拓扑用不上 |
| `cytoscape.js` / `vis-network` | 命令式引擎 + 自带样式体系，design token 要另起一套，最重 |

**重新评估触发条件**（满足其一再引 react-flow）：节点规模上百（端口级拓扑：进程→端口→远端）；用户明确要求缩放/平移/minimap。
