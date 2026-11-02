# Windows 支持方案（W0–W4）

> 目标：让 Prune 在 Windows 上不只是「能编译」，而是核心功能可用。
> 状态：**W0 ✅ / W1 ✅ / W2–W4 待开工**（更新于 2026-11-02）

## 1. 结论与前提

Prune 原本是 macOS-first 实现：壳层（Tauri）用 `#[cfg]` 门隔离得很干净，
但 `prune-core` 的**数据口径**（内存采样、磁盘白名单）是纯 macOS 视角。

两条已拍板的口径决策：

| 决策点 | 结论 | 理由 |
|---|---|---|
| 无等价物的 macOS 特性在 Windows 上怎么呈现 | **隐藏**（不显示「不可用」占位） | 避免 UI 出现一堆不可操作项；沿用现有「不伪造数值」原则 |
| footprint 深探的 Windows 替代 | **Private Bytes**（`GetProcessMemoryInfo`） | 语义最接近 footprint 的「真实占用」，且零新增重依赖 |

## 2. 分期

| 阶段 | 范围 | 状态 |
|---|---|---|
| **W0 编译基线** | Windows target 交叉编译、清理 unix-only 代码导致的编译失败 | ✅ |
| **W1 低成本补丁** | 路径口径统一、白名单平台化、reveal/kill 分流、workspace 路径 | ✅ |
| **W2 内存口径** | vm_stat/sysctl → GlobalMemoryStatusEx；footprint → Private Bytes；swap/工作区降级 | 待开工 |
| **W3 壳层体验** | 面板定位实测、托盘常驻、失焦隐藏；Accessory 的 Windows 等价物验证 | 待开工 |
| **W4 打包分发** | NSIS 安装包、签名策略、CI（Windows runner 跑 check + test） | 待开工 |

## 3. W0 编译基线

Windows 上原本编译不过的两处：

| 问题 | 位置 | 处理 |
|---|---|---|
| `libc::kill` 在 Windows 不存在 | `src-tauri/src/commands.rs` | 下沉到 `prune_core::mem::process::kill`，unix 走 signal、Windows 走 sysinfo |
| `use crate::popover` 未加 cfg（`popover` 模块仅 macOS 编译） | `src-tauri/src/tray.rs` | 加 `#[cfg(target_os = "macos")]` |

交叉编译脚本：`scripts/check-windows.sh`

```bash
brew install mingw-w64            # rusqlite bundled 需要 C 工具链
RUSTUP_DIST_SERVER=https://rsproxy.cn scripts/check-windows.sh
```

> 用 `x86_64-pc-windows-gnu` 而不是 `msvc`：macOS 上没有 MSVC 工具链，
> rusqlite 的 bundled SQLite 需要本地 C 编译器才能完成 check 阶段。

## 4. W1 改动清单

| # | 内容 | 文件 |
|---|---|---|
| 1 | 平台路径口径统一：`home_dir` / `home_dir_or_root` / `config_dir` / `config_dir_in` / `system_roots` | `crates/prune-core/src/paths.rs`（新增） |
| 2 | 替换所有 `HOME` 硬编码（GUI、CLI、MCP、sampler） | `src-tauri/src/{scan,sampler}.rs`、`crates/prune-cli/src/{main,mcp}.rs` |
| 3 | 白名单增加 `os` 标记（`All` / `MacOnly` / `WindowsOnly`），`resolve` / `entry` / `is_whitelisted` 按平台过滤 | `crates/prune-core/src/disk/whitelist.rs` |
| 4 | Windows 规则集（9 条，全部 `~/AppData/Local/...`） | 同上 |
| 5 | `reveal_path` 平台分流：`open -R` / `explorer /select,` / `xdg-open` | `src-tauri/src/commands.rs` |
| 6 | `kill_process` 平台分流 | `crates/prune-core/src/mem/process.rs` |
| 7 | 编辑器 workspaceStorage 走 `config_dir_in`（macOS `Library/Application Support` / Windows `AppData/Roaming`） | `crates/prune-core/src/mem/workspace.rs` |
| 8 | 噪声路径判定归一化 `\` → `/`，补 Windows 噪声段（`/AppData/`、`/Windows/`、`/Program Files/`、`/ProgramData/`） | 同上 |
| 9 | 项目根推断兼容 `\` 分隔与盘符（`C:/Users/...`） | 同上 |
| 10 | `open_file_paths`：Windows 无 `lsof` → 返回空表（上层按不可用处理） | 同上 |
| 11 | `host_arch` 增加 `ARCH` 兜底（Windows 无 `uname`） | `crates/prune-core/src/classify/mod.rs` |
| 12 | `volume_info` 非 unix 分支改用 `sysinfo::Disks`（存储域分母不能为空） | `crates/prune-core/src/disk/mod.rs` |

### Windows 白名单规则集

| key | 路径（相对 home） | 级别 |
|---|---|---|
| `win-temp` | `AppData/Local/Temp` | L2 |
| `win-npm` | `AppData/Local/npm-cache` | L2 |
| `win-pnpm` | `AppData/Local/pnpm/store` | L2 |
| `win-yarn` | `AppData/Local/Yarn/Cache/v6` | L2（暂不参与 A1 判死） |
| `win-pip` | `AppData/Local/pip/Cache` | L2 |
| `win-nuget` | `AppData/Local/NuGet/Cache` | L2 |
| `win-electron` | `AppData/Local/electron/Cache` | L2 |
| `win-crashdumps` | `AppData/Local/CrashDumps` | L2 |
| `win-playwright` | `AppData/Local/ms-playwright` | L3 |
| `win-cypress` | `AppData/Local/Cypress/Cache` | L3 |

> `win-yarn` 的 `a1_scope` 保持 `false`：A1「目录名即平台元数据」的判死逻辑只在 macOS
> 实测过，Windows 上先不接管，避免误判。
>
> 若管理员改过 `%LOCALAPPDATA%`（非默认位置），上述 home 相对路径会失配，当前不覆盖。

## 5. W2 预告（内存口径，未开工）

| macOS 现状 | Windows 方案 | 降级处理 |
|---|---|---|
| `vm_stat` 页统计 + 压缩内存 | `GlobalMemoryStatusEx`（物理总量/可用） | 压缩内存字段隐藏 |
| `kern.memorystatus_vm_pressure_level` | 可用内存比例映射三级 | 明确标注为估算 |
| `sysctl vm.swapusage` + swapfile 目录 | 页面文件统计（`GetPerformanceInfo`） | 颠簸速率隐藏 |
| `footprint` 深探（含 swapped） | `GetProcessMemoryInfo` Private Bytes | swapped 字段隐藏 |
| `lsof` 工作区归属 | 不可用 | 归属列隐藏 |

UI 呈现统一按「隐藏」处理：由前端根据 `*_available` 字段决定是否渲染对应卡片，
后端只负责把「有没有数据」说清楚，不伪造数值。

## 6. 风险

- **真机验证是硬门槛**：`cargo check` 只能证明能编译，托盘定位、透明面板、失焦隐藏、
  Temp 目录删除权限都必须在 Windows 上走查（W3）。
- **Gitee 仓库没有 GitHub Actions**：W4 的 CI 要么接 Gitee Go，要么在 GitHub 建镜像仓；
  在此之前 `scripts/check-windows.sh` 是本仓库唯一可自动化的 Windows 关口。
- **`%LOCALAPPDATA%` 被改写**的罕见场景目前不支持，若要支持需把 `WhitelistEntry.rel`
  换成「base 枚举 + 相对路径」，属后续增强。
