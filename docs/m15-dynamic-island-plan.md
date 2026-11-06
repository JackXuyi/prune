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

**（4）`MemMonitor` 派生 `Default` 会让三条通道天生哑火**（M15.6a 单测当场抓出）

武装位（armed）必须初始为 `true`——新造的监视器要能触发。`#[derive(Default)]`
给 bool 的初值是 `false`，结果是 Critical 也不报。手写 `Default::default()`，
并在注释里说明为什么不能派生。教训：**「能编译」和「语义正确」之间隔着一组
布尔初值**，判定类结构体的 Default 必须逐字段想一遍。

### 11.3 验收状态

| 项 | 结果 |
| --- | --- |
| `island_geo` 单测（2x / 1x / 负坐标 / 无刘海 / 数据异常 / 形状对齐） | 7 passed |
| `prune-core::island` 单测（CPU 持续 / 尖峰 / 启动保护 / 滞回 / 重武装 / 钳制 / 频控 + 内存三通道 / 双闸 / would_allow） | 23 passed |
| `cargo test --workspace` | 254 passed, 1 ignored |
| `cargo build -p prune-app` | 0 warning |
| `tsc --noEmit` | 0 错误 |
| `vite build` | 377.11 kB / gzip 113.00 kB |
| clippy | 9 条警告全在既有文件，灵动岛新代码 0 条 |
| 内存占用 ≤ 60 MB | **未测**（岛与主进程同体，无法单独归因） |
| 真机冒烟（全屏可见 / 不抢焦点 / 透明区穿透 / hover 展开 / 内存触发实弹） | **未跑**，需 `pnpm tauri dev` 目测 |

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

## 12. 内存异常事件方案（M15.6，已落地 2026-11-06）

目标：岛的第二条腿。对应 §3.3 白名单里的 P1「Swap 破阈值」与 P2「疑似内存泄漏」中
**本期只做前者**，泄漏嫌疑（M3 判定链）留到 P2 单独立项。

### 12.1 触发定义：三条通道，任一命中

内存和 CPU 的本质区别：CPU 是**速率**量（要「达标率」），内存是**水平**量（看状态持续）。
且 macOS 有内核权威信号 —— `kern.memorystatus_vm_pressure_level`（1=Normal / 2=Warning /
4=Critical），系统自己都在用它决定杀谁，比任何自算阈值都可信。

| 通道 | 判定 | 持续要求 | 滞回（重新武装条件） | 依据 |
| --- | --- | --- | --- | --- |
| **A 压力 Critical** | `pressure == Critical` | 立即（1 个采样） | 回到 Normal | 内核已判「快杀进程了」，无需再忍 |
| **B 压力 Warning** | `pressure == Warning` | `mem_sustain_sec`（默认 30 s） | 回到 Normal | Warning 会长期挂着，必须持续确认 |
| **C Swap 破阈值** | swap 占用率 ≥ `swap_pct`（默认 80%） | `mem_sustain_sec` | 占用率降到 `swap_pct − 10` | 高 swap + 持续不降 = 真紧张；可配置关闭 |

三条通道**独立滞回**。C 为什么滞回用 −10 而不是回 Normal：swap 占用是慢变量，
从 95% 等到 0% 等于永远不再报。

刻意**不**用「换页颠簸速率（swapouts/s）」做触发：它对 Finder 索引、Spotlight 重建
这类短促 IO 风暴太敏感，误报率不可控。展开态里展示它，但让它只当「严重程度」的证据。

### 12.2 数据源（全部现成，零新依赖）

| 信号 | 出处 | 成本 |
| --- | --- | --- |
| 压力等级 | `mem::system::read_pressure()`（sysctl，`PressureLevel` 三档） | µs 级，2 s 一拍可承受 |
| swap 占用 | `swap::collect().used_ratio()`（`sysctl vm.swapusage`） | µs 级 |
| 换页速率 | `MemSnapshot.thrash`（现有差分逻辑） | 已算好 |
| 压缩器占用 | `VmStat.compressor_bytes()` | 2 s 一拍可承受 |
| 进程换出量 | `ProcMem.swapped`（footprint 深探合并）+ `swapped_ratio()` | 10 s 轮已有 |

采样粒度决策：压力/swap 走**与 CPU 同拍的 2 s 通道**（不吃 10 s 的 MemSnapshot 轮 ——
30 s 窗口只有 3 个点，Warning 持续判定会变成「3 个点里 2 个」，抖动太大）。

### 12.3 与 CPU 事件的互斥

岛同时只显示一条。规则按 P0 > P1：

- CPU 展示中内存触发 → **抢占**（P0 优先级高于 P1，用户该先知道谁在烧）；
  被抢占的内存事件不丢 —— 频控窗口过后若仍异常会自然重报；
- 内存展示中 CPU 触发 → 同样抢占；
- 两条都非 CPU：后触发者等当前条自然结束（6 s），若届时仍异常且频控允许则弹。

### 12.4 频控改造

现有 `RateLimiter` 是全局单实例。改为**每事件一个实例**（CPU、MEM 各自 10 min 冷却
+ 每小时 6 次），另加一个**全局共享**实例兜底（合计 ≤ 6 次/小时）——否则 CPU 报满
后内存事件永远弹不出来，反之亦然。

### 12.5 展示设计

**折叠态**（229×50，17 pt 内容条）：**图标 + 事件标签 + 读数**，三段式。

| 等级 | 图标颜色 | 读数示例 |
| --- | --- | --- |
| Warning | 🟡 橙 `#FF9F0A` | `▐█▌ 换出 10.4G` |
| Critical | 🔴 红 `#FF453A` | `▐█▌ 换出 10.4G` |

标签设计两条理由：

1. **颜色不是可靠的事件区分**——内存 Warning 用橙、CPU 也用橙，色弱用户和快速瞥视
   都分不出是谁触发的；「换出」两个字是颜色之外的第二识别通道（色弱友好）；
2. **纯数字无语义**——折叠态只有 `10.4G` 时，不悬停展开就不知道这个数在说哪件事；
   2–4 字符标签（CPU / 换出）把事件类型压进一行，宽度余量充足
   （`▐█▌ 换出 10.4G` ≈ 90 pt，229 pt 折叠宽度用不到一半）。

数字用 **GB 而不是百分比**：CPU 用 % 是因为 100% 有天然上限语义；swap 的痛点是
「已经换出多少」，GB 直觉更强，且与展开态同一口径。

**展开态**（380×180）：

```
🧠 10.4G 换出 · 内核压力：严重          ← 大数字 + 触发通道标注
30 个采样持续超阈值 · 换出速率 12.3/s · 压缩器占 2.0G   ← 统计行
Chrome        换出 3.2G (footprint 5.1G)              ← Top 3 按 swapped 排序
Xcode         换出 1.8G (footprint 4.4G)
Electron      换出 1.1G (footprint 2.0G)
```

Top 3 排序：**按 `ProcMem.swapped` 降序**（无深探数据者按 footprint 补位）——
「换出受害者」比「内存大户」更贴触发语义；点击行为与 CPU 一致，跳主窗口 stats tab。

**payload**（`kind: "mem"`，折叠/展开按 kind 分支）：

```json
{
  "kind": "mem",
  "pressure": "critical",        // normal | warning | critical
  "trigger": "pressure",         // pressure | swap | both
  "swapUsedGb": 10.4,
  "swapRatioPct": 94,
  "swapoutsPerSec": 12.3,
  "compressorMb": 2048,
  "notchH": 33,
  "top": [{ "name": "Chrome", "pid": 512, "swappedMb": 3277, "totalMb": 5222 }]
}
```

### 12.6 配置模型（新增 2 键，共 8 键）

| 键 | 默认 | 钳制 | 说明 |
| --- | --- | --- | --- |
| `island.mem_sustain_sec` | 30 | 10–300 | B/C 通道共用的持续窗口 |
| `island.swap_pct` | 80 | 50–95 | **已定义未消费**，本期接上；设置卡滑杆已留占位 |

不加 `island.mem_enabled`：总开关 `island.enabled` 已管全局；swap 通道想单独关，
把 `swap_pct` 拉到 95 即可（钳制上限就是关闭位），不值得多一个键 + 多一个开关 UI。

### 12.7 改动清单与排期（M15.6a–d 已全部落地）

| 阶段 | 内容 | 工期 |
| --- | --- | --- |
| M15.6a | `prune-core::island` 新增 `MemMonitor`（三通道判定 + 独立滞回）+ 单测 | 0.5 d |
| M15.6b | `island.rs` 采样循环接压力/swap（与 CPU 同拍）；payload 构造；RateLimiter 拆分 per-event + 全局兜底 | 0.25 d |
| M15.6c | `Island.tsx` 按 kind 分支图标/颜色/文案；**折叠态补事件标签（CPU 已落地部分对齐三段式）**；`SettingsBoard` swap 滑杆接通 | 0.5 d |
| M15.6d | 文档/README 状态更新 | 0.25 d |

合计 **1.5 d**。验收门禁沿用：`cargo test --workspace` + `tsc` + `vite build`，
新增 `MemMonitor` 单测覆盖三通道触发/滞回/频控拆分。

### 12.8 决策记录（2026-11-06 已确认，可开工）

| # | 决策点 | 结论 | 已固化到 |
| --- | --- | --- | --- |
| 1 | swap 通道默认状态 | **默认开**（三通道全做；`swap_pct` 拉到钳制上限 95 即关闭位） | §12.1 / §12.6 |
| 2 | 折叠态读数 | **GB**（`10.4G`，等宽数字；swap 没有 100% 上限语义，GB 直觉更强） | §12.5 |
| 3 | CPU 展示中内存触发 | **抢占**（P0 语义优先；被丢事件靠频控窗口自然重试兜底） | §12.3 |
| 4 | 折叠态信息结构（2026-11-06 追加） | **图标 + 标签 + 读数三段式**（`⚡ CPU 87%` / `▐█▌ 换出 10.4G`；标签是颜色之外的事件第二识别通道） | §12.5 / §13.1 / §13.3 |

无遗留待确认项，按 §12.7 排期执行：M15.6a → M15.6b → M15.6c → M15.6d。

## 13. UI Wireframe（全部尺寸为真实 pt，与 `island_geo` 常量一一对应）

### 13.0 空间总览：窗口、挖孔、菜单栏的叠层关系

```
 ◄────────────────────────── 屏幕宽 1710 pt ──────────────────────────►
 ┌────────────────────────────────────────────────────────────────────┐
 │ App 菜单 ······ ······ ◄── 岛窗口 380×220 ──► ······ 状态图标 ⌘⏦🔋 │
 │ (菜单栏 level 25)           (level 26，盖住菜单栏中部)             │
 └────────────────────────────────────────────────────────────────────┘
                                  ║
                    ╔══════════════╧═╧══════════════╗
                    ║   物理挖孔 185×33（居中，x=855.5）║
                    ╚═══════════════════════════════╝
                     ▲ 挖孔里的像素永远不可见 —— 岛的一切内容从 y=33 开始
```

窗口 y=−2（`EDGE_BLEED=2`：顶边越界 2 pt，防止 1px 白缝）。折叠态黑底只露
挖孔两侧各 22 pt 压在菜单栏底色上 —— 这就是「从刘海里长出来」的视觉来源。

### 13.1 CPU 折叠态（229×50，事件触发后出现）

```
        ◄───────────── 229 pt ─────────────►
        ┌──────────────────────────────────┐ ← y=0 方角（与菜单栏浑然一体）
        │ ▒▒ ╔═══════════════════════╗ ▒▒ │
        │ ▒▒ ║    物理挖孔 185×33     ║ ▒▒ │ ← 0–33：中间看不见
        │ ▒▒ ╚═══════════════════════╝ ▒▒ │   两侧 ▒ 各 22pt 黑条
        │                                  │
        │            ⚡ CPU 87%             │ ← 33–50：17pt 内容条，水平居中
        │                                  │   ⚡=#FF9F0A · 标签「CPU」11pt Medium
        │                                  │   数字 13pt 等宽（三段式：图标+标签+读数）
        └──────────────────────────────────┘ ← 底部两角圆角 20px

标签是设计更新（2026-11-06）：M15.4 首版折叠态只有 `⚡ 87%`，三段式标签已在
M15.6c 对齐（CPU / 内存两条折叠链共用 `CollapsedRow`）。
```

### 13.2 CPU 展开态（380×180，悬停 300 ms 后）

```
        ◄────────────────────── 380 pt ──────────────────────►
        ┌────────────────────────────────────────────────────┐ ← y=0
        │ ▒ ╔════════════════════════════════════════════╗ ▒  │
        │ ▒ ╚════════════════════════════════════════════╝ ▒  │ ← 两侧黑边 97pt
        │                                                    │
        │  ⚡ 87%  CPU 持续偏高                               │ ← 主行：26pt 数字
        │     30 个采样里 92% 超过阈值 · 峰值 98%             │ ← 统计行 11pt 灰 55%
        │                                                    │
        │  chrome                              42%           │ ← Top 3，11pt，
        │  node --max-old-space-size=8192      38%           │   名字截断 250pt，
        │  Xcode                               21%           │   CPU% 橙色等宽
        └────────────────────────────────────────────────────┘
          padding: 12 / 18 / 16 · 全区可点击 → 主窗口 stats tab · 右键 = 收起
```

### 13.3 内存折叠态（M15.6 新增，Warning / Critical 两档）

```
        Warning（🟡 橙 #FF9F0A）          Critical（🔴 红 #FF453A）
        ┌──────────────────────────┐      ┌──────────────────────────┐
        │ ▒▒ ╔═════════════════╗ ▒▒ │      │ ▒▒ ╔═════════════════╗ ▒▒ │
        │ ▒▒ ╚═════════════════╝ ▒▒ │      │ ▒▒ ╚═════════════════╝ ▒▒ │
        │                          │      │                          │
        │        ▐█▌ 换出 10.4G    │      │        ▐█▌ 换出 10.4G    │
        │                          │      │                          │
        └──────────────────────────┘      └──────────────────────────┘
          三段式：图标（内存条 SVG，11×13）+ 标签「换出」11pt Medium
          + 读数 GB 等宽 13pt；标签与读数同色，随等级橙 / 红
          「换出」是事件类型的第二识别通道（颜色之外，色弱友好）
```

### 13.4 内存展开态（M15.6 新增）

```
        ◄────────────────────── 380 pt ──────────────────────►
        ┌────────────────────────────────────────────────────┐
        │  ▐█▌ 10.4G 换出 · 内核压力：严重                    │ ← 主行 26pt GB 数字
        │     30 个采样持续超阈值 · 换出 12.3/s · 压缩器 2.0G │ ← 统计行（三通道
        │                                                    │    命中来源进 payload）
        │  Chrome                    换出 3.2G / 5.1G        │ ← Top 3 按 swapped
        │  Xcode                     换出 1.8G / 4.4G        │   降序；「换出/总量」
        │  node                      换出 1.1G / 2.0G        │   双读数右对齐
        └────────────────────────────────────────────────────┘
```

### 13.5 状态机与时间参数

```
                 事件触发（CPU 或 内存，互斥见 §12.3）
  ┌──────┐ ────────────────────────────► ┌───────────┐ ─悬停 300ms→ ┌──────────┐
  │ idle │                               │ collapsed │              │ expanded │
  └──────┘ ◄────── hide（面板隐藏）────── └───────────┘ ◄─移出500ms─ └──────────┘
     ▲                                      │    │                        │
     │        移出后 2.5s 无 hover          │    └── 6s 无人干预 ────────────┘
     └──────────────────────────────────────┘        （expanded 时 6s 计时暂停）
```

- 所有过渡 260 ms `cubic-bezier(0.32, 0.72, 0, 1)`；出现动画 `island-in` 220 ms
  （scaleY 0.55 → 1，transform-origin 顶部 —— 视觉上从刘海「长出来」）；
- `prefers-reduced-motion` 下全部降级为淡入；
- 命中测试矩形：折叠 229×50 / 展开 380×180（`island_geo::shape_rect`），
  形状之外整窗穿透，不挡下层窗口点击。
