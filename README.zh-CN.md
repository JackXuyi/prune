# Prune

[English](./README.md) · **简体中文**

### 你的 Mac 上，存着一批你这辈子都加载不了的文件。Prune 负责把它们找出来。

一个免费、MIT 许可的开发机桌面工具：找出**真的可以回收的磁盘空间**——跨平台二进制、重复的包管理器存储、白名单内的开发缓存、Xcode 与模拟器残渣；同时按**系统自己的记账口径**报告内存，把已经换出到磁盘的那部分也算进去。

**Prune 不是内存监控，也不是通用系统清理工具。** 它是一台浪费探测器：指出被浪费掉的东西、把证据摆出来，决定权留给你。它不读任何传感器、没有云端账号、没有「一键优化」按钮。

Tauri 2 · Rust · React 19 · 无需账号 · 零遥测 · 不申请提权。

[![release](https://img.shields.io/github/v/release/JackXuyi/prune?label=release&color=blue)](https://github.com/JackXuyi/prune/releases/latest)
[![platform](https://img.shields.io/badge/platform-macOS%2013%2B%20%C2%B7%20Windows%2010%2B%20beta-black)](#安装)
[![license](https://img.shields.io/github/license/JackXuyi/prune?color=green)](./LICENSE)
[![stack](https://img.shields.io/badge/stack-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20React%2019-informational)](#从源码构建)
[![telemetry](https://img.shields.io/badge/telemetry-zero-brightgreen)](#agent-能看人才能删)
[![downloads](https://img.shields.io/github/downloads/JackXuyi/prune/total?label=downloads)](https://github.com/JackXuyi/prune/releases)

---

## 目录

- [为什么做这个](#为什么做这个)
- [谁适合用](#谁适合用)
- [多端支持](#多端支持)
- [它做什么 / 不做什么](#它做什么--不做什么)
- [六个 tab](#六个-tab)
- [菜单栏：先看这个数字](#菜单栏先看这个数字)
- [清理：三级分类，四道护栏](#清理三级分类四道护栏)
- [内存与应用](#内存与应用)
- [网络、端口、局域网](#网络端口局域网)
- [事件流：灵动岛](#事件流灵动岛)
- [最初两分钟](#最初两分钟)
- [CLI](#cli)
- [MCP 服务](#mcp-服务)
- [Agent 能看，人才能删](#agent-能看人才能删)
- [安装](#安装)
- [从源码构建](#从源码构建)
- [状态与路线图](#状态与路线图)
- [相关文档](#相关文档)
- [关键词](#关键词)
- [License](#license)

## 为什么做这个

三个实测出来的事实（2026-10，一台真实开发机，样本量 n = 1——你的机器数字会不同，但这三个机制一样）：

| 事实 | 数字 |
| --- | --- |
| Yarn 缓存里躺着 **334 个跨平台二进制包**（linux 196 / win32 74 / android 25），arm64 机器上永远加载不了 | **7.74 GB**，浪费比 8.3 : 1 |
| npm + Yarn + pnpm 三套包管理器**重复存着同一批依赖** | **29.25 GB** |
| 活动监视器显示 next-server 只占 **113 MB**，**真实占用是 3432 MB**（97% 已被换出到磁盘） | RSS 口径撒了谎 |

系统监控工具不碰磁盘，清理工具不认识包管理器缓存。Prune 站在中间；内存一律报 `phys_footprint` 而不是 RSS，让数字与机器实际付出的代价对齐。

后面每一条产品规则都从这三件事长出来：**只报可核验的事实，只删可再生的东西，不替人做决定。**

## 谁适合用

**适合你**：在 256–512 GB 的机器上写代码，AI 工具和 Node 项目常驻，经历过「启动磁盘已满」，希望在删掉某个 8 GB 缓存之前先知道它是什么。

**大概不适合你**：想要「一键自动清空」（Prune 从未提供），想要通用硬件监控（Prune 不读任何传感器），或者需要全盘扫描、照片去重、杀毒、企业机群遥测看板。

## 多端支持

Prune 是 macOS 优先，不是 macOS 专用：一套 Rust 内核，三种交付形态。

| 平台 | GUI | CLI + MCP | 状态 |
| --- | --- | --- | --- |
| **macOS 13+**（Apple Silicon / Intel） | `.dmg` | ✓ | 参考平台。数据源：`footprint` / `lsof` / `nettop` / `networkQuality` |
| **Windows 10+ x64** | `.exe` / `.msi` | 可从源码构建（`scripts/check-windows.sh` 交叉编译并真实链接出 `prune.exe`） | **beta**——代码层完成，每个 release 都出安装包；真机走查清单（托盘定位、深色任务栏、Temp 删除）还没走过 |
| **Linux** | —— | `prune-core` + `prune-cli` 在 CI（ubuntu runner）编译并跑测试 | 未发布任何构建，也没有在真实桌面上走过——只算源码可用 |

### Windows 上具体差在哪

下面每一条都不做模拟：没有等价口径的字段一律隐藏，绝不填 0 冒充。

| 能力 | macOS | Windows |
| --- | --- | --- |
| 缓存白名单 | 生效 52 条（40 条 macOS 专属 + 12 条共用） | 生效 28 条（16 条 Windows 专属 + 12 条共用） |
| 跨平台二进制判死（A1） | Yarn v6——目录名本身携带平台信息 | 不启用：Yarn 先在 Windows 上实测，实测前不接管 |
| npm / Yarn / pnpm 重复存储 | ✓ | ✓（`%LOCALAPPDATA%` 布局） |
| 清理（白名单 + 废纸篓 + 20 GB 熔断）、项目体检、休眠归档 | ✓ | ✓ |
| 进程真实内存 | `phys_footprint`，含被换出部分 | Private Bytes——**不含**换出量的近似口径 |
| Swap 仪表 | 总量 / 已用 + swapfile 个数 + 今日新增 | 只有总量 / 已用 |
| 内存压力 | 内核压力等级 | `dwMemoryLoad` 占用率分档（< 80 / 80–91 / ≥ 92） |
| 换出受害者榜 | ✓ | 隐藏（没有换出量可排） |
| 泄漏案件簿 | ✓ | ✓（按 Private Bytes 判定） |
| 进程的项目归属 | 句柄 + 标签 + 继承 | 标签 + 继承（无 `lsof` 等价物） |
| 应用磁盘三层（本体 / 缓存 / 数据） | ✓ | 不可用——三层都要 `.app` 与 `~/Library` 路径 |
| 按应用网速、按应用 Top 8 | `nettop`，1 s 差分 | 不可用——需要 ETW 实时会话 |
| 系统口径速率 + 5 min 走势 + 每日流量 | ✓ | ✓（`sysinfo` 计数器） |
| 网络诊断（DNS / 网关 / 出口） | ✓ + 系统测速与 RPM | 只有诊断；测速卡自己隐藏 |
| 端口反查 | `lsof`——非 root 会漏掉别的用户的进程，`coverage` 会说明漏了多少 | `GetExtendedTcpTable`——不需要管理员权限且不漏，代价是「属于哪个项目」一列恒为空 |
| 局域网设备 / 时间线 / 拓扑 | `arp -an` + `lsof` 连接对端 | `GetIpNetTable2` + `GetExtendedTcpTable` |
| 事件流（灵动岛） | 刘海原生面板 | 未实现——灵动岛相关命令是空实现，什么都不做 |
| 托盘图标动画 + 宠物心情 | 模板图标，系统自动反色 | 彩色帧（黑模板图在深色任务栏上看不见） |
| 菜单栏面板 | 原生 popover | 托盘锚定窗口 + 300 ms 焦点宽限期 |
| 界面语言（中文 / English） | ✓ | ✓ |

Windows 有一处反而更强：端口反查不需要管理员权限，也不会漏掉别的用户的监听（macOS 非 root 会静默漏）。其余差异都是系统缺少对应设施，不是产品缺功能。

## 它做什么 / 不做什么

| ✅ 做 | ❌ 不做 |
| --- | --- |
| 探测本机架构**永远加载不了**的缓存（跨平台二进制）；删除动作只落在固定白名单内，且一律进废纸篓 | 通用硬件监控 |
| 统计 npm / Yarn / pnpm 之间的重复存储 | 「释放内存让 Mac 变快」 |
| 显示进程**真实**内存占用（`phys_footprint` 口径，含换出量）——swap 是活跃机制，不是垃圾文件 | 全盘扫描、用户文档、照片去重 |
| Swap 仪表盘：内存压力正在吃掉多少磁盘、谁被换出 | 任何不在白名单内的删除 |
| 「疑似内存泄漏」按案件簿呈现（证据 + 曲线 + 克制措辞） | 给不出证据的结论 |
| 端口反查：谁占着 3000 / 3118，以及它属于哪个项目 | 防火墙、抓包、按域名拆流量 |
| 应用维度资源卡：进程 / CPU / 内存 / 磁盘 / 端口 / 连接 / 网速 | GPU 与能耗 |
| 网络 tab：系统口径速率、5 min 走势、网卡分布、按应用 Top 8、今日 / 近 7 天 | 限速、断网、掐连接 |
| 网络诊断：DNS / 网关 / 出口耗时 + captive portal + 阈值透明的单条结论 + Markdown 报告 | Wi-Fi 信号、切 DNS（写系统配置）、信道扫描 |
| 灵动岛事件流：CPU 暴走 / 内存异常 / 端口冲突 / 局域网新面孔 | 常驻挂件、音乐控制、文件拖拽收纳 |

其中按应用网速、应用磁盘三层、事件流三项目前是 macOS 专属，内存那两行在 Windows 上读的是 Private Bytes 而不是 `phys_footprint`——具体差在哪见[多端支持](#多端支持)。

## 六个 tab

| Tab | 回答什么 |
| --- | --- |
| **总览**（默认页） | 可回收 GB、内存 / CPU / 硬盘、监听端口、下载与上传——六张只读卡，只导航 |
| **应用** | 每个应用一张卡：进程、CPU、内存、磁盘、端口、连接、网速；默认 Top 12、六字段排序、24 h / 7 天 / 30 天历史 |
| **内存** | 真实占用、swap 仪表、换出受害者榜、进程家族、泄漏案件簿 |
| **存储** | 卷容量、按级别的可回收拆分、逐条明细、清理、项目体检、休眠归档 |
| **网络** | 下载 / 上传（只算物理网卡）、走势、按应用 Top 8、每日流量、网络诊断 |
| **端口** | 谁在监听、绑在哪个地址、属于哪个项目；局域网设备、时间线、拓扑 |

除了存储 tab 的清理动作与归档 / 恢复链路，其余全部只读——而这两处也是整个产品里唯一能让文件移动的地方。

## 菜单栏：先看这个数字

```
菜单栏  ⚡ 54.9 GB 可回收        压力 ● 绿
├──────────────────────────────────────────┤
│ 一、内存                                  │
│   ⚠ swap 14.0 GB（93% 上限）              │
│   ↳ 正在吃掉磁盘 16 GB · 今日 +6          │
│   ▸ 换出受害者：next-server 3.2 GB(97%)   │
│ 二、可回收                                │
│   🟢  7.74 GB  跨平台二进制（永远用不上） │
│   🟢 22.5 GB  包管理器等缓存              │
│   🟡 21.7 GB  项目依赖 / 模拟器           │
│   🔴  3.0 GB  Docker（只给命令，不执行）  │
└──────────────────────────────────────────┘
```

首屏主数字是「可回收 GB」，不是内存百分比——百分比是描述性的，GB 是可行动的。菜单栏面板（380 × 432）与主窗口共用同一个读数层（`lib/statsReadout.ts`），两块屏不可能各算一遍。

菜单栏图标本身也是读数：剪刀的开合速度（或小猫的跳跃频率）编码 CPU 繁忙程度，精确值放在 tooltip（`Prune · CPU 12%`）。

## 清理：三级分类，四道护栏

**68 条白名单缓存路径**——53 条一键安全（🟢），12 条需逐项确认（🟡：项目依赖、模拟器、模型权重），3 条只输出命令、永不执行（🔴：Docker VM、OrbStack、Colima）。

| 护栏 | 实现 |
| --- | --- |
| 路径白名单 | 只接受已登记的路径；自由输入路径在 `prune-core` 层就被拒绝 |
| 废纸篓而非 `rm -rf` | 所有删除走系统废纸篓（`trash` crate），可恢复 |
| Dry-run 前置 | 首次执行先列出清单确认 |
| 字节熔断 | 单次超过 20 GB 强制二次确认，即使是 🟢 级 |

Prune 不要 root、不要全盘访问、不要辅助功能。macOS 唯一一次隐私弹窗（「本地网络」）只在你主动执行局域网扫描时出现。

**项目体检**把「N 个 node_modules 共 X GB」拆成按项目的依赖账本：依赖体积、上次安装、项目最后活动、多锁文件并存。结论只陈述事实（依赖与项目双双 ≥ 60 天没动、≥ 1.5 GB 偏胖），不代删。

**休眠归档**是可逆的替代方案：把僵尸项目的 `node_modules` 打包进受管归档区，然后重读校验（条目数 + 字节数）——**校验通过才把原件移入废纸篓**。恢复前先做冲突预览，用户代码永不进归档。

## 内存与应用

- 数据源是系统自带的 `/usr/bin/footprint -p <pid> -j`：非特权、JSON、含 swapped 明细。不碰 Mach API，不要任何权限。Windows 走 `GetProcessMemoryInfo` 的 Private Bytes——这是对 macOS footprint 口径的近似，不是等价物（它看不到被换出到 pagefile 的部分）；读不到的字段（换出量、受害者榜）直接不渲染。
- 采样：每 10 s 一次廉价 RSS 全量，每 5 min 对 Top N 做一次深探（8 路并发 ≈ 0.6 s）。
- **泄漏按案件簿处理**：立案（`CASE-MMDD-NN`）→ 侦查（生长曲线 + 证据数字）→ 嫌疑人离场且未复现时自动归档。措辞红线固定：只说嫌疑，不下结论。
- 读不到的值显示「不可读 + 原因」，永远不显示 0——0 和「读不到」是两件事。

## 网络、端口、局域网

**网络**只算物理网卡（`utun` / `awdl` 单列不计入，开 VPN 不会双计），另有 `nettop` 口径的按应用 Top 8（仅 macOS）、今日与近 7 天流量（落 SQLite），以及一轮诊断：对 DNS / 网关 / 出口测真实 TCP connect 耗时（不是 ICMP）、检测 captive portal、给出一条阈值全部展开（50 / 100 / 30 / 300 ms）的结论，一键复制 Markdown 报告。macOS 另有系统测速与 RPM（bufferbloat）；Windows 无等价物，测速卡自己隐藏。

**端口**回答「谁占着 3000」：持有进程、绑定地址、项目目录、established 连接数，外加 `coverage`——告诉你还有多少监听是无特权进程看不见的。「没查到」永远不会被报成「空闲」。服务指纹给每个监听标注「推断 / 确认」双强度（先静态推断，主动探针是可选开关）。

**局域网**有两种节奏。后台的事件监测是观察者：只读邻居表（60 s 一拍，零主动探测、零 `lsof`）；局域网页的扫描是你显式发起的动作，只有那时才会探测网段。两者共用同一份稳定 MAC 身份、首次发现 / 最后活跃、拓扑视图，并在出现一个从未见过的设备时提报一次。

## 事件流：灵动岛

四类事件从刘海里长出来，各有独立频控与冷却：

1. **CPU 暴走**——持续燃烧才触发，带滞回，单次编译不会误报
2. **内存异常**——内核压力 Critical，或 swap 破阈值
3. **端口冲突**——关注的端口从空闲变被占，或出现第二个持有者
4. **局域网新面孔**——网段里出现从未见过的设备

折叠态是「图标 + 事件标签 + 读数」，悬停展开看「谁在烧 / 谁被换出」。6 秒自走，不抢焦点，也不常驻当挂件。

**仅 macOS**：这个面板是贴着刘海的 `NSPanel`；Windows 上灵动岛相关命令都是空实现，不会触发任何事件。

## 最初两分钟

1. 打开。存储 tab 立即开始扫描——没有五步 onboarding，没有注册。
2. 首扫结束在一张只报 🟢 体量的引导卡上。
3. 一键回收：dry-run → 白名单 → 废纸篓，只碰 🟢 条目。
4. 成绩单弹出：本次清掉多少、累计清掉多少。

没有账号、没有网络请求、没有「给个好评」弹窗。那一屏上唯一的可选动作是把六个数字复制到剪贴板，方便你贴进 issue 或访谈记录。

## CLI

单二进制双形态（`cargo build --release -p prune-cli` → `target/release/prune`），与 GUI 共享 `prune-core`：

```bash
prune scan [--json]                 # 全盘扫描：主数字 / A1 明细 / A3 重复 / A6 Docker 命令提示
prune reclaim [--level green]       # 预览零风险清理项（默认 dry-run；--yes 才真删，走废纸篓）
prune mem [--swapped] [--json]      # 一次性内存读数（footprint 口径，深探 Top 20）
prune mem --watch [--interval N]    # 持续采样，每 60s 输出泄漏嫌疑判定
prune port [PORT] [--json] [--all]  # 谁占着这个端口（不带 PORT 列关注层；--all 含内部 IPC）
prune app <家族名> [--json]         # 一个应用的资源画像：进程/内存/CPU/端口/连接/磁盘
prune lan [--scan] [--json]         # 局域网设备（默认只读缓存；--scan 主动探测网段）
prune mcp                           # MCP stdio server（AI Agent 查询入口）
```

## MCP 服务

**MCP 工具（8 个，全部只读）：** `mem_snapshot`（一次性内存读数）/ `mem_leaks`（读 GUI 序列库的泄漏嫌疑）/ `disk_report`（全盘扫描，约 15 s）/ `disk_scan`（白名单逐条细分 + 可选子目录占比，秒级）/ `port_listeners`（端口反查，`coverage` 说明可见范围）/ `net_diag`（暴露面诊断结论：谁在对外监听）/ `app_resources`（应用维度五域画像）/ `lan_devices`（局域网设备，只读缓存不发包）。

接进任何支持 MCP 的 Agent，就能问「这台机器什么在吃磁盘」「我有什么暴露在局域网」「这个进程在漏吗」——Agent 负责读，你负责决定。

## Agent 能看，人才能删

这不是宣传语，是四条可验证的代码层约束，并且有测试固化：

1. **Agent 能看**：8 个 MCP 工具全部是查询，没有一扇执行的门。
2. **人才能删**：清理只发生在 GUI，背后是白名单 + 废纸篓 + 20 GB 熔断；不存在一条 Agent 能调的删除 IPC。
3. **分发层按名拒绝**：`reclaim` / `clean` / `execute` / `delete` 等执行类请求返回明确的只读错误并说明原因，而不是笼统的 unknown tool；有单测断言这条臂必须存在。
4. **白名单兜底**：即使未来开放执行通道，可操作的也只有已登记的可再生路径——自由输入路径在 `prune-core` 层就死掉。

再加一条产品级承诺：**零遥测**。没有分析 SDK、没有回连、没有带设备 id 的 URL。激活相关的计数只写本地 SQLite，只有你复制它时才会离开这台机器。

## 安装

从 [Releases](https://github.com/JackXuyi/prune/releases/latest) 下载对应平台的最新安装包：

| 平台 | 下载哪个文件 |
| --- | --- |
| macOS（Apple Silicon，M1 及之后） | `Prune_*_aarch64.dmg` |
| macOS（Intel） | `Prune_*_x64.dmg` |
| Windows 10+ x64（beta） | `Prune_*_x64-setup.exe`（推荐）或 `Prune_*_x64_en-US.msi` |

**macOS 安装三步**

1. 双击 dmg，把 **Prune** 拖进 **Applications**。
2. 首次打开：若弹出「无法打开，因为无法验证开发者」——到 **系统设置 → 隐私与安全性**，滚到底部点 **「仍要打开」**（只需一次）。
3. 若没有「仍要打开」按钮（部分 macOS 版本行为不同），在终端执行一次：
   `xattr -dr com.apple.quarantine /Applications/Prune.app`

**Windows**：双击 `Prune_*_x64-setup.exe`；若 SmartScreen 弹「Windows 已保护你的电脑」，点 **更多信息 → 仍要运行**。

**Linux**：没有发布 GUI 构建。执行 `cargo build --release -p prune-cli` 可得到 CLI / MCP 二进制——它能读到什么、读不到什么，见[多端支持](#多端支持)。

**为什么会有这一步**：当前产物采用 ad-hoc 签名（未购买 Apple 开发者账号、未公证），Gatekeeper 不认识它——这就是拦截弹窗的全部原因。安装包本身完整；应用内更新不走这条链路（updater 有独立验签），装好后从托盘菜单「检查更新」升级不会再遇到弹窗。这一段不是免责声明，是把事实摆在你面前，判断留给你。

## 从源码构建

```bash
pnpm install
pnpm tauri dev      # 开发调试
pnpm tauri build    # 打包 .app / .dmg
cargo build --release -p prune-cli   # CLI + MCP 二进制
cargo test --workspace               # Rust 测试
pnpm check:docs                      # 文档诚信门禁（MCP 工具对账 + 公开表述 sweep）
```

依赖：Node 20+、pnpm 9+、Rust stable，以及 Tauri 2 的平台前置条件。

## 状态与路线图

源码版本：**v0.10.0**——免费、MIT 许可，未来两个季度没有商业化计划。顶部徽章跟随 Releases 页最新**已构建发布**的版本。

| 方向 | 状态 |
| --- | --- |
| 磁盘浪费探测 + 分级清理 | 已交付（A1 跨平台二进制、重复存储、68 条白名单、项目体检、休眠归档） |
| 内存真相、swap、泄漏案件簿 | 已交付（footprint 口径、受害者榜、案件簿） |
| 应用 / 网络 / 端口 / 局域网 tab | 已交付（应用 30 天历史、网络诊断、局域网时间线与拓扑） |
| 事件流（灵动岛） | 已交付（四类事件、阈值可配） |
| CLI + 只读 MCP 服务 | 已交付（8 个工具，执行类请求按名拒绝） |
| 激活闭环 | 0.10.0 已交付（首扫 → 一键 🟢 → 成绩单；计数只存本地） |
| Windows | **beta**：每个 release 都出安装包，核心功能可用，缺口见[多端支持](#多端支持)；不承诺进一步移植排期 |
| 自动清理、遥测、云端账号 | 不做 |

完整里程碑历史见 [CHANGELOG.md](./CHANGELOG.md)；向前看的部分（M1–M8、门禁阈值、死亡线、明确不做清单）见 [docs/prune-roadmap-2027h1.md](./docs/prune-roadmap-2027h1.md)。

## 相关文档

- [产品功能方案 v2](./docs/prune-feature-spec-v2.md) —— 定位、模块、安全分级
- [Tauri 技术方案](./docs/prune-tauri-technical-plan.md) —— 架构、依赖、里程碑
- [产品 Roadmap 2027 H1](./docs/prune-roadmap-2027h1.md) —— 里程碑、指标门禁、明确不做
- [网络诊断方案](./docs/net-diag-plan.md) —— 分层探测、判定规则、双平台适配
- [调研档案](./docs/research/) —— 竞品拆解 / 竞品实测 / 可回收空间扫描报告 / 内存剖析（决策档案）
- [CHANGELOG](./CHANGELOG.md) —— 每个已交付里程碑，包括那些其实是修 bug 的

## 关键词

`macOS 磁盘空间分析` · `开发者缓存清理` · `node_modules 体积` · `yarn 缓存` · `pnpm store` · `npm 缓存` · `Xcode DerivedData` · `iOS 模拟器清理` · `AI CLI 缓存`（Cursor / Claude / Codex / Ollama / HuggingFace / LM Studio）· `swap 监控` · `phys_footprint` · `内存泄漏检测` · `谁占着 3000 端口` · `局域网设备发现` · `MCP server` · `Tauri` · `Rust` · `Apple Silicon` · `Windows` · `跨平台` · `零遥测` · `开源系统工具`

## License

[MIT](./LICENSE)。白名单刻意做成一个数据文件：缓存位置的变化比发版快，而一条错路径就是一次数据丢失事故，所以每条登记都自带断言测试。
