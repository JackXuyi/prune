# 端口服务类型识别调研（Web / MCP / 数据库…，双平台）

> **结论先行**：可行，且 **Windows 与 macOS 共用同一套方案主干**——L1 探针是纯 `std::net` TCP 握手，双平台零差异；L0 静态推断在 Windows 上换了两个证据源（exe 路径 + sysinfo `cmd()` 替代 cwd 项目嗅探）。分两层做：**L0 静态推断**零网络副作用，可默认开启，产出「疑似」级标签；**L1 主动探测**（HTTP / MCP / banner 握手）才能给出「确认」级标签，但它**打破 M7 §416「Prune 只读不碰网络」的纪律**，必须做成显式动作（行内按钮或设置开关）+ 结果缓存。MCP 的网络探测在协议层面完全可行（单次 `initialize` POST 即可判定），但**只能发现 HTTP 型 MCP**——本地大多数 MCP server 走 stdio，根本不占端口，这是产品文案必须写清的边界。

---

## 1. 现状盘点

当前端口链路（`crune-core/src/net/`）每条 PortEntry 只有这些身份信息：

| 已有字段 | 来源 | 对「服务类型」的线索量 |
|---|---|---|
| `port` | lsof / GetExtendedTcpTable | 高（WATCHED_PORTS 注释已按 前端/数据库/代理 分好类） |
| `comm` | lsof `c` 行 | 中（`mysqld`、`nginx` 一眼可判；`node`、`python3` 含糊） |
| `project` / `project_label` | cwd（lsof `-d cwd`） | 中（有项目的多半是 dev server，但不知道是什么栈）。**仅 macOS/Linux** |
| `note` | 内置（仅 AirPlay 5000/7000） | — |

**Windows 侧现状**（`win_ports.rs` + `mem/process.rs`）：

- 端口采集走 iphlpapi owner-pid 表，**零权限且天然带 pid**（比 macOS lsof 的静默跳过干净），但只产出 pid——comm 由上层进程表回填（`ports.rs:428`）。
- **cwd 不可得**（`host.rs` Windows 分支恒 None），项目归属整条链路缺失。
- 进程表里 `exe()` 已采集（`process.rs:149`）；`cmd()` 目前只为编辑器家族进程复制（`process.rs:153`，省字符串分配），**为监听 pid 放宽这条过滤即可拿到 Windows 的命令行**（sysinfo 在 Windows 上支持 `cmd()`，底层走 NtQueryInformationProcess）。

关键历史决策：**M7 计划第 416 行明确把「协议识别（这是 HTTP 还是 Redis）」排除在范围外**，理由是「探测要发包，Prune 只读不碰网络」。本次调研本质上是在重新评估这条边界——不是推翻它，而是把它细化成「默认零发包、用户显式授权才发包」。

## 2. 三层方案

### L0 静态推断（零网络 IO，可默认开）

纯查表 + 已有数据的组合推断，全部可单测：

| 证据源 | 规则示例 | 局限 |
|---|---|---|
| 端口表 | 3306→MySQL、5432→Postgres、6379→Redis、5173→Vite、9229→Node Inspector、50051→gRPC、27017→MongoDB | 非标端口漏报（dev server 换端口就认不出） |
| 进程名 | `mysqld`/`postgres`/`redis-server`/`mongod`/`nginx`/`caddy`/`docker-proxy`/`frpc` → 各自类型 | `node`/`python3`/`java` 万金油进程判不出 |
| argv / 命令行 | `node vite` → dev server、`python -m http.server` → 静态服务、`node xxx/mcp-server-...` → **疑似 MCP**。**双平台可得**：macOS 走 `sysctl KERN_PROCARGS2`；Windows 走 sysinfo `cmd()`（进程表现成，只需把监听 pid 纳入复制范围，见 §1） | fork 抹过 argv 的进程判不出；Windows 对提权/系统进程 `cmd()` 可能拿不到（返回空，降级 exe 路径） |
| exe 路径 | `...\MySQL\bin\mysqld.exe`、`C:\nginx\nginx.exe` 直接判类型；`node.exe` + cmd 参数组合判定 | macOS 已有（`process.rs:149`）；主要补 Windows 的进程名缺口 |
| 项目目录文件 | 已有 cwd → 查 `vite.config.*`、`next.config.*`、`Cargo.toml`、`pyproject.toml` → 栈图标 + 类型。**仅 macOS/Linux** | Windows 无 cwd，此证据源整条缺失（见下节 Windows 专项） |

L0 只能产出 `Suspicious(疑似)` 级别，UI 文案要写「推断」，不能撒谎。

### Windows 专项：三个差异与一个连带风险

| # | 差异 | 处理 |
|---|---|---|
| 1 | 无 cwd → 无项目归属、无项目文件嗅探 | 用 exe 路径 + `cmd()` 组合顶上：exe 是各语言运行时 → 看 cmd 首个脚本参数；exe 直接是服务程序（mysqld.exe 等）→ 按进程名表判 |
| 2 | `proc_names` 目前只传 `HashMap<pid, String>`（进程名） | 升级为 `struct ProcIdentity { comm, exe: Option<String>, cmd: Vec<String> }`，macOS/Linux 的 lsof 路径只需填 comm，Windows 从 MemState 进程表带出 exe/cmd（监听 pid 数量 8–20 个，复制成本可忽略） |
| 3 | 服务类程序常以 Windows 服务方式跑在 `svchost.exe` 下 | **不强求归属到具体服务**（`netstat -b`/SCM 查询需提权，违背零权限原则）；`comm == svchost.exe` 走现有 SYSTEM_COMMS 折叠规则即可 |
| ⚠️ | **连带风险**：`is_watched` 的第 4 条规则依赖 `has_project`（`mod.rs:299`）——Windows 上项目归属恒空，**非标端口的 loopback dev server 会被错误折叠**（例如 uvicorn 起在 8765） | 补一条平台无关的兜底信号：`comm ∈ {node, python*, java, deno, bun, dotnet...} && scope == Loopback && 端口 ≥ 1024` 也进关注层（宁可多看一条，折叠规则的本意是压 Electron/微信类 IPC 噪音，运行时解释器进程不属于该类） |

### L1 主动探测（准确，但发包）

**双平台完全同源**：全部是纯 `std::net::TcpStream` 对 `127.0.0.1:<port>` 的握手，不依赖任何平台 API，Windows/macOS 一套代码（比 L0 还省事——Windows 没有权限差额问题，探测覆盖率反而更完整）。对 TCP 监听端口做一次有界握手，每类协议的判据：

| 目标 | 探测方式 | 判据 | 副作用 |
|---|---|---|---|
| **Web（通用 HTTP）** | TCP connect → `HEAD / HTTP/1.1\r\nHost: localhost\r\n\r\n` | 响应以 `HTTP/` 开头；`Server`/`X-Powered-By` 头可细分（Vite/Express/Caddy/uvicorn…） | 极低：普通 HTTP server 对 HEAD / 只记一条 405/404 访问日志 |
| **MCP（Streamable HTTP）** | `POST` JSON-RPC `initialize`（`Accept: application/json, text/event-stream`）到 `/mcp` → 失败再试 `/`、`/api/mcp` | 响应体含 `protocolVersion` + `serverInfo`（可能以 JSON 或 SSE `data:` 两种形态返回，substring 匹配可同时覆盖） | 低：非 MCP server 返回 405/404；**有状态 server 每次 initialize 会建一个 session**，需低频探测或补发 DELETE |
| **MCP（发现捷径）** | `GET /.well-known/mcp/server-card.json`（SEP-1649/1960） | 200 + 合法 JSON server card，直接拿到 `serverInfo` 与 endpoint | 零（纯 GET 元数据），**应作为 MCP 探测第一跳** |
| **MCP（旧 SSE transport）** | `GET /sse` | `event: endpoint` 事件 → 旧版 MCP（2024-11-05） | 低 |
| **数据库 / SSH（banner 类）** | TCP connect 后被动读首包（不发业务命令） | MySQL greeting 串、Redis `+PONG`（需发 PING）、`SSH-2.0-` 前缀 | 极低：连接即有首包，无需注入 |

探测预算：本机 loopback TCP 握手 <1ms，一次 HTTP 往返 1–5ms；对关注层（实测 8–20 个端口）并发探测全程 <100ms。缓存键 `(pid, port, start_time)`——进程活着、端口没变则结果不变，**复用 `ProjectCache` 的缓存模式**（TTL + pid 消失即丢）。

### L2 mDNS/Bonjour（不推荐）

本机 dev server 几乎都不 advertise，覆盖率太低；M18 的 mDNS 设施留给局域网设备页即可。

## 3. 数据结构建议

```rust
/// 端口提供的服务类型。None = 推断不出（不是「不是服务」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    Web,        // 通用 HTTP（探针确认）
    DevServer,  // vite/next/webpack 等（argv/项目文件推断）
    Database,   // mysql/postgres/redis/mongo…
    Mcp,        // 网络型 MCP（探针确认，含 serverInfo 细分）
    Proxy,      // http/socks 代理
    Debug,      // node inspector / java jdwp
    Other,      // 认出是服务但归不了类
}

// PortEntry 增量字段（serde default，旧缓存兼容）：
//   service: Option<ServiceKind>          ← L0 推断
//   service_detail: Option<String>        ← L1 探针拿到的 serverInfo.name 等
//   service_confirmed: bool               ← L0=false / L1=true（「不撒谎」口径的载体）
```

UI 落点：`PortList.tsx` 每行 Badge，与 exposure 徽标并列；「推断 vs 探测确认」用两种视觉强度区分。

## 4. MCP 专项说明（用户重点关注）

1. **能探到的**：HTTP 型 MCP（Streamable HTTP 主流 / SSE 旧版），包括远程部署回环到本机的（cloudflared、supergateway 类桥接）。判定只需一个 `initialize` POST，无需 session、无需 tools/list。
2. **探不到的**：**stdio 型 MCP server**（绝大多数本地 MCP，如 prune 自带的 `prune mcp`）——它们由 agent 作为子进程拉起， stdin/stdout 通信，不占任何端口。端口页对它们天然不可见，这不是实现缺陷而是模型边界。
3. **误判风险**：低。判据是 JSON-RPC 协议级响应（`protocolVersion`），不是路径猜测；普通 web server 对 POST initialize 只会 404/405。
4. **附带发现**：`.well-known/mcp/server-card.json` 零副作用且信息最全，可作为探测链第一跳，命中即同时拿到类型 + serverInfo + endpoint。

## 5. 边界与纪律

| 项 | 处理 |
|---|---|
| 打破「只读不碰网络」 | L1 探测必须是**用户显式动作**：行内「探测」按钮（默认）或设置开关（对关注层自动跑）。绝不静默全网段扫描 |
| 探测范围 | 只对 `watched` 且 TCP 的端口；loopback / Interface / Any 均可（都是本机自己的服务） |
| 伪造响应/蜜罐 | 探针只读响应头与协议握手，不执行任何返回内容；结果只进展示层 |
| 会话泄漏 | 对 MCP server 反复 initialize 会堆积 session：缓存命中期间不重探（进程活着一轮就够），可在探后补发 DELETE |
| 失败语义 | 探测失败 = `service_confirmed=false`，保留 L0 推断值；不显示「未知服务」这种假确定 |
| Windows `cmd()` 拿不到 | 提权/系统进程的命令行读取可能返回空：降级用 exe 路径 → 再降级进程名表 → 都不行则 `service=None`（不猜） |

## 6. 建议落地顺序

- **P0（L0 静态推断）**：`proc_names` 升级为 `ProcIdentity`（comm + exe + cmd）→ 端口表升级 + 进程名/exe/cmd 三层推断 → macOS/Linux 补项目文件嗅探；Windows 同步落地 exe/cmd 路径 + `is_watched` 运行时兜底规则（修连带风险）；`ServiceKind` 上 `PortEntry`；UI Badge。零纪律冲突，独立可交付，双平台同 PR。
- **P1（L1 探针）**：纯 `std::net` 实现，一次写完双平台（Windows 优先跑真机验证——它没有 lsof 覆盖盲区，探针价值最大）。先做 Web 探针 + MCP 探针（`.well-known` → initialize → SSE 兜底），行内按钮触发 + `(pid, port, start_time)` 缓存；banner 类（DB/SSH）随后。
- **P2**：设置项「自动探测关注层端口」；`service_detail`（serverInfo）展示。
