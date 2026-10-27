# NSPopover 验收记录

> 2026-10-27 · 对应 `ns-popover-plan.md`。M3 收尾项「菜单栏原生化」落地。

## 1. 实施结果

| 项 | 结果 |
|---|---|
| WKWebView 搬迁 | ✅ `panel.ns_view()` → 摘出 → NSViewController.view（四边贴边 autoresizing） |
| NSPopover | ✅ 380×560、Transient、Minimal 默认外观、内容圆角 10pt 裁切 |
| 锚点 | ✅ 透明 borderless 窗口（等大 tray 图标、不拦点击、CanJoinAllSpaces、Status 层），每次 show 前按 `tray.rect()` 重定位 |
| 坐标换算 | ✅ 物理/左上原点 → Cocoa 逻辑点/左下原点，单测覆盖 2x/1x/负 x 三 case |
| 抖动抑制 | ✅ delegate `popoverDidClose:` 记时戳 + 300ms 重开抑制（含时钟回拨保守方向） |
| 降级链 | ✅ Uninit→Ready/Failed 状态机；初始化失败恒走旧窗口路径，不重试 |
| 门禁 | ✅ workspace **127 项单测全绿**（prune-app 7→11，+4）· cargo check 零警告 · pnpm build 通过 |
| 冒烟 | ✅ release 二进制启动 6s 无 panic，内存采样/泄漏判定/序列写入后台流水线正常 |

## 2. objc2 0.6.5 实施细节（留档）

| 坑 | 处理 |
|---|---|
| `declare_class!` 已更名为 `define_class!` 且语法重写 | `#[unsafe(super(NSObject))]` / `#[thread_kind]` / `#[name]` 属性式 |
| `#[method]` → `#[unsafe(method(...))]` | 编译器指路后修正 |
| `alloc()` 需要 `MainThreadMarker` | init 入口断言主线程，mtm 透传给 NSWindow/NSViewController |
| `Allocated<T>` 不实现 MessageReceiver | 自定义类构造用 `msg_send![Self::class(), new]` |
| `NSPopover::initWithContentViewController` 未生成 | `+new` 后 `setContentViewController(Some(&vc))` |
| MainThreadOnly 对象不能进 static Mutex | `MainThreadBox<T>`（仅解除 Send 限制，访问恒在主线程） |
| objc2 部分绑定居 assert 为安全 | 删多余 unsafe 块，最终零警告 |

## 3. 实施偏差（相对方案）

| # | 方案 | 实际 | 理由 |
|---|---|---|---|
| D-P1 | §4 移除 `attach_blur_to_hide` | **保留** | popover 成功后 blur 永不触发（无害）；若走降级路径则旧窗口仍需失焦隐藏 |
| D-P2 | §4 `detachable 恒 false` | 未显式设置 | objc2 未生成 `setDetachable(bool)`（属性是 VC 槽位），默认行为即不可分离 |

## 4. 人工目测清单（GUI 交互无法自动化）

- [ ] 左键点 tray 图标 → popover 从图标下方弹出，箭头指向图标
- [ ] 点 popover 外任意处 → 自动收起
- [ ] 快速连点图标 → 不出现「收起又立即弹出」的闪跳
- [ ] 深色/浅色系统外观切换 → popover 边框材质跟随
- [ ] 菜单栏图标挪位后再点 → 位置正确跟随
- [ ] 外接显示器（不同缩放）→ 弹出位置不错位
- [ ] 全屏 App 内点击 → 可正常弹出（FullScreenAuxiliary）
- [ ] 右键菜单「打开主窗口」不受影响

## 5. 已知边界

- 锚点窗口仅是几何参照：点击它不响应（`ignoresMouseEvents`），收起时机完全由 Transient 行为接管。
- 若某系统版本 `tray.rect()` 返回 None（M0 风险清单），popover 初始化失败 → 自动降级旧窗口路径，功能不丢、只是没有原生箭头。
- webview 搬家后原 panel 窗口永久空置（Tauri 侧仍持有，退出时统一回收），不做动态重建（方案 §7 不做项）。
