# Prune · Tauri 技术方案

> 2026-10-22 · 基于本机实测（Rust 1.98.1 已装 / `footprint` 命令权限已验证）
> 上游功能定义见 `prune-feature-spec-v2.md`，本文只解决**怎么实现**。

---

## 0. 结论先行

| # | 结论 |
|---|---|
| 1 | 技术栈 **Tauri 2 + React 19 + TypeScript + TailwindCSS 4 + Zustand + Rust 核心** |
| 2 | 🔑 **内存数据源改用系统自带 `/usr/bin/footprint` 命令**（非特权、JSON 输出、含换出量）—— 绕开整个 Mach API 权限地狱 |
| 3 | **不启用 App Sandbox**（否则子进程无法访问 mach） |
| 4 | Rust 核心抽成 `prune-core` crate，GUI 与 CLI 共享 —— 这是 Tauri 相对 SwiftUI 的结构性优势 |
| 5 | ⚠️ **必须解决 WebView 自身内存**：采用「后台 Rust 常采 + WebView 即用即毁」，否则"资源守门员"自己吃掉 150 MB |

---

## 1. 决定性实测：内存数据不用碰 Mach API

### 1.1 三条路径的实测结果

| 路径 | 命令/API | 实测结果 | 判定 |
|---|---|---|---|
| A · 自调 `task_for_pid` | mach2 FFI | 需实测，且预期非 root 失败 | ⚠️ 高风险 |
| B · `proc_pidinfo` RSS | sysinfo / libproc | 可用，但**漏掉已换出部分**（next-server 只显示 113 MB / 真实 3432 MB） | ❌ 口径失真 |
| C · **fork `/usr/bin/footprint`** | `footprint -p <pid> -j` | ✅ **非特权成功，JSON 带 swapped 明细** | ✅ **采纳** |

### 1.2 实测证据

```
$ /usr/bin/footprint -p 22325 --swapped -j /tmp/fp1.json
node [22325]: 64-bit    Footprint: 3432 MB

{
  "unit": "byte",
  "processes": [{
    "name": "node",
    "pid": 22325,
    "footprint": 3598607488,        ← 3.35 GB，与活动监视器截图吻合
    "categories": {
      "app-specific tag 16": {
        "dirty":   2168487936,
        "swapped": 2166620160,      ← 换出量 2.02 GB，B4「换出受害者」直接可算
        ...
      }
    }
  }]
}
```

**关键约束（实测）**：
- `footprint -a`（全量）→ `footprint: Must run as root.` ❌ **不可用**
- `footprint -p <pid>`（单进程）→ ✅ 非特权可用
- 单次耗时 **0.12s**，输出 38 行 / 5.9 KB JSON

### 1.3 由此确定的采样架构

| 频率 | 数据源 | 范围 | 成本 |
|---|---|---|---|
| **10 s** | `sysinfo` 进程枚举（RSS、cmd、ppid、exe） | 全量 ~600 进程 | 极低（单次 <20 ms） |
| **5 min** | `footprint -p <pid> -j` | 仅 Top N（RSS > 80 MB 或非系统进程，约 20~40 个） | 0.12s × N，**并发 8 路 → 约 0.6 s** |
| **按需** | 用户点开某进程时补一次 | 单进程 | 0.12 s |

用 RSS 做廉价的筛选器，只对"值得深挖"的进程付 footprint 的成本。这既省电又解决了 `-a` 需要 root 的问题。

---

## 2. 整体架构

```
┌──────────────────────────────────────────────────────┐
│ 前端 · React 19 + TS + Tailwind 4                     │
│  ├ MenuBarPanel   菜单栏面板（首屏）                   │
│  ├ MainWindow     6 tab：统计（默认）/应用资源/内存×2/存储/端口 │
│  ├ stores/        Zustand（mem / disk / port / netSpeed）│
│  └ charts/        自绘 SVG sparkline（不引图表库）     │
└───────────────┬──────────────────────────────────────┘
      IPC: invoke() 拉数据  |  Channel 推实时采样流
┌───────────────▼──────────────────────────────────────┐
│ Rust 常驻进程（无 WebView 时仍运行）                   │
│  ┌────────────────────────────────────────────┐      │
│  │ prune-core（独立 crate，GUI/CLI 共享）        │      │
│  │  ├ mem/     sysinfo 枚举 + footprint 调用    │      │
│  │  ├ swap/    /System/Volumes/VM/ 监控         │      │
│  │  ├ disk/    ignore+rayon 并行扫描            │      │
│  │  ├ classify/ 跨平台二进制 / 家族归类          │      │
│  │  ├ reclaim/  安全删除（废纸篓 + 白名单）      │      │
│  │  ├ net/     端口反查 + 连接计数 + 网速差分（lsof / iphlpapi / nettop）│
│  │  ├ appdisk/ 应用磁盘三层（bundle/cache/data）+ 批量本体  │
│  │  └ db/      rusqlite 时间序列                │      │
│  └────────────────────────────────────────────┘      │
│  sampler：10s RSS / 5min footprint                    │
│  port_sampler：30s（仅端口 tab 可见）                 │
│  net_sampler：1s nettop 轮询（仅应用资源 tab 可见）   │
└──────────────────────────────────────────────────────┘
```

---

## 3. 目录结构

```
prune/
├── package.json                 # vite + react 19
├── vite.config.ts
├── tailwind.config.ts
├── src/                         # 前端
│   ├── App.tsx
│   ├── panels/MenuBarPanel.tsx
│   ├── windows/MainWindow.tsx
│   ├── components/{StatsBoard,AppMemoryList,AppResourceCard,AppResourceGrid,CellBar,PortList,ReclaimBoard,LeakCard,Sparkline,primitives}.tsx
│   ├── stores/{memStore,diskStore,portStore,netStore}.ts
│   └── lib/ipc.ts               # invoke 封装
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       ├── lib.rs               # Tauri 入口 + 命令注册
│       ├── tray.rs              # 菜单栏图标与面板定位
│       ├── sampler.rs           # 后台采样线程
│       ├── port_sampler.rs      # 端口采样（可见性门控）
│       ├── net_sampler.rs       # 网速采样（1 s 轮询 + 门控，M10）
│       └── commands.rs
├── crates/
│   ├── prune-core/              # 共享核心（lib）
│   └── prune-cli/               # `prune` 命令行（bin）
└── docs/
```

---

## 4. 依赖清单

### 4.1 Rust（`src-tauri/Cargo.toml` + `crates/prune-core`）

| crate | 用途 | 备注 |
|---|---|---|
| `tauri` 2 | 应用框架 | 关闭 `app-sandbox` |
| `tauri-plugin-updater` | Sparkle 替代 | 需签名 |
| `tauri-plugin-autostart` | 替代 SMAppService | |
| `tauri-plugin-single-instance` | 单实例 | |
| `sysinfo` | 进程枚举 / RSS / 系统内存 | 跨平台、快 |
| `walkdir` + `ignore` | 磁盘扫描 | `ignore` 尊重 .gitignore，支持并行 |
| `rayon` | 并行扫描 + 并发调 footprint | |
| `rusqlite`（bundled） | 时间序列存储 | 单文件，无需额外依赖 |
| `serde` / `serde_json` | footprint JSON 反序列化 | |
| `tokio` | 采样调度 / 异步 spawn 子进程 | |
| `dirs` | 标准缓存路径定位 | |
| `trash` | **删除走废纸篓**（安全护栏硬性要求） | |
| `notify` | 缓存目录增量监听 | |

**不需要**：mach2 / libproc / libc FFI —— 这是本方案最大的简化。

### 4.2 前端

| 包 | 用途 |
|---|---|
| `react` 19 / `react-dom` 19 | |
| `@tauri-apps/api` 2 | IPC / 窗口控制 |
| `@tauri-apps/plugin-*` | updater / autostart |
| `zustand` | 状态 |
| `tailwindcss` 4 | 样式 |
| `vite` | 构建 |

**不引入图表库** —— 趋势图自绘 SVG sparkline，避免给 WebView 加负担。

---

## 5. 关键模块实现

### 5.1 内存：调用系统 footprint 命令

```rust
// crates/prune-core/src/mem/footprint.rs
pub async fn probe(pid: i32) -> Result<ProcessFootprint> {
    let json_path = std::env::temp_dir().join(format!("prune-fp-{}.json", pid));
    let out = tokio::process::Command::new("/usr/bin/footprint")
        .args(["-p", &pid.to_string(), "--swapped", "-j"])
        .arg(&json_path)
        .output().await?;
    if !out.status.success() { return Err(ProbeDenied.into()); }  // 权限失败 → 降级为 RSS
    let raw = tokio::fs::read_to_string(&json_path).await?;
    let parsed: FootprintJson = serde_json::from_str(&raw)?;
    let _ = tokio::fs::remove_file(&json_path).await;
    Ok(ProcessFootprint {
        pid,
        footprint: parsed.processes[0].footprint,
        swapped: parsed.processes[0].categories.values().map(|c| c.swapped).sum(),
        compressed: ...,
    })
}
```

**降级链（必须实现）**：`footprint` 失败 → 退回 `sysinfo` RSS 并标注「口径不含换出量」，绝不静默报错。

**并发**：`rayon` / `tokio` 并发 8 路，Top 40 进程约 0.6 s 完成一轮。

### 5.2 Swap：内存与磁盘的桥

```rust
// 直接读 /System/Volumes/VM/swapfile* 的文件数与大小
// + sysctl vm.swapusage 拿已用/总量
// 输出：已用 14.04 GB / 上限 15 GB / 文件数 16 / 今日新增 6
```

数据源全部是公开只读路径，无权限问题。

### 5.3 磁盘：并行扫描

```rust
use ignore::WalkBuilder;
let walker = WalkBuilder::new(root)
    .max_depth(Some(6))
    .hidden(false)          // 缓存目录本身是隐藏的
    .git_ignore(true)
    .build_parallel();      // 并行遍历
```

**实测对照**：昨天用 shell 逐个 `du` 扫 6336 个 node_modules 跑了 4 分钟被 kill；Rust `ignore` 并行遍历预期 **< 15 s**，且可增量。这一项就是选择 Rust 核心（而非纯 Swift + shell）的直接理由。

### 5.4 分类器：跨平台二进制（杀手功能 A1）

```rust
pub fn classify(entry: &CacheEntry, arch: Arch, has_rosetta: bool) -> Verdict {
    match platform_of(&entry.name) {
        Some(p) if p != arch && !p.is_darwin() => Verdict::Deadweight,   // linux/win32/android → 🟢
        Some(Platform::DarwinX64) if arch == Arm64 && !has_rosetta => Verdict::Deadweight,
        Some(Platform::DarwinX64) => Verdict::NeedsConfirm,              // 有 Rosetta → 🟡
        _ => Verdict::Keep,
    }
}
```

输入目录白名单：`~/.npm/_cacache`、`~/Library/Caches/Yarn/v6`、`~/Library/pnpm/store`、`~/.bun/install/cache`、`~/Library/Caches/ms-playwright`。
本机实测产出：334 个条目 / 7.74 GB / 浪费比 8.3:1。

### 5.5 安全删除（代码层硬护栏）

```rust
pub fn reclaim(paths: &[PathBuf]) -> Result<ReclaimReport> {
    for p in paths {
        ensure_in_whitelist(p)?;          // 1. 白名单校验，越界直接 Err
        ensure_reproducible(p)?;          // 2. 必须判定为可再生资源
    }
    if total_bytes(paths) > 20 * GB { return Err(NeedsExplicitConfirm); }  // 3. 熔断
    trash::delete_all(paths)?;           // 4. 走废纸篓，绝不 rm -rf
    Ok(report)
}
```

四条护栏全部是编译期/运行期约束，不是 UI 提示。

---

## 6. macOS 菜单栏实现（Tauri 特有难点）

| 需求 | 做法 |
|---|---|
| 不显示 Dock 图标 | `app.set_activation_policy(ActivationPolicy::Accessory)` |
| 状态栏图标 | `TrayIconBuilder`（Tauri 2 内置），模板图标支持深浅色 |
| 点击弹出面板 | `TrayIconEvent::Click` → 用 `tray.rect()`（仅 macOS）取图标坐标 → 定位无边框窗口 |
| 面板样式 | `decorations: false` + `transparent: true` + `shadow: false`，需 `macOSPrivateApi: true` |
| 失焦关闭 | 监听窗口 blur → 关闭 |

**风险**：`tray.rect()` 在某些 macOS 版本/多屏幕下取不到准确坐标。降级方案：取不到时定位到屏幕右上角、或改用 `NSPopover`（需 objc2 FFI）。**这是 Tauri 方案相对 SwiftUI `MenuBarExtra` 的主要劣势，M0 需验证。**

> **M3 更新（2026-10-27）**：NSPopover 已通过 objc2 0.6 FFI 落地（`src-tauri/src/popover.rs`），
> WKWebView 搬入 popover、Transient 收起、原生箭头；`tray.rect()` 缺失时自动降级旧窗口路径。
> 见 `docs/ns-popover-plan.md` / `docs/ns-popover-acceptance.md`。

**M6 更新：托盘 CPU 动画**（`docs/m6-tray-anim-plan.md`）：4 帧剪刀开合图（编译期内嵌），
按整机 CPU 档位换帧——`<10%` 静止 / `10–40%` 2 fps / `40–70%` 5 fps / `>70%` 10 fps；
读不到 CPU = 永远静止。macOS 换帧必须走 `set_icon_with_as_template(icon, true)`
（先 set 再改 template 会渲染两次闪烁）；动画循环与采样循环解耦，tokio spawn。
开关在右键菜单，持久化到 `config_dir()/settings.json` 的 `trayCpuAnim` 键；
tooltip 每轮更新为 `Prune · CPU 12%`（动画关闭时保留）。

---

## 7. ⚠️ WebView 内存：本方案最大代价与解法

一个"资源守门员"自己吃掉 150 MB，在 swap 已用 14 GB 的机器上尤其讽刺。

| 策略 | 常驻内存 | 打开延迟 | 建议 |
|---|---|---|---|
| 面板与主窗口都常驻隐藏 | **120~180 MB** | <50 ms | ❌ 不可接受 |
| **后台 Rust 常采 + 两者即用即毁** | **25~40 MB** | 200~400 ms | ✅ **推荐** |
| 面板常驻、主窗口即用即毁 | 60~90 MB | 面板 <50 ms | 🟡 备选（D8） |

推荐策略要点：
- 采样在 Rust 线程常驻（~15 MB），WebView 不创建
- 用户点击 tray → 创建 WebviewWindow → 渲染 → **关闭时 `destroy()` 而非 hide**
- 数据已在内存/Rust 侧，WebView 创建后直接 `invoke` 取，几乎无等待感

---

## 8. Tauri vs SwiftUI 代价对照（诚实版）

| 维度 | SwiftUI 原生 | Tauri 2 | 判定 |
|---|---|---|---|
| 常驻内存 | 20~40 MB | 25~40 MB（优化后） | ≈ 持平 |
| 冷启动 | ~200 ms | ~500 ms（含面板 200~400 ms） | 🟡 略输 |
| 包体积 | ~2 MB | ~10~15 MB | 🟡 略输 |
| 菜单栏原生感 | `MenuBarExtra` 原生 | 需模拟 + 定位降级 | 🔴 **明显劣势** |
| 磁盘扫描性能 | FileManager（慢） | `ignore` + `rayon`（**快 10×+**） | 🟢 **明显优势** |
| CLI 复用 | 需另写 Swift binary | **同一 `prune-core` crate** | 🟢 **明显优势** |
| 跨平台潜力 | 无 | Windows/Linux 可延展 | 🟢 优势 |
| 你的上手速度 | 需学 SwiftUI | **React/TS 已熟 + PLOS 用过 Tauri** | 🟢 **决定性优势** |
| Mach API 需求 | — | **已被 footprint 命令绕开** | 🟢 意外收获 |

**结论：你的技术栈熟悉度 + Rust 扫描性能 + CLI 复用，三项优势盖过菜单栏原生感的劣势。** 且菜单栏劣势可通过 `NSPopover` FFI 在 M3 补齐。（已兑现：M3 NSPopover 落地，见 §6 更新）

---

## 9. 里程碑

| 里程碑 | 内容 | 验收 |
|---|---|---|
| **M0** | 三项验证（§10） | 可行性结论 |
| **M1** | `prune-core` 磁盘扫描 + 分类器 + 菜单栏 + 🟢 清理 | 首屏说出「X GB 永远加载不了」，清理后构建无失败 |
| **M2** | 内存模块（footprint 采样 + swap + 换出榜）+ 主窗口 | footprint 读数与活动监视器偏差 <10% |
| **M3** | 泄漏判定（R²）+ CLI + `NSPopover` 原生化（✅ 2026-10-27 落地） | — |

---

## 10. M0 验证清单（动工前）

| # | 验证项 | 方法 | 判定 |
|---|---|---|---|
| 1 | ~~`task_for_pid` 权限~~ | **已解决** —— 改用 `footprint` 命令，实测非特权通过 | ✅ 关闭 |
| 2 | **Tauri tray 面板定位是否可靠** | 最小 Tauri 模板 + `tray.rect()`，在当前 macOS 版本实测 | 🔴 **新的生死线** |
| 3 | `footprint` 并发 40 进程总耗时 | `rayon` 8 路并发实测 | 需 < 1.5 s |
| 4 | Rust 全盘扫描耗时 & CPU | 扫 `~` 下 node_modules，测 wall time | 需 < 20 s 且不卡 UI |
| 5 | `footprint` 对系统进程是否失败 | 抽查 20 个系统进程 | 确认降级链生效 |

> **#2 是新的生死线**：若 tray 定位在本机不可靠且 `NSPopover` FFI 成本过高，菜单栏体验会显著劣化——此时应重新评估 SwiftUI。

---

## 11. 风险清单

| 风险 | 等级 | 缓解 |
|---|---|---|
| tray 面板定位不准 | 🔴 | M0 #2 验证；降级为右上角定位；~~M3 用 NSPopover~~ ✅ M3 已落地 NSPopover（旧路径保留为降级） |
| `footprint` 输出格式随系统版本变化 | 🟠 | 只依赖 `footprint` / `swapped` 两个字段；解析失败即降级 RSS |
| 依赖系统自带命令（未来可能移除） | 🟠 | 降级链常驻；保留 mach2 路径作为长期后备 |
| 删除事故 | 🔴 | 白名单 + 废纸篓 + 熔断 + 不提权 |
| 未启用 App Sandbox 影响上架 | 🟡 | 计划独立分发（对标 Portlivo），不走 Mac App Store |
| WebView 内存 | 🟡 | §7 即用即毁策略 |

---

## 12. 与上游文档的关系

| 上游 | 本文的处理 |
|---|---|
| `prune-feature-spec-v2.md` §5 A1 跨平台二进制 | 实现见 §5.4，实测 7.74 GB |
| 同上 §6 B1 footprint 口径 | **数据源改为 `/usr/bin/footprint`**，绕开 Mach API |
| 同上 §6 B4 换出受害者榜 | 直接取 JSON 的 `swapped` 字段，实测 2.02 GB |
| 同上 §7 安全护栏 | 见 §5.5，`trash` crate 实现废纸篓删除 |
| 同上 §10 M0 #1 | **已关闭**（实测通过），新增 M0 #2 为新的生死线 |
| `footprint-prd.md` §3 技术可行性 | 已被本文取代（不再需要 mach2/libproc） |
