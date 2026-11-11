# Prune 竞品全景调研 · 2026-11

> 范围：以 Prune v0.5.0 当前完整功能面（磁盘浪费探测 + 缓存回收 + 内存真相 + 端口/网络 + 监控）为基准的市面同类产品调研。
> 与 2026-10 的 [footprint-competitive-analysis.md](./footprint-competitive-analysis.md)（内存侧）互补，本文新增磁盘清理侧 2026 年新竞品。

---

## 0. 结论先行

1. **「开发者缓存清理」赛道在 2026 年急剧升温**：DevCleaner（6 月上线）、CacheCleaner、Mole（tw93）、CleanMyMac CLI（MacPaw）几乎同季度入场，全部采用「风险分级 + 预览 + 本地运行」的安全叙事 —— Prune 的安全护栏（白名单/废纸篓/dry-run/熔断/零权限）已不再是独门优势，而是**品类入场券**。
2. **监控侧威胁升级**：Vitals 1.4（2026-09）已上线 MCP server + App 级分组 + 30 天历史 + dev servers 按项目管理，且商业化成功（1350+ 份）；它 roadmap 上的「存储地图与清理工具」落地后将与 Prune 正面相撞。
3. **Prune 的真差异化仍在三处，且没有竞品跟进**：
   - **跨平台二进制探测**（7.74 GB「永远加载不了」的缓存，浪费比 8.3:1）—— 所有竞品都是「按路径扫缓存」，没有按架构判定可用性；
   - **多包管理器重复存储统计**（npm/Yarn/pnpm 同一依赖三份存）；
   - **phys_footprint 真实内存口径**（含换出量）+ 换出受害者榜 + 泄漏侦探案件簿。
4. **风险提示**：Prune 独有的「探测」能力竞品没有，但竞品的「规模」在膨胀——DevCleaner 已覆盖 30+ 工具生态 + AI 应用缓存 + 项目休眠归档，Mole 是清理+监控+卸载+treemap 五合一。**覆盖广度是 Prune 最容易被追赶的短板**。

---

## 1. 竞品全景分层

| 层 | 代表产品 | 价格 | 与 Prune 的关系 |
|---|---|---|---|
| **L1 内置** | Activity Monitor、Storage、`brew cleanup`/`pnpm store prune` 等原生命令 | 免费 | 被当作「要超越的基线」 |
| **L2 开发者专项清理** ⚠️ 直接竞品 | **DevCleaner**、**CacheCleaner**、CleanMyMac CLI、DevCleaner for Xcode、npkill/kondo/cargo-sweep | 免费 ~ $59 买断 | **正面竞争：磁盘侧** |
| **L3 五合一维护套件** ⚠️ 直接竞品 | **Mole**、CleanMyMac、Sensei | $19 买断 ~ $40/年 | 清理+监控双线重叠 |
| **L4 开发者视角监控** ⚠️ 直接竞品 | **Vitals**、Mac Performance Monitor、ProcXray、FavTray | 免费 ~ $39 | **正面竞争：内存/端口/网络侧** |
| **L5 通用监控** | iStat Menus、Stats、eul | 免费 ~ $14.99 | 红海，不构成威胁 |
| **L6 磁盘可视化** | DaisyDisk、GrandPerspective、OmniDiskSweeper | 免费 ~ $9.99 | 相邻（Prune 明确不做 treemap） |

---

## 2. 价格与定位总表（2026-11 实测）

| 产品 | 价格 | 开源 | 零遥测 | 权限要求 | 关键定位 |
|---|---|---|---|---|---|
| **DevCleaner** (devcleaner.app) | Free / Pro $59 买断 或 $19.99/年 | ❌ | ✅ 本地扫描 | 基础功能免权限 | 菜单栏 + 30+ dev/AI 生态风险分级清理 + 项目休眠 |
| **CacheCleaner** (cachecleaner.app) | **免费开源**（自称品类唯一开源 GUI） | ✅ | ✅ | 免权限 | 2.8 MB 原生，Xcode/包管理器/Docker/AI 模型缓存 + node_modules/target/.build 全盘扫描，只读 |
| **Mole** (tw93, mole.fit) | CLI 免费 MIT / GUI **$19 买断**（2 台） | CLI ✅ | ✅ 本地 | Full Disk Access（否则结果不全） | 五合一：清理 + 软件管理/卸载 + 优化 + treemap + 状态监控 + 菜单栏 HUD |
| **CleanMyMac CLI** (MacPaw) | 随 CleanMyMac（~$40/年） | ❌ | ❌ | — | Terminal 优先：`clean dev-tools`（18 种工具链）/ `purge` 项目产物 / `analyze`，预览确认后删 |
| CleanMyMac（本体） | ~$40/年 | ❌ | ❌ | 全盘 | 通用清理套件，开发者项不深 |
| DevCleaner for Xcode | 免费 | ❌ | — | — | 仅 ~/Library/Developer，单一场景 |
| npkill / kondo / cargo-sweep | 免费 MIT/Apache | ✅ | ✅ | — | CLI 单项专家：node_modules / 多生态 build 目录 / Rust target |
| **Vitals** | **$9 促销 → $39 常价**（已售 1350+） | ❌ | ✅（仅激活发哈希硬件 ID） | 可选特权助手 | App 级分组 + 30 天历史 + dev servers/端口/Docker + **MCP 已上线**；清理工具在 roadmap |
| **Mac Performance Monitor** | **免费开源 MIT** | ✅ | ✅ | 可选特权助手 | 泄漏指示 + 压力事件 + SQLite 历史 + `mpm` CLI + MCP |
| ProcXray | $15 一次性 | ❌ | ✅ | — | 进程画像最深（env/cwd/dylib/签名校验） |
| DaisyDisk | $9.99 | ❌ | — | 全盘 | 磁盘可视化最佳 |

---

## 3. 功能对比矩阵 · 磁盘清理侧

| 能力 | **Prune** | DevCleaner | CacheCleaner | Mole | CleanMyMac CLI | npkill/kondo | DaisyDisk |
|---|---|---|---|---|---|---|---|
| 包管理器缓存扫描（npm/Yarn/pnpm/brew/pip/cargo/go…） | ✅ | ✅ 30+ 生态 | ✅ | ✅ 11 类 | ✅ 18 种 | — | — |
| **AI 应用缓存**（Cursor/Claude/Ollama/LM Studio） | ❌ | ✅ | ✅ | ✅ | ❌（部分） | — | — |
| Xcode 专项（DerivedData/模拟器） | ⚠️ 缓存级 | ✅ 深 | ✅ | ✅ | ✅ | — | — |
| 项目内 node_modules/.build 全盘扫描 | ✅ 6336 个 <15s | ✅（Ghost Projects） | ✅ | ✅ | ✅ purge | ✅ 专精 | 手动 |
| **跨平台二进制探测**（按架构判死缓） | ✅ **独家** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **多包管理器重复存储统计** | ✅ **独家**（29 GB 级） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 项目健康卡（僵尸依赖/久未重装） | ✅ M24 | ✅（Ghost/Artifacts） | ❌ | ❌ | ⚠️ purge 按新旧预选 | ❌ | ❌ |
| **项目休眠归档 + 一键还原** | ❌ | ✅ Pro（独家能力） | ❌ | ❌ | ❌ | ❌ | ❌ |
| 自动/定时清理、退出 App 触发清理 | ❌（刻意不做） | ✅ Pro（Auto Clean / Smart Triggers） | ❌ | ❌ | ❌ | ❌ | ❌ |
| 风险分级（绿/黄/红） | ✅ 🟢🟡🔴 + 20 GB 熔断 | ✅ Safe/Warning/Danger/Dangerous+ | ✅ 默认全不勾选 | ✅ 按影响分组 | ✅ 预览确认 | ❌ | — |
| **凭据/会话文件硬拒绝清单** | ✅ 白名单机制 | ✅ deny-list | ✅ | ⚠️ 不确定项默认跳过 | ⚠️ | — | — |
| 删除进废纸篓 | ✅ 永远 | ✅ | ✅ | ⚠️ 默认永久删，可设置走废纸篓 | ✅ 确认后删 | ❌ rm | — |
| Docker 处理 | ✅ **只给命令不执行** | ⚠️ Pro 可执行 | ⚠️ | ⚠️ 可执行 | ✅ 可执行 | ❌ | — |
| 清理历史 / 成绩单 | ✅ cut_log 两年 + Wrapped 式报告 | ✅ 30 天 | ❌ | ❌ | ❌ | ❌ | ❌ |
| 菜单栏快捷清理 | ✅ 面板 + 可回收 GB 主数字 | ✅ Quick Clean | ❌ | ✅ HUD（无清理） | ❌ | ❌ | ❌ |
| CLI | ✅ 单二进制双形态 | ❌ | ❌ | ✅（本体就是 CLI） | ✅（本体就是 CLI） | ✅ | ❌ |
| MCP server | ✅ 5 工具（reclaim 白名单兜底） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |

---

## 4. 功能对比矩阵 · 监控诊断侧

| 能力 | **Prune** | Vitals | MPM（免费开源） | Mole | Stats/iStat | ProcXray | FavTray |
|---|---|---|---|---|---|---|---|
| 菜单栏读数/面板 | ✅ 六行统计面板 + 图标动画编码 | ✅ 全可定制 | ✅ | ✅ HUD | ✅ 最全 | ❌ | ✅ |
| 统计总览（CPU/内存/磁盘/网络/端口） | ✅ 六卡只读 | ✅ 六域 + GPU/电池 | ✅ | ✅ | ✅ | ❌ | ✅ |
| App 级分组资源卡 | ✅ Top 12 + 六字段排序 | ✅ 最强（含浏览器拆分） | ✅ 按组 | ⚠️ 进程级 | ❌ | ⚠️ 进程树 | ❌ |
| **真实内存口径 phys_footprint（含换出）** | ✅ **独家** | ❌（RSS 口径） | ❌ | ❌ | ❌ | ❌ | ❌ |
| Swap 仪表 + 换出受害者榜 | ✅ **独家** | ⚠️ 提及 swap 但不做受害者归因 | ⚠️ paging strain 判定 | ❌ | ⚠️ | ❌ | ❌ |
| 泄漏嫌疑判定 | ✅ **侦探案件簿**（立案/侦查/结案/复发） | ✅ 内存增长告警 | ✅ 措辞约束完全一致 | ❌ | ❌ | ⚠️ 内容营销有教程 | ❌ |
| 历史趋势 | ⚠️ CPU 10 min / 网络 5 min + 网络日流量 90 天 | ✅ 30 天 per-app（强 30 倍） | ✅ SQLite 可回溯 | ⚠️ 60 秒 | ⚠️ | ❌ | ❌ |
| 端口反查（谁占 3000 + 属于哪个项目） | ✅ 关注层 + IPC | ✅ Projects 视图 | ❌ | ❌ | ❌ | ✅ | ✅ |
| **端口冲突/被占灵动岛提报** | ✅ **独家**（M19） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 按应用网速 + 今日/近 7 天流量 | ✅ | ✅ | ❌ | ✅ | ✅ | ❌ | ❌ |
| 网络诊断（DNS/网关/出口分层 + 主因结论 + RPM） | ✅ **独家**（whyfi 思路） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **局域网新面孔提醒** | ✅ **独家**（M23，零主动探测） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 灵动岛式事件提报（CPU 暴走/内存异常/端口/新设备） | ✅ **独家**（M15/M19/M23 四类事件） | ⚠️ 通知告警 | ⚠️ 压力事件 | ❌ | ❌ | ❌ | ❌ |
| MCP server | ✅ | ✅ 1.4（2026-09） | ✅ | ❌ | ❌ | ❌ | ❌ |
| Dev servers 按项目分组、可确认停止 | ⚠️ 端口反查归属项目（不代停） | ✅（确认后停） | ❌ | ❌ | ❌ | ❌ | ✅ |
| 磁盘 treemap | ❌ 刻意不做 | roadmap | ❌ | ✅ | ❌ | ❌ | ❌ |
| 卸载器/软件更新 | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ | ❌ |

---

## 5. Prune 独有能力汇总（2026-11 口径，无竞品跟进）

| 独有能力 | 价值锚点 | 被跟进风险 |
|---|---|---|
| 跨平台二进制探测 | 实测 7.74 GB / 浪费比 8.3:1，竞品全部「按路径」不「按架构」 | 中：需要架构判定知识，DevCleaner 的 Ghost Projects 最接近但语义不同 |
| 多包管理器重复存储统计 | 实测 29.25 GB | 中：pnpm 生态外用户多，需要跨包管理器索引对齐 |
| phys_footprint 真实内存 + 换出受害者 | RSS 撒谎案例：113 MB 显示 vs 3432 MB 真实 | 低：竞品均在 RSS 口径 |
| 泄漏侦探案件簿（叙事化） | 案件号/嫌疑人档案/证物曲线/复发判定 | 低：形式独有，MPM 逻辑等价 |
| 端口冲突保镖 + 局域网新面孔 + 灵动岛事件流 | 主动提报 vs 被动查看的范式差 | 中：Vitals 的 dev servers 告警方向相同 |
| 网络分层诊断 + RPM | 「为什么慢」一键归因 + Markdown 报告 | 低：无竞品做 |
| 零权限 + 零遥测 + 单二进制双形态（GUI/CLI/MCP 同核） | 安全与工程信任叙事 | 低（MPM/Vitals 需可选特权助手，Mole 需 FDA） |

---

## 6. 缺口与建议（按优先级）

| 级别 | 缺口 | 竞品已做 | 建议 |
|---|---|---|---|
| **P0** | AI 应用缓存覆盖（Cursor/Claude/Ollama/LM Studio）是 2026 年增长最快的缓存类别，竞品全部已支持，Prune 完全缺失 | DevCleaner / CacheCleaner / Mole | 纳入 A1 扫描路径白名单，🟡 级（模型权重与配置分离判定） |
| **P0** | Xcode 深扫（DerivedData/DeviceSupport/模拟器）只有缓存级 | DevCleaner for Xcode / DevCleaner | 专项扫描组，白名单成本低收益高 |
| **P1** | 历史深度 30 倍差距（Vitals 30 天 per-app vs Prune 5–10 min） | Vitals / MPM | 沿用网络日流量落库模式（SQLite + 保留期），先做 per-app 日粒度即可，不做任意时间序列（守住 README 口径红线） |
| **P1** | 项目休眠归档（archive + 一键还原）是 DevCleaner Pro 的付费锚点 | DevCleaner Pro | 可作为 Prune 商业化时的 Pro 候选功能；免费版先做「僵尸项目账本」已覆盖发现侧（M24） |
| **P2** | Mole 五合一的「广度挤压」（清理+卸载+treemap+监控）会吸走「只想要一个工具」的用户 | Mole | 不追广度；在 README/官网强调「守门员 vs 万金油」的定位对立面 |
| **P2** | 竞品 MCP 普及（Vitals 已上、MPM 已上）后，Prune MCP 的 5 工具只读面偏窄 | Vitals / MPM | 考虑补 `disk_scan --json` 细分视图与 `net_diag` 只读工具；reclaim 保持不加 `--yes` 的 MCP 底线 |
| 观察 | macOS 26 原生菜单栏管理压缩通用监控空间，不影响本品类 | — | 无需动作 |

---

## 附：信息来源

- DevCleaner 官网 / App Store / Launly 上架页（2026-11-11）
- CacheCleaner《Best Mac cleaners for developers, 2026》指南
- Mole 官网 / 小众软件开发者自荐帖 / macappbox（v1.14.0，2026-09-16）/ macfreeup 评测
- MacPaw《Meet CleanMyMac CLI》官方公告（brew 安装，early developer preview）
- Vitals：AppSumo 页 / 作者 attacomsian.com 博客（Vitals 1.4 MCP）/ appsinsight 评估（2026-10-05）
- Prune 仓库内 2026-10 调研档案（内存侧五层全景，本文不重复）
