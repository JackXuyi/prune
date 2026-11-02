# W1 验收记录：Windows 平台适配（W0 + W1）

> 对应方案：`docs/windows-support-plan.md`
> 日期：2026-11-02　范围：W0 编译基线 + W1 低成本补丁

## 1. 验收矩阵

| 项 | 验证方式 | 结果 |
|---|---|---|
| macOS 全量编译 | `cargo check --workspace --all-targets` | ✅ 0 error（仅 2 条历史 example 警告） |
| macOS 全量单测 | `cargo test --workspace` | ✅ 142 passed / 0 failed（1 ignored 为历史用例） |
| 白名单平台过滤 | `entries_are_filtered_by_platform` 等 6 个新用例 | ✅ |
| 路径口径 | `paths::tests` 4 个用例 | ✅ |
| 噪声判定（反斜杠） | `noise_paths_handle_windows_separators` | ✅ |
| Windows 规则结构 | `windows_entries_are_home_relative_appdata_paths` | ✅ |
| **Windows 交叉编译** | `scripts/check-windows.sh` | ⚠️ **未执行**（mingw-w64 安装被取消） |

> ⚠️ 是本轮唯一的缺口：`cargo check --target x86_64-pc-windows-gnu` 需要
> `x86_64-w64-mingw32-gcc`（rusqlite bundled 要在本机编 SQLite）。
> 补跑方式：`brew install mingw-w64 && RUSTUP_DIST_SERVER=https://rsproxy.cn scripts/check-windows.sh`。

## 2. 改动清单

| 文件 | 改动 |
|---|---|
| `crates/prune-core/src/paths.rs` | 新增：`home_dir` / `home_dir_or_root` / `config_dir` / `config_dir_in` / `system_roots` |
| `crates/prune-core/src/lib.rs` | 注册 `paths` 模块 |
| `crates/prune-core/src/disk/whitelist.rs` | 新增 `Os` 枚举与 `os` 字段；`resolve` / `entry` / `is_whitelisted` 按平台过滤；新增 10 条 Windows 规则；补 4 个结构约束用例 |
| `crates/prune-core/src/disk/mod.rs` | `volume_info` 非 unix 分支改用 `sysinfo::Disks` |
| `crates/prune-core/src/mem/process.rs` | 新增跨平台 `kill(pid, force)`：unix 走 signal，Windows 走 `sysinfo` |
| `crates/prune-core/src/mem/workspace.rs` | `known_workspaces` 走 `config_dir_in`；`path_is_noise` 归一化 `\`→`/` 并补 Windows 噪声段；`common_project_root` 支持盘符；`open_file_paths` Windows 返回空表；`file_uri_to_path` 去掉盘符前导斜杠 |
| `crates/prune-core/src/classify/mod.rs` | `host_arch` 增加 `ARCH` 兜底 |
| `src-tauri/src/commands.rs` | `reveal_path` 三分支（`open -R` / `explorer /select,` / `xdg-open`）；`kill_process` 改为调用 core 的跨平台实现 |
| `src-tauri/src/tray.rs` | `use crate::popover` 加 `#[cfg(target_os = "macos")]`（否则 Windows 编译不过） |
| `src-tauri/src/{scan,sampler}.rs` | 主目录统一走 `prune_core::paths` |
| `crates/prune-cli/src/{main,mcp}.rs` | 同上；`default_db_path` 改用 `paths::config_dir()` |
| `src-tauri/Cargo.toml` | 移除已无引用的 `libc` 依赖 |
| `scripts/check-windows.sh` | 新增：Windows 交叉编译检查入口 |

## 3. 行为影响（macOS 侧）

- 白名单在 macOS 上**只少不加**：所有原有条目保留，仅打了 `MacOnly` 标记，
  `resolve` / `is_whitelisted` 的过滤结果与之前完全一致。
- 主目录、配置目录取值逻辑等价（仍读 `$HOME`），仅新增 Windows 分支。
- `volume_info` macOS 分支未改。
- 删除链路未改：`reclaim` → `trash` crate（本机废纸篓），Windows 上对应回收站。

## 4. 遗留

| 项 | 阶段 |
|---|---|
| Windows 交叉编译实跑（等 mingw 就绪） | W0 收尾 |
| 内存口径（vm_stat → GlobalMemoryStatusEx、footprint → Private Bytes） | W2 |
| 托盘定位 / 透明面板 / 失焦隐藏在 Windows 实测 | W3 |
| NSIS 安装包 + CI | W4 |
| `%LOCALAPPDATA%` 非默认位置 | 后续增强 |
