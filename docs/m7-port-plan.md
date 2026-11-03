# Prune · 端口占用（M7）详细方案

> 状态：方案待评审 · 前置：M1 磁盘 / M2 内存 / M3 泄漏 / M4 域分离 / M5 CPU / M6 tray 动画 均已完成
> 一句话：**端口不是第四个资源域，是「进程」这个实体的反查索引**——回答"谁占着 3000"，而不是"全机有多少端口"。

## 0. 结论先行

1. **不做成资源域**：M4 的域划分依据是「用户动作不同」（内存=减压、存储=腾空间）。端口的动作（结束进程）与内存完全一致，按 M5 的同一套逻辑本不该独立；但端口是**离散多值标签**（端口 ↔ 进程 N:N），排不进"应用行"这个连续量位置，所以需要一个**以端口为主键的索引视图**。定位是工具视图，不占面板 hero、不与内存/存储并列成"三大资源"。
2. **数据几乎零成本**：`lsof -nP -a -iTCP -sTCP:LISTEN -F pcn` 实测 **30–40 ms**，输出 30 条，与 `netstat -anv -p tcp` 的 LISTEN **30 条逐条一致**（交叉校验通过）。零权限、零新依赖——`mem/workspace.rs:396` 早就在用 `/usr/sbin/lsof`。
3. **70% 的监听端口是噪音，所以「关注度分层」是核心设计而非可选优化**：本机 27 个唯一端口里，19 个是 Electron / 微信 / Cursor Helper / rapportd 的内部 IPC 随机端口。全列出来 = 用户被淹没 = 功能失败。
4. **「端口 → 项目目录」是 Prune 独有能力**，别的端口工具只给 pid：实测端口 `3118`（node，`*:3118` 暴露全部网卡）的 cwd 是 `/Users/xuyi/Desktop/git/meetily/frontend`，端口 `1430` 是 `/Users/xuyi/Desktop/git/prune`。成本 20 ms/进程，只对监听进程跑。
5. **三个假事实必须先堵掉**（不处理就是撒谎，比不显示更糟）：
   - **「没查到」≠「端口空闲」**：非 root 的 lsof 只覆盖同 uid 进程，系统/其他用户进程可能静默缺失。
   - **TIME_WAIT 幽灵占用**：实测 326 条 TIME_WAIT，lsof 一条都看不到（内核态无 fd），但 `EADDRINUSE` 真实发生。必须能解释。
   - **双栈 1 服务 2 行**：同一 socket 在 v4/v6 各占一个 fd，实测 5000/7000/58578 三个端口重复出现，必须合并成一行。
6. **四阶段 P0→P3，约 1.7 人天**，每阶段独立可验收（数据层 → 主窗口 → 联动 → CLI/MCP 与文档）。
7. **明确不做**：抓包、流量统计、连接历史、端口扫描（探测外部主机）、防火墙规则管理、自动 kill 占位进程（§11 逐条给理由）。

## 1. 需求理解与边界

### 1.1 三个真问题

| 用户当下的问题 | 现在能不能答 | 端口补上之后 |
|---|---|---|
| 「`npm run dev` 报 EADDRINUSE，3000 是谁占着？」 | 完全无入口，只能自己 `lsof -i:3000` | 搜 `3000` → 一行给出进程名 + 项目 + 暴露面 → 右键结束 |
| 「我起了几个 dev server？哪些忘了关？」 | 无入口 | 关注层列表按端口排序，每行带项目目录 |
| 「我这个服务是不是暴露到局域网了？」 | 无入口（`0.0.0.0` 与 `127.0.0.1` 只在命令行里可见） | 暴露面列直接标「仅本机 / 全部网卡」，🔴 标全部网卡 |

**三条都不是"端口监控"**——没人需要长期盯着端口。它们都是**排障动作的起点**，生命周期以秒计。这决定了：不做历史曲线、不做告警、不常驻高频采样。

### 1.2 边界（写死，评审时不要再扩）

| ✅ 做 | ❌ 不做 |
|---|---|
| TCP 监听端口（LISTEN）+ 所属进程 | 抓包 / 协议识别（"这是 HTTP 还是 Redis"） |
| UDP 绑定端口（过滤无端口的通配 fd） | 流量速率 / 字节统计 |
| 端口 → 进程 → 项目目录（复用 M2.5 的 cwd/workspace 归属） | 端口历史曲线 / 端口变化告警 |
| 暴露面分级（仅本机 / 全部网卡 / 指定网卡） | 主动扫描外部主机端口（那是 nmap，不是 Prune） |
| 每端口的已建立连接数（折叠展示） | 连接级明细（对端 IP、地域） |
| TIME_WAIT 解释（"无进程持有但内核未释放"） | 修改 sysctl 缩短 TIME_WAIT |
| 复制端口号 / 在浏览器打开 / 结束进程 | 自动 kill 或"一键释放全部端口" |

### 1.3 信息架构：三个选项

| 维度 | A · 主窗口第 4 tab「端口」（推荐） | B · 并入「应用」tab 做子视图 | C · 独立「网络」资源域 |
|---|---|---|---|
| 心智 | 端口是索引，有独立主键与搜索框 | tab 不增加 | 与内存/存储并列，用户误以为要"管网络" |
| 排序 | 按端口号，天然有序 | 应用 tab 按内存排序，端口按端口号，两套心智挤一处 | 同 A |
| 面板（380px） | 不加 tab，只在应用行挂端口徽标 | 同 | 放不下 4 列，硬加会压垮首屏 |
| 改动量 | 中：新 tab + 新组件 | 中：列表要支持双模式 | 大：面板 hero 要分三区 |
| 风险 | 低 | 中（污染应用列表语义） | 高（违背 M4 域分离） |

**推荐 A。** 理由：端口需要一个**以端口为主键的可搜索视图**，这是应用列表给不了的；但它没有独立的资源语义（不产生压力、不占容量），不该升格成域，因此只进主窗口不进面板 hero。

```
主窗口 tabs：内存 · 应用 | 内存 · 概况 | 存储 | 端口
面板 tabs：  应用 | 概况        ← 不变
```

## 2. 现状诊断（代码级，可复查）

### 2.1 全仓零端口能力

| 位置 | 现状 |
|---|---|
| `crates/prune-core/src/lib.rs:15-25` | 模块：classify / db / disk / leak / mem / paths / reclaim / swap / win——**无 net** |
| `src/types/mem.ts` `ProcMem` | 无任何端口字段 |
| `crates/prune-core/src/mem/process.rs:107-147` | `scan()` 只取 sysinfo 的内存/CPU，sysinfo 0.39 **不暴露 socket** |
| `src/windows/MainWindow.tsx:30` | `MainTab = "apps" \| "overview" \| "disk"`——三 tab |
| `crates/prune-cli/src/main.rs:42-74` | 子命令：mcp / scan / reclaim / mem——无 port |
| `crates/prune-core/src/mem/workspace.rs:396` | **已在用** `/usr/sbin/lsof -Fn`（每进程一次，24 进程 / 12 并发）→ 端口可以复用同一套调用模式 |

结论：端口要从零建，但**不需要新依赖**——lsof 的依赖形态、超时处理、并发池模式在 `workspace.rs` 已有成熟实现可直接抄。

### 2.2 本机实测基线（2026-11-03 16:20，Apple M2 / 16 GB / macOS）

| 项 | 实测值 |
|---|---|
| `lsof -nP -a -iTCP -sTCP:LISTEN -F pcn` | **30–40 ms** |
| 同上不加 `-sTCP:LISTEN`（全 TCP，`-F pcnPT`） | **30 ms**，306 条 |
| `netstat -anv -p tcp` | **<10 ms** |
| 全量 fd `lsof -nP -F pcfn` | **270 ms**，35910 行 |
| 单进程 `lsof -a -p <pid> -d cwd -Fn` | **20 ms** |
| LISTEN 条数：lsof vs netstat | **30 : 30**（逐条地址端口一致） |
| 唯一监听端口 | **27**（3 个双栈重复：5000 / 7000 / 58578） |
| 全 TCP 状态分布 | 218 ESTABLISHED / 30 LISTEN / 27 CLOSED / 24 FIN_WAIT_1 / 4 SYN_SENT |
| TIME_WAIT（netstat） | **326 条**（lsof 一条不可见） |
| UDP 绑定 | 15 条，其中 **4 条 `*:*`**（无端口通配，必须过滤） |
| 噪音比 | **19 / 27 = 70%** 是内部 IPC 随机端口 |
| 关注层端口清单 | 1430(node,/prune) · 3118(node, meetily/frontend) · 5000(ControlCenter) · 7000(ControlCenter) · 7890(ApexCore) · 58578(rapportd) · 8830/8829/8828(Cursor Helper) |

### 2.3 从基线直接读出的四条结论（写进口径）

1. **成本不构成约束**：30 ms 与 `scan()`（20–33 ms）、单进程 lsof（20 ms）同量级。放进任何一轮都不会让预算爆掉。真正的约束是**语义**不是性能。
2. **双栈是默认行为不是异常**：macOS 上 `listen()` 不指定 address 时默认 v6 + v4-mapped，所以 3/27 端口天然两条 fd。UI 必须合并，否则用户看到"5000 出现两次"第一反应是 Prune 有 bug。
3. **TIME_WAIT 占绝对多数（326 vs 30）**：这就是"lsof 说没有，但端口就是起不来"的根源。不解释这一条，功能会在最常见的排障场景上失信。
4. **`lsof -p` 与 `lsof -i` 是 OR 关系**：实测 `lsof -p 626 -iTCP -sTCP:LISTEN -Fn` 会把微信的 cwd、txt、mmap 全部吐出来（`n/Applications/WeChat.app/...`）。**必须加 `-a`**（AND），否则解析器会把文件路径当 socket 地址。这是本方案最容易写错的一行代码。

## 3. 口径定义（本方案的正确性核心）

### 3.1 字段表

| 字段 | 含义 | 量纲 / 范围 | macOS | Windows | `null` / 缺省语义 |
|---|---|---|---|---|---|
| `binding.port` | 监听/绑定端口 | 1–65535 | lsof `n` 行末尾 | `MIB_TCPROW_OWNER_PID.dwLocalPort`（网络序，需 ntohs） | — |
| `binding.proto` | 传输层 | `tcp` / `udp` | `P` 行 | TCP/UDP 分表查询 | — |
| `binding.scope` | 绑定范围 | `loopback` / `any` / `interface` | 地址解析 | 同 | 解析失败 → `unknown`，UI 不标暴露面 |
| `binding.addr` | 原始绑定地址 | 字符串 | `127.0.0.1` / `*` / `[::1]` / `192.168.0.115` | 点分 / `0.0.0.0` | — |
| `binding.v6` | 是否 IPv6 socket | bool | 地址含 `:` | 表来源 tcp6 | — |
| `binding.pid` | 持有者 | i32 | `p` 行 | `dwOwningPid` | 0 = 内核持有（Windows）→ 映射为 `null` |
| `entry.conns` | 该端口的 ESTABLISHED 数 | u32 | 全 TCP 轮统计 | 同 | 未采集 → `null`，UI 不显示 |
| `snapshot.timeWait` | 内核 TIME_WAIT 条数 | u32 | `netstat` | `GetTcpTable` 枚举 State=12 | 采集失败 → `null`，整条解释隐藏 |
| `snapshot.coverage` | 可见性说明 | 结构体 | 见 §3.2 | Windows 完整 | — |

### 3.2 三个假事实陷阱（P0 级，必须堵）

#### 3.2.1 「没查到」≠「端口空闲」

非 root 的 lsof 对无权访问的进程**静默跳过**，不报错、不警告（实测 stderr 为空）。因此 Prune 永远无法断言"这个端口没人占"。

方案：

```rust
pub struct Coverage {
    /// 本机可见的 LISTEN 总数（与 netstat 交叉校验后的值）
    pub listen_total: usize,
    /// 其中能归属到进程的条数（lsof 拿到的）
    pub attributed: usize,
    /// 交叉校验：netstat 说 N 个监听，lsof 只给 M 个（M < N 说明有不可见进程）
    pub kernel_only: usize,
    /// 非空 = 覆盖面受限，UI 必须显示这句话
    pub note: Option<String>,
}
```

- 每次扫描同时跑 lsof 与 netstat（后者 <10 ms），**用 netstat 的 LISTEN 数做分母**：`attributed < listen_total` 时 `note = "有 N 个监听端口无法归属到进程（可能属于其他用户或系统进程）"`。
- 查不到端口时，UI 文案固定为 **「当前用户可见范围内没有进程监听 3000」**，绝不写「端口 3000 空闲」。
- 兜底动作：只给一条可复制的命令 `sudo lsof -nP -i:3000`，**Prune 自己永不提权**（沿用 README 安全护栏第 5 条，与 Docker 项同一处理方式）。

实测本机 `attributed == listen_total == 30`（单用户机器），但这是**实证结论不推广**——UI 逻辑不依赖它。

#### 3.2.2 TIME_WAIT 幽灵占用

TIME_WAIT 的连接已无 fd，lsof 完全看不到，但内核仍占着四元组，`bind()` 依然可能失败。实测 326 条 TIME_WAIT vs 30 条 LISTEN。

方案：

- 扫描时并行跑 `netstat -anv -p tcp`（<10 ms），统计 `TIME_WAIT` 条数，进 `snapshot.timeWait`。
- 端口详情（或搜索无结果时）显示：`内核 TIME_WAIT 12 条（无进程持有，通常 60 s 内自动释放）`。
- 绝不把 TIME_WAIT 渲染成"端口被占用"——它没有持有者，右键菜单不该出现"结束进程"。
- 进阶（可选，P2）：解析 TIME_WAIT 行的本地端口，若用户搜索的端口正好在里面，直接给出解释。`netstat` 输出第 4 列 `addr.port` 可直接提取。

#### 3.2.3 双栈与 SO_REUSEPORT：1 个端口 N 个持有者

- **双栈**：同一 socket 在 v4/v6 各一条（实测 5000/7000/58578）。按 `(port, proto)` 聚合，`listeners` 数组里两条 → UI 一行显示，暴露面取**信息量更高的那个**（`any` > `interface` > `loopback`），并标 `IPv4+IPv6`。
- **SO_REUSEPORT**：Node cluster / nginx worker 会让多个**不同 pid** 监听同一端口。`listeners.len() > 1` 时 UI 显示 `2 个进程`，展开可见；结束进程必须让用户选具体哪个，不能"一键全 kill"。
- **同一进程多 fd 同端口**（实测 rapportd 的 f8/f9 都是 `*:58578`）：按 `(pid, port, addr, v6)` 去重，否则一个服务显示成 2 条。

### 3.3 关注度分层（产品核心）

| 层 | 判定规则 | 默认呈现 | 本机实测 |
|---|---|---|---|
| **关注** | 常见开发端口（3000-3002 / 4200 / 4321 / 5000① / 5173 / 5174 / 6006 / 7000① / 8000-8002 / 8080-8081 / 8888 / 9000 / 9090）+ 数据库（3306 / 5432 / 6379 / 27017 / 9200 / 11211）+ 全部 `<1024` | 展开 | 8 个 |
| **折叠** | ephemeral 高位（`>=49152`）与非常见高位端口 | 收进「其他 19 个」 | 19 个 |
| **不显示** | UDP 的 `*:*`（无绑定端口通配，实测 4 条）、非 LISTEN 的 socket fd、`lsof` 的 cwd/txt 行 | 过滤 | — |

① 5000/7000 是 macOS 的 **AirPlay 接收器**（`ControlCenter` 持有），是 Django/Flask 开发者最经典的"端口凭空被占"。方案要求对这一对端口做**专门标注**：`AirPlay 接收器 · 系统设置 → 通用 → 隔空投送与接力 可关闭`。这是把静态列表变成排障助手的关键一处，成本 10 行常量表。

> 分层表放在 `prune-core/src/net/mod.rs` 的常量里，CLI / GUI / MCP 三处共用，不允许前端各自维护一份。

### 3.4 暴露面语义

| scope | `addr` 形态 | UI 文案 | 语气 |
|---|---|---|---|
| `loopback` | `127.0.0.1` / `[::1]` | 仅本机 | 中性 |
| `interface` | 具体网卡 IP（`192.168.0.115`） | 指定网卡 192.168.0.115 | 中性 |
| `any` | `*` / `0.0.0.0` / `[::]` | **全部网卡**（同网段设备可访问） | 🔴 提示色，不吓人 |

**文案红线**：可以陈述"同网段设备可访问"这个事实，**绝不写**"存在被入侵风险 / 建议立即关闭"。判定权交回用户（与 M5 §6.3、M3 泄漏措辞同一原则）。

## 4. 数据模型变更

### 4.1 Rust（新模块 `crates/prune-core/src/net/`）

```rust
// net/mod.rs
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Proto { Tcp, Udp }

#[serde(rename_all = "lowercase")]
pub enum BindScope { Loopback, Interface, Any, Unknown }

pub struct PortBinding {
    pub port: u16,
    pub proto: Proto,
    pub scope: BindScope,
    pub addr: String,
    pub v6: bool,
    pub pid: Option<i32>,      // None = 内核持有（Windows 的 pid 0）
    pub start_time: u64,       // 防 pid 复用串台，规则同 merge_deep
}

pub struct PortEntry {         // 按 (port, proto) 聚合
    pub port: u16,
    pub proto: Proto,
    pub listeners: Vec<PortBinding>,
    /// 该端口的 ESTABLISHED 连接数；None = 本轮未统计
    pub conns: Option<u32>,
    /// 聚合后的暴露面（取 listeners 中最宽的那个）
    pub exposure: BindScope,
    /// 是否命中关注层规则（§3.3）
    pub watched: bool,
}

pub struct PortSnapshot {
    pub entries: Vec<PortEntry>,   // 按 port 升序
    pub coverage: Coverage,
    pub time_wait: Option<u32>,
    pub at: i64,
    pub scan_ms: u64,
}
```

解析与采集（`net/ports.rs`）：

| 函数 | 职责 |
|---|---|
| `parse_lsof_f(&str) -> Vec<PortBinding>` | 解析 `-F pcnPT` 的机器格式（单测重点：多 fd 去重、`-a` 缺失时过滤非 socket 行） |
| `parse_lsof_cwd(&str) -> Option<String>` | 解析 `-F n` 的 cwd 行 |
| `scan_listeners() -> Result<Vec<PortBinding>, String>` | 一次 `lsof -nP -a -iTCP -sTCP:LISTEN -F pcnPT` |
| `scan_udp() -> Result<Vec<PortBinding>, String>` | 一次 `lsof -nP -a -iUDP -F pcn`，过滤 `*:*` |
| `count_time_wait() -> Option<u32>` | `netstat -anv -p tcp` 数 TIME_WAIT |
| `attribute_projects(&mut [PortEntry], ...)` | 对监听 pid（去重后通常 8–12 个）并发跑 cwd + 复用 `workspaces` 缓存 |
| `group(entries) -> Vec<PortEntry>` | 聚合 + 双栈合并 + 关注度打标 |

平台分支：

| 平台 | 实现 | 权限 |
|---|---|---|
| macOS | `lsof` + `netstat`（现状即最优：30 ms / 零权限） | 同 uid 可见，跨用户静默缺失 |
| Windows | `GetExtendedTcpTable`（`iphlpapi`）→ `MIB_TCPROW_OWNER_PID`；UDP 走 `MIB_UDPROW_OWNER_PID` | 全进程可见（不需要提权），`pid=0` 映射 `None` |
| Linux | `ss -tlnpH` 优先；无 `ss` 时 `/proc/net/tcp` + inode 反查（贵，O(fd)） | 同 uid |

Windows 需要 `Cargo.toml` 增 feature：`Win32_NetworkManagement_IpHelper` + `Win32_Networking_WinSock`（`windows-sys 0.61` 已有依赖，只加 feature）。**不手写结构体布局**——沿用 W2 的原则（`win.rs` 注释）。

### 4.2 TypeScript（`src/types/net.ts` 新增）

镜像上文结构（camelCase）。新增 `src/lib/format.ts` 的 `formatPort(port, proto)`：`3000` / `53/udp`。

**不进 `MemSnapshot`**：端口采样频率（30 s）与内存快照（10 s）不同步，混在一个 payload 里必然出现"端口数据 30 s 旧、内存 10 s 新"的时间戳混乱，且 358 进程的 `ProcMem` 每个都挂 `ports: []` 会让每 10 s 的广播无谓膨胀。端口走**独立 State + 独立事件 `port://snapshot`**，前端按 `pid + startTime` join。

## 5. 采样与成本

### 5.1 为什么不进 10 s fast round

`fast_round` 实测 20–33 ms；端口扫描 30–40 ms 是 **+100% 以上**，且端口变化频率比内存低两个数量级（一次 dev server 启动/停止）。按固定频率采样是纯浪费。

### 5.2 触发策略（三级）

| 触发 | 条件 | 去抖 | 说明 |
|---|---|---|---|
| **按需（主）** | 主窗口切到「端口」tab / 用户点刷新 / 搜索输入 | 3 s TTL | 唯一的常规路径：有人看才扫 |
| **定时（辅）** | 端口 tab 可见时 30 s 一次 | — | 覆盖"边看边起服务"的场景 |
| **应用联动** | 应用 tab 打开时读最近一次快照 | — | 徽标允许陈旧，tooltip 标 `N 秒前` |

单飞闸门沿用 `DeepInFlight` 的模式（`sampler.rs:72-92`）：同一时刻只允许一轮端口扫描，后到的直接返回上一轮结果。

**无可见消费者时不采样**：主窗口关闭 / 不在端口 tab → 定时器不跑，常驻开销为 0。这一点写进验收（§12-9）。

### 5.3 成本表

| 项 | 数值 | 说明 |
|---|---|---|
| 单轮（LISTEN + UDP + netstat） | **≈ 45 ms** | 三次子进程，可并发 → 实测预期 ≈ 35 ms |
| 项目归属（8–12 个 pid × cwd，并发 6） | **≈ 25 ms** | 只对监听进程，不是全部 358 个 |
| 单轮合计 | **≈ 60–70 ms** | 只在触发时发生 |
| 常驻开销（tab 可见、30 s 轮） | **0.2%** | 60 ms / 30 s |
| 常驻开销（tab 不可见） | **0** | 不采样 |
| payload | ≈ 3.5 KB（27 端口 × 平均 2 绑定） | 对比 MemSnapshot ≈ 55 KB |

## 6. UI 设计

### 6.1 主窗口「端口」tab（960×640）

```
端口                                             [3000________]  [刷新]
27 个监听 · 8 个关注 · 19 个内部端口已折叠 · 仅当前用户可见

端口   进程              项目                     暴露面        连接
3000   node              prune                    仅本机         2
3118   node              meetily/frontend         全部网卡 🔴    2
5000   ControlCenter     AirPlay 接收器 · 可在系统设置关闭      仅本机   —
7000   ControlCenter     AirPlay 接收器 · 可在系统设置关闭      仅本机   —
7890   ApexCore          —                        仅本机         0
5173   node              AI                       仅本机         1
───── 其他 19 个（Electron / 微信 / Cursor 内部 IPC）─────  [展开]
```

- grid：`[64px_minmax(0,1fr)_minmax(0,1.1fr)_104px_48px]`，端口号等宽右对齐。
- 搜索框：输入数字前缀即过滤（`30` 命中 3000/3001/3010）；输入进程名/项目名也过滤（三字段 OR）。
- 关注层始终在折叠区之上，折叠区默认收起，头部显示条数。
- 行右键菜单（复用 `ProcKillMenu` 的两步确认）：结束进程（SIGTERM）/ 强制结束（SIGKILL）/ 复制端口（`3000`）/ 复制地址（`http://127.0.0.1:3000`）/ 在浏览器打开 / 在 Finder 中显示项目。
- 行点击 → 展开详情：全部 binding（含 v4/v6 拆分）、pid、启动时间、该端口的 TIME_WAIT 提示、`sudo lsof -nP -i:3000` 兜底命令（只读展示，不执行）。
- 搜索无结果 → 不是空列表，而是解释块（§3.2.1 + §3.2.2 组合的三种可能原因）。

### 6.2 应用 tab 联动

- `FamilyMem` 行尾（总量右侧）在**有监听端口时**追加徽标 `:3000`（最多 2 个，其余 `+N`），灰色、`text-[10px]`、可点击 → 跳端口 tab 并预填搜索。
- 只有关关注层端口才显示徽标——否则 Cursor 每行都挂 `:50272`，纯噪音。
- 进程详情（`ProcessInspector`）新增「监听端口」一行：`3000 (tcp, 仅本机) · 3118 (tcp, 全部网卡)`；无则 `未监听端口`。**不写「无」**——`未监听` 与 `查不到` 是两回事，后者要说清。

### 6.3 面板（380×560）

**不加 tab、不新增行**。只在应用行（`AppMemoryList` compact 模式）总量后补 `·2 端口`（有监听才出现），点击 → `openMain(undefined, "port")`。

理由：380px 放不下 `端口 | 进程 | 项目 | 暴露面` 四列，硬塞会压垮 M4 好不容易稳住的 hero 高度预算；面板的职责是"一眼看总量 + 快速跳转"，不是排障。

### 6.4 文案硬约束

| 允许 | 禁止 |
|---|---|
| 「当前用户可见范围内没有进程监听 3000」 | 「端口 3000 空闲」 |
| 「全部网卡 · 同网段设备可访问」 | 「存在被入侵风险 / 建议立即关闭」 |
| 「内核 TIME_WAIT 12 条 · 无进程持有」 | 「端口被占用（TIME_WAIT）」 |
| 「19 个内部端口已折叠」 | 「发现 19 个可疑端口」 |
| 「AirPlay 接收器 · 可在系统设置关闭」 | 「系统占用，无法释放」 |
| 「2 个进程共同监听（SO_REUSEPORT）」 | 「端口冲突，建议清理」 |

## 7. 安全联动

| 项 | 做法 |
|---|---|
| 结束进程 | 复用 `commands.rs::kill_process` 的现有护栏（`pid <= 1`、自身）+ M5 §7 的系统进程名单；端口列表里 `ControlCenter` / `rapportd` 这类行渲染「系统」徽标且不出现右键结束入口 |
| `pid: None`（内核持有） | 不渲染任何操作按钮 |
| SO_REUSEPORT 多持有者 | 菜单列出每个 pid 的进程名，逐个选择；**不提供「全部结束」** |
| 永不提权 | 兜底只给 `sudo lsof` 命令文本，不执行、不请求 |
| 浏览器打开 | 只拼 `http://127.0.0.1:<port>`（loopback 恒定），不用 `0.0.0.0` 或外部 IP，避免把用户导到局域网地址 |
| MCP | `port_listeners` 只读；不暴露任何结束进程的工具 |

## 8. 分期与任务分解

### P0 · 数据层（~0.6 天）

| 文件 | 改动 | 验收 |
|---|---|---|
| `crates/prune-core/src/net/mod.rs`（新） | 类型 + 关注度常量表 + 聚合/双栈合并/关注度打标 | 单测：双栈合并成 1 条、`*:*` 过滤、SO_REUSEPORT 保留 2 条、关注层命中 |
| `crates/prune-core/src/net/ports.rs`（新） | lsof/netstat 调用与解析（macOS）、Windows `GetExtendedTcpTable` 分支 | 单测：`-F` 解析（含缺 `-a` 的脏输入）、cwd 解析、TIME_WAIT 计数 |
| `crates/prune-core/src/net/host.rs`（新） | cwd → 项目归属（CLI 进程可信 / GUI 退 workspace 缓存） | 实测：端口 3118 → `meetily/frontend` |
| `crates/prune-core/src/lib.rs` | 注册 `pub mod net;` | `cargo check` |
| `src-tauri/src/sampler.rs` | `PortState` + `port_round()` + 单飞闸门 + `port://snapshot` | 日志打印耗时与条数 |
| `src-tauri/src/commands.rs` | `port_snapshot` / `port_refresh` | 前端可调 |

### P1 · 主窗口端口 tab（~0.6 天）

| 文件 | 改动 |
|---|---|
| `src/types/net.ts`（新） / `src/lib/ipc.ts` / `src/lib/format.ts` | 类型镜像 + `portSnapshot()` / `portRefresh()` + `formatPort` |
| `src/stores/portStore.ts`（新） | snapshot / scanning / error / visible（供 §5.2 的门控） |
| `src/components/PortList.tsx`（新） | 关注层 + 折叠区 + 搜索 + 右键菜单（复用 `ProcKillMenu`） |
| `src/windows/MainWindow.tsx` | 第 4 tab；`ui://tab` 接受 `port`；刷新按钮接端口 |
| `src/lib/devFixture.ts` | 补 27 条端口 fixture（浏览器态可迭代） |

验收：搜索 `3118` 一行命中并显示项目名；折叠区展开/收起；右键结束进程两步确认可用。

### P2 · 联动与面板（~0.3 天）

| 文件 | 改动 |
|---|---|
| `src/components/AppMemoryList.tsx` | 关注层端口徽标（最多 2 个 + `+N`），点击跳转 |
| `src/windows/MainWindow.tsx` | 进程详情加「监听端口」行 |
| `src/panels/MenuBarPanel.tsx` | 应用行 `·N 端口`；点击 `openMain(undefined, "port")` |

### P3 · CLI / MCP / 文档（~0.3 天）

| 文件 | 改动 |
|---|---|
| `crates/prune-cli/src/main.rs` | `prune port [--json] [--all]` / `prune port <PORT>` |
| `crates/prune-cli/src/mcp.rs` | `port_listeners` 工具 |
| `README.md` | 「✅ 做」加一行端口索引；CLI 段落补 `prune port` |
| `docs/prune-feature-spec-v2.md` §2.1 | 边界表加一行 |
| `docs/prune-tauri-technical-plan.md` §2 | 模块表加 `net/` |
| `docs/windows-support-plan.md` | 补端口平台差异（Windows 完整可见 / 无需提权） |

## 9. 待确定问题（评审拍板）

| # | 问题 | 默认建议 |
|---|---|---|
| 1 | 「端口」做第 4 tab 还是并进应用 tab？ | 第 4 tab（§1.3 推荐 A） |
| 2 | 是否显示每端口的 ESTABLISHED 数？ | 显示，但放行尾淡色、仅关注层出现（对"有人在用吗"有价值，且全 TCP 轮同样是 30 ms） |
| 3 | UDP 端口默认显示还是折叠？ | 默认折叠（实测 15 条里 11 条是 mDNS/系统，噪音比 TCP 更高） |
| 4 | 关注层端口号清单要不要让用户自定义？ | 本期不做；常量表落 `net/mod.rs`，后续可加本地配置文件 |
| 5 | 端口数据是否入库？ | 不入库（无历史价值，且要 schema 迁移） |
| 6 | 面板是否显示端口总数？ | 不显示（面板不放端口信息，只有行内徽标） |

## 10. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 把「查不到」渲染成「空闲」 | 高 | **高（假事实）** | netstat 做分母 + `Coverage.note` + 固定文案；验收 §12-4 专门测 |
| 双栈导致 1 服务 2 行 | 高 | 中（用户以为有 bug） | 按 `(port,proto)` 聚合；单测锁定 5000/7000/58578 形态 |
| 缺 `-a` 把 cwd/txt 当 socket | 中 | 中（地址列出现文件路径） | 单测喂真实脏输出（实测已确认此行为）；解析器只认 `TST=LISTEN` 或地址含 `:` |
| TIME_WAIT 被当成"占用" | 中 | 高（误导排查方向） | 独立字段 + 独立解释文案 + 不渲染结束入口 |
| ephemeral 噪音淹没关注层 | 高 | 高（功能不可用） | §3.3 分层，实测噪音比 70%；验收要求关注层 ≤10 行 |
| 端口与进程竞态（lsof 期间进程退出） | 中 | 低 | 校验 `start_time`，对不上标「已退出」，不归属项目 |
| Windows 端口号字节序 | 中 | 高（端口显示成 3000→47104） | `u16::from_be()` 单测；`MIB_TCPROW` 端口字段是网络序 |
| 旧前端无 `net` 模块 | 低 | 低 | 新 tab 独立，不改 MemSnapshot，零兼容负担 |
| 隐私/安全观感（列出暴露端口像扫描器） | 低 | 中 | 只读、只本机、不探测外部；文案只陈述事实 |

## 11. 不做项（含理由，避免二次讨论）

| 不做 | 理由 |
|---|---|
| 端口历史曲线 / 变化告警 | 端口生命周期以秒计，盯着看没有决策价值；与"浪费探测器"定位无关 |
| 流量 / 字节统计 | 需 nettop / NEFilter，代价与权限都超预算（README 已明确不做通用网络监控） |
| 协议识别（"这是 HTTP/Redis"） | 探测要发包，Prune 只读不碰网络 |
| 主动端口扫描（外部主机） | 那是 nmap；且触碰法律边界 |
| 一键释放全部端口 / 自动 kill | 误杀 dev server 会丢用户的工作状态；必须逐个确认（沿用结束进程的两步确认） |
| 防火墙规则 / 关闭 AirPlay | 需要 root 或系统设置跳转；只给"在哪里关"的文字 |
| `/etc/services` 端口名映射 | 会输出"http-alt"这类对开发者无用的名字，关注层常量表更精准 |
| 入 SQLite 做趋势 | 无消费场景（§9-5） |

## 12. 验收标准

| # | 项 | 通过条件 |
|---|---|---|
| 1 | 单测 | `cargo test --workspace` 全绿；新增 ≥8 条（`-F` 解析、双栈合并、`*:*` 过滤、SO_REUSEPORT、关注层命中、TIME_WAIT 计数、cwd 解析、端口字节序） |
| 2 | 一致性 | Prune 列出的 LISTEN 集合与 `lsof -nP -iTCP -sTCP:LISTEN` 逐条一致（本机应为 30 条 / 27 端口） |
| 3 | 新服务识别 | `python3 -m http.server 3000` 启动后，切端口 tab（或点刷新）≤3 s 内出现该行，进程名/项目/暴露面正确 |
| 4 | 权限诚实 | 用另一用户起监听（或系统进程）时，`coverage.note` 出现，且查不到端口时文案是「可见范围内没有」而非「空闲」 |
| 5 | 双栈 | `node` 默认 `::` 监听的服务只显示 1 行，标注 `IPv4+IPv6`（不得出现两行） |
| 6 | TIME_WAIT | 端口详情能看到内核 TIME_WAIT 条数；该条目无结束进程入口 |
| 7 | 项目归属 | 端口 3118 → `meetily/frontend`；端口 1430 → `prune`（与实测一致） |
| 8 | 暴露面 | `*:3118` 显示「全部网卡」并带提示色；`127.0.0.1:7890` 显示「仅本机」 |
| 9 | 零常驻 | 停在内存 tab 5 分钟，日志中端口扫描次数为 0 |
| 10 | 跨平台 | `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` 0 error；Windows 上端口号与 `netstat -ano` 一致 |
| 11 | 类型/构建 | `tsc --noEmit` 0 error；`vite build` 通过 |
| 12 | 性能 | 单轮 ≤120 ms；tab 可见时 30 s 轮常驻 ≤0.5%；payload ≤6 KB |

## 13. 与上层文档的关系

| 文档 | 需要改什么 |
|---|---|
| `README.md` | 「✅ 做」加「端口占用反查：谁占着 3000、暴露在哪个网卡、属于哪个项目」；CLI 段补 `prune port`；MCP 工具列表补 `port_listeners` |
| `docs/prune-feature-spec-v2.md` §2.1 | 边界表补一行，并写明「不做流量/协议/扫描」 |
| `docs/prune-tauri-technical-plan.md` §2 | 模块表加 `net/`：lsof 机器格式 + netstat 交叉校验 + 关注度常量表 |
| `docs/m5-cpu-plan.md` §4 | 加一句「CPU 是应用的连续属性；端口是离散多值标签，见 M7」——避免后续把两者混为一谈 |
| `docs/windows-support-plan.md` | 新增端口小节：`GetExtendedTcpTable`、无需提权、端口号为网络序 |
| `docs/m2-memory-plan.md` §4 | 数据模型处加一句「端口字段独立于 MemSnapshot，见 M7」 |

## 14. 提交拆分（沿用仓库 commit 规范）

| 阶段 | 提交 |
|---|---|
| P0 | `feat(net): 端口探测与关注度分层（lsof -F 解析 + netstat 交叉校验 + 双栈合并）` |
| P1 | `feat(main): 端口 tab（搜索、足迹折叠、暴露面、右键结束）` |
| P2 | `feat(ui): 应用行端口徽标与进程详情监听端口` |
| P3 | `feat(cli): prune port 子命令与 MCP port_listeners` + `docs: 端口占用方案与边界修订` |
