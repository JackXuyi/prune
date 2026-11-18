# 开机自启动方案（autostart）

## 背景

Prune 是菜单栏守门员形态（Accessory 常驻），用户希望登录 macOS 后自动在菜单栏待命，不用每次手动启动。这是 Tauri 2 的官方插件场景：`tauri-plugin-autostart`（macOS 走 `~/Library/LaunchAgents/` 下的 LaunchAgent plist，Windows 走注册表 Run 键，Linux 走 desktop 文件）。

## 链路

```
设置页 Switch
  → invoke("autostart_set", { enabled })        自定义 command（不走插件 JS API）
    → app.autolaunch().enable()/disable()       插件 ManagerExt
      → macOS: 写/删 LaunchAgent plist          系统登录后拉起二进制
读取状态
  → invoke("autostart_get")
    → app.autolaunch().is_enabled()             直接问系统，不做本地缓存
```

## 设计决策

1. **用官方插件，不手写 SMAppService**：插件已处理三平台差异；Prune 当前只发 macOS，但插件让未来跨平台零成本。
2. **IPC 收口为自定义 command，不申请插件前端权限**：capabilities 纪律是「最小权限，仅 core:default」（default.json）。前端若直调 `@tauri-apps/plugin-autostart` 需要追加 `autostart:allow-*` 权限；改为 Rust 侧 command 包装，与 updater「不走 JS ACL」同一约定。
3. **状态不落 settings.json**：`is_enabled()` 直接读系统（plist 是否存在），用户在「系统设置 → 登录项」里手动增删也能反映出来；避免两份真相。
4. **默认关**：常驻菜单栏 App 自动开机拉起是侵入性行为，用户显式授权才开。
5. **前端乐观更新 + 失败回滚**：开关要跟手；enable/disable 失败（罕见，权限/磁盘问题）回滚开关并在设置页底部报错。

## 代码落点

| 位置 | 改动 |
|---|---|
| `src-tauri/Cargo.toml` | 新增 `tauri-plugin-autostart = "2"` |
| `src-tauri/src/autostart.rs` | 新模块：`autostart_get` / `autostart_set` 两个 command |
| `src-tauri/src/lib.rs` | `mod autostart` + Builder 注册插件 + invoke_handler 注册命令 |
| `src/lib/ipc.ts` | `autostartGet()` / `autostartSet(enabled)` 封装 |
| `src/components/SettingsBoard.tsx` | 「开机自启动」卡片（外观卡片之后） |
| `src/i18n/messages/namespaces/settings.ts` | `settings.autostart.*` zh/en 文案 |

## 边界风险

- **dev 裸二进制**：LaunchAgent 记录的是当前可执行文件路径。dev 下启用自启动，登录后会拉起 `target/debug/prune-app`，而 dev 前端依赖 vite dev server——不在 dev 会话时窗口空白。验收以 release/bundled 产物为准；dev 下功能仍可用但语义失真。
- **插件 init 参数**：`MacosLauncher::LaunchAgent` + 无附加参数。Prune 无需启动参数。
- **首次调用时序**：插件注册在 Builder 上，setup 前已可用；command 里直接 `app.autolaunch()` 无竞态。

## 验收清单

- [x] `cargo check` / `cargo clippy` 通过（无新增警告）
- [x] `cargo test` 通过
- [x] `pnpm build`（tsc）通过
- [ ] 真机：设置页打开开关 → `~/Library/LaunchAgents/` 出现 plist
- [ ] 真机：关闭开关 → plist 移除
- [ ] 真机（release）：开启后注销重登 → 菜单栏图标自动出现，无主窗口弹出
