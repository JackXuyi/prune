# Windows 支持方案（W0–W4）

> 目标：让 Prune 在 Windows 上不只是「能编译」，而是核心功能可用。
> 状态：**W0 ✅ / W1 ✅ / W2 ✅ / W3–W4 待开工**（更新于 2026-11-02）

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
| **W2 内存口径** | vm_stat/sysctl → GlobalMemoryStatusEx；footprint → Private Bytes；swap/换出按口径隐藏 | ✅ |
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
# 工具链二选一（脚本自动探测，也可用 MINGW_BIN 指定）
brew install mingw-w64
# 或下载自包含的 llvm-mingw（无依赖，国内网络更稳）

RUSTUP_DIST_SERVER=https://rsproxy.cn scripts/check-windows.sh          # 只做编译检查
scripts/check-windows.sh --build                                       # 额外真链接 CLI，产出 .exe
```

> 用 `x86_64-pc-windows-gnu` 而不是 `msvc`：macOS 上没有 MSVC 工具链，
> rusqlite 的 bundled SQLite 需要本地 C 编译器才能完成 check 阶段。

### 实测结果（2026-11-02）

工具链 llvm-mingw 20261020（ucrt/macos-universal），`check` 与 `--build` 均通过，
`prune.exe` 被 `file` 识别为 `PE32+ executable (console) x86-64`。

交叉编译抓到一处**静态审计漏掉、macOS 上永远编译得过**的缺陷：
`commands.rs` 的 `reveal_impl` 直接调用 `Command::raw_arg`，但它其实是
`std::os::windows::process::CommandExt` 的 trait 方法 → Windows 侧 `E0599`。
已改为函数内 `use` + 统一用 `raw_arg` 传 `/select,"<path>"`。

另外三件工具链/构建事实（详见 `docs/w1-windows-acceptance.md` §5）：

- llvm-mingw 用 compiler-rt，没有 `libgcc.a`/`libgcc_eh.a`，而 rustc 的 windows-gnu
  target 会硬链接它们 → 脚本自动生成 shim 目录并注入 `-L`。
- `gcc -print-libgcc-file-name` 在 llvm-mingw 下谎报，必须用真实链接探针判断。
- `src-tauri` 的 `cdylib` 在 windows-gnu 上撞 65535 导出上限（GNU 链接器限制），
  真实 Windows 走 MSVC 不受影响，故 `--build` 只链 CLI。

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

## 5. W2 内存口径（已完成）

| macOS 现状 | Windows 方案 | 落地方式 |
|---|---|---|
| `vm_stat` 页统计 + 压缩内存 | `GlobalMemoryStatusEx`（物理总量/可用/占用率） | `SystemMem` 改成平台中性模型；压缩内存 `Option`，Windows 为 `None` → UI 隐藏 |
| `kern.memorystatus_vm_pressure_level` | 用 `dwMemoryLoad` 占用率分档 | `Pressure.metric` 区分 `kernel` / `occupancy`，UI 换文案（不把 84 当非法 level） |
| `sysctl vm.swapusage` + swapfile 目录 | pagefile 统计（`GetPerformanceInfo`，经 sysinfo） | swap 用量可用；`fileCount` / `newToday` 为 `None` → UI 隐藏 |
| `footprint` 深探（含 swapped） | `GetProcessMemoryInfo` 的 Private Bytes | `MemSource::Private` 单独标注口径；`swapped` 为 `None` → 换出列/受害者榜隐藏 |
| `lsof` 工作区归属 | 无等价物 | 句柄那一级不可用；标签/继承仍工作，因此**只缺一级**而不是整体隐藏（见 §5.1） |

UI 呈现统一按「隐藏」处理：由前端根据 `null` 字段决定是否渲染对应卡片，
后端只负责把「有没有数据」说清楚，不伪造数值。

### 5.1 与预告的差异（已评估，按更诚实的方案落地）

| 预告 | 实际 | 原因 |
|---|---|---|
| 工作区归属整体隐藏 | 仅缺「句柄」一级，标签/继承保留 | Windows 上 `workspaceStorage` 与 `--folder-uri` 都能读到，隐藏会白丢真实数据；`WorkspaceRef.source` 已如实标注置信度 |
| 压力用「可用内存比例」映射 | 用 `MEMORYSTATUSEX.dwMemoryLoad` | 系统自己给的占用率比「100 − avail/total」更接近任务管理器口径，且省一次计算 |
| 统一用 `*_available` 布尔 | 改成 `Option<T>` | 一处语义两种表达（`false` 与「读到 0」）容易分歧；`Option` 让「没有口径」在类型上不可忽略 |

## 6. 风险

- **真机验证是硬门槛**：`cargo check` 只能证明能编译，托盘定位、透明面板、失焦隐藏、
  Temp 目录删除权限都必须在 Windows 上走查（W3）。
- **Gitee 仓库没有 GitHub Actions**：W4 的 CI 要么接 Gitee Go，要么在 GitHub 建镜像仓；
  在此之前 `scripts/check-windows.sh` 是本仓库唯一可自动化的 Windows 关口。
- **`%LOCALAPPDATA%` 被改写**的罕见场景目前不支持，若要支持需把 `WhitelistEntry.rel`
  换成「base 枚举 + 相对路径」，属后续增强。
