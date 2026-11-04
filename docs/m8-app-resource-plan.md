# M8 应用维度资源统计方案（A 期：五域聚合）

> 状态：方案待评审（未实施）
> 前置：M2（内存/CPU 域）、M5（CPU 口径）、M7（端口域）已全部落地
> 新增域：「网络连接统计」与「应用磁盘占用」——其余三个资源已有数据，缺的只是聚合与展示

---

## 0. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 应用维度是**聚合视图**，不是第 5 个资源域 | 五个资源全挂到现有「应用」tab 的家族行 + 新增**家族详情卡** | 用户动作仍是「看这一个应用、结束它的进程」，与 M4 域分离依据一致；加 tab 会重复「应用」tab |
| 五个资源里**两个已有、一个半免费、两个要新增采集** | 进程/内存/CPU/端口 已有；网络连接 = M7 同一次 lsof 白捡；磁盘 = 新增按需 du | 详见 §2 现状诊断 |
| 网络只做**连接统计**，不做流量 | macOS 无公开 per-process 字节计数 API | nettop 走私有 XPC，文本解析脆弱且无契约保证 |
| 磁盘做**三层口径**：本体 / 缓存 / 数据 | bundle（.app）du + Caches du + Application Support du（只读） | 实测 Cursor 的 App Support 高达 **35 GB**——这是本功能最有价值的一个读数 |
| 磁盘 du 全部**按需 + TTL 缓存**，不进任何定时轮 | 单应用 0.25–3 s，进 fast round 会让常驻开销翻倍 | 详见 §6 |
| 系统进程、CLI 进程（node/cargo）磁盘显示「—」 | 无 .app bundle、无稳定缓存归属 | 宁可空白，不猜一个数字 |

---

## 1. 需求理解与边界

**需求**：在应用（家族）维度看到每个应用的完整资源画像：进程数、网络、端口、CPU、磁盘。

**用户问题**（这是功能存在的理由，每个 UI 决策对照它检查）：

1. 「Cursor 现在占了我多少资源？」——一眼看全，不用切 4 个 tab 拼答案
2. 「谁连了这么多连接？」——ApexCore 101 条 ESTABLISHED 这种异常，现在完全不可见
3. 「除了内存，这个应用在我磁盘上还躺了多少东西？」——App Support 35 GB 是隐藏大头

**边界（不做项详见 §11）**：

- ✅ 做：家族行内多域摘要徽标 + 展开式**家族资源卡**（六格数据）
- ❌ 不做：新 tab、流量统计（字节数）、全局跨域排序器、实时网络刷新

---

## 2. 现状诊断（代码级，可复查）

| 资源 | 现状 | 缺口 |
|---|---|---|
| 进程 | `FamilyMem.process_count` ✅（M2 B3 家族归类，`.app` 兜底取名） | 无 |
| 内存 | `FamilyMem.footprint/rss/swapped` ✅ + fast 轮 10 s | 无 |
| CPU | `FamilyMem.cpu` ✅（M5 C0，单核口径、下界语义） | **UI 未展示**（M5 的 C1 行内徽标未做，本次顺带补） |
| 端口 | M7 `PortEntry.listeners[].pid` ✅ + 应用行徽标已做 | 无 |
| 网络 | **无**。`net::scan_tcp()` 已跑 `lsof -F pcnPT` 全量 TCP，但 pid+状态解析后被**丢弃**，只留 `(port, established)` 计数 | 加一个 per-pid 聚合字段，**零额外进程成本** |
| 磁盘 | `DiskReport` 是**类别维度**（node_modules/包管理器缓存/Docker），不是应用维度 | 全新：family → .app bundle → Caches / App Support 路径解析 + 按需 du |

**实测（2026-11-04，M2/16G）**：

| 项 | 结果 |
|---|---|
| 全 TCP `lsof -F pcnPT` | ~30 ms / 1275 行 / ESTABLISHED 205 条 |
| 连接 top | ApexCore 101 / Chrome Helper 42 / Electron 21 / Cursor 全家 14 |
| CLOSE_WAIT | 全机仅 1 条（zoom.us）——它是异常信号，平时应该安静 |
| `du -sk` 20 个 /Applications/*.app（热缓存） | **2.88 s**，Xcode 5.2 GB 最大 |
| `du -sk` Cursor 的 Caches+App Support | **0.25 s**，App Support **35 GB** |
| `mdfind kMDItemFSSize`（想走 Spotlight 免 du） | 本机返回空（Spotlight 索引不可依赖），**只能 du** |

---

## 3. 口径定义（正确性核心）

### 3.1 连接（新）

| 字段 | 口径 | 措辞 |
|---|---|---|
| `established` | 该家族所有 pid 的 ESTABLISHED 条数（lsof 同一次输出） | 「N 条活跃连接」 |
| `close_wait` | 同上但只计 CLOSE_WAIT | `> 0` 时标警示：「有连接等待关闭，可能泄漏」——**这是概率提示不是结论** |
| 监听端口数 | 沿用 M7 **关注层**口径（`watched` 的端口才计入） | 沿用 M7 徽标 |

**明确不算连接的**：LISTEN（是服务不是连接）、TIME_WAIT（M7 已有独立解释，不重复）、UDP。

### 3.2 磁盘（新，三层）

| 层 | 路径来源 | 可清理？ |
|---|---|---|
| **本体 bundle** | 家族任一成员 `exe` 里的 `*.app` 段 → 该 .app 的 `du` | ❌ 只读展示 |
| **缓存 cache** | `~/Library/Caches/<名称匹配>`（先试家族名，再试 bundle id） | ⚠️ 只读展示；清理入口统一走「存储」tab 的白名单，本卡不放删除按钮 |
| **数据 data** | `~/Library/Application Support/<名称>`（Electron 系巨户，实测 Cursor 35 GB） | ❌ 用户数据，**绝不提供清理**，只标「用户数据」 |

### 3.3 沿用域（不改）

CPU 单核口径可 >100%、家族求和为下界（`cpu_covered < process_count` 时）；内存 footprint 优先、未深探退 RSS——全部照 M5/M2 原口径。

---

## 4. 必须堵的假事实（措辞与语义）

| # | 假事实 | 真相 | 对策 |
|---|---|---|---|
| 1 | 「连接多 = 网络重」 | 连接数不等于流量，101 条空闲长连接几乎不耗带宽 | 文案只说「连接」，不出现「网络占用」 |
| 2 | 「App Support 35 GB 是垃圾」 | 它含用户数据（工作区、聊天记录、模型文件） | 只读展示 + 明确标「用户数据，不含清理建议」 |
| 3 | 「du 就是它占的磁盘」 | APFS clone/dedup 让多应用共享的框架文件被重复计入（Xcode 的 5 GB 里就有系统 clone） | 详情卡脚注：「按文件名义大小合计，含 APFS 克隆共享部分」 |
| 4 | 「没有磁盘数据 = 没占用」 | 可能只是没扫过（未展开/未缓存） | 未扫时显示「点按加载」，失败显示「不可用」，区分于「—」 |
| 5 | 「CLOSE_WAIT > 0 就是 bug」 | 也可能是对端正常慢关闭 | 措辞「可能泄漏」+ 可点开看哪条连接（不进本期，挂待定 #3） |

---

## 5. 数据模型变更

### 5.1 prune-core `net`（连接统计，纯增量）

```rust
// mod.rs
pub struct ConnStats {
    pub established: u32,
    pub close_wait: u32,
}

// PortSnapshot 增量字段（serde default，旧前端不破）
#[serde(default)]
pub conns_by_pid: HashMap<i32, ConnStats>,
```

`parse_lsof_tcp` 目前已经拿到每行的 pid 与 `TST=` 状态——只是没存。改动约 +15 行 + 2 个单测。

### 5.2 prune-core 新模块 `appdisk`（磁盘归属与测量）

```rust
pub struct AppDisk {
    /// 家族任一成员 exe 解析出的 .app 路径（None = 系统进程/CLI）
    pub bundle: Option<DiskLeaf>,   // path + bytes
    pub cache: Option<DiskLeaf>,    // ~/Library/Caches/<name> du
    pub data: Option<DiskLeaf>,     // ~/Library/Application Support/<name> du
    pub at: i64,                    // 测量时间戳（TTL 依据）
}
```

- 家族 → bundle：`exe` 中找 `.app` 段（`process.rs::app_bundle_name` 同逻辑复用）；同名匹配 `~/Library/Caches` 与 `App Support`，不匹配再查一次 bundle id（读 `Info.plist` 的 CFBundleIdentifier，成本 < 1 ms）。
- Windows：bundle → exe 所在目录；Caches → `%LOCALAPPDATA%\<exe 名>`。首期 Windows 只实现 bundle 层，cache/data 匹配规则挂 windows-support-plan §8 待真机验证。

### 5.3 壳层（src-tauri）

- `port_sampler`：`conns_by_pid` 随 `PortSnapshot` 原样下发（apps tab 的 hydrate 一次即可用，无额外轮询）。
- 新 `app_disk(family)` IPC：单飞 + TTL 30 min + mtime 检查（.app 或缓存根 mtime 未变直接回缓存值）。du 用 stdlib `fs::read_dir` 递归或直接 `du -sk` 子进程（对齐仓库现状：`mem/workspace.rs` 已用子进程模式）。

### 5.4 前端类型（src/types/net.ts 增量 + appdisk.ts 新）

家族聚合**在前端做**（不新增后端聚合命令）：`FamilyMem + ports + conns_by_pid + AppDisk` 在组件层按 pid join，避免后端每轮都做一次无谓合并。

---

## 6. 采样与成本

| 数据 | 触发 | 成本 | 常驻开销 |
|---|---|---|---|
| 连接统计 | 随 M7 端口轮（30 s、tab 门控、单飞） | **+0 ms**（同一次 lsof） | 0（tab 不可见不扫） |
| CPU/内存/进程 | 已有 fast 轮 10 s | 不变 | 不变 |
| 端口徽标 | apps tab hydrate 一次 | 不变 | 0 |
| 磁盘 du | **仅用户展开家族详情卡时**，TTL 30 min 内回缓存 | 0.25–3 s/应用 | **0**（不进任何定时轮） |

磁盘 du 用 `tokio::task::spawn_blocking`，不阻塞采样线程；并发上限 1（同一时间只测一个应用，用户不会同时展开两个卡）。

---

## 7. UI 设计

### 7.1 应用行摘要（AppMemoryList 现有行内徽标扩展）

现有：端口徽标（M7）→ 顺 M5 遗留，补 **CPU 徽标**（`FamilyMem.cpu`，`None` 显示省略）+ **连接徽标**（仅 established ≥ 5 时显示，`≥20` 加警示色）。徽标保持「沉默即正常」：没有异常就不出现，不让每行变成仪表盘。

### 7.2 家族资源卡（点家族行展开，新组件 `AppResourceCard`）

```
┌ Cursor ─────────────────────────────┐
│ 进程 9    内存 12.4 GB   CPU 23%（下界）│
│ 端口 3 个（2 个对外）  连接 14 条活跃     │
│ 磁盘（点按加载 / TTL 后重测）             │
│   本体 0.6 GB · 缓存 2.1 GB ·          │
│   数据 35 GB  ⓘ 用户数据，不可清理        │
└─────────────────────────────────────┘
```

- 六格：进程 / 内存 / CPU / 端口 / 连接 / 磁盘，前五格已有数据即时显示，磁盘格按需。
- 磁盘格三行分别带 ⓘ 说明（§4 假事实对应文案），**不放任何删除按钮**——清理动作统一回「存储」tab。
- CLOSE_WAIT > 0 时连接格下加一行黄色提示（§3.1 措辞）。

### 7.3 不加的东西

不加新 tab；不做「按 CPU/连接排序」的切换器（挂 §9 待定 #4）；菜单栏面板不加六格（空间不够，保持现状端口徽标）。

---

## 8. 分期与任务分解

| 阶段 | 内容 | 预估 |
|---|---|---|
| A0 数据层 | `net::ConnStats` + `conns_by_pid`（含单测）；`appdisk` 模块（解析 + du + TTL 缓存，含单测：路径解析规则全分支） | 0.5 d |
| A1 壳层 | `port_sampler` 透传 conns；`app_disk` 命令（单飞 + TTL）+ lib.rs 注册 | 0.3 d |
| A2 UI | AppResourceCard 组件 + 应用行 CPU/连接徽标 + MainWindow 接线 + devFixture 样本（含 35 GB Cursor 样例与 CLOSE_WAIT 样例） | 0.5 d |
| A3 CLI/MCP/文档 | `prune app <家族名>`（六格文本输出）；MCP `app_resources`（只读）；README/feature-spec/technical-plan 同步；windows-support-plan §8 | 0.3 d |

合计 ~1.6 人天。每阶段过 `cargo test` + `tsc --noEmit` 门槛，提交拆分沿用仓库惯例（feat(net)/feat(disk)/feat(ui)/feat(cli)/docs）。

---

## 9. 待确定问题（评审拍板）

| # | 问题 | 默认建议 |
|---|---|---|
| 1 | 家族资源卡展开位置：嵌在列表行内 vs 右侧详情栏（现 ProcessInspector 的位置） | **行内展开**：列表已是用户心智入口，右侧栏给了单进程 |
| 2 | 连接徽标阈值 | established ≥ 5 显示、≥ 20 警示色（实测分布 42/21/14/9/8——5 的阈值能突出真异常，Chrome 42 会被警示是合理的） |
| 3 | CLOSE_WAIT 是否提供「看哪条连接」的明细 | **不做**（lsof 远端地址可后续加，首期只给计数与提示） |
| 4 | 按域排序切换器（内存/CPU/连接） | **不做**，排序仍按内存；等用户真的要再上 |
| 5 | CLI 家族名匹配用精确还是模糊（`prune app cursor` → Cursor？） | 模糊（大小写不敏感子串，多命中时报全名让用户重试） |
| 6 | Windows 磁盘三层首期范围 | 只做 bundle 层（exe 目录），cache/data 规则进 windows-support-plan 待真机验证 |

---

## 10. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| du 超时/权限失败（保护目录） | 中 | 单格显示「不可用」 | 每 leaf 独立 Option，失败不拖垮其余格 |
| App Support 目录名与家族名不匹配（Electron 常见 `@scope/name`） | 中 | cache/data 格空白 | 兜底链：家族名 → Info.plist bundle id → 放弃（显示「—」） |
| 连接数瞬时波动让徽标闪烁 | 高 | 徽标频繁出没 | 徽标只增不减衰减（沿用 M7 端口徽标同思路）：刷新前保持上次值 |
| 35 GB 数字吓到用户以为是垃圾 | 中 | 误删用户数据 | 「用户数据，不可清理」脚注 + 不放删除入口（§4-2） |
| lsof 输出跨用户缺行导致连接数偏低 | 低 | 只读统计偏低，无害 | 连接数不做 coverage 校验（端口域已做过，连接无「正确分母」） |
| 大 .app du 卡 3 s | 中 | 用户等 | spawn_blocking + TTL + 「测量中…」占位 |

---

## 11. 不做项（含理由，避免二次讨论）

| 不做 | 理由 |
|---|---|
| 流量统计（字节收发） | macOS 无公开 API；nettop 私有 XPC 无契约，解析一次系统更新就碎 |
| 网络按分钟刷新 | 连接数是慢变量，30 s 门控轮足够；真流量问题该用活动监视器 |
| GPU/能耗域 | 用户没提，sysinfo 也不提供；等真实诉求 |
| 磁盘进定时轮 | 3 s 级成本进 10 s fast round = 常驻开销翻倍，违反本产品采样纪律 |
| 家族卡内嵌清理动作 | 白名单 + 废纸篓是存储域的领地（M4 域分离）；跨域放按钮迟早绕过白名单 |
| 全局「资源 Top 榜」跨域 hero | 与「应用」tab 重复；hero 已有两个，再加会挤掉压力读数 |

---

## 12. 验收标准

- [ ] 单测：`parse_lsof_tcp` 的 `conns_by_pid`（多 pid、CLOSE_WAIT 分流、无 pid 行跳过）；`appdisk` 路径解析全分支（.app / 无 .app / bundle id 兜底 / mtime 缓存命中）
- [ ] `cargo test --workspace` 全绿；`tsc --noEmit` 0 错误
- [ ] 实机：Cursor 家族卡显示 9 进程 / 端口 / 连接 ≥ 5（对照 lsof 手工校验）；磁盘三格与 `du -sk` 手工值一致 ±1%
- [ ] 磁盘格未点按时显示「点按加载」，不自动扫；TTL 内二次展开不重扫
- [ ] CLOSE_WAIT=0 时提示不出现（沉默即正常）
- [ ] CLI `prune app cursor` 输出六格；MCP `app_resources` 可查
- [ ] 系统进程（如 rapportd）磁盘格显示「—」而非报错

---

## 13. 提交拆分（沿用仓库 commit 规范）

1. `feat(net): 连接统计 per-pid 聚合（复用 M7 lsof 轮）`
2. `feat(disk): 应用磁盘三层口径与按需测量（appdisk）`
3. `feat(ui): 家族资源卡 + CPU/连接行内徽标`
4. `feat(cli): prune app 与 MCP app_resources`
5. `docs: M8 应用维度资源统计方案与上层文档同步`
