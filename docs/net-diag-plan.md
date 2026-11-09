# 网络诊断方案（参考 whyfi.network）

> 状态：**已实施（2026-11-09，D0–D3）**；Windows 真机走查并入 W4 批次（本机 mingw 工具链缺失，完整 check-windows 门禁待补跑；Windows API 用法已通过 scratch crate 对 windows-gnu target 的 `cargo check` 验证）
> 前置：网络 tab（net-speed-tab-plan.md，A0–A4 已实施）；Windows 支持体系（windows-support-plan.md，W0–W3 ✅）；本方案只做**诊断**，不改既有速率 / 流量口径
> 参考：https://whyfi.network/ —— macOS 菜单栏 Wi-Fi 诊断工具，核心是「分层定位慢在哪」

---

## 0. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 补的是「**诊断**」：定位慢在哪一层，不是再看一遍流量 | WhyFi 的核心价值 = 分层定位（Wi-Fi → 路由器 → 互联网 → DNS）；Prune 网络 tab 目前只会「展示数字」，回答不了「为什么慢」 | 流量统计与诊断是两个问题，现有 tab 不覆盖后者 |
| 分层 = **DNS / 网关(LAN) / 出口(WAN)**，v1 不含 Wi-Fi 信号 | 信号读取在 macOS 15 上无免权限路径（见 §1.2 实测），CoreWLAN 引依赖值得单独一期（Windows 侧反有免提权 API） | v1 双平台交付三层 + 结论，信号层 v2 双平台一起补 |
| 探测方式用 **TCP connect 耗时**，不用 ICMP ping | `TcpStream::connect_timeout` 零依赖、零权限；ICMP 要 raw socket 或 spawn `ping` | 措辞写「连接耗时」，绝不冒充 ICMP ping（假事实红线） |
| 网关发现双平台分流：macOS `route -n get default`（实测可用）/ Windows `GetIpForwardTable2` | 本机 192.168.1.1，1 次调用拿到 gateway + interface；Windows 的 `IpHelper` feature 已在 `prune-core/Cargo.toml` 启用，**零新依赖** | 双平台同步支持；Windows 分支进 `win.rs`（沿用既有封装惯例） |
| 测速 + bufferbloat：macOS 用系统自带 **`networkQuality -c`**；**Windows 上隐藏测速卡** | 实测输出 JSON（吞吐 / RPM / 负载下延迟），零新依赖；Windows 无系统级等价物，自实现 HTTP 测速口径难对齐 Apple RPM | 沿用 windows-support-plan 拍板：无等价物 → 隐藏（不显示「不可用」占位）；自实现列为 D5 备选 |
| 诊断是**手动触发**，不常驻轮询 | WhyFi 常驻是因为它卖「持续监控」；Prune 的纪律是省电 + 常驻循环只做流量 | 每轮探测 ~2 s（5 个探测点 × 3 次），点一下出结论 |
| 诊断结果**不落库** | 诊断是快照，回答「现在」；落历史会引出「要不要对比昨天」的无底洞 | 与「峰值按会话」同理，先不背历史包袱 |
| **不做**一键切 DNS / Radar / 信道扫描 | 切 DNS 是写系统配置（违背「网络 tab 只读」的既有边界）；Radar/信道依赖 CoreWLAN 实时流 | 已有边界：Prune 的破坏性动作只在存储与进程明细 |
| captive portal 检测做（便宜且实用） | 探测 `captive.apple.com/hotspot-detect.txt`，一次 HTTP 判定酒店/机场网络 | WhyFi 同款，成本 ≈ 0 |
| 诊断报告**复制为 Markdown** | WhyFi 的「导出给 ChatGPT/Claude」思路好，Prune 已有 Markdown 习惯 | 剪贴板一行代码，无文件落盘 |

---

## 1. WhyFi 功能盘点 → Prune 差距分析

### 1.1 逐项对照

| WhyFi 能力 | 说明 | Prune 现状 | 采纳 | 理由 / 替代 |
|---|---|---|---|---|
| 分层诊断（信号/路由器/互联网/DNS） | 慢的时候指出是哪一层 | ❌ 无 | ✅ v1 核心 | 三层探测 + 判定规则 |
| 信号强度 / 噪声 / SNR / 发射速率 | Wi-Fi 层数据 | ❌ 无 | ⏩ v2 | macOS 15 无免权限 CLI 路径，需 CoreWLAN（§1.2）；Windows 有免提权 wlanapi，v2 双平台一起补 |
| 路由器 ping / 互联网 ping / jitter | LAN/WAN 延迟 | ❌ 无 | ✅ v1 | TCP connect 耗时替代 ICMP |
| DNS 解析耗时 | DNS 层 | ❌ 无 | ✅ v1 | `getaddrinfo` 计时（走系统解析器，含缓存路径，口径如实标注） |
| Speed test + bufferbloat | Cloudflare 测速 + 负载下延迟 | ❌ 无 | ✅ v1 | `networkQuality -c`（系统自带，实测 JSON 可用） |
| Radar（实时信号找最佳位置） | 4 次/秒 RSSI 流 | ❌ 无 | ❌ | 依赖 CoreWLAN 常驻流，与省电纪律冲突，且属 Wi-Fi 专域 |
| 信道扫描 / 邻近网络列表 | 拥塞分析 | ❌ 无 | ❌ | CoreWLAN scan；诊断价值有但非痛点，列入 v2 备选 |
| 一键切 DNS（8.8.8.8 / 1.1.1.1） | 改系统设置 | ❌ 无 | ❌ | 写系统配置 = 破坏性动作，违背网络 tab「只读 + 只导航」边界 |
| Captive portal 检测 | 酒店/机场登录页识别 | ❌ 无 | ✅ v1 | 一次 HTTP 探测 |
| 报告导出（给 AI） | 纯文本报告 | ❌ 无 | ✅ v1 | 复制 Markdown 到剪贴板 |
| 菜单栏表情图标（绿/橙/红） | 常驻健康度 | ❌ 无 | ❌ v1 | 常驻判定需要常驻探测，与手动触发冲突；若 v2 做信号层再议 |

### 1.2 信号层数据源实测（2026-11-09，macOS 15.7.7）

| 路径 | 实测结果 | 结论 |
|---|---|---|
| `airport -I` | 仅输出 deprecated 警告，无数据 | ❌ 已废弃，不可依赖 |
| `wdutil info` | 需要 sudo | ❌ 不接受提权 |
| `system_profiler SPAirPortsDataType` | 本机返回空（两次复测一致） | ❌ 不可靠 |
| `ipconfig getsummary en0` | 只有 SSID/BSSID，无 RSSI/噪声 | ❌ 缺关键字段 |
| CoreWLAN（objc2 + objc2-core-wlan） | 未试，需引 2 个 macOS-only 依赖 | ✅ v2 唯一正路，单独一期 |

**结论：v1 三层诊断不含信号层，UI 上信号位留「Wi-Fi 信号（v2）」占位说明，不出现编造数据。**

### 1.3 其余探测路径实测（同日同机）

| 探测 | 命令 | 实测 | 结论 |
|---|---|---|---|
| 网关发现 | `route -n get default` | gateway 192.168.1.1 / interface en0 | ✅ 可用 |
| 网关 TCP 探测 | `nc -z -G 2 192.168.1.1 80` | TCP 80 通，< 16 ms | ✅ 网关普遍监听 80/443 |
| captive portal | `curl captive.apple.com/hotspot-detect.txt` | 200 + `Success`，DNS 29 ms / connect 37 ms | ✅ 一次请求同时可测 DNS + WAN |
| 测速 | `networkQuality -c -d` | JSON：`base_rtt` / `il_h2_req_resp[]` 等 | ✅ 完整跑 ~30 s，字段以实施时实跑为准 |

**Windows 侧（推演，未实测）**：本机为 macOS，Windows 分支先过交叉编译（`scripts/check-windows.sh`），真机走查并入 W4 的 Windows 真机批次。Windows 逐项等价物见 §3.6。

---

## 2. 需求理解与边界

**用户问题**（每个 UI 决策都要能对上其中一条）：

1. 「网速明明显示有量，但网页就是慢」——是 DNS、路由器还是 ISP？分层探测 + 一句话结论
2. 「视频开会一卡一卡，是带宽不够还是延迟抖？」——测速卡给吞吐 + RPM（bufferbloat 官方口径）
3. 「酒店 Wi-Fi 连上了却弹不出登录页」——captive portal 检测直接告诉你
4. 「想让 AI 帮我分析，怎么把情况说清」——复制 Markdown 诊断报告

**边界**：

- ✅ 做：三层探测（DNS / LAN / WAN）+ 分层结论 + jitter + captive portal + 手动测速（networkQuality）+ Markdown 报告复制
- ✅ 做：**v1 双平台同步**（macOS + Windows），平台差异集中在网关发现 / SSID / 测速三处（§3.6）
- ✅ 做：判定阈值全部**透明展示**（结论旁注明判定依据与阈值，阈值是经验值不是真理）
- ❌ 不做：Wi-Fi 信号 / SNR / 信道扫描（v2，双平台：CoreWLAN + wlanapi）
- ❌ 不做：ICMP ping、raw socket、任何提权
- ❌ 不做：切 DNS、限速、断网、防火墙（破坏性，违背只读边界）
- ❌ 不做：诊断历史落库、对比昨天、告警推送
- ❌ 不做：菜单栏常驻健康度图标（依赖常驻探测，v1 纪律不破）

---

## 3. 口径定义（正确性核心）

### 3.1 探测项

| 层 | 探测 | 方法 | 次数 | 取值 |
|---|---|---|---|---|
| DNS | 解析 `www.apple.com` | `std::net::ToSocketAddrs`（getaddrinfo）计时 | 3 次，首结果**丢弃**（含冷启动偏差） | 后 2 次中位数 |
| LAN | 网关 `gateway:80` TCP connect | `TcpStream::connect_timeout` 计时 | 3 次 | 中位数；全失败 = 「连不上路由器」 |
| WAN | `1.1.1.1:443` TCP connect | 同上 | 5 次 | 中位数 + 平均绝对偏差作 jitter |
| Portal | `captive.apple.com/hotspot-detect.txt` | HTTP GET，读状态与 body | 1 次 | `Success` = 正常；重定向/其他 = 疑似 portal；网络错 = unknown |

- 全程**串行**，一轮总耗时预算 ≤ 5 s（超时各自 2 s）
- DNS 走系统解析器：**命中本地缓存时数值会很低**，脚注写明「走系统 DNS（含缓存），低值不代表无 DNS 问题」
- WAN 用 `1.1.1.1:443`：固定 IP 免 DNS 干扰；443 几乎不被路由器拦
- jitter 用平均绝对偏差（MAD）而不是标准差：探测次数少（5），MAD 更稳，措辞「抖动」
- portal 的 HTTP GET 用**裸 TcpStream 手写请求**（`GET /hotspot-detect.txt HTTP/1.1` + Host 头）：项目无 HTTP 客户端依赖，明文 HTTP 一个请求足够，不为此引 reqwest/ureq

### 3.2 状态机

| 状态 | 含义 | UI |
|---|---|---|
| `idle` | 没跑过 | 「点击开始诊断（约 5 s）」 |
| `running` | 探测中 | 逐层显示进度（DNS ✓ → 网关 ✓ → …），不显示部分结论 |
| `done` | 完成 | 三行读数 + 结论 + 报告复制按钮 |
| `failed` | 整轮失败（如网关都没拿到） | 「无法诊断：{note}」，不显示编造的 0 |

`failed` 与「探测到全断网」必须区分：后者是 `done` + 结论「连不上路由器」，前者是探测机制本身失败。

### 3.3 分层判定（结论规则）

先取基线，再看哪层显著异常（阈值展示在结论旁）：

| 条件（按优先级短路） | 结论 | 建议文案 |
|---|---|---|
| 网关 3 次全失败 | 「连不上路由器」 | 检查 Wi-Fi / 网线是否连接 |
| DNS 2 次全失败 | 「DNS 解析失败」 | 网关通但域名解析不了，多为 DNS 配置或运营商 DNS 故障 |
| portal = 疑似 | 「需要网页登录（酒店/机场 Wi-Fi）」 | 打开浏览器完成认证 |
| LAN 中位数 > 50 ms | 「路由器响应慢（{x} ms）」 | 重启路由器试试 |
| (WAN − LAN) > 100 ms | 「出口/运营商延迟高（差值 {x} ms）」 | 你的宽带问题，Prune 无能为力，找 ISP |
| WAN MAD > 30 ms | 「链路抖动大（±{x} ms）」 | 信号弱 / 拥挤信道 / 运营商波动，多为 Wi-Fi 环境问题 |
| DNS 中位数 > 300 ms | 「DNS 偏慢（{x} ms）」 | 可在系统设置换 DNS 试试 |
| 以上都不满足 | 「各层正常」 | 慢感可能来自对端服务本身 |

- 阈值（50 / 100 / 30 / 300 ms）为**经验值**，写死在代码常量并显示在 UI，不做动态基线
- 判定规则与阈值**双平台同一套**——分层结论的可信度不因平台打折
- 允许多条件同时成立：结论按上表取**第一条命中的主因**，其余异常在分层读数上标色（orange）
- 信号缺失（v1）在结论后注明：「未含 Wi-Fi 信号层（v2）」

### 3.4 测速 / bufferbloat

| 项 | 口径 |
|---|---|
| 触发 | 手动按钮「开始测速（约 30 s）」，测试中禁止重复点击 |
| 方法 | spawn `networkQuality -c`，解析 stdout JSON |
| 读数 | 下/上行吞吐、responsiveness（RPM，Apple 官方「负载下响应度」）、`base_rtt` |
| bufferbloat 表述 | RPM 分级（Apple 给 Poor/Low/Medium/Good），中文映射「很差/较差/尚可/良好」；不用自造公式 |
| 失败 | 进程超时 90 s 杀掉 → `failed` + note；解析失败同样 `failed`，不显示部分数字 |
| Windows | **整卡隐藏**（capability 为 false），不显示「不可用」占位；自实现测速见 §9 待拍板 #2 |
| 与 WhyFi 差异 | WhyFi 走 Cloudflare 自测；我们用系统自带，零依赖且是 macOS 官方口径——差异在脚注说明 |

### 3.5 报告导出

纯文本 Markdown，一键复制，含：时间、SSID（来自 `ipconfig getsummary`，读不到就省略）、三层读数、判定结论、测速读数（跑过才有）、口径脚注。**不含**进程列表、IP 明细之外的本机敏感信息（网关 IP 报告里保留，属必要事实）。

### 3.6 平台适配（v1 双平台同步的关键节）

**原则沿用 windows-support-plan 的拍板：无等价物的 macOS 特性在 Windows 上隐藏（不显示「不可用」占位）；等价物齐的探测项双平台同一套判定规则与阈值。**

| 探测项 | macOS | Windows | 双平台一致性 |
|---|---|---|---|
| 网关发现 | 解析 `route -n get default`（实测可用，取 `gateway` + `interface`） | `GetIpForwardTable2` 遍历找 IPv4 默认路由（`0.0.0.0/0`，protocol 不限），封装进 `win.rs::default_gateway()`；`IpHelper` + `WinSock` feature **已启用**，零新依赖 | 差异只在这一个函数；上层探测与判定完全同源 |
| DNS | `getaddrinfo` 计时 | 同左（Windows 命中 dnsapi 缓存同样有偏差，脚注同款） | ✅ 同源 |
| LAN TCP connect | `TcpStream::connect_timeout(gateway, 80)` | 同左 | ✅ 同源 |
| WAN TCP connect | `1.1.1.1:443` | 同左 | ✅ 同源 |
| captive portal | 裸 TcpStream HTTP GET | 同左 | ✅ 同源；**口径差异**：不读系统代理（Windows 企业网常见系统代理），浏览器走代理而探测不走，脚注写明「探测不经代理，结果可能与浏览器体验不一致」 |
| 测速 / bufferbloat | spawn `networkQuality -c` | **无系统等价物 → 测速卡隐藏**（W2 swap 隐藏先例）；自实现 Cloudflare HTTP 测速列为 D5 备选 | ❌ 平台差异，UI 层 `#[cfg]` 门控 + 前端 capability 事件 |
| SSID（报告用） | `ipconfig getsummary en0` | `netsh wlan show interfaces` 解析；**输出 locale 敏感**（中英文关键字不同），解析失败就省略该行，不下假结论 | 报告 SSID 行双平台均 optional |
| Wi-Fi 信号 | v2（CoreWLAN） | v2（`wlanapi` / Native Wifi API，`Win32_NetworkManagement_WiFi` feature；Windows 反而免提权，难度更低） | v2 一期双平台一起补 |

**实现分层**：

- `prune-core/src/net/diag.rs`：探测与判定**纯函数双平台同源**；平台差异只暴露两个口：`default_gateway() -> Option<IpAddr>` 与 `wifi_ssid() -> Option<String>`（macOS / Windows 各一份 `#[cfg]` 实现）
- Windows 网关单测标 `#[cfg(target_os = "windows")]`（CI / W4 真机跑）；解析类纯函数（如路由表筛选逻辑抽出）双平台可测
- 交叉编译门禁：`scripts/check-windows.sh` 进 D0/D1 验收，Windows 分支编译不过 = 不算完成

---

## 4. 数据契约

### 4.1 Rust（`crates/prune-core/src/net/diag.rs`，纯函数 + 探测器分离）

```rust
pub enum DiagStatus { Idle, Running, Done, Failed }

pub struct LayerProbe {          // 单层结果
    pub median_ms: Option<f64>,  // None = 探测失败（不是 0）
    pub samples: Vec<f64>,       // 原始样本，UI 可展开
    pub note: Option<String>,
}

pub struct NetDiag {
    pub at: i64,
    pub status: DiagStatus,
    pub note: Option<String>,       // failed 时必填
    pub dns: Option<LayerProbe>,
    pub lan: Option<LayerProbe>,    // 附 gateway: Option<String>
    pub wan: Option<LayerProbe>,    // 附 jitter_ms: Option<f64>
    pub captive: Option<CaptiveState>,  // none | suspected | unknown
    pub verdict: Option<Verdict>,   // label + reason + 阈值引用
    pub speed: Option<SpeedResult>, // 手动测速，独立于整轮诊断
}

pub struct SpeedResult {
    pub at: i64,
    pub down_bps: f64,
    pub up_bps: f64,
    pub rpm: f64,
    pub rpm_level: String,     // Apple 原文，前端映射中文
    pub base_rtt_ms: f64,
}
```

### 4.2 IPC

| 命令 / 事件 | 签名 | 说明 |
|---|---|---|
| `net_diag_run` | `() -> NetDiag` | 同步跑一轮（前端 loading 态）；重复调用返回 running 状态 |
| `net_diag_speed_run` | `() -> SpeedResult \| FailedNote` | 测速，独立触发，二者不互相阻塞；**仅 macOS 注册**（`#[cfg]`） |
| `net_diag_report` | `() -> String` | 生成 Markdown 报告（前端写剪贴板） |
| `net_diag_capability` | `() -> { speed_test: bool }` | 前端用 capability 渲染测速卡，**不硬编码 `navigator.platform`**，避免与后端注册面漂移 |

诊断命令是**按需短任务**，不进常驻循环；tokio `spawn_blocking` 包住串行探测。

### 4.3 阈值常量

```rust
pub const LAN_SLOW_MS: f64 = 50.0;
pub const WAN_LAN_GAP_MS: f64 = 100.0;
pub const JITTER_HIGH_MS: f64 = 30.0;
pub const DNS_SLOW_MS: f64 = 300.0;
```

---

## 5. 布局（网络 tab 底部追加「诊断」卡）

```
┌──────────────────────────────────────────────────────────┐
│ 诊断                                            [开始诊断] │
│  DNS        ▁ 28 ms          （正常，阈值 300 ms）        │
│  路由器     ▂ 3 ms · 192.168.1.1（正常，阈值 50 ms）      │
│  出口       ▇ 210 ms · 抖动 ±8 ms（偏高：差出口阈值 100 ms）│
│  → 主因：出口/运营商延迟高。找运营商，本地无法解决。        │
│  Wi-Fi 信号：v2 提供，本版未含                            │
│  [复制诊断报告]                                           │
├──────────────────────────────────────────────────────────┤
│ 测速 & 负载响应                                [开始测速]  │
│  ↓ 86 Mbps   ↑ 21 Mbps   RPM 320（尚可）                  │
│  口径：系统 networkQuality（Apple 服务器），约 30 s        │
└──────────────────────────────────────────────────────────┘
```

- 三行读数语义色：正常 `accent` / 偏高 `warn` / 失败红（沿用现有三色板，不新造色）
- 每行右侧**同时显示阈值**，结论不搞黑盒
- samples 可展开看原始 3/5 次探测值（省电版 Sparkline 或纯文本均可，实施时定）
- 测速与诊断是两个按钮、两次消耗网络的行为，不合并成一个「一键全套」（合并后 30 s 等待体验差）
- **Windows**：诊断卡照常渲染；测速卡不渲染（capability 门控）；报告里 SSID 行解析失败则省略

---

## 6. 分期

| 阶段 | 内容 | 人天 |
|---|---|---|
| D0 | `prune-core/src/net/diag.rs`：探测器 + 纯函数（中位数/MAD/判定）+ 单测（探测注入 trait mock；判定表全分支覆盖）+ **平台口**：`default_gateway()`（macOS route 解析 / Windows `GetIpForwardTable2` 进 `win.rs`）、`wifi_ssid()` 双实现 | 0.45 |
| D1 | Tauri 命令 4 个（含 capability）+ `networkQuality` spawn/解析（`#[cfg(target_os = "macos")]`，JSON 字段以实跑核对）+ 超时处理 + **`scripts/check-windows.sh` 过编译** | 0.25 |
| D2 | 前端类型 / `netDiagStore` / `NetBoard` 诊断卡 + 测速卡（capability 门控）+ 报告复制 | 0.4 |
| D3 | README / feature-spec 边界表（诊断 ✅，信号/切 DNS ❌）/ release note | 0.1 |
| **合计** | | **≈ 1.2** |
| D4（v2，独立一期） | 信号层双平台：macOS CoreWLAN（objc2）+ Windows `wlanapi`（免提权，难度更低）+ SNR + 信号结论并入判定 | 0.7~0.9 |
| D5（备选） | Windows 自实现 Cloudflare HTTP 测速（下载/上传吞吐 + 负载下延迟），对齐 WhyFi 口径 | 0.5+ |

无新依赖（双平台：`std::net` + 系统命令/系统 API；`IpHelper` feature 已启用）；Windows 真机走查并入 W4 批次。

---

## 7. 假事实规避清单

| 陷阱 | 规避 |
|---|---|
| TCP connect 冒充 ICMP ping | 全部措辞「连接耗时」，脚注说明与 ping 的差异 |
| 探测失败显示 0 ms | `median_ms = None` → 「探测失败」，`done` 才有数字 |
| DNS 命中缓存 → 误判「DNS 正常」 | 脚注写明走系统解析器含缓存；连续 2 次失败才判 DNS 故障 |
| 首次探测含冷启动偏差 | 首样本丢弃 |
| 整轮失败显示空结论 | `failed` + note，与「连不上路由器」严格区分 |
| 阈值黑盒 | 阈值常量 + UI 逐行展示 |
| networkQuality 字段猜错 | 实施时实跑核对 JSON 字段，解析加容错（缺字段 → failed + note） |
| 测速中重复点击 | running 态禁用按钮 |
| portal 误判 | 只在「body ≠ Success 且网络可达」时报疑似；unknown 不下结论 |
| 报告夹带隐私 | 报告内容白名单化（时间/SSID/三层/结论/测速），不含进程与流量明细 |
| Windows 测速卡出现假「不可用」 | capability = false 时整卡不渲染，不留占位（W2 先例） |
| Windows netsh 输出 locale 不同 → 解析出假 SSID | 解析失败即省略 SSID 行，绝不用猜的关键字兜底 |
| Windows 分支「编译过了就算完」 | 真机走查并入 W4 批次；本机只能过 `check-windows.sh` 编译门禁，验收清单标注哪些项必须真机 |

---

## 8. 验收清单

1. 点「开始诊断」≤ 5 s 出三层读数 + 一条主因结论；结论旁可见阈值
2. 拔网线/断 Wi-Fi 跑诊断：结论「连不上路由器」，不是整轮 failed
3. 飞行模式：整轮 failed + note，无编造数字
4. 连酒店式 portal 网络（或 mock）：报「需要网页登录」
5. 测速按钮：约 30 s 出吞吐 + RPM + 分级；测试中按钮禁用；kill networkQuality → failed 不崩
6. 复制报告：剪贴板 Markdown 含时间/三层/结论，粘贴给 AI 可独立读懂
7. 断网时 tab 其余区块（速率/今日）不受影响；诊断与测速互不阻塞
8. `cargo test --workspace` 全绿（判定表全分支 + 探测 mock）；`npx tsc --noEmit` 0 错误
9. 网络 tab 原 A0–A4 全部验收项不回归

**Windows（编译门禁 + W4 真机批次）**：

10. `scripts/check-windows.sh` 通过（D0/D1 各跑一次）；macOS 上 `cargo build` 不受 Windows cfg 影响
11. 真机：默认路由下诊断跑通，网关 IP 与 `route print` 一致；无 Wi-Fi（网线）时 SSID 省略
12. 真机：测速卡不渲染；诊断卡、报告复制、三层读数与 macOS 行为一致
13. 真机：系统代理开启时 portal 探测结论与浏览器行为可能不一致（脚注文案在报告中正确出现）

---

## 9. 待拍板

| # | 议题 | 建议 |
|---|---|---|
| 1 | v1 范围确认：三层 + portal + 测速 + 报告（信号 v2），双平台同步 | 按此执行 |
| 2 | Windows 测速：v1 隐藏 vs 直接立项 D5 自实现 Cloudflare 测速 | v1 先隐藏（等价物缺失沿既有纪律）；若 Windows 用户反馈强烈再立项 D5 |
| 3 | 探测目标域名 `www.apple.com`、WAN 目标 `1.1.1.1` | 可换，但换私有域名会引入自家缓存偏差 |
| 4 | 测速按钮放诊断卡内还是独立卡 | 独立卡（§5），避免一次等待 35 s |
| 5 | D4 信号层是否立项 | 建议 v1 上线观察后再定；Windows 侧成本低于 macOS（wlanapi 免提权） |

---

## 10. 实施记录（2026-11-09）

### 10.1 落地文件

| 阶段 | 文件 | 说明 |
|---|---|---|
| D0 | `crates/prune-core/src/net/diag.rs`（新） | 探测器 + 纯函数（median / MAD / 判定表 / portal 解析 / 双平台输出解析）+ `run_diag` 编排 + `report_markdown`；平台口 `default_gateway()` / `wifi_ssid()` 双实现 |
| D0 | `crates/prune-core/src/win.rs` | `default_gateway()`：`GetIpForwardTable2` 取 IPv4 默认路由（多路由取 Metric 最小；on-link 跳过）；`NextHop` 按标准 sockaddr_in ABI 读字节，不手解 union 字段路径 |
| D0 | `crates/prune-core/Cargo.toml` | 新增 `Win32_NetworkManagement_Ndis` feature（`MIB_IPFORWARD_ROW2` 内嵌 `NET_LUID_LH` 必需） |
| D1 | `src-tauri/src/net_diag.rs`（新） | 4 个命令 + `NetDiagState`（最近一轮 + 测速，报告导出用）+ `networkQuality` spawn/轮询/90 s 超时/JSON 容错解析（含单测） |
| D1 | `src-tauri/src/lib.rs` | 注册 4 个命令 + manage 状态 |
| D2 | `src/types/net.ts` / `src/lib/ipc.ts` / `src/stores/netDiagStore.ts`（新）/ `src/lib/devFixture.ts` | 类型镜像、IPC 封装、store（capability/run/speed/复制反馈）、浏览器 fixture |
| D2 | `src/components/NetBoard.tsx` | `DiagCard`（三层读数 + 阈值逐行 + details 原始样本 + 主因结论 + 报告复制）+ `SpeedCard`（capability 门控，Windows 不渲染） |
| D2 | `src/windows/MainWindow.tsx` | 网络 tab 门控里 init 诊断 store（只拉 capability，不起采样循环） |
| D3 | `README.md` / `prune-feature-spec-v2.md` | 边界表 ✅ 诊断 / ❌ 信号·切 DNS·信道扫描；路线图补 M17；相关文档加链接 |

### 10.2 与方案的三处偏差

1. **判定用纯函数全覆盖，未做探测器 trait mock**：判定表 8 分支 + median/MAD + portal/路由/netsh/ipconfig 解析都是纯函数单测（15 项），探测器只是 20 行以内的薄壳——trait 注入的收益覆盖不了它的样板成本。效果等价：`cargo test` 不碰网络。
2. **`net_diag_run` 返回 `Result`**：Tauri 2 要求含 `State` 的 async 命令必须返回 Result（E0277 编译期强制），方案 §4.2 的 `() -> NetDiag` 签名修正为 `() -> Result<NetDiag, String>`。
3. **`ipconfig getsummary` 的 SSID 格式与假设不符**：实测为 `  SSID : <value>`（冒号前有空格）且非 ASCII 时 hex 编码，首次解析实现被单测抓出；改为精确 key 匹配（`SSID` / `SSIDSTR`，忽略 `BSSID`/`_SSID`/`SSIDTYPE`）+ UTF-8 校验兜底的 hex 解码。

### 10.3 验收状态

| 项 | 状态 |
|---|---|
| `cargo test --workspace` | ✅ 270 passed / 1 ignored（core 210 含 diag 15 项；app 42 含 speed 解析；cli 18） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `npx vite build` | ✅ 74 modules，built in 5.64s |
| Windows API 编译验证 | ✅ scratch crate 对 `x86_64-pc-windows-gnu` target `cargo check` 通过（0.33 s）；完整 `check-windows.sh` 门禁 ⏳ 待装 mingw-w64 后补跑 |
| Windows 真机走查（验收 11–13） | ⬜ 并入 W4 批次 |
| `pnpm tauri dev` 真机走查（验收 1–9） | ⬜ 待做 |

### 10.4 UI 二级页化（追加于 2026-11-09）

- 网络 tab 顶部新增「监测 / 诊断」分段切换（`Segmented`，role=tablist），监测页 = 原 M14 全部内容；诊断页 = 分层探测 + 测速。
- 诊断读数图表化：`LayerChart`（填充条 = 中位数，竖刻度 = 判定阈值，圆点 = 原始样本，横轴 `max(阈值×1.6, 读数×1.08)`）；探测失败不画柱（0 柱会被读成「很快」）。
- 测速图表化：下/上传 `CompareBar` 对照条 + RPM 数字 + 分级 Badge（Poor→danger / Low→warn / Medium·Good→accent，只做文案色映射不发明阈值）。
- 探测目标确认：DNS 层 `www.apple.com:443`（diag.rs `DNS_PROBE_HOST`），UI 文案一致，本项无代码改动。
- 验收：tsc --noEmit 0 错误、vite build ✅。

### 10.5 出口层「探测失败」根因与修复（2026-11-09，真机排查）

**现象**：出口层（1.1.1.1:443）5 次样本全失败，UI 显示「探测失败」且 note 为空。

**根因（本机实测，杭州）**：
- `ping 1.1.1.1` 可达（77.9 ms）但 TCP 443 直连 8 秒仍静默超时（无 RST）——大陆网络对境外公共 DNS IP（1.1.1.1 / 8.8.8.8）的 443 直连 TCP 有干扰；「ping 通」具有迷惑性
- 对照：`www.apple.com` 解析到国内 CDN（61.164.114.x）直连通；国内固定 IP 223.5.5.5:443 直连 10 ms 通
- 本机系统代理（127.0.0.1:7890）开着：浏览器/curl 0.3 ms「连通」是连到了本地代理，诊断器按设计不走代理直连，行为正确

**代码问题**：`filter_map(.ok())` 吞掉失败原因 → note 为空，违背「失败要显示原因」。

**修复**：
1. WAN 层首选 `1.1.1.1:443`，5 次全失败自动回退 `223.5.5.5:443`（仍是固定 IP 免 DNS，判定规则与量纲不变），实际目标与原因写入 note（成功也写，目标事实不能静默换）
2. LAN/DNS 层失败原因同样带出（不再吞 `.ok()`）
3. 报告导出：成功行附 note；WAN 标签注明首选+回退目标
4. UI：LayerChart 有读数时也显示 note（warn 色）；出口层副文案注明回退机制

**真机验证**（scratch crate 跑 run_diag）：1.1.1.1:443 5 次超时 → 回退 223.5.5.5:443，中位数 11.1 ms，verdict「各层正常」，note 完整。cargo test 270 passed / tsc 0 错。

### 10.6 「复制诊断报告」失效根因与修复（2026-11-09）

**现象**：点击无任何反应，报告未进剪贴板，按钮也无失败反馈。

**根因（两条叠加）**：
1. Tauri 自定义协议（`tauri://localhost` / `http://tauri.localhost`）下 `navigator.clipboard` 不被授权，`writeText` 抛错
2. 兜底 `document.execCommand("copy")` 要求**同步用户手势**，而报告文本要先经一次 `net_diag_report` IPC 往返——`await` 之后手势已失效，兜底同样失败
3. `copied` 只在成功时置 true，失败路径无任何 UI 反馈 → 表现为「点了没反应」

**修复**：
- 剪贴板改由 Rust 原生写入：`net_diag_copy_report` 命令 → macOS `pbcopy` / Windows PowerShell `Set-Clipboard`（显式设 `[Console]::InputEncoding=UTF8`，否则中文乱码；`clip.exe` 按 OEM 页处理不可用）。沿用「系统命令优先、零新依赖」纪律
- 移除 `net_diag_report` 命令（文本只在内核消费一次，不再跨 IPC 传回前端）
- 新增 `copyFailed` 状态：失败按钮变红显示「✗ 复制失败」（1.5 s 后复位），成功「✓ 已复制」用 accent 色
- 浏览器开发态（无 Tauri）保留 Web 剪贴板兜底

**验证**：`pbcopy` 中文往返正常；cargo test 270 passed、tsc 0 错。
