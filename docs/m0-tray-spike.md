# M0 #2 · Tauri tray 面板定位验证

> 2026-10-23 · 对应 `prune-tauri-technical-plan.md` §6 / §10 #2
> 结论一句话：**`tray.rect()` 可用，坐标是「物理像素 + 主屏左上原点」，与 Tauri 窗口定位同系，不需要 Cocoa 翻转。**

---

## 1. 实测环境

| 项 | 值 |
|---|---|
| 机型 | Mac mini（Apple Silicon / arm64） |
| 显示器 | 外接屏，scale factor 2（物理高 2214 px / 逻辑高 1107 pt） |
| 栈 | Tauri 2.12.5 · tray-icon 0.25.1 · macOS |
| 面板配置 | 380×560 · `decorations:false` + `transparent:true` + `shadow:false` + `alwaysOnTop` + `macOSPrivateApi:true` |

## 2. 实测数据

```
[m0-tray] tray.rect() 原始值: position=(2066, 0) size=(68, 76)
[m0-tray] 主屏: 物理高 2214 px · scale 2 · 逻辑高 1107 pt
```

## 3. 关键判断：坐标系是什么

| 假设 | 是否成立 | 依据 |
|---|---|---|
| Cocoa 坐标（主屏左下原点、y 向上、逻辑 pt） | ❌ | 若成立，y 应 ≈ 1031（1107 − 76），实测 **y = 0** |
| **主屏左上原点、y 向下** | ✅ | 菜单栏图标恰好在屏幕最顶部，y = 0 吻合 |
| **物理像素（非逻辑点）** | ✅ | size = 68×76 为偶数大值，÷2 = 34×38 pt，符合菜单栏图标尺寸 |
| 与 Tauri `set_position` 同系 | ✅ | Tauri 窗口定位即物理像素、左上原点、全局坐标 |

**由此推翻技术方案 §6 里「`tray.rect()` 返回 Cocoa 坐标需翻转」的担心** —— 至少在本机 macOS 版本上不需要翻转，直接：

```
panel_x = rect.x + rect.width  − panel_width  − margin
panel_y = rect.y + rect.height + margin
```

（首版按 Cocoa 翻转实现，算出 panel y = 1115 > 逻辑高 1107，面板会落到屏幕外 —— 实测数据直接暴露了这个问题。）

## 4. 已落地实现

| 能力 | 实现 |
|---|---|
| 多屏定位 | `available_monitors()` 按 rect 坐标找 tray 所在屏幕，取该屏 `scale_factor` 换算，并把面板钳制在屏内 |
| 逻辑/物理兜底 | `rect.position` 若返回 `Logical` 变体，先乘 scale 再参与计算（当前 macOS 未出现，保留兼容） |
| 取不到 rect | 降级到主屏右上角（不崩、不静默） |
| 面板开关 | 左键 toggle；`WindowEvent::Focused(false)` → hide |
| Accessory | `ActivationPolicy::Accessory`，菜单栏常驻、不占 Dock |
| 兜底入口 | 右键菜单：打开主窗口 / 退出（Accessory 模式下没有 Dock 图标可点） |

## 5. 待补充验证

- [ ] 目测：面板是否精准落在图标正下方、右缘对齐
- [ ] 多屏：把 tray 图标所在屏幕换成副屏 / 不同 scale 的屏幕再点一次
- [ ] 反复开关 10 次，确认位置不漂移
- [ ] 全屏应用、Stage Manager、屏幕缩放切换后的表现
- [ ] 系统睡眠唤醒后 `rect()` 是否仍准确

## 6. 对里程碑的影响

技术方案 §10 把 #2（tray 定位）列为「新的生死线」。本机实测证明**定位可行且坐标系清晰**，这条风险可降级为「已解决，待多屏回归」。

剩余 M0 项：

- #3 `footprint` 并发 40 进程总耗时（判定 < 1.5 s）
- #4 Rust 全盘扫描耗时 & CPU（判定 < 20 s 且不卡 UI）
- #5 `footprint` 对系统进程的失败率与降级链

## 7. 复现

```bash
pnpm tauri dev          # 左键点菜单栏剪刀图标
# 观察 stdout 的 [m0-tray] 日志
```
