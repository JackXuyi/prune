# NSPopover 原生化方案

> 2026-10-27 · 技术方案 §6/§8/§11 的 M3 收尾项：用原生 `NSPopover` 替换 M0 spike 的
> 「手动定位无边框窗口」，补齐菜单栏原生感（指向箭头 / Transient 自动收起 / 系统材质）。

## 1. 现状与目标

| | 现状（M0 spike） | 目标（NSPopover） |
|---|---|---|
| 容器 | 无边框 NSWindow，手动 set_position | NSPopover，系统定位 + 指向箭头 |
| 收起时机 | blur 事件 → hide | `Transient` 行为，点外部自动收起（含全屏/切空间） |
| 原生感 | 矩形悬浮块 | 箭头指向菜单栏图标、系统外观 |
| 失败降级 | — | ObjC 链路任一步失败 → 回退旧窗口路径（代码保留） |

Tauri 不暴露 NSStatusItem / NSPopover，因此走 **objc2-app-kit FFI**。版本对齐
Tauri 已锁定的 `objc2-app-kit 0.3.2`（Cargo.lock 已存在）——**不引入新的编译产物**。

## 2. 核心思路：WebView 不重建，只搬家

Tauri 2 的 IPC（invoke / emit / 自定义协议）绑定在 wry 创建的 WKWebView 上，
手工再建一个 WKWebView 无法复用 Tauri 链路。因此：

```
NSWindow(panel, 隐藏)                      NSPopover
┌─────────────────────┐    removeFromSuperview  ┌──────────────┐
│  contentView        │ ─────────────────────▶ │ VC.view      │
│    └─ WKWebView     │     addSubview + 约束   │  └─ WKWebView│
└─────────────────────┘                        └──────────────┘
```

1. `panel.ns_view()` 拿到 WKWebView 指针（Tauri 2 macOS 专有 API）；
2. 首次弹出时把它从原窗口 contentView 摘下，挂到 NSViewController.view；
3. 原窗口从此永不再 show（仍由 Tauri 管理，退出时统一回收）；
4. `popover.contentViewController = VC`，尺寸沿用 380×560。

搬家是一次性的、幂等的：`shown` 状态由 PopoverState 管理，重复 toggle 安全。

## 3. 锚点：tray 图标的坐标换算

NSPopover 的 `showRelativeToRect:ofView:preferredEdge:` 需要一个**窗口内的 NSView**。
Tauri 不暴露 NSStatusItem，无法直接拿状态栏按钮 —— 解法：

- 建一个 1 个 tray 图标大小的**透明锚点窗口**（borderless、不可成为 key、
  `CanJoinAllSpaces | FullScreenAuxiliary`，与状态栏图标行为一致）；
- 位置来自 `tray.rect()`（M0 已验证可用），锚点视图铺满窗口；
- `preferredEdge = .minY`：弹层出现在锚点**下方**，箭头向上指向图标。

### 坐标换算（唯一需要动脑的地方）

| 坐标系 | 原点 | 单位 |
|---|---|---|
| `tray.rect()` | 主屏**左上** | 物理像素（M0 spike 实测结论） |
| Cocoa 屏幕坐标 | 主屏**左下** | 逻辑点 |

```
logical_x = rx / scale
logical_y_top = ry / scale
cocoa_y_bottom_of_window = primary_height_pts − (ry + rh) / scale
```

纯函数 `tray_rect_to_cocoa(rect, scale, primary_h_pts)` 承担换算，**单测覆盖**
（2x / 1x / 多屏负坐标三种 case）。

## 4. 行为细节

| 点 | 决策 |
|---|---|
| 收起后状态同步 | NSPopoverDelegate `popoverDidClose:` 记录时间戳（Transient 收起也触发） |
| 连点抖动 | 点图标收起的瞬间，Transient 也会因「点外部」收起一次 → toggle 对 `距上次收起 < 300ms` 的显示请求直接忽略 |
| 外观 | 默认 appearance；VC.view `wantsLayer` + `cornerRadius 10` + `masksToBounds`，webview 方角内容被裁成 popover 圆角 |
| 锚点窗口 | 背景全透明、`ignoresMouseEvents = true`（不拦点击），仅作几何参照 |
| 主线程 | 全部 ObjC 调用经 `run_on_main_thread`（tray 事件线程不保证主线程） |
| 主窗口 | 「打开主窗口」菜单路径不变 |
| 失焦隐藏 | `attach_blur_to_hide` 移除 —— popover 接管收起时机，原窗口不再显示，blur 永不触发 |

## 5. 降级链

```
ns_view() 失败 / objc2 异常 / tray.rect() 缺失
  → popover 初始化失败 → log + 回退旧路径（窗口 toggle，M0 代码保留为 fallback）
```

初始化只在首次 toggle 尝试一次；成功后固定走 popover，失败后固定走旧路径（不反复重试）。

## 6. 测试与验收

- **单测**：坐标换算纯函数（3 case）+ toggle 状态机（模拟时间戳抖动抑制，逻辑抽为可测纯函数）。
- **编译门禁**：cargo check / cargo test / pnpm build。
- **真机冒烟**：release 启动数秒无 panic（ObjC 初始化在首次点击才发生，静态验证 + 用户目测）。
- **人工目测清单**（写入验收文档）：箭头指向图标、点外部收起、连点不闪、深浅色跟随、
  多屏拖动菜单栏后位置正确。

## 7. 明确不做

- 不做 popover 内 webview 的动态重建 / 即用即毁（§7 内存策略另行评估，本期维持常驻 webview 搬家）；
- 不做 popover 拖拽分离（`detachable` 恒 false）；
- 不改前端布局（380×560 不变；前端已有圆角背景，与 popover 圆角叠加不冲突）。
