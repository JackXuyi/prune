# M22 · 局域网入口去重方案：端口页「局域网」子页下线

> 状态：已实施（2026-11-23，门禁全绿；拓扑按 D1 休眠保留）
> 日期：2026-11-23
> 关联：m18-lan-devices-plan.md（局域网挂端口页的来历）/ m20-lan-topology-plan.md / m20b-topology-polish-plan.md（拓扑入口）/ device 页 eul 重设计（一页两域）

## 1. 背景：重复是怎么长出来的

局域网设备浏览目前有**两个入口**，渲染的是同一份 `lanStore` 快照、同一套组件：

| 入口 | 挂载点 | 头部 | 拓扑入口 | 搜索 |
|---|---|---|---|---|
| ① 端口页「局域网」子页 | `PortList.tsx` PortBoard 子页分段（`page === "lan"`） | LanRibbon + LanHero（`showHeader=true`） | **有**（`showTopoToggle=true`，全 app 唯一） | 端口页搜索框复用（`searchLanPlaceholder`） |
| ② 设备页「网络」域 | `DeviceBoard.tsx`（一页两域） | DeviceRibbon（含 LAN 2 段）+ LanHero（tone=device） | 无（2026-11-20 产品决策暂隐藏） | 无（LanBoard 内无搜索框传入） |

历史链路：M18 时局域网挂在端口 tab 下是为了「不新增一级导航」（m18 spec §11）；后来设备页 eul 重设计把「本机硬件 + 局域网」定为设备页的两域结构，LAN 成了一等公民——端口页的局域网子页从此变成第二入口。端口页域色是琥珀（port 域），LAN 设备卡是 net 域色，语义也不合。

## 2. 决策：去掉端口页的「局域网」子页

**保留设备页的网络域，下线端口页局域网子页。**

理由：

1. **定位归属**：设备页 = 本机硬件 + 局域网（M21 外设页确立的边界：设备页管本机+局域网，外设页管插上/连上的外设）。局域网在设备页是设计语言内的正当成员；在端口页是历史挂靠。
2. **头部重复最重**：端口页 lan 子页的 LanRibbon + LanHero 与设备页的 DeviceRibbon(LAN 2 段) + LanHero 是同一读数画两遍；设备页的画法已与页面级 Ribbon 融合，拆不掉。
3. **语义干净**：端口页收窄为「端口 / 监听 / 暴露面」单域，无分段胶囊，导航更简单；暴露面视角仍由 DeviceRibbon 的「对外暴露」段 + LanDevice 卡片承接。
4. **删得动**：端口页 lan 子页是 `PortList.tsx` 内一段条件渲染 + 4 个 i18n 键，无 store / Rust 侧改动。

被否决的备选：去掉设备页网络域、局域网只留端口页 —— 与 device-eul-mockup 已确认的一页两域设计冲突，设备页会退化成纯硬件页，改动面更大（DeviceRibbon 7 段砍 2 段、LanHero 挂载迁移），否决。

## 3. 设计决策

| # | 决策 | 说明 |
|---|---|---|
| D1 | 拓扑视图保留代码、不上 UI（2026-11-23 产品决策） | 端口页 lan 子页下线后拓扑失去唯一入口；**不恢复设备页入口，也不删除拓扑代码**。`LanTopology.tsx` 与 LanBoard 内的拓扑逻辑（`showTopoToggle` prop / `view` state / `ViewChip` / 隐藏容器）**原样保留**，DeviceBoard 维持现状传 `showTopoToggle={false}`——即 2026-11-20「组件保留、入口隐藏」决策的延续，只是「端口页保持原样」那一侧随子页一起消失。拓扑处于「代码在库、无 UI 消费」的休眠态，后续若要恢复只需一处挂载点传 `showTopoToggle` 默认值。 |
| D2 | 端口页回归单页 | 删除 `PortSubPage` / `PortSubTabs` 分段胶囊与 `page` state；搜索框保留（只搜端口），placeholder 固定 `ports.searchPlaceholder`。 |
| D3 | 搜索框不再喂 LanBoard | `query` 只作用于端口过滤；`searchLanPlaceholder` 等 LAN 专用键一并删除。 |
| D4 | LanBoard 自身零逻辑改动 | 组件结构不动（含拓扑逻辑）；`query` prop 保留（设备页不传即空串）；仅注释更新（端口页挂载点消失，唯一调用方是设备页且入口隐藏）。 |
| D5 | 权限弹窗纪律不变 | 设备页进入不自动扫描、扫描按钮在 LanHero 口径块——现有行为原样保留，本方案零触碰。 |
| D6 | 文档回扫 | 端口页 lan 子页下线推翻 m18 spec §11 的挂载承诺、并使 m20/m20b 中「端口页局域网子页不受影响」等表述过时；实施完成后对 docs/ 与 README 做一次「端口页 局域网 / 拓扑入口」残留回扫（改档说明，不整篇删除历史方案文档）。 |

## 4. 代码落点

全部在前端，`src-tauri/**` 零改动：

| 文件 | 改动 |
|---|---|
| `src/components/PortList.tsx` | 删 `LanBoard` import、`PortSubPage` 类型、`PortSubTabs` 组件、`page` state、`page === "lan"` 分支；头部行去掉分段胶囊只留搜索框；`title` 注释同步（「双子页」→ 单页） |
| `src/components/DeviceBoard.tsx` | 调用处零改动（维持 `<LanBoard showHeader={false} showTopoToggle={false} />`）；仅注释更新（端口页 lan 子页下线引用本方案，拓扑入口保持隐藏） |
| `src/components/LanBoard.tsx` | 零逻辑改动（拓扑逻辑按 D1 原样保留）；仅注释：`showTopoToggle` 说明改为「唯一调用方设备页、入口隐藏；端口页挂载点已随 M22 下线」 |
| `src/components/LanTopology.tsx` | 零改动（休眠保留，无 UI 消费） |
| `src/i18n/messages/namespaces/ports.ts` | 删 `subpage`（zh+en）、`searchLanPlaceholder`（zh+en）；顺手清死键 `subLan`、`subPorts`（全仓无消费，zh+en 配对删）；en `title` 上方 `subPorts`/`subLan` 注释同步 |
| `src/i18n/messages/namespaces/lan.ts` | 零改动（`lan.topo.*` 键由 LanBoard 拓扑代码消费，随 D1 保留） |

不动的引用（已核实在用，不受影响）：

- `StatsBoard.tsx` 的设备磁贴走 `lanStore.snapshot` + `init`（只拉缓存，不扫描）——与本方案无关。
- `lanStore`、`types/lan.ts`、`LanDeviceCard`、`LanRibbon`、`LanHero` 继续由设备页消费。
- `LanDeviceDetailModal` 仍由卡片路径消费，不随拓扑休眠受影响。

## 5. 边界风险

| 风险 | 处置 |
|---|---|
| 老用户从端口页找局域网的习惯断裂 | RailNav「设备」仍是显式一级导航；可在设备页注释里留指路，不做引导弹层（改动过重） |
| 拓扑代码进入「无 UI 消费」休眠态，可能被误判死代码 | D1 属产品决策的有意保留（后续恢复只需一处挂载点）；在 LanBoard/LanTopology 注释里写明休眠原因与本方案编号，防后续清理误删 |
| i18n 死键清理误删在用键 | `subLan`/`subPorts`/`subpage.*`/`searchLanPlaceholder` 删除前逐键全仓 Grep 复核（含动态拼接场景——MessageKey 严格类型，动态拼接过不了编译，可静态确认）；`lan.topo.*` 键由保留的拓扑代码消费，**不在清理范围** |
| 历史文档过时（m18 §11 挂载承诺 / m20、m20b 的端口页表述） | 实施完成后按 D6 回扫 docs/ 与 README 残留，改档不删档 |
| 搜索框宽度/布局在无胶囊后视觉失衡 | 搜索框仍 `ml-auto` 靠右，头部行只剩单元素；落码时真机看一眼 |

## 6. 验收清单

- [x] `cargo check` / `cargo clippy` 绿（虽无 Rust 改动，例行门禁；clippy 仅存量警告 Cargo.toml / report.rs）
- [x] `tsc` / `npm run build` 绿
- [x] `node scripts/check-design.mjs` 绿
- [x] 端口页：无分段胶囊，搜索框只搜端口，`ports.subpage.*` 键已删且 zh/en 编译通过
- [ ] 设备页：LanBoard 卡片网格正常，无「卡片 / 拓扑」切换入口（`showTopoToggle={false}` 现状保持）；`LanTopology.tsx` 与拓扑逻辑零改动在库 —— 待真机 UI 走查
- [ ] 设备页进入不触发扫描/权限弹窗；扫描按钮、进度条、空态、离线折叠、详情弹窗行为与现状一致 —— 待真机 UI 走查
- [x] 全局搜索无 `PortSubPage` / `PortSubTabs` / `searchLanPlaceholder` / `ports.subpage` / `ports.subLan` 残留（`LanTopology` / `showTopoToggle` / `lan.topo` 属 D1 有意保留，不做残留检查）
- [x] docs/ 与 README 残留回扫完成（D6：CHANGELOG 加 Unreleased 条目；m18 spec / m18 cards / m20 拓扑方案 / ui-v5 清单加状态注记）
