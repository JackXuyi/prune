# Prune 🔪

**你的 Mac 上，存着一批你这辈子都加载不了的文件。Prune 负责把它们找出来。**

> 🔍 macOS 开发机资源守门员 —— 不是内存监控，不是系统清理，是「浪费探测器」。

![平台](https://img.shields.io/badge/platform-macOS%2013+-black)

![技术栈](https://img.shields.io/badge/Tauri%202%20·%20Rust%20·%20React%2019-blue)

![隐私](https://img.shields.io/badge/零埋点-零遥测-green)

![状态](https://img.shields.io/badge/status-M0%20验证中-orange)

---

## 为什么做这个

三件实测出来的事实（2026-10，一台真实开发机）：

| 事实                                                                                 | 数字                      |
| ---------------------------------------------------------------------------------- | ----------------------- |
| Yarn 缓存里躺着 **334 个跨平台二进制包**（linux 196 / win32 74 / android 25），arm64 机器上**永远加载不了** | **7.74 GB**，浪费比 8.3 : 1 |
| npm + Yarn + pnpm 三套包管理器**重复存着同一批依赖**                                              | **29.25 GB**            |
| 活动监视器显示 next-server 只占 113 MB，**真实占用是 3432 MB**（97% 已被换出到磁盘）                       | RSS 口径撒了谎               |

没人管这些：系统监控工具不碰磁盘，清理工具不认识包管理器缓存。**Prune 站在中间。**

## 它做什么 / 不做什么

| ✅ 做                                    | ❌ 不做                       |
| -------------------------------------- | -------------------------- |
| 探测本机架构**永远加载不了**的缓存（零误删）               | 通用硬件监控（iStat / Stats 已足够好） |
| 统计三套包管理器的重复存储                          | 「释放内存让 Mac 变快」（伪科学）        |
| 显示进程**真实**内存占用（phys_footprint 口径，含换出量） | 全盘扫描、用户文档、照片去重             |
| Swap 仪表盘：内存压力正在吃掉多少磁盘                  | 任何不在白名单内的删除                |
| 「疑似内存泄漏」判定（概率 + 曲线 + 克制措辞）             | 提升任何权限（root / 全盘访问 / 辅助功能） |
| 端口反查：谁占着 3000 / 3118，以及它属于哪个项目       | 防火墙、抓包、网络流量监控            |
| 应用维度资源卡：进程 / 内存 / CPU / 端口 / 连接 / 磁盘一次看全 | 按域名/对端拆流量、累计流量统计、GPU/能耗 |
| 统计 tab（默认页）：内存 / 硬盘占用大小、CPU 百分比、端口数量 | 历史趋势图表（只有 CPU 10 min 走势，不做任意指标的时间序列） |
| 应用资源 tab：每个应用一张卡（进程 / CPU / 内存 / 磁盘 / 端口 / 连接 / 网速），默认 Top 12、可排序 | 按域名/对端拆流量、Windows 按进程网速（需 ETW）、跨应用磁盘清理 |

## 首屏长什么样

```
菜单栏  ⚡ 54.9 GB 可回收        压力 ● 绿
├────────────────────────────────────────┤
│ 一、内存                                │
│   ⚠ swap 14.0 GB（93% 上限）            │
│   ↳ 正在吃掉磁盘 16 GB · 今日 +6        │
│   ▸ 换出受害者：next-server 3.2 GB(97%) │
│ 二、可回收                              │
│   🟢 7.74 GB  跨平台二进制（永远用不上）│
│   🟢 22.5 GB  包管理器等缓存            │
│   🟡 21.7 GB  项目依赖 / 模拟器         │
│   🔴 3.0 GB   Docker（只给命令，不执行）│
└────────────────────────────────────────┘
```

**首屏主数字是「可回收 GB」，不是内存百分比** —— 百分比是描述性的，GB 是可行动的。

主窗口的默认页是**统计 tab**：内存（已用 / 总量）、硬盘（已用 / 总量，statvfs 粗略口径）、
CPU（整机每核平均 + 10 min 走势）、端口（监听数 / 关注层 / 对外 / 活跃连接）。
四张卡只读、只导航——清理与结束进程的入口在各自的明细 tab。

**菜单栏图标也是读数**：剪刀的开合速度 = CPU 繁忙程度（近 10 秒均值）——闲时完全静止，
越忙剪得越快（快档 = 该点开看看是谁在烧）。图标不显示数字、不变颜色，只编码「快慢」一个
维度；精确值放在 tooltip（`Prune · CPU 12%`）。右键菜单可关闭动画（设置持久化）。

## 安全护栏（代码层约束，不是 UI 提示）

1. **路径白名单**：只删除预定义的可再生缓存路径，拒绝任何自由输入路径
2. **废纸篓而非 `rm -rf`**：所有删除可恢复
3. **Dry-run 前置**：首次执行先列出清单确认
4. **熔断**：单次删除 > 20 GB 强制二次确认（即使 🟢 级）
5. **永不提权**：不要 root、不要全盘访问、不要辅助功能
6. Docker 等高风险项**只输出命令，永不执行**

## 技术方案

- **Tauri 2 + React 19 + TypeScript + TailwindCSS 4 + Zustand**
- **Rust 核心（`prune-core` crate）**：GUI 与 CLI 共享
- 内存数据源：系统自带 `/usr/bin/footprint -p <pid> -j`（非特权、JSON、含 swapped 明细）—— **不碰 Mach API、不要任何权限**
- 磁盘扫描：`ignore` + `rayon` 并行（6336 个 node_modules，shell 要 4 分钟，Rust < 15 s）
- 删除：`trash` crate 走废纸篓
- 采样：10 s 廉价 RSS 全量 + 5 min 对 Top N 深探 footprint（并发 8 路 ≈ 0.6 s）

## CLI 与 MCP

单二进制双形态（`cargo build --release -p prune-cli` → `target/release/prune`）：

```bash
prune scan [--json]                 # 全盘扫描：主数字 / A1 明细 / A3 重复 / A6 Docker 命令提示
prune reclaim [--level green]       # 预览零风险清理项（默认 dry-run；--yes 才真删，走废纸篓）
prune mem [--swapped] [--json]      # 一次性内存读数（footprint 口径，深探 Top 20）
prune mem --watch [--interval N]    # 持续采样，每 60s 输出泄漏嫌疑判定（序列不落盘）
prune port [PORT] [--json] [--all]  # 谁占着这个端口（不带 PORT 列关注层；--all 含内部 IPC）
prune app <家族名> [--json]         # 一个应用的资源画像：进程/内存/CPU/端口/连接/磁盘
prune mcp                           # MCP stdio server（AI Agent 查询入口）
```

MCP 工具：`mem_snapshot`（一次性读数）/ `mem_leaks`（泄漏嫌疑，读 GUI 序列库）/ `disk_report`（全盘扫描）/
`port_listeners`（端口反查，只读；`coverage` 说明权限可见范围，`timeWait` 说明内核残留连接）/
`app_resources`（应用维度五域画像，只读；连接多 ≠ 流量大，磁盘含 APFS 克隆共享部分）。
安全底线：`reclaim` 不带 `--yes` 绝不删除；白名单 + 20GB 熔断在 prune-core 代码层兜底。

## 开发

```bash
pnpm install
pnpm tauri dev      # 开发调试
pnpm tauri build    # 打包 .app / .dmg
```

## 路线图

- [x] **M0** 可行性验证：Tauri tray 定位 / footprint 并发耗时 / 扫描性能
- [x] **M1** 磁盘浪费探测 MVP：跨平台二进制探测 + 🟢 级清理 + 菜单栏
- [x] **M2** 内存真相：footprint 口径 + swap 仪表 + 换出受害者榜
- [x] **M3** 泄漏嫌疑判定（B5）+ CLI + MCP + A6 只读（NSPopover 原生化推迟，tray 方案实测可用）
- [x] **M8** 应用维度资源卡：连接统计（M7 同源）+ 磁盘三层（本体/缓存/数据，按需 du）+ `prune app`
- [x] **M9** 统计 tab（默认页）：内存 / 硬盘大小 + CPU 百分比 + 端口数量，四卡只读总览
- [x] **M10** 应用资源 tab：每个应用一张卡（进程 / CPU / 内存 / 磁盘 / 端口 / 连接 / 网速），默认 Top 12 折叠 + 六字段排序；网速走 `nettop` 1 s 差分（仅非回环）

## 相关文档

- [产品功能方案 v2](./docs/prune-feature-spec-v2.md) —— 定位、模块、安全分级
- [Tauri 技术方案](./docs/prune-tauri-technical-plan.md) —— 架构、依赖、里程碑
- [调研档案](./docs/research/) —— Portlivo 拆解 / 竞品实测 / 可回收空间扫描报告 / 内存剖析 / Footprint PRD（决策档案）

## License

MIT（暂定）
