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
| **Windows 交叉编译** | `scripts/check-windows.sh`（llvm-mingw 工具链） | ✅ **0 error / 0 warning**（仅剩 2 条历史 example 警告） |
| **Windows 真链接** | `scripts/check-windows.sh --build` | ✅ 产出 `prune.exe`（PE32+ x86-64，48 MB debug） |

> W0 补跑结论见 §5。`cargo check --target x86_64-pc-windows-gnu` 需要
> `x86_64-w64-mingw32-gcc`（rusqlite bundled 要在本机编 SQLite）；
> 本机用 llvm-mingw 20261020（ucrt，macOS universal）跑通，
> 复现命令：`MINGW_BIN=/path/to/llvm-mingw/bin scripts/check-windows.sh --build`。

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
| 内存口径（vm_stat → GlobalMemoryStatusEx、footprint → Private Bytes） | W2 |
| 托盘定位 / 透明面板 / 失焦隐藏在 Windows 实测 | W3 |
| NSIS 安装包 + CI | W4 |
| `%LOCALAPPDATA%` 非默认位置 | 后续增强 |

## 5. W0 补跑记录（2026-11-02）

### 5.1 工具链

`brew install mingw-w64` 在本机不可用（`formulae.brew.sh` SSL 握手失败，
且 brew 的 locks 目录出现 `Operation not permitted`），改用自包含的
**llvm-mingw 20261020（ucrt / macos-universal）**，解压即用、无额外依赖。

llvm-mingw 与 GNU mingw-w64 的差异需要在脚本里补一层：它用 compiler-rt 替代
GCC 运行库，不提供 `libgcc.a` / `libgcc_eh.a`，而 rustc 的 windows-gnu target
会硬链接 `-lgcc -lgcc_eh`。`check-windows.sh` 因此做一次**真实链接探针**，
失败时自动生成 shim（`libgcc.a → libclang_rt.builtins`、
`libgcc_eh.a → libunwind.a`）并注入 `CARGO_TARGET_..._RUSTFLAGS=-L <shim>`。

> 踩坑：不能用 `gcc -print-libgcc-file-name` 判断——llvm-mingw 的 gcc wrapper
> 会把它谎报成 `libclang_rt.builtins-x86_64.a`（文件确实存在，但库名不叫 gcc），
> 于是「检测通过、链接失败」。只有真链一次才可信。

### 5.2 交叉编译抓到的真实缺陷

| 缺陷 | 位置 | 说明 | 修复 |
|---|---|---|---|
| `raw_arg` 不是固有方法 | `src-tauri/src/commands.rs` `reveal_impl` | 它是 `std::os::windows::process::CommandExt` 的 trait 方法，没 `use` 就直接 `E0599`。**这是 mac 上永远发现不了的错误**，静态审计也没抓到 | 函数内 `use std::os::windows::process::CommandExt;`，并统一用 `raw_arg` 传原始命令行片段 |
| 平台专属 import 告警 | `mem/footprint.rs`、`mem/workspace.rs`、`tray.rs`、`disk/whitelist.rs` | `Command`/`Stdio`/`Instant`/`AtomicU64`/`PathBuf`/`SEQ`/`temp_json_path`/`PANEL_W`/`PANEL_H` 在 Windows 侧无消费者 | 逐个加 `#[cfg(target_os = "macos")]` / `#[cfg(unix)]`；测试里的 symlink 变量移进 cfg 块 |
| cdylib 导出上限 | `src-tauri`（`crate-type` 含 `cdylib`） | windows-gnu 下链接报 `too many exported symbols (got 123591, max 65535)` | **不修**：GNU 链接器限制，真实 Windows 构建走 MSVC 无此问题；`--build` 只链 CLI |

### 5.3 结果

| 命令 | 结果 |
|---|---|
| `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` | ✅ exit 0，0 error，0 新增 warning |
| `cargo build --target x86_64-pc-windows-gnu -p prune-cli` | ✅ `target/x86_64-pc-windows-gnu/debug/prune.exe`，`file` 识别为 `PE32+ executable (console) x86-64` |
| `cargo test --workspace`（macOS 回归） | ✅ 142 passed / 0 failed |
| `cargo check --workspace --all-targets`（macOS 回归） | ✅ 0 error |

**含义**：壳层 + 核心库 + CLI 的 `cfg(windows)` 分支全部可编译，且 C 依赖
（bundled SQLite）与 Windows 系统库（`winapi`/`ntapi`/`userenv`/`ws2_32`/`dbghelp`）
能真链接。**这不等于 Windows 上功能可用**——内存口径与磁盘规则仍是 macOS 视角，
归 W2/W3。
