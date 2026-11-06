# M15 · 灵动岛（Notch Island）可行性方案 · v2

> 2026-11-06 初版 / 2026-11-06 修订（三项决策已定）
> 一句话结论：**技术上完全可行（刘海几何已实测，NSPanel 路径成熟）；形态是「事件驱动的短时提报」，不是常驻挂件。**

**v2 已确认的三项决策：**

| # | 决策 | 对方案的影响 |
| --- | --- | --- |
| 1 | 首优事件 = **CPU 暴走** | M15.3 只做 CPU，swap / 泄漏 / 清理顺延；需要新增 2 s CPU 采样通道（现有 10 s 快照粒度不够） |
| 2 | **要悬停即展开** | 展开态动画全量保留；窗口透明区的点击穿透从「可选优化」升为**必须解决**（见 §6 坑 9）；hover 检测要分两层兜底 |
| 3 | **阈值可配置** | 新增 `island.*` 配置键 + `get_f64/get_u64` 读取；改阈值必须**清空判定窗口**（见 §5.6） |

---

## 1. 事实基线（本机实测，2026-11-06）

`NSScreen.safeAreaInsets` / `auxiliaryTopLeftArea` / `auxiliaryTopRightArea` 实测：

| 屏 | 分辨率（pt / @2x） | `safeAreaInsets.top` | 辅助区 left / right | 推导刘海 |
| --- | --- | --- | --- | --- |
| 内建 Liquid Retina（MacBook Air M2 13"） | 1710 × 1107 | **33 pt** | (0, 1074, 763, 33) / (948, 1074, 762, 33) | **185 × 33 pt**，中心 x = 855.5（= 屏宽正中） |
| 外接 LU28R55 | 1920 × 1080 | **0** | nil | 无刘海 |

三条推论：

1. **刘海宽度可精确算**：`屏宽 − auxLeft.width − auxRight.width`，无需硬编码机型表；
2. **`safeAreaInsets.top > 0` 是「有没有物理刘海」的可靠判据**，外接屏为 0 —— 天然区分「真岛」与「假岛」；
3. **岛必须贴在屏幕顶边**，Tauri 左上原点下 `y = 0`（M0 spike 已验证 tray/window 同系，不需要 Cocoa 翻转）。

## 2. 技术判定：三条硬约束

| 约束 | 结论 | 依据 |
| --- | --- | --- |
| 盖在菜单栏之上 | ✅ 窗口 level 提到 `NSStatusWindowLevel`（25）之上 | 菜单栏本身是 status level 窗口 |
| 盖在全屏 App 之上 | ⚠️ **普通 NSWindow 做不到，无论 level 多高**。全屏 App 是独立 Space，窗口服务器拒绝跨 Space 合成 | Tauri #11791；Cap / screenpipe / Thuki 全部走 NSPanel |
| 不抢焦点 | ✅ `NSPanel` + `nonactivatingPanel` + `canBecomeKey = false` | NSPanel 相对 NSWindow 的核心差异 |

生态：`tauri-nspanel 2.1` 已发 crates.io，支持 Tauri 2，`PanelBuilder` 可直接建 panel。项目已有 `objc2-app-kit 0.3.2` 与手写 FFI 经验（`popover.rs`），两条路都通。

## 3. 产品形态

### 3.1 为什么不做常驻

| 如果做成常驻仪表盘 | 后果 |
| --- | --- |
| 信息与菜单栏图标重复 | 菜单栏剪刀已在编码 CPU 忙闲 |
| 占用刘海区 | 刘海右侧是系统菜单栏图标、iPhone 实时活动的落点 |
| 自身内存开销 | 第三个 WebView 在 macOS 26 上实测可达 110 MB（同代码 Sequoia 仅 29 MB） |

形态定为 **Live Activity 语义**：平时不存在，有事才出现，几秒后自己走。

### 3.2 三态模型

| 态 | 触发 | 尺寸 | 内容上限 |
| --- | --- | --- | --- |
| **Minimal** | 默认 | 0（窗口隐藏） | 无 |
| **Compact** | 事件出现，无人交互 | **229 × 50 pt**（刘海 185 + 22×2，高 33+17） | 左图标 + 右 ≤ 5 字符数字 |
| **Expanded** | 悬停 / 点击 | **380 × 180 pt**（沿用现有 panel 宽度） | 标题 + 主数字 + 一句归因 + 一个跳转 |

尺寸依据：iPhone 公开规范 `compact 每侧 ≈ 50 pt、高 36 pt`、`expanded 高 84–160 pt`；Mac 第三方实测折叠 208×50、展开 440×176~235。本项目刘海 185 宽，故取 229×50。

### 3.3 事件白名单（按 v2 排期排序）

| 优先级 | 事件 | 折叠态 | 展开态 | 停留 | 状态 |
| --- | --- | --- | --- | --- | --- |
| **P0** | CPU 暴走 | ⚡ + 百分比 | 「谁在烧」Top 3 + 跳转 | 6 s | **M15.3 本期** |
| P1 | Swap 破阈值 | 🔴 + GB | 换出受害者 Top 3 | 6 s | 后续 |
| P1 | 清理 / 扫描进行中 | 进度环 + % | 阶段 + 已回收 GB | 任务全程 | 后续 |
| P2 | 疑似内存泄漏 | 💧 + 泄漏量 | 进程名 + 曲线 | 8 s | 后续 |
| P2 | 清理完成 | 🟢 + GB | 明细三项 | 6 s | 后续 |
| P3 | 新版本可用 | ↑ + 版本号 | 说明首行 | 6 s | 后续 |

**不进岛**：常驻读数、网速（菜单栏已有）、需要输入的操作、任何营销内容。

### 3.4 CPU 暴走的判定算法（P0 唯一，值得单独立）

现有 CPU 数据来自 10 s 快照轮（`MemState.snapshot.cpu`），60 s 窗口只有 6 个点，粒度太粗，也分不清「尖峰」和「持续」。**新增 2 s 轻量采样通道**（`sysinfo::System::refresh_cpu_usage`，< 1 ms/次，不发子进程），60 s 窗口 = 30 个点。

判定不用「均值 ≥ 阈值」——那会被单个 100% 尖峰拉高。用**达标率**：

```
窗口      = 最近 sustainSec 内的全部采样（2 s 一个）
达标率    = 采样值 ≥ threshold 的点数 / 总点数
样本充足  = 总点数 ≥ sustainSec / 4          ← 防止刚启动样本不足就判
触发      = 样本充足 且 达标率 ≥ tolerance
```

默认 `threshold=90`、`sustainSec=60`、`tolerance=90%`：60 s 内 ≥ 90% 的采样点超过 90%，即容忍 3 个点回落。

三条保护：

| 保护 | 规则 | 理由 |
| --- | --- | --- |
| **启动保护** | 应用启动后 `sustainSec` 内不判定 | 否则用 3 个点就弹 |
| **滞回** | 触发后需降到 `threshold − 10` 以下才允许再次触发 | 否则 CPU 长期 95% 会每 10 分钟弹一次 |
| **频控** | 同类 10 min 一次；全局 6 次/小时；手动划走则本会话不再报 | 见 §3.5 |

### 3.5 频控

- 同类事件 **10 分钟内只报一次**；全局 **每小时 ≤ 6 次**；
- 用户手动划走 / 右键忽略 → 同类事件本次会话不再报；
- 专注模式期间静默，只累积不弹；
- 设置三档：全部 / 仅严重 / 关闭。

## 4. 技术方案三选一

| 维度 | **A. 独立 panel + WebView** | **B. 原生 NSView 自绘** | **C. 复用现有 panel WebView** |
| --- | --- | --- | --- |
| 视觉表现 | 高（React + CSS） | 中（Core Graphics / Core Text 手写） | 高 |
| 内存增量 | **+30~110 MB** | **+2~5 MB** | 0 |
| 开发量 | 低（≈ 5 天） | 高（≈ 8~10 天） | 中，但**与 NSPopover 互斥** |
| 毛玻璃 | ⚠️ `backdrop-filter` 在透明窗口失效 | ✅ 原生 `NSVisualEffectView` | 同 A |
| 动画 | CSS / WAAPI | CAAnimation 最顺滑 | 同 A |

**推荐 A 起步，B 作为内存不达标的退路**。门槛：footprint 增量 > 60 MB 则转 B。A 的降本路径：隐藏 5 分钟后销毁 WebView，下次显示重建（常驻趋近 0，代价首次 +200~400 ms）。岛前端**不引 React**，用 vanilla + CSS 变量独立打包。

## 5. 推荐架构

### 5.1 窗口

| 参数 | 值 | 理由 |
| --- | --- | --- |
| 类型 | `NSPanel`（`tauri-nspanel` PanelBuilder） | 唯一能覆盖全屏 Space 的窗口类 |
| style mask | `nonactivatingPanel` + `borderless` | 点击不激活 App、不切 Space |
| `can_become_key_window` | **false** | 抢焦点 = 打断用户 |
| level | `NSStatusWindowLevel + 1`（26） | 盖住菜单栏，低于系统弹窗 |
| collection behavior | `canJoinAllSpaces` + `fullScreenAuxiliary` + `stationary` | 全屏、切 Space、Mission Control 都在 |
| `hidesOnDeactivate` | false | 切 App 不消失 |
| `ignoresMouseEvents` | **false**，但要重写 `hitTest`（见坑 9） | 既要点得到岛，又不能挡住岛外 |
| 阴影 | `hasShadow = false` | 透明窗口自绘阴影在 key/non-key 切换时会闪 |
| 尺寸 | **380 × 220 开死，永不 resize** | 动画只改内部形状，不撕裂、不被裁 |

「窗口开大、形状在内部动」是本方案最容易踩错的一点。用 `set_size` 做展开动画会同时出现逐帧抖动、内容裁切、与 Spring 动画不同步三个问题。

### 5.2 几何

```
有刘海屏：centerX  = screen.frame.origin.x + screen.frame.width / 2
          top      = 0（Tauri 左上原点）
          width    = 屏宽 − auxLeft.width − auxRight.width   ← 185 pt
          height   = safeAreaInsets.top                       ← 33 pt
```

三个校准细节（第三方实测踩出来的）：

1. **窗口顶边要超出屏幕顶边 1~2 pt**，否则顶部两个方角暴露，观感从「刘海长出来的」变成「悬浮黑盒子」；
2. 只圆**下方两个角**（折叠 12 pt / 展开 20 pt），顶部保持方角——物理刘海本身是方缺口；
3. 刘海区域内是物理挖孔，**画在那里的像素永远看不见**，所有文字从 `safeAreaInsets.top` 之下开始。

无刘海屏（本机外接 LU28R55）：**默认不显示**。顶栏中央是菜单栏地盘，强占会打架。可选开关，回退尺寸 224 × 38，默认关闭。

### 5.3 事件流

```
[新增] island_cpu    2 s 采样（sysinfo refresh_cpu_usage，<1ms）
[已有] sampler      10 s 全量快照（内存/swap/进程）
[已有] net_sampler   1~2 s 网速
        │
        ├─ 阈值判定（prune-core 纯函数，读 Thresholds 结构体）
        │     输出 IslandEvent { kind, severity, payload }
        ├─ 频控 + 滞回闸门（纯函数，可单测）
        └─ island.rs ──emit──▶ 岛前端（显示 → 动画 → 计时 → 收回）
                     └─orderOut──▶ 隐藏
```

**判定逻辑落在 `prune-core`**（纯函数 + 单测），`island.rs` 只做窗口与事件桥接——与项目「业务逻辑一律在 core，壳只做桥接」的既有约定一致，也让「为什么弹了 / 没弹」可以单测复现。

### 5.4 交互（v2：悬停即展开）

| 输入 | 行为 | 参数 |
| --- | --- | --- |
| 鼠标进入折叠态 | 延迟后展开 | **300 ms**（防鼠标划过误触） |
| 鼠标移出展开态 | 延迟后收起 | **500 ms**（给用户从折叠区移到展开区的时间） |
| 在展开态内移动 | 保持展开，不重新计时 | — |
| 单击 | 打开主窗口并跳转（CPU → 统计 tab） | — |
| 右键 | 本次会话不再提示此类事件 | — |
| 点击后 | 保持 1.2 s 再收起 | 给点击反馈 |
| 键盘 | 不做。岛不抢焦点 | — |

**hover 检测两层兜底**（M15.0 验证哪条成立）：

| 层 | 方式 | 特点 |
| --- | --- | --- |
| 主 | `NSTrackingArea`（`.activeAlways` + `.mouseEnteredAndExited` + `.inVisibleRect`） | 精确、零轮询；需确认在 `hitTest` 返回 nil 的透明区是否仍触发 |
| 备 | 1/15 s 轮询 `NSEvent.mouseLocation` 判断是否落在形状矩形内 | 一定可用；读鼠标位置**不需要任何权限**，连辅助功能都不要。代价 ≤ 66 ms 延迟 |

### 5.5 视觉

- 纯黑底 `#000`，顶部与刘海无缝衔接，不画顶部圆角；
- 文字：折叠 13 pt Medium；展开主数字 28 pt Semibold（沿用主窗口字号）；
- 颜色沿用语义：🟢 正常 / 🟡 注意 / 🔴 严重；
- **毛玻璃用纯色替代**：`backdrop-filter: blur()` 在 Tauri 透明窗口上不生效，硬做会「玻璃没糊、背景透出来」。要真玻璃只能嵌 `NSVisualEffectView`（B 方案能力）。

### 5.6 配置模型（v2 新增）

沿用 `settings.json` 扁平键风格（`SETTING_MENU_NUMBERS` / `SETTING_CPU_ANIM` 同款），**不引入嵌套 JSON**，也不新增配置文件。

| 键 | 类型 | 默认 | 范围 | 说明 |
| --- | --- | --- | --- | --- |
| `island.enabled` | bool | `true` | — | 总开关 |
| `island.cpu_pct` | f64 | `90` | 60–100 | CPU 阈值（%） |
| `island.cpu_sustain_sec` | u64 | `60` | 10–600 | 判定窗口（秒） |
| `island.cpu_tolerance_pct` | u64 | `90` | 50–100 | 窗口内达标率要求 |
| `island.swap_pct` | f64 | `80` | 50–100 | 预留（P1 用） |
| `island.max_per_hour` | u64 | `6` | 1–30 | 全局频控 |

实现要点（四条，缺一条就会出 bug）：

1. **`settings.rs` 补 `get_f64` / `get_u64`** —— 现在只有 `get_str` / `get_bool`，`as_f64()` 读不到会静默返回 None，表现为「配置了但不生效」；
2. **越界钳制到范围内并回写**，不是静默接受。用户把阈值设成 0 会导致岛每秒弹一次，必须在写入时挡掉；
3. **改阈值必须清空判定窗口 + 复位滞回状态**。窗口里的旧样本是按旧阈值算的，不清空会用旧数据误触发（例：90%→70%，30 个旧点全达标，立刻误弹）；
4. **读写失败不阻塞**：`settings.json` 打不开就用内存默认值，岛功能降级但不崩（与现有序列库同一降级哲学）。

IPC：新增 `island_settings_get`（一次返回整组）与 `island_settings_set`（整体写 + 校验 + 清窗口）。不做「一个键一个命令」——前端设置面板一次要读 6 个值。

配置 UI 位置：主窗口加一张「灵动岛」设置卡（开关 + 两个滑杆 + 一个数字输入），复用 `primitives.tsx`。**不在岛内部放滑杆**——岛只显示，不承载设置。

## 6. 已知坑（都有出处）

| # | 坑 | 应对 |
| --- | --- | --- |
| 1 | 普通窗口 level 再高也盖不住全屏 App | 必须 NSPanel + `fullScreenAuxiliary` |
| 2 | `nonactivatingPanel` 永不成为 key → `Focused` 不触发、键盘无效 | 收起靠计时器，不靠 blur |
| 3 | `tauri_panel!` 宏同模块调两次会 `use` 冲突 | 每个 panel 放独立内部模块 |
| 4 | 透明窗口 + CSS 模糊失效 | 纯色/渐变，或原生 `NSVisualEffectView` |
| 5 | 亮暗色切换、插拔屏会连发 3~5 次 `didChangeScreenParameters` | 防抖 300 ms 后重算；屏消失则迁到内建屏或隐藏 |
| 6 | 动画期间 resize 会撕裂 | 窗口按最大态开，只动内部形状 |
| 7 | macOS 26 上 WKWebView 内存是 Sequoia 的 ~4×（110 vs 29 MB） | 内存门禁 60 MB，超标转 B |
| 8 | 折叠态宽度超 229 pt 会撞上刘海右侧系统菜单栏图标 | 先做图标占位探测，探测失败就不加宽 |
| **9** | **窗口 380×220 比形状大，透明区会吃掉鼠标事件——用户点不到下面的窗口**（v2 新增，悬停决策直接引出） | 重写 contentView 的 `hitTest:`：点不在岛形状路径内返回 nil。**不能整个窗口设 `ignoresMouseEvents`**，否则连岛自己都点不到 |
| **10** | **从屏幕顶部下拉菜单时，鼠标经过岛区会触发展开，挡住菜单项**（v2 新增） | ① 只在折叠态 229×50 内响应，不是整个窗口；② 300 ms 延迟，快速划过不触发；③ 左侧菜单（x<200）与中央刘海区（760~1140）不重叠，实际冲突概率低 |

## 7. 分期落地（v2 调整后）

| 阶段 | 内容 | 验收标准 | 工期 |
| --- | --- | --- | --- |
| **M15.0 冒烟** | `tauri-nspanel` 接入 + 纯黑胶囊贴刘海 + **hitTest 穿透与 hover 同时成立** | ① 全屏 Safari 视频下可见 ② 点击不切 Space ③ 岛外透明区能点到下面的窗口 ④ 折叠区 hover 能触发 ⑤ 内建屏 185×33 对齐 | 1 天 |
| **M15.1 几何与多屏** | 几何纯函数 + 防抖 + 无刘海屏策略 | 单测覆盖 2x / 1x / 负坐标 / 外接屏 4 case；插拔不漂移 | 0.5 天 |
| **M15.2 三态与悬停** | 折叠 229×50 ↔ 展开 380×180，Spring 参数 + 300/500 ms 悬停时序 | 60 fps 无撕裂；划过不误触；从折叠移到展开不收敛 | 1.5 天 |
| **M15.3 CPU 暴走链路** | 2 s 采样通道 + 达标率判定 + 滞回 + 频控 + 折叠/展开内容 | 判定纯函数单测（尖峰 1 点 / 持续 30 点 / 样本不足 3 case）；真机触发一次完整链路 | 1.5 天 |
| **M15.4 配置与门禁** | `get_f64/get_u64` + 6 个配置键 + 设置卡 + 改阈值清窗口 + 内存实测 | 越界钳制有效；改阈值不误触发；footprint 增量 ≤ 60 MB | 0.5 天 |

**合计 5 天**（A 方案）。B 方案在 M15.0 之后另行评估，约 +8 天。

## 8. 明确不做

- ❌ 常驻仪表盘（与菜单栏重复，且吃内存）；
- ❌ 岛内输入框（一旦要 key 就会打断当前 App）；
- ❌ 音乐控制、文件拖拽收纳（boring.notch 已做，与 Prune 定位无关）；
- ❌ 无刘海外接屏默认造假刘海；
- ❌ Windows 端（`tauri-nspanel` 仅 macOS）；
- ❌ 一个配置项一个 IPC 命令（一次读整组）。

## 9. 待确认（v2 剩余 2 项）

1. **CPU 暴走要不要带「归因白名单」**？用户正在跑 `cargo build` / Xcode build 时 CPU 100% 是合理负载，弹岛可能被判为噪音。A. 照弹（信息本身有价值，展开能看到是谁）；B. 识别到编译类进程就降级为静默。我倾向 A，先观察真实打扰率再决定。
2. **设置卡放在哪个 tab**？现有 4 个 tab（统计 / 可回收 / 端口 / 网络）之外新开「设置」，还是塞在统计页底部？我倾向新开——配置项会随 P1/P2 事件继续长。

## 10. 参考

- Apple HIG · Live Activities（灵动岛尺寸、三态、角半径 44 pt）
- `NSScreen.safeAreaInsets` / `auxiliaryTopLeftArea` / `auxiliaryTopRightArea`（macOS 12+）
- `tauri-nspanel` 2.1（`PanelBuilder`、`nonactivatingPanel`、`can_become_key_window`）
- boring.notch（TheBoredTeam，SwiftUI，开源，折叠 208×50 / 展开 440×176~235）
- DSH Notch（顶部方角必须落到屏幕顶边之外；穿透窗口的鼠标轮询方案）
- WindowShade（刘海底角 10/12 pt；菜单栏图标占位探测）
- 项目既有：`docs/m0-tray-spike.md`（坐标系）、`docs/ns-popover-plan.md`（objc2 FFI + WebView 搬家）

---

## 11. 实施记录（M15.0–M15.4 已落地）

代码：`src-tauri/src/island.rs`（窗口 / 采样 / 命令）、`src-tauri/src/island_geo.rs`（几何纯函数）、
`crates/prune-core/src/island.rs`（判定 / 滞回 / 频控）、`src/panels/Island.tsx`（三态 UI）、
`src/components/SettingsBoard.tsx`（设置卡）。

### 11.1 与方案的四处偏差

| # | 方案写的 | 实际做的 | 为什么 |
| --- | --- | --- | --- |
| 1 | hover 主用 `NSTrackingArea`，轮询做兜底 | **直接做了轮询**（15 Hz）+ 动态切 `ignoresMouseEvents` | `NSTrackingArea` 要精确命中形状就得重写 `hitTest:`，那需要 `define_class!` 建 NSView 子类再把 WKWebView 重挂进去 —— 风险与工期都不划算。轮询 ≤ 66 ms 延迟，相对 300 ms 展开阈值可忽略，且读鼠标位置零权限 |
| 2 | 每次显示前重算落点 | **setup 时算一次 + 缓存** | `NSScreen::screens` 要求主线程；`cpu_loop` / `hover_loop` 跑在 tokio 线程上拿不到 `MainThreadMarker`，调用会静默返回空 vec —— 症状是「岛永不出现且无报错」。这个坑在编译期完全看不出来 |
| 3 | 配置每次现读 `settings.json` | **内存缓存 `CFG`，写时同步落盘** | CPU 循环 2 s 一拍，每拍 read→parse 一个 JSON 没必要；且外部改坏文件时会抖动 |
| 4 | 配置逐键写 | 新增 `settings::set_many`，**一次事务写 6 个键** | 逐键写 6 次 = 6 轮 read+write，中途失败会留下「前 3 项新、后 3 项旧」的半套配置 |

另加一个方案里没写的命令 `island_status`：设置面板需要知道「本机有没有刘海」，
否则纯外接屏用户会对着一个永远不响的开关调阈值。

### 11.1b 第五处偏差：屏幕变化靠轮询而非通知

方案写的是监听 `NSApplicationDidChangeScreenParametersNotification` + 300 ms 防抖。
实际做法：**在 2 s 的 CPU 采样循环里比对屏幕布局指纹**，变了就重算落点并挪窗。

| 维度 | 通知方案 | 实做的轮询 |
| --- | --- | --- |
| 改分辨率时的行为 | 一个动作连发十几下通知，得自己防抖 | 2 s 一拍天然把整串合并成一次 |
| 线程约束 | 回调在主线程，但**跑在 ObjC 里 —— panic 即 abort**，写起来如履薄冰 | `available_monitors()` 走 `CGDisplay`，**不要求主线程** |
| 延迟 | ~300 ms | ≤ 2 s（屏幕变化不是高频事件，可接受） |
| 副作用 | 需要 `objc2` 再拉一个 feature | 零新增依赖 |

挪窗用 Tauri 的 `Window::set_position`，不用 `setFrameOrigin`：后者是裸 ObjC 调用，
出错就是 `panic_cannot_unwind`（这个坑在 M15.0 已经踩过一次）。

### 11.2 实施中发现的两个问题

**（1）滞回重新武装的条件写错了会「刚武装就再触发」**

第一版用「窗口里只要出现过低于 `threshold − 10` 的点就重新武装」。结果：窗口里
还有 30 个 95% 的点时，出现 1 个 70% 就武装，下一秒达标率仍是 92% —— 立刻又报一次，
白白吃掉每小时 6 次的额度。正确做法两条一起做：**只看当前采样值**，且**武装时清空
窗口并重新计时**。单测 `rearms_after_dropping_below_threshold_minus_hysteresis` 锁住这个行为。

**（2）`objc2` 把 nullable 的 `NSRect` 返回映射成结构体本体，不是 `Option`**

`auxiliaryTopLeftArea()` 无刘海时返回零矩形，不是 `nil`。当成 `Option` 去 `.map()`
会报 `CGRect is not an iterator`。要用 `size.width > 0` 判断。

**（3）`PanelBuilder::transparent(true)` 只透明 NSPanel，不透明 WebView —— 症状是「岛下面一大片空白」**

透明链有三层：DOM（`body { background: transparent }`）→ **WebView 层**
（wry 的 `drawsBackground`）→ NSPanel（`clearColor` + `setOpaque: false`）。
`PanelBuilder::transparent` 只作用于第三层，且是在窗口 build **之后**补的；底层
`WebviewWindowBuilder::new` 默认 transparent=false → WKWebView `drawsBackground=YES`
→ WebView 层自己画一块不透明底，DOM 再透明也盖不住它。修法：在 `with_window`
闭包里对 `WebviewWindowBuilder` 再调一次 `.transparent(true)` —— 它会同时设
window 和 webview 两层（tauri 2.12 `webview_window.rs:1146`），webview 那层才会
触发 `drawsBackground=false`。DOM 测量完全测不到这个问题，因为测的是 DOM 不是 WebView。

### 11.3 验收状态

| 项 | 结果 |
| --- | --- |
| `island_geo` 单测（2x / 1x / 负坐标 / 无刘海 / 数据异常 / 形状对齐） | 7 passed |
| `prune-core::island` 单测（持续 / 尖峰 / 样本不足 / 启动保护 / 滞回 / 重武装 / 钳制 / 频控） | 11 passed |
| `cargo test --workspace` | 183 passed, 1 ignored |
| `cargo build -p prune-app` | 0 warning |
| `tsc --noEmit` | 0 错误 |
| `vite build` | 376.86 kB / gzip 112.56 kB |
| 内存占用 ≤ 60 MB | **未测**（岛与主进程同体，无法单独归因） |
| 真机冒烟（全屏可见 / 不抢焦点 / 透明区穿透 / hover 展开） | **未跑**，需 `pnpm tauri dev` 目测 |

提报链路已实测打通一次（阈值临时调成 60% / 10 s 造负载）：

```
就位 x=665.5 y=-2.0 刘海 185×33 → 前端已就绪 → show visible=true
→ alert 已广播 → 前端 phase=collapsed box=229x50 bg=rgb(0,0,0) opacity=1
```

**仍未验证的四条**（需要真机 + 眼睛，不能靠单测）：

1. 全屏 App 之上是否真的可见（NSPanel + `fullScreenAuxiliary` 的组合行为）；
2. 折叠态 17 pt 内容条的实际可读性 —— 这是物理极限，可能需要把折叠高度从 50 调到 56；
3. 岛下方透明区是否能点到下面的窗口（动态 `ignoresMouseEvents` 的切换时机）；
4. hover 300 ms 展开 / 500 ms 收起的手感。

> 截图验证在这台机器上做不到：未授予「屏幕录制」权限时 `screencapture` 只返回壁纸，
> 连菜单栏都没有，会把「画得很正常」误判成「什么都没画」。这一轮就为此浪费了半小时。
