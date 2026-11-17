# 主窗口打开时显示 Dock 图标

> 状态：**已实施**（2026-11-17，`cargo check` / `clippy` 通过；Dock 显示/隐藏与
> Reopen 行为待真机 UI 验收）。实施时注意：tao 的 `set_dock_visibility` 底层走
> `TransformProcessType`，快速切换会触发 macOS 残留多个 Dock 图标的系统 bug，
> 故全程沿用 `set_activation_policy`（与启动代码同一机制），防连点由 AtomicBool 去重承担。

> 目标：Prune 保持「常驻菜单栏、不占 Dock」的守门员形态不变；当用户打开主窗口时，
> 程序坞（Dock）出现 Prune 图标；主窗口隐藏后图标随之消失。全程不改 Info.plist、
> 不改启动行为。

## 一、背景与现状

- 启动时 `lib.rs:135` 设置 `ActivationPolicy::Accessory`，App 不出现在 Dock，只有菜单栏图标。
- 打开主窗口的所有入口已收敛到唯一函数 `tray::show_main`（`tray.rs:1041`）：
  - 面板按钮 / 灵动岛 → IPC `commands::open_main`（`commands.rs:90`）→ `show_main`
  - 托盘菜单 / 双击 → `open_main_from_tray`（`tray.rs:908`）→ `show_main`
- 主窗口关闭路径同样唯一：`lib.rs:270-278` 的 `CloseRequested` 拦截 → `hide()`（只藏不销毁）。
- 结论：**两个收口点各插一行策略切换即可**，无需触碰任何前端代码。

## 二、整体链路

```
打开主窗口（面板 / 岛 / 托盘）
        ↓
tray::show_main()
        ↓ set_activation_policy(Regular)   ← 先切策略
        ↓ position_main + show + focus
Dock 出现图标，App 获得菜单栏与前台焦点

主窗口点红叉
        ↓ CloseRequested（lib.rs 拦截，prevent_close）
        ↓ main.hide()
        ↓ set_activation_policy(Accessory) ← 后切策略
Dock 图标消失，回到纯菜单栏常驻形态
```

## 三、关键设计决策

| 决策 | 原因 |
|---|---|
| 用运行时 `AppHandle::set_activation_policy` 切换，不动 Info.plist | Tauri 2 在 macOS 上支持运行时改 NSApp 激活策略；`macOSPrivateApi: true` 已开启，无需额外配置。Info.plist 写死 `LSUIElement` 反而与启动时代码设置重复、易失同步 |
| 切换点收口在 `tray::show_main` 与 `lib.rs` 的 CloseRequested，不散落在各调用方 | 打开路径只有一个函数、关闭路径只有一个事件分支；在收口点改一次，面板/岛/托盘/深链所有入口自动生效 |
| 新建 `dock.rs` 小模块，提供 `ensure_dock_visible()` / `ensure_dock_hidden()` | 策略切换要带状态去重（见下），且 `#[cfg(target_os = "macos")]` 守门集中一处，Windows 侧零改动 |
| 用 `AtomicBool` 记录当前是否处于 Regular，重复调用直接短路 | 策略切换不是幂等无感操作：连续切换会让 Dock 图标反复弹跳、菜单栏闪烁。打开入口（面板连点、三连击去重兜底）可能短时间多次到达 |
| 先切 Regular 再 `show + set_focus` | 顺序反了会导致窗口已显示但 App 仍是 Accessory，`set_focus` 抢不到前台焦点，用户看到窗口浮在后面 |
| 先 `hide` 再切回 Accessory | 反过来会先丢焦点再藏窗口，视觉上闪一下 |
| CloseRequested 的注释同步更新 | 原注释写「Accessory 模式没有 Dock 图标可再打开」——功能落地后这句话的前提变了，防止后来人被误导 |
| 处理 `RunEvent::Reopen` | 切到 Regular 后用户可能点 Dock 图标。Tauri 2 的 Reopen 事件（macOS）到达时若主窗口不可见则调 `show_main`，保证 Dock 点击永远有响应 |

## 四、代码落点

| 文件 | 改动 |
|---|---|
| `src-tauri/src/dock.rs`（新建） | `ensure_dock_visible(app)` / `ensure_dock_hidden(app)`：`cfg(target_os = "macos")` 内做策略切换 + AtomicBool 去重；非 macOS 空实现（模块级 cfg，命令注册模式与 island 模块一致） |
| `src-tauri/src/lib.rs` | `mod dock;`；CloseRequested 分支在 `hide()` 后调 `dock::ensure_dock_hidden`；`.run()` 闭包补 `RunEvent::Reopen` → 主窗口不可见时 `tray::show_main`；更新原 Accessory 相关注释 |
| `src-tauri/src/tray.rs` | `show_main` 开头调 `dock::ensure_dock_visible(app)`（在 `position_main` 之前） |

伪代码（`dock.rs` 核心逻辑）：

```rust
static DOCK_REGULAR: AtomicBool = AtomicBool::new(false);

pub fn ensure_dock_visible(app: &AppHandle) {
    if DOCK_REGULAR.swap(true, Ordering::Relaxed) { return; }  // 已是 Regular
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
}

pub fn ensure_dock_hidden(app: &AppHandle) {
    if !DOCK_REGULAR.swap(false, Ordering::Relaxed) { return; } // 已是 Accessory
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}
```

## 五、边界与风险

| 场景 | 行为 | 说明 |
|---|---|---|
| 全屏 App 的 Space 内打开主窗口 | 图标照常出现 | `FullScreenAuxiliary` 已处理窗口弹窗；Dock 切换与 Space 无关 |
| Cmd+Q 退出 | 进程直接退出，无需处理 | 退出时 App 整体消失，策略无所谓 |
| 主窗口显示期间打开 `update` 弹窗 | Dock 图标已在（若 main 可见）；main 隐藏后弹 update 窗口则图标不出现 | 按需求只跟随主窗口。update 窗口是低频短会话，不扩展；如后续要跟随，在 `check_and_prompt` 里加一行 `ensure_dock_visible` 即可 |
| 快速连点打开入口 | 去重短路，图标不闪 | AtomicBool swap 一次完成判断+置位 |
| App 失焦（点了别的 App）但主窗口未关 | 图标保持显示 | 与常规 macOS App 一致：窗口在就显示图标；点红叉才收 |
| Windows / Linux | 完全无行为变化 | `cfg` 守门；Windows 主窗口本来就占任务栏 |
| `tauri dev` 开发模式 | 同正式包行为 | 策略切换不依赖打包环境 |

已知小代价：切到 Regular 后 App 会获得默认菜单栏（应用名 + Edit 等空菜单）。主窗口当前
无自定义菜单，视觉上与常规 App 一致，属可接受形态；若嫌默认菜单多余，可后续用
`Menu::default` 裁剪，不在本期范围。

### 追加：dev 模式 Dock 显示通用 exec 图标（已修复）

真机验收发现：dev 模式跑的是裸二进制（无 .app bundle），切到 Regular 后 Dock 只能显示
macOS 通用「exec」图标（release 打包有 bundle 图标，不受影响）。修复：`dock::set_app_icon`
在启动时把 `icons/icon.png`（512²，`include_bytes!` 编译进二进制）经 `run_on_main_thread`
设给 `NSApp.setApplicationIconImage`——dev 补齐缺失图标，release 下重复设置同款图标无害。
依赖变化：objc2-app-kit 加 `NSImage` feature，objc2-foundation 加 `NSData` feature
（只加 feature 不引入新 crate）。

## 六、验收清单

- [ ] `cargo check` / `cargo clippy` 通过（含 `--target aarch64-apple-darwin` 实机）
- [ ] 冷启动：Dock 无图标，仅菜单栏图标（现状不回归）
- [ ] 面板点任意行打开主窗口 → Dock 出现图标且带前台焦点
- [ ] 托盘「打开主窗口」/ 双击 → 同上
- [ ] 主窗口点红叉 → 窗口隐藏，Dock 图标消失，菜单栏图标仍可交互
- [ ] 主窗口可见时点击 Dock 图标 → 窗口聚焦（不重复弹出/闪烁）
- [ ] 面板快速连点打开主窗口 → 图标只出现一次，无弹跳闪烁
- [ ] 灵动岛/面板的所有 `openMain` 深链（disk/net/ports/memory/resources）→ 行为不变
- [ ] 检查更新弹窗流程不受影响
- [ ] Windows 构建 `pnpm tauri build` 通过（dock 模块零影响）

## 七、工作量

单点改动，Rust 侧三处 + 新模块一个：预计 0.5 天含验收。不涉及前端、不涉及
capabilities / i18n / 版本号。
