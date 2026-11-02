# W2 验收记录：内存口径跨平台化

> 对应方案：`docs/windows-support-plan.md` §5
> 日期：2026-11-02　范围：`prune-core::mem` + `swap` + 快照模型 + 前端呈现

## 1. 一句话结论

Windows 上「内存仪表」从**无数据**变成**有据可依**：物理内存/已用/可用/swap/压力/进程占用都能读到，
读不到的（压缩内存、swapfile 计数与今日新增、换页颠簸、换出量）**在类型上就是 `None`，UI 直接不渲染**——
不用 0 冒充、不显示「不可用」占位。

## 2. 口径对照

| 读数 | macOS | Windows | 缺失时 |
|---|---|---|---|
| 物理总量 | `sysctl hw.memsize` | `MEMORYSTATUSEX.ullTotalPhys` | — |
| 可用 | `vm_stat` Pages free | `ullAvailPhys` | — |
| 已用 | 总量 − free − inactive − speculative（排除可回收缓存） | 总量 − 可用 | — |
| 压缩内存 | `Pages occupied by compressor` × page size | **无** | `compressorBytes: null`，卡片隐藏 |
| 内存压力 | `kern.memorystatus_vm_pressure_level`（1/2/4） | `dwMemoryLoad` 占用率分档（<80 / 80–91 / ≥92） | `pressure: null`，整段隐藏 |
| 压力原始值含义 | `metric: "kernel"` | `metric: "occupancy"` | — |
| swap 用量 | `sysctl vm.swapusage` | `GetPerformanceInfo`（经 sysinfo `total_swap`/`used_swap`） | — |
| swapfile 个数 / 今日新增 | `/System/Volumes/VM/swapfile*` 统计 | **无此概念**（pagefile 由系统托管） | `fileCount/newToday: null`，两处文案隐藏 |
| 换页速率 | Swapins/Swapouts 差分 | **无** | `thrash: null`，颠簸行与「换出受害者」整块隐藏 |
| 进程占用 | `footprint -p --swapped -j` → phys_footprint | `GetProcessMemoryInfo` → **Private Bytes** | 该进程降级 RSS |
| 进程换出量 | `categories[*].swapped` 求和 | **无** | `swapped: null`，换出列显示 — |
| 工作区归属 | 标签 → 继承 → `lsof` 句柄 | 标签 → 继承（**无句柄**） | `source` 如实标注置信度 |

## 3. 改动清单

| 文件 | 改动 |
|---|---|
| `crates/prune-core/src/win.rs`（新增） | `GlobalMemoryStatusEx` + `K32GetProcessMemoryInfo` 封装；用官方 `windows-sys` 绑定而非手写 `extern` |
| `crates/prune-core/Cargo.toml` | 新增 `[target.'cfg(windows)'.dependencies] windows-sys 0.61`（仅 4 个 feature） |
| `mem/system.rs` | `SystemMem` 改平台中性（total/free/used/compressor:Option/swap 计数:Option）；`vm_stat` 解析降为内部 `VmStat`；`read_pressure` 返 `Option<Pressure>`；新增 `map_occupancy_pct` |
| `swap.rs` | `file_count` / `new_today` 改 `Option`；`collect` 按平台分派（macOS swapfile 统计 / Windows pagefile） |
| `mem/footprint.rs` | `FootprintSample.swapped` 改 `Option`；新增 `probe_windows`（Private Bytes）；`swapped` 在 Windows 恒 `None` |
| `mem/mod.rs` | `MemSource` 新增 `Private` + `MemSource::deep()`；新增 `PressureMetric`；`MemSnapshot` 的 `compressor_bytes`/`pressure`/`thrash` 改 `Option`、`pages_free_bytes` → `free_bytes`；`build_snapshot` 去掉重复的 `physical_total` 入参 |
| `src-tauri/src/sampler.rs` | 装配适配：系统读数失败 → `SystemMem::unavailable()`（不再伪造 0）；只有拿到换页计数才产 `thrash`；`merge_deep` 用 `MemSource::deep()` |
| `crates/prune-cli/src/main.rs` | `mem` 命令无口径时打印「不可用」而不是 0 |
| `src/types/mem.ts` | 同步 `Option` → `| null`；新增 `memSourceLabel`；`MemSource` 加 `private`、`Pressure` 加 `metric` |
| `src/windows/MainWindow.tsx` | 概况页按口径渲染：新增「已用」、压缩内存按 `null` 隐藏、压力行按 `metric` 换文案、换出与颠簸按 `thrash` 隐藏 |
| `src/panels/MenuBarPanel.tsx` | 同上（面板侧压缩、swapfile 文案、颠簸、受害者榜） |
| `src/lib/devFixture.ts` | fixture 跟进新字段 |

## 4. 验证

| 项 | 命令 | 结果 |
|---|---|---|
| macOS 单测 | `cargo test --workspace` | ✅ **145 passed** / 0 failed（新增 `to_system_keeps_total_free_used_consistent`、`occupancy_pct_bands_are_monotonic`、`unavailable_reading_has_no_fake_zeros`） |
| macOS 编译 | `cargo check --workspace --all-targets` | ✅ 0 error |
| macOS 实跑 | `cargo run -p prune-cli -- mem` | ✅ 物理 16.0 GB · 压缩器 5.8 GB · swap 10.3/11.0 GB（94%）· 压力 Warning |
| Windows 编译 | `scripts/check-windows.sh` | ✅ 0 error / 0 新增 warning |
| Windows 真链接 | `scripts/check-windows.sh --build` | ✅ 产出 `prune.exe`（PE32+，48 MB）——证明 `kernel32` 的 `K32GetProcessMemoryInfo` 等符号解析成功 |
| 前端类型 | `npx tsc --noEmit` | ✅ 0 error |
| 前端构建 | `npx vite build` | ✅ 303.96 kB（gzip 91.05） |

## 5. 仍未验证（W3 真机）

- `GlobalMemoryStatusEx` / `Private Bytes` 的**数值正确性**只能在 Windows 上对照任务管理器确认；
  交叉编译只证明符号与结构体对得上。
- Windows 上受保护进程（系统进程、反作弊）`OpenProcess` 会失败 → 表现为 `deep_available=false` 之外的
  单进程降级；真实比例待实测。
- `dwMemoryLoad` 分档阈值（80 / 92）是按任务管理器常见读数定的经验值，需要在真机上调。

## 6. 设计取舍（都写进了代码注释）

1. **`Option` 而不是 `*_available` 布尔**：一处语义（没有口径）不该有两种表达，`Option` 让前端漏处理直接编译报错。
2. **`MemSource::Private` 单独一档**：Private Bytes ≠ footprint（不含换出），合并成一个枚举值等于在 UI 上说谎。
3. **压力带 `metric`**：Windows 的 84 是百分比，macOS 的 2 是 level，同一字段两个含义必须显式区分。
4. **工作区不做整体隐藏**：Windows 的标签/继承归属仍然有效，隐藏会白丢真实数据；只承认「句柄那一级」缺失。
5. **FFI 用 `windows-sys` 不手写**：`PROCESS_MEMORY_COUNTERS` 字段布局写错在 macOS 上永远测不出来
   （编译通过、类型正确、数值是垃圾），这类风险不值得为省一个依赖去承担。
