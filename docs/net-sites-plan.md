# 站点连通性 / 下载速度检测方案（F15）

> 状态：**已实施（2026-11-15，S0–S3）**，随 v0.11.0 发布；实施偏差与验收状态见 §15
> 前置：网络 tab（net-speed-tab-plan.md，A0–A4 ✅）；网络诊断（net-diag-plan.md，D0–D3 ✅）；设置的 app 专属目录化（settings.rs，M13 ✅）
> 定位：诊断页回答「**网络哪一层慢**」；本页回答「**我要用的那几个网站，连得上吗、快不快**」。两个问题不重叠，不合并。

---

## 0. 结论先行

| 决策 | 结论 | 一句话理由 |
|---|---|---|
| 补的是「**站点维度**」，不是又一层网络诊断 | 诊断页测的是 DNS / 网关 / 出口（1.1.1.1、apple.com 这类固定目标）；它永远回答不了「GitHub 为什么打不开」——那需要**按站点**测，且每个站的失败形态不同 | 目标不同：一层链路 vs 一个具体服务 |
| 探测工具用**系统 `curl`**（`-w` 输出计时字段），不引 HTTP/TLS 依赖 | 协议栈（TLS 握手、HTTP/2、SNI）自己实现要引 rustls/native-tls；curl 在 macOS 10.15+ 与 Windows 10 1803+ 自带，`-w` 一次拿到 6 个时间点。沿用项目「系统命令优先、零新依赖」纪律（同 `networkQuality` / `route` / `netsh`） | 零依赖 + 与浏览器更接近的真实 TLS 栈 |
| 每站**一次请求**同时产出连通性与速率 | DNS / TCP / TLS / TTFB 是同一连接上的串行阶段，拆成两次请求只会让两半读数不同源 | 10 站一轮 ≈ 5–15 s（并发 4），流量上限 ≈ 20 MB |
| 读数拆成**五个阶段**（DNS → TCP → TLS → TTFB → 传输），不是「通/不通」两态 | 本机实测同一时刻就有 5 种形态：DNS 挂、TCP 不通、TLS 被掐、握手成功但首字节永不到、HTTP 403 但站点活着。红灯只能说明「不通」，说不出「通到哪一步断的」 | 这是本功能相对 ping / 单点测速的唯一增量价值 |
| 状态判定用 `verdict` + `stage` 两个字段，**不下「被墙」断言** | 我们能确定的事实只有「TCP 无响应」「TLS 无响应」，不能确定原因（运营商 / 对端 / 企业防火墙 / 用户自己禁了） | 假事实红线：措辞只写观测到的事实 |
| 速率口径 = `bytes / (total − ttfb)`，且**样本不足不给数** | curl 自带 `%{speed_download}` 是 `bytes / total`，把 DNS+握手+TTFB 都摊进分母；首页小文件会被严重低估（实测 apple 256 KB：`speed_download` 763 KB/s，真实传输阶段 1.22 MB/s）。样本 < 200 KB 或传输时长 < 300 ms 时只显示「样本不足」 | 小样本给数字 = 假精确 |
| 默认站表 **10 个**国外常用站，**可编辑**（增删改 + 恢复默认），存 `settings.json` | 站表是用户资产（有人只要 GitHub+OpenAI，有人要测自家海外 VPS）；硬编码等于每次都改代码 | 与「白名单」相反：这里测的是用户自己的目标，不需要白名单 |
| **直连**（显式 `--noproxy '*'` + `-q` 忽略 `.curlrc`），不读系统/环境代理 | 诊断页已有同款口径（不读系统代理）；且 curl 会读 `http_proxy` 环境变量，不显式关掉的话同一份配置在两台机器上量出两种结果 | 口径可复现 > 讨好开代理的用户；「经代理测」进 v2 备选 |
| 不跟随重定向（不加 `-L`） | 3xx 本身是读数（劫持 / http→https / 地区跳转）；跟随后用户看到的 TTFB 是跳转后的站，与配置的域名不是一回事 | 「你配的站」和「最终落地页」不混为一谈 |
| 结果**不落库**、不进常驻循环 | 与诊断同纪律：快照回答「现在」；进常驻就是每 30 s 打 10 个境外站，既费流量也不礼貌 | 手动触发 + 单站重测 |
| **不做**：不限速、不切代理、不做 traceroute/ping、不判「被墙」 | 破坏性动作与 raw socket 都在既有边界之外 | 边界不破 |

---

## 1. 现状与差距

| 能力 | 现在 | 站点维度要补什么 |
|---|---|---|
| 网络 tab「监测」 | 系统口径 ↓/↑ 速率、网卡、按应用、今日/7 天 | 与站点无关，不动 |
| 网络 tab「诊断」 | DNS / 网关 / 出口三层 + 主因结论 + `networkQuality` 测速 | 目标是固定公网地址，不能按站点测 |
| 端口 / LAN / 应用资源 | 与本功能无关 | 不动 |
| CLI / MCP | 只读工具集 | v1 不挂 MCP（见 §14 #6） |

**缺口一句话**：现在能说「出口 11 ms，各层正常」，但用户真正的问题是「GitHub 拉代码要 30 秒」。链路正常 ≠ 某个站能打开。

---

## 2. 实测证据（2026-11-13 18:2x，本机 macOS 15.7.7，`curl 8.7.1 (SecureTransport) LibreSSL/3.3.6`）

单次 `curl -q -sS -o /dev/null --noproxy '*' --connect-timeout 4 --max-time 8 -w ...`，单位为秒（下文换算成 ms）：

| 站点 | DNS | TCP | TLS | TTFB | 总耗时 | bytes | HTTP | 形态 |
|---|---|---|---|---|---|---|---|---|
| apple.com | 14.6 | 29.9 | 78.8 | 125.8 | 335 | 256 049 | 200 | 正常（解析到国内 CDN `115.231.32.12`） |
| cloudflare.com | 24.6 | 384.8 | 548.0 | 713.9 | 2 844 | 1 334 522 | 200 | 正常但握手偏慢 |
| amazon.com | 24.7 | 192.6 | 603.0 | 1 100 | 5 831 | 1 985 432 | 200 | 正常，首页 2 MB |
| microsoft.com | 25.5 | 313.6 | 890.3 | 1 031 | 1 643 | 201 253 | 200 | 正常，握手 890 ms |
| openai.com | 18.0 | 86.2 | 160.7 | 249.5 | 252 | 9 626 | **403** | 站点活着、拒绝非浏览器 UA |
| netflix.com | 33.3 | 188.1 | 389.2 | 1 166 | 1 166 | 13 | **403** | 同上 |
| github.com | 13.8 | 122.2 | 256.9 | **0** | 10 002（超时） | 0 | 000 | TLS 握手成功，首字节永不到 |
| x.com | 14.5 | 161.0 | **0** | 0 | 4 002（超时） | 0 | 000 | TCP 通，TLS 握手卡死 |
| google.com | 15.7 | **0** | 0 | 0 | 4 002（超时） | 0 | 000 | DNS 正常，TCP 不通 |
| youtube.com | 14.9 | **0** | 0 | 0 | 4 005（超时） | 0 | 000 | 同上 |
| wikipedia.org | 34.7 | **0** | 0 | 0 | 5 002（超时） | 0 | 000 | 同上 |
| reddit.com | 25.2 | **0** | 0 | 0 | 4 001（超时） | 0 | 000 | 同上 |

**这十行就是需求的全部理由**：其中 5 个站是「不通」，但断点各不相同（DNS 正常但 TCP 不通 / TCP 通但 TLS 不通 / TLS 通但没数据 / HTTP 403 但站点活着）。这四种形态在 UI 上必须可分辨——这正是本功能相对「一个红灯」的增量。

两条次级实测：

| 测项 | 结果 | 对设计的约束 |
|---|---|---|
| `speed.cloudflare.com/__down?bytes=5000000` | total 6.70 s、ttfb 0.60 s、5 000 000 B → 传输阶段 ≈ 820 KB/s；同期 `%{speed_download}` = 746 KB/s | 速率必须扣掉 TTFB，否则小文件被系统性低估 |
| `--range 0-1048575` | apple 返回 `206`（尊重 Range，实收 256 049 B）；cloudflare `__down` 返回 `200` + 全量 5 MB（不尊重 Range） | Range 只能当「尽量少下」，真正的兜底是 `--max-time`；实际流量可能超上限，须在脚注写明 |
| `--limit-rate 100k --max-time 2`（逼出「传输中途被超时打断」） | 退出码 **28**，但 `-w` 仍输出 `bytes=206 646`、`ttfb=0.570`、`total=2.001` → 速率 ≈ 144 KB/s | 超时退出 ≠ 无读数：**有 bytes 就按数据判「慢」**，只有 0 字节才判「不通」（§4.5 退出码表） |

---

## 3. 需求理解与边界

**用户问题**（每条 UI 决策都要能对上一条）：

1. 「公司网络能上内网，GitHub / OpenAI 到底通不通？」——一站一行，说清断在哪一步
2. 「换了 Wi-Fi / 换了机场，那些站是不是快了？」——同一份站表，重测即可对比
3. 「这个站到底慢在握手还是慢在下行？」——TLS 与 TTFB 分列，不是笼统一个「慢」
4. 「我要给运维 / AI 描述问题」——复制 Markdown 结果

**边界**：

- ✅ 做：10 个默认站 + 可编辑站表（增删改、恢复默认）；每站五阶段读数；状态 + 汇总；单站重测；取消；Markdown 复制；capability（curl 缺失时整页不可用）
- ✅ 做：双平台同源（macOS / Windows 都是 `curl`，差异只在空设备路径 `-o /dev/null` vs `-o NUL`）
- ❌ 不做：限速、切代理、改系统配置、断开/重连
- ❌ 不做：ICMP ping / traceroute / raw socket / 任何提权
- ❌ 不做：对「为什么不通」下结论（不写「被墙」「运营商封锁」，只写「TCP 无响应」）
- ❌ 不做：站点历史 / 趋势 / 告警（本轮快照，与诊断同纪律）
- ❌ 不做：浏览器级体验模拟（不执行 JS、不加载子资源，只测首页文档）
- ❌ 不做：常驻轮询（每 30 s 打 10 个境外站既费流量也不礼貌）

---

## 4. 口径定义（正确性核心）

### 4.1 五个阶段（同一次请求内）

```
DNS ──time_namelookup──▶ TCP ──time_connect──▶ TLS ──time_appconnect──▶ 首字节 ──time_starttransfer──▶ 传完 ──time_total──▶
```

| 字段 | curl 变量 | 含义 | 缺省 |
|---|---|---|---|
| `dnsMs` | `time_namelookup` | 域名解析耗时（含 A+AAAA，系统解析器含缓存） | `null` = 解析失败 |
| `tcpMs` | `time_connect` | TCP 三次握手完成时刻（**累计**，从启动算） | `null`/0 = 连不上 |
| `tlsMs` | `time_appconnect` | TLS 握手完成时刻（**累计**） | 0 = 握手未完成 |
| `ttfbMs` | `time_starttransfer` | 首字节到达（**累计**） | 0 = 服务器无响应 |
| `totalMs` | `time_total` | 整次请求结束 | — |

> **累计值 vs 增量**：curl 给的是「从进程启动算起」的累计时刻。UI 展示**增量**更可读（TLS 握手 79 ms 而不是 78.8 ms 累计），因此落库字段保留累计原值，前端派生增量（`tls − tcp`、`ttfb − tls`）。派生只在两值都存在时进行，缺一即不显示——绝不用 0 兜底。

### 4.2 状态判定（`verdict` + `stage`，纯函数）

| 条件（按优先级短路） | `verdict` | `stage` | UI 文案方向 |
|---|---|---|---|
| curl 退出码 6（无法解析）或 `dnsMs=null` | `unreachable` | `dns` | 「域名解析失败」 |
| `tcpMs` 缺失 / 0 | `unreachable` | `tcp` | 「无法建立连接」 |
| `tlsMs` 缺失 / 0 且 URL 是 https | `unreachable` | `tls` | 「TLS 握手无响应」 |
| `ttfbMs` 缺失 / 0 | `stalled` | `ttfb` | 「连上了，但迟迟没有响应」 |
| `httpCode ≥ 400` | `refused` | `http` | 「站点返回 403 / 429（可能拒绝非浏览器请求）」 |
| TCP 累计 > 300 ms | `slow` | `tcp` | 「握手慢」 |
| TLS 增量 > 400 ms | `slow` | `tls` | 「握手慢」 |
| 首字节累计 > 1 s | `slow` | `ttfb` | 「首字节慢」 |
| 传输 < 100 KB/s（样本够） | `slow` | `transfer` | 「传输慢」 |
| 退出码 28 且已有字节（8 s 没下完） | `slow` | `transfer` | 「慢，且本轮被截断」 |
| 其余 | `ok` | — | 正常 |

- **`refused` 不是失败**：403/429 证明 DNS/TCP/TLS/HTTP 全链路通了，只是站点拒绝这次请求。这是有效读数，不能涂成红灯。
- **`stalled` 独立于 `unreachable`**：github.com 实测就是这一态（握手全过、首字节 0）。把它归进「不通」就丢了最有价值的信息。
- `stage` 只用于文案与着色，不参与汇总计数。

### 4.3 速率口径

```
transferBps = bytes / (total − ttfb)     // 秒
```

- **样本门槛**：`bytes ≥ 200_000` **且** `(total − ttfb) ≥ 0.3 s` 才给速率数字；否则 `transferBps = null`，UI 显示「样本不足（{bytes} / {传输时长} ms）」
  - 依据：apple 实测 256 KB / 209 ms → 1.22 MB/s，这是 TCP 慢启动区间，不是可持续吞吐；报出来就是假精确
- **`bytes = 0`**（403 / 重定向 / 空体）：不给速率，不显示 `0 B/s`
- **速率只在同站前后对比时有意义**：不同站的首页体积与 CDN 不同，跨站比速率是比内容不是比网络——脚注写明，UI 不做跨站排名

### 4.4 阈值常量（写死 + UI 逐行展示，与诊断同纪律）

```rust
pub const SITE_TCP_SLOW_MS: f64 = 300.0;      // TCP 握手累计 > 300 ms
pub const SITE_TLS_SLOW_MS: f64 = 400.0;      // TLS 增量 > 400 ms
pub const SITE_TTFB_SLOW_MS: f64 = 1_000.0;   // 首字节累计 > 1 s
pub const SITE_SLOW_BPS: f64 = 100_000.0;     // 传输阶段 < 100 KB/s
pub const SITE_MIN_SPEED_BYTES: u64 = 200_000;
pub const SITE_MIN_SPEED_MS: f64 = 300.0;
pub const SITE_MAX_BYTES: u64 = 2 * 1024 * 1024; // Range 上限，尽量少下
pub const SITE_MAX_TIME: Duration = Duration::from_secs(8);
pub const SITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
pub const SITE_CONCURRENCY: usize = 4;
pub const SITE_MAX_COUNT: usize = 20;
```

阈值是经验值，不宣称真理；UI 在页脚一行展示（`速率低于 100 KB/s 判慢 · 首字节超 1 s 判慢 · …`）。

### 4.5 curl 调用（确定性优先）

```
curl -q -sS -o <null-device>
     --noproxy *            # 不读 http_proxy/https_proxy，口径可复现
     --connect-timeout 4 --max-time 8
     --range 0-2097151      # 尽量只下 2 MiB；不支持的站靠 --max-time 兜底
     -A PruneSiteProbe/<version>
     -H "Accept: */*"
     -w "%{time_namelookup}\t%{time_connect}\t%{time_appconnect}\t%{time_starttransfer}\t%{time_total}\t%{size_download}\t%{http_code}\t%{remote_ip}\n"
     <url>
```

- `-q` **必须是第一个参数**（否则用户的 `~/.curlrc` 会改掉一切，包括 `-L`、代理、UA）
- `-o /dev/null`（macOS/Linux）/ `-o NUL`（Windows，`cfg!` 分支）——不落地临时文件
- **不加 `-L`**（§0 决策）、**不加 `--compressed`**（body 字节数才可比；gzip 变体会让不同站点的体积口径漂移）
- **不发 HEAD**：HEAD 只测到 TTFB，没有速率；且不少 CDN 对 HEAD 行为不一致（缓存未命中/命中差异比 GET 更大）
- 退出码映射（只用于 `note` 与 `stage`，不直接当 verdict）：
  | 码 | 含义 | 处理 |
  |---|---|---|
  | 6 | 无法解析主机 | `unreachable/dns` |
  | 7 | 连接被拒 | `unreachable/tcp` |
  | 28 | 超时（可能带部分数据） | 有 bytes → 按数据判 `slow`；无 bytes → 按阶段判 `unreachable`/`stalled` |
  | 35 | TLS 握手失败 | `unreachable/tls` |
  | 60/51 | 证书问题 | `unreachable/tls` + note（证书错不影响「是否连得上」的判断，但要说出来） |
  | 56 | 接收数据时连接被重置 | 有 bytes → `slow`；无 bytes → `stalled/ttfb` |

### 4.6 并发、预算与取消

- 4 个 worker 线程从索引队列取站点（`AtomicUsize`），结果写回 `Mutex<Vec<Option<SiteProbe>>>` **保持配置顺序**
- 单站最坏 8 s（`--max-time`）→ 10 站最坏 ≈ 24 s，典型 3–8 s
- 每完成一站 `emit("net-sites://progress", probe)`；前端逐行点亮，不必等整轮
- **取消**：置 `AtomicBool`，worker 在**取下一个站之前**检查——在飞的请求最多再跑 8 s 自然结束。不做杀子进程（要共享 `Child` 句柄 + 平台差异，收益不抵复杂度），UI 文案写「已取消，正在结束当前站点…」
- 同一时刻只允许一轮（`Mutex<()>` try_lock 失败即返回 `busy`），避免连点把自己带宽打满

---

## 5. 默认站表（10 个）

| # | 名称 | URL | 选它的理由 |
|---|---|---|---|
| 1 | Google | `https://www.google.com/` | 搜索基准，几乎所有人的第一参照 |
| 2 | YouTube | `https://www.youtube.com/` | 视频类，首页文档较大 |
| 3 | GitHub | `https://github.com/` | 开发者刚需，实测是最有价值的「stalled」样本 |
| 4 | Wikipedia | `https://www.wikipedia.org/` | 纯文本、无登录、体积小，最干净的连通性基准 |
| 5 | X | `https://x.com/` | 社媒代表；实测为「TLS 卡死」样本 |
| 6 | Cloudflare | `https://www.cloudflare.com/` | CDN 侧代表，1.3 MB 首页，速率样本充足 |
| 7 | Apple | `https://www.apple.com/` | 对照组：国内有 CDN，通常最快 |
| 8 | Microsoft | `https://www.microsoft.com/` | 企业网络常见白名单站点 |
| 9 | Amazon | `https://www.amazon.com/` | 电商代表，首页 ≈ 2 MB，速率样本最足 |
| 10 | OpenAI | `https://openai.com/` | AI 服务代表；实测 403 但站点活着，正好演示「可达 ≠ 可用」 |

**关于「大陆直连下多数为红」**：这是读数不是缺陷。默认站表的价值在于「一眼看到自己缺什么」；需要全绿的场景是「经代理」，属 v2 备选（§14 #3）。文档与页面脚注都要写清这一点，避免首启被当成 bug。

备选站（文档注明可换）：Reddit、Netflix、Telegram（`https://telegram.org/`）、Stack Overflow、Hugging Face。

---

## 6. 数据契约

### 6.1 Rust（`crates/prune-core/src/net/sites.rs`，新）

```rust
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,        // 稳定短 id（uuid 的 8 位前缀 / 默认站的固定 slug），重测与进度事件按它定位
    pub name: String,      // ≤ 32 字符
    pub url: String,       // http/https，≤ 512 字符
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SiteVerdict { Ok, Slow, Stalled, Refused, Unreachable, Error }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FailStage { Dns, Tcp, Tls, Ttfb, Transfer, Http }  // Transfer 是实施时补的（§15.2）

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SiteProbe {
    pub id: String,
    /// 探测当时的名称 / URL 快照（§15.2）：站表随后可能被改，报告要记录实际打的地址
    pub name: String,
    pub url: String,
    pub at: i64,
    /// 累计时刻（ms）。None = 该阶段未到达，**不是 0 ms**
    pub dns_ms: Option<f64>,
    pub tcp_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub bytes: u64,
    pub http_code: Option<u16>,
    pub remote_ip: Option<String>,
    /// bytes / (total − ttfb)；样本不足时为 None
    pub transfer_bps: Option<f64>,
    pub verdict: SiteVerdict,
    pub stage: Option<FailStage>,
    /// 失败原因（退出码解释 / curl stderr 摘要），成功时通常 None
    pub note: Option<String>,
    pub curl_exit: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SiteRun {
    pub at: i64,
    pub results: Vec<SiteProbe>,
    pub cancelled: bool,
    pub elapsed_ms: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SiteList {
    pub sites: Vec<Site>,
    /// 站表与内置默认是否一致（前端决定是否显示「恢复默认」）
    pub customized: bool,
    /// true = 存下的站表读不出来，已回落默认（§15.2）
    pub recovered: bool,
}
```

纯函数（全部可单测，**测试不碰网络**）：

| 函数 | 职责 |
|---|---|
| `default_sites() -> Vec<Site>` | §5 的 10 条 |
| `validate_sites(&[Site]) -> Result<Vec<Site>, Vec<SiteError>>` | 归一化 + 校验（见 §8） |
| `build_curl_args(site, version) -> Vec<String>` | 参数拼装（单测断言「必须含 `-q` 且在下标 0」「不含 `-L`」「含 `--noproxy *`」） |
| `parse_curl_output(line, exit_code) -> RawTiming` | `-w` 输出解析（容错：少于 8 段 → Err，不猜） |
| `judge(raw, url) -> SiteProbe` | §4.2 判定表 + §4.3 速率门槛，全分支覆盖 |
| `summarize(&[SiteProbe]) -> SiteSummary` | 各状态计数（UI 汇总条） |
| `report_markdown(&SiteRun, locale) -> String` | 复制用报告（渲染进 `i18n/report.rs`，与诊断报告同文件分函数） |

### 6.2 TS（`src/types/net.ts` 追加）

一对一镜像上面 4 个结构 + `SiteSummary`；`verdict` / `stage` 用字符串联合类型。`SiteCapability { curl: boolean; version: string | null }`。

---

## 7. IPC（`src-tauri/src/net_sites.rs`，新）

| 命令 / 事件 | 签名 | 说明 |
|---|---|---|
| `net_sites_list` | `() -> SiteList` | 读 settings（缺失/损坏 → 默认站表） |
| `net_sites_save` | `(sites: Vec<Site>) -> Result<SiteList, String>` | 校验 + 归一化 + 落 settings；校验失败返回可读原因，不写半套配置 |
| `net_sites_reset` | `() -> SiteList` | 删 key，回落默认 |
| `net_sites_capability` | `() -> SiteCapability` | `curl` 是否存在 + 版本（`curl --version` 首行）。**不硬编码平台**，同 `net_diag_capability` 的理由 |
| `net_sites_run` | `() -> Result<SiteRun, String>` | 一轮全测（并发 4）；`busy` 时立即返回而非排队 |
| `net_sites_probe_one` | `(id: String) -> Result<SiteProbe, String>` | 单站重测（从当前站表取 URL） |
| `net_sites_cancel` | `() -> ()` | 置取消标志 |
| `net_sites_copy_report` | `() -> Result<usize, String>` | 复用 `net_diag::write_clipboard`（已是 `pub(crate)`），macOS `pbcopy` / Windows `Set-Clipboard` |
| 事件 `net-sites://progress` | `SiteProbe` | 每完成一站发一次 |
| 事件 `net-sites://done` | `SiteRun` | 整轮结束（含取消态），前端收尾 |

- `net_sites_run` 走 `async fn + spawn_blocking`（线程池 + 子进程，硬约束：不能占主线程，M30 教训）
- 语言在命令入口读一次（`crate::i18n::current(&app)`），note 与报告同一语言，中途切语言不打断本轮

---

## 8. 持久化与校验

- 键：`netSites`（`settings.json`，`Vec<Site>` JSON 数组）；**不另建文件**，复用 `settings::{get_str/read_root, set}`
- 校验（`validate_sites`，纯函数）：
  - `url`：scheme ∈ {http, https}；必须有 host；长度 ≤ 512；**拒绝 userinfo**（`user:pass@` 不落盘）；拒绝 `<`/`>`/空白/控制字符（防参数注入到 curl argv）
  - `name`：trim 后非空，≤ 32 字符（按字符不按字节）；空名回落 host
  - `id`：缺失/重复 → 重新生成；保存时保持用户顺序
  - 数量：1–20（`SITE_MAX_COUNT`）；0 条 = 合法配置（空态页），不是错误
  - 重复 URL：按键去重（同 URL 保留第一条），返回被丢弃条数供前端提示
- 损坏数据（手改坏了 settings）：解析失败 → 用默认站表并在页面顶部提示「站表读取失败，已回落默认」，**不静默**、不覆盖用户文件

---

## 9. UI（网络 tab 第三个二级页「站点」）

> 实施时改为「卡片网格 + 详情弹窗」（2026-11-15，用户要求）：一屏十站靠卡片看总量，
> 五段读数靠弹窗看细节。行式布局下 10 行 × 5 个读数会变成一张读不完的表。

```
卡片网格（auto-fill 238px，960 窗口 3 列）
┌──────────────────────────────┐   点卡片 ↓
│ ● apple.com          可达 ›  │
│ TCP 30ms  TLS 49ms  TTFB 47ms│
│ ▁▁▁▁  样本不足（250 KB / 209 ms）│
└──────────────────────────────┘

详情弹窗（420 px）
┌────────────────────────────────────────────┐
│ ● apple.com  可达                       ✕  │
│ https://www.apple.com/                     │
├────────────────────────────────────────────┤
│ 耗时分布                                   │
│ ████░░░░▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒        │
│ ■ 握手(DNS/TCP/TLS) ■ 等首字节 ■ 收数据    │
│ 阶段      本段      累计                   │
│ DNS       18 ms     18 ms                  │
│ TCP       12 ms     30 ms                  │
│ TLS       49 ms     79 ms                  │
│ 首字节    47 ms     126 ms                 │
│ 传输      209 ms    335 ms                 │
│ 传输速率 样本不足（250 KB / 209 ms）       │
│ HTTP 206  取样 250 KB                      │
│ 出口 115.231.32.12   curl 退出码 0         │
├────────────────────────────────────────────┤
│ 检测于 21:35                [重测] [关闭]  │
└────────────────────────────────────────────┘
```

- **卡片只报状态，不报细节**（2026-11-15 用户要求，实施后修订）：
  - 有读数的站：状态点 + 名称 + 三段增量读数（TCP 累计、TLS 与 TTFB 取本段）+ 速率条（以本轮最快站为满格）+ 速率数字，`slow`/`refused` 另加状态 chip（`偏慢：TLS` / `403 · 站点拒绝`）；
  - 没读数的站只放一句状态：`不可达：TCP` / `不可达：TLS` / `无响应` / `出错`，按 `verdict` 着色。**断点秒数、HTTP 码、curl 原文一律不进卡片**——实测把 `探测本身出错：write-out has 7 fields…` 塞进 238 px 的卡里会溢出到隔壁卡片，十张并排时屏幕上全是 curl 原文，反而看不出「哪几站有问题」。短结论仍挂在 `title` 上，鼠标悬停可看一句；
  - 整卡是一个 `button`，Tab 可达、Enter 可开。
- **详情弹窗**（`role="dialog"` + `useDialogA11y` 焦点陷阱 + Escape 关闭）：
  - **耗时瀑布**：按累计时刻把整轮耗时切成 DNS / TCP / TLS / 首字节 / 传输五段，按语义分三类着色（握手 / 等首字节 / 收数据），断点那一段提亮成 warn/danger；
  - **没等到读数的那一截画成「无响应等待」段**——只画已知段会让 4 s 超时的站看起来只花了 16 ms；
  - **本段 / 累计两列都给**：阈值口径不同（TCP 判累计 300 ms，TLS 与首字节判本段 400 ms / 1 s），只给一列必然要额外解释；
  - 传输速率（含样本量与时长）、HTTP 码、取样体积、出口 IP、curl 退出码、curl 原文；被 8 s 上限截断时补一句「速率按已收字节算」；
  - **0 字节时不出速率行**（`样本不足（0 B / — ms）` 是废话），**一个阶段读数都没有（`error`）时不出瀑布与读数表**——那会是一排「—」，此时唯一有效信息就是原文，只留结论行
- **失败/拒绝**：卡片上只放一句 `stage` 文案（`连上了，但迟迟没有响应（10.0 s 无首字节）`），完整原因在弹窗顶部
- **重测**：只在弹窗里（卡片保持「打开详情」单一语义）；整轮检测时弹窗内容跟随进度实时更新
- **编辑态**（点「编辑」整页切换）：每行两个 input（名称 / URL）+ 删除；底部「添加站点」+「恢复默认」+「保存」/「取消」。校验失败在行内红字（不弹窗）。进入编辑态先关掉弹窗（站可能被删）
- **状态色**：`ok` = accent，`slow`/`refused` = warn，`stalled`/`unreachable` = danger，`error` = neutral。不新造色（复用 `charts/tone.ts` 语义）
- **curl 缺失**（capability.curl = false）：整页替换为一段说明，**不渲染带 0 的空表**（W2 先例：无等价物就隐藏，不做假占位）
- **运行中**：按钮变「取消」+ 顶部「检测中 3/10」；卡片状态点 pulse，未检测的站显示「待测」，不显示 `--`
- 副标（顶栏）：`按站点测：DNS → TCP → TLS → 首字节 → 传输 · 手动触发，不常驻`

---

## 10. i18n 与文档门禁

- 前端文案全部进 `src/i18n/messages/namespaces/net.ts` 的 `sites.*`（zh + en 各一份，`check-i18n.mjs` 强制对齐）；`src/**` 除注释与 `devFixture.ts` 外不得出现中文
- 新增 key 组（约 40 条）：`sites.title/subtitle/run/cancel/edit/save/reset/add/remove/summary.*/state.*/stage.*/row.*/empty/unavailable/thresholds/copy/copied/copyFailed/validate.*`
- Rust 侧用户可见错误进 `prune-core/src/i18n/err.rs`（结构体字段，编译期防拼错）：`sites_curl_missing`、`sites_curl_spawn`、`sites_arg_invalid`、`sites_busy`、`sites_save_failed`、`sites_too_many`、`sites_report_none`、`sites_clip_denied`（后者复用现有 `clip_*` 亦可）
- 文档：`README.md` 边界表加一行（✅ 站点连通性与速率 / ❌ 判「被墙」· 经代理测 · 站点历史）；`prune-feature-spec-v2.md` 同步；`CHANGELOG.md` 记 0.11.0
- `scripts/check-docs-honesty.mjs`：排他表述 sweep 会扫描 CHANGELOG，**不要写「唯一/独家」句式**（本功能无排他主张，正常叙述即可）

---

## 11. 分期

| 阶段 | 内容 | 人天 |
|---|---|---|
| S0 | `prune-core/src/net/sites.rs`：结构 + 默认站表 + `validate_sites` + `build_curl_args` + `parse_curl_output` + `judge` + `summarize` + `report_markdown`；单测覆盖判定表全分支、阈值边界、解析容错、参数纪律（`-q` 首位 / 无 `-L` / 有 `--noproxy`） | 0.4 |
| S1 | `src-tauri/src/net_sites.rs`：8 个命令 + worker pool + 取消 + 两个事件 + capability + settings 读写 + 注册（`lib.rs`）；`scripts/check-windows.sh` 过编译（`-o NUL` 分支） | 0.35 |
| S2 | 前端：`types/net.ts` / `ipc.ts` / `stores/siteStore.ts`（新）/ `SitesPage` 组件 + 编辑态 + `NetBoard` 第三分段 + `devFixture` 假数据 + i18n 双语 | 0.5 |
| S3 | README / feature-spec / CHANGELOG / 文档链接；真机走查 + 验收清单 | 0.15 |
| **合计** | | **≈ 1.4** |

无新依赖（`std::process::Command` + 系统 curl）。Windows 真机走查并入下一批 Windows 批次（本机只能过编译门禁）。

---

## 12. 假事实规避清单

| 陷阱 | 规避 |
|---|---|
| 把「TCP 无响应」写成「被墙」 | 只说观测事实（`stage` + 退出码解释），不推断原因；文案里不出现「封锁/墙/GFC」 |
| 403 / 429 涂成红灯 | 独立 `refused` 态：链路全通，站点拒绝本次请求 |
| 探测失败显示 0 ms / 0 B/s | 五阶段全部 `Option`；`null` → 文案，不渲染数字（沿用诊断页「不可读 ≠ 0」铁律） |
| 小文件速率冒充吞吐 | `bytes ≥ 200 KB` 且传输 ≥ 300 ms 才给速率；否则「样本不足（{bytes} / {ms}）」 |
| `%{speed_download}` 直接用 | 自算 `bytes / (total − ttfb)`，扣掉握手与 TTFB 摊销 |
| 跨站比速率 | 脚注写明「首页体积与 CDN 不同，速率仅同站前后对比有意义」；汇总条不排名 |
| 用户 `.curlrc` 或环境代理改变口径 | `-q` 置首 + `--noproxy *`；单测锁死参数 |
| 跟随跳转后把落地页当配置的站 | 不加 `-L`；3xx 标「重定向（未跟随）」 |
| Range 不被尊重导致流量超预期 | 脚注写「上限 ≈ 8 s 带宽，非严格 2 MiB」 |
| 站点反爬把 UA 换成浏览器 | 不伪装浏览器 UA（`PruneSiteProbe/<version>` 明示身份）；403 如实显示 |
| 站表损坏静默重置 | 解析失败 → 顶部提示「已回落默认」并保留原文件不覆盖 |
| curl 缺失时渲染空表 / 一串 0 | capability 门控整页不可用说明 |
| 单次读数被当稳定性能 | 页面标「单次采样 · HH:MM」+ 行内重测；不做「平均」假象 |
| 连点触发多轮把自己带宽打满 | 单轮互斥（`busy` 立即返回）+ 4 并发上限 |

---

## 13. 验收清单

1. 首启进入「站点」页：10 个默认站、未检测态、脚注与阈值可见
2. 点「全部检测」：逐行点亮（进度事件），`可达/偏慢/站点拒绝/无响应/不可达` 汇总等于 10
3. 断开 Wi-Fi 再测：全部 `unreachable/dns` 或 `unreachable/tcp`，**不出现 0 ms**
4. 配一个不存在的域名：`unreachable/dns` + note 含退出码 6
5. 配 `https://x.com/`（或等效）：TLS 阶段超时 → `unreachable/tls`，TCP 读数仍在（证明分层有效）
6. 配 `https://openai.com/`：403 → `refused`，其余读数完整，速率显示「样本不足」而非 0
7. 单站重测只更新那一行；编辑站表后保存 → 重启应用仍在；「恢复默认」回到 10 条
8. 校验：填 `file:///etc/passwd`、`ftp://x`、`https://u:p@host/`、空名、21 条 → 行内报错且不写盘
9. 取消：点取消后按钮复位，已完成的行保留，未完成的行回到未测态，`cancelled = true`
10. 复制结果：剪贴板 Markdown 含时间 / 站表逐行读数 / 失败原因 / 口径脚注，粘给 AI 可独立读懂
11. 并发验证：系统监视器里 curl 峰值 ≤ 4 个进程；整轮流量 ≤ 约 20 MB（10 站）
12. `cargo test --workspace` 全绿（sites 新增 ≥ 15 例）；`npx tsc --noEmit` 0 错误；`pnpm build` 过 i18n + docs 两道门禁
13. `scripts/check-windows.sh` 通过（`NUL` 分支编译）；Windows 真机：站表、检测、复制、取消与 macOS 行为一致
14. 网络 tab 的「监测」「诊断」两页零回归（A0–A4、D0–D3 验收项复跑）

---

## 14. 待拍板

| # | 议题 | 建议 |
|---|---|---|
| 1 | 默认站表是否保留 403 站（OpenAI / 备选 Netflix） | 保留：403 本身就是「可达但被拒」的读数，也是演示 `refused` 态的唯一样本 |
| 2 | 每站单次采样 vs 3 次取中位数 | v1 单次（一轮 3–8 s 才有人用）+ 单站重测；3 次中位数作为「深度检测」进 v2 |
| 3 | 是否提供「经系统代理测」开关 | v1 不做（口径可复现优先）；v2 可加显式开关，且必须在结果上标注「经代理」，否则与前后的直连读数不可比 |
| 4 | curl 依赖是否可接受（Windows 老版本可能缺失） | 接受：capability 门控 + 明确文案；引 rustls 换 HTTPS 要多 ~1.5 MB 体积与一套证书链策略，收益不抵 |
| 5 | 站表是否随 iCloud/导出备份 | 不做：随 `settings.json` 走，不额外做导入导出 |
| 6 | 是否挂 CLI / MCP 工具（`site_probe`） | v1 不挂：MCP 是只读诊断面，站点检测会发起外向请求，语义边界需单独讨论（M29 只读底线） |
| 7 | 结果是否落 SQLite 做「换网前后对比」 | v1 不落（快照纪律）；若 S3 走查后确有对比需求，再单独立项（要解决基线定义与站点内容变化两个伪差异来源） |

---

## 15. 实施记录（2026-11-15）

### 15.1 落地文件

| 阶段 | 文件 | 说明 |
|---|---|---|
| S0 | `crates/prune-core/src/net/sites.rs`（新） | 结构 / 默认站表 / URL 校验 / curl 参数拼装 / `-w` 解析 / 判定 / 汇总 / 报告入口 + 20 项单测 + 1 个 `#[ignore]` 真机冒烟 |
| S0 | `crates/prune-core/src/i18n/err.rs` | 新增 18 条用户可见错误串（curl 缺失 / 校验失败 / busy / 站点已不在表里…），中英各一份并进 CJK 扫描 |
| S0 | `crates/prune-core/src/i18n/report.rs` | `render_sites`：站点报告的 Markdown（表头 10 列、断点写进状态列、增量列、口径脚注）+ 3 项单测 |
| S0 | `crates/prune-core/src/net/mod.rs` | 注册 `pub mod sites` |
| S1 | `src-tauri/src/net_sites.rs`（新） | 8 个命令 + `SiteState`（单轮互斥 / 取消标志 / 最近一轮）+ `thread::scope` 4 并发 + 两个事件 + settings 读写 |
| S1 | `src-tauri/src/lib.rs` | 注册 8 个命令 + `manage(SiteState)` |
| S2 | `src/types/net.ts` | 站点类型镜像 + `SITE_THRESHOLDS` + `siteDelta` / `formatRate` / `siteTone` / `summarizeSites` |
| S2 | `src/lib/ipc.ts` | 8 个 IPC 封装 |
| S2 | `src/stores/siteStore.ts`（新） | 站表 / 本轮结果 / 进度订阅 / 单站重测 / 复制反馈 / 列表错误 |
| S2 | `src/components/SitesPage.tsx`（新） | 头部操作、汇总条、10 行读数、状态 chip、细节展开、编辑态、curl 缺失说明 |
| S2 | `src/components/NetBoard.tsx` | 第三分段「站点」+ 副标 + 渲染 `SitesPage` |
| S2 | `src/lib/devFixture.ts` | `devSites` / `devSiteProbes`（五种形态都出现的预览数据） |
| S2 | `src/i18n/messages/namespaces/net.ts` | `net.sites.*` 约 50 条 + `subSites` / `subpage.sites`，中英同步 |
| S3 | `README.md` / `README.zh-CN.md` / `CHANGELOG.md` / 版本号 | 边界表加一行、tab 说明加「站点检测」、文档链接、0.11.0 版本段；`pnpm bump 0.11.0` |

### 15.2 与方案的三处结构偏差

1. **`FailStage` 多一个 `Transfer`**：方案 §4.2 只列了 dns / tcp / tls / ttfb / http，但「传输慢」必须有自己的阶段值，否则 UI 只能说「偏慢」而说不出慢在哪一段。判定顺序里它排在 ttfb 之后。
2. **`SiteProbe` 带 `name` / `url` 快照**：站表可以被随时改，报告却要回答「当时打的到底是哪个地址」。让探测结果自描述，比「报告时再回查站表」更抗漂移（站被删了也还能出报告）。
3. **`SiteList` 多一个 `recovered`**：方案 §8 要求「解析失败 → 回落默认 + 顶部提示」，但只有 `sites` + `customized` 两个字段时前端分辨不出「用户主动清空」与「文件坏了」。加一个布尔，提示才有可能。

4. **UI 由「一行一站 + `<details>`」改成「卡片网格 + 详情弹窗」**（2026-11-15，用户要求）：卡片放三个读数和速率条，五段读数、本段/累计、curl 原文都进弹窗。行式布局在 960 窗口下把 10 行 × 5 个数挤成一张读不完的表；卡片把「十站总量」与「单站细节」分成两个层级。§9 已按实施结果重写。

### 15.3 真机发现（2026-11-15，macOS 15.7.7，curl 8.7.1）

- **UA 决定成败**：默认 curl UA 下 `github.com` 是 `stalled`（握手 257 ms、首字节 0、10 s 超时）；换成 `PruneSiteProbe/0.11.0` 后同一分钟变成 `206 / 525 675 B / 首字节 310 ms`。结论：**要有身份、但不伪装浏览器**——明示 `PruneSiteProbe` 既让对端可识别，也比默认 UA 少踩反爬。
- **Range 与 IPv6**：`apple.com` 返回 `206` + 260 629 B（尊重 Range），`remote_ip` 是 IPv6（`2408:8742:...`）——出口 IP 列按字符串原样展示，不假设 v4。
- **样本门槛确实生效**：同一台机器上 apple 传输阶段只有 83 ms（260 KB），`transferBps` 按设计返回 `null`，UI 显示「样本不足（254 KB / 83 ms）」，没有编出一个 3 MB/s。
- 10 站实测分布（同一分钟）：可达 3（apple / cloudflare / amazon）、偏慢 1（microsoft，TLS 增量 577 ms）、站点拒绝 1（openai 403）、无响应 1（github 用默认 UA 时）、不可达 4（google / wikipedia / x / youtube，分别停在 TCP 与 TLS）——与方案 §2 的形态表一致。

- **`%{remote_ip}` 为空时的解析陷阱**（2026-11-15 二次走查复现）：TCP/TLS 没通时 `remote_ip` 是空串，`-w` 行尾是 `...\t000\t\n`，最初的 `trim_end()` 把空字段连分隔符一起削掉（8 段 → 7 段），整站被判 `error`。真机上 google / youtube / github / wikipedia / x 五站全中，页面上是五张写着 `write-out has 7 fields, expected 8` 的卡。改为只按 `\t` 切分、不裁剪行尾（`lines()` 已处理 `\r\n`），并补回归测试用真实输出断言「判 `unreachable/Tcp`，不是 `error`」。

### 15.4 验收状态

| 项 | 状态 |
|---|---|
| `cargo test --workspace` | ✅ 见 §15.5 实测数字（sites 20 项 + 报告 3 项，另 1 项 `#[ignore]` 真机冒烟） |
| `cargo clippy --all-targets`（app + core） | ✅ 0 warning（仅 workspace manifest 的既有 `edition` 提示） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `node scripts/check-i18n.mjs` | ✅ 无硬编码中文 |
| `node scripts/check-docs-honesty.mjs` | ✅ MCP 8 工具对账一致、排他表述零命中 |
| 浏览器预览走查（960×640 真实窗口 + 1280×900，深/浅两主题） | ✅ 卡片网格 3 列/4 列自适应、无读数卡只显示一句状态且不溢出、详情弹窗（瀑布 + 本段/累计表 + 元数据 + curl 原文；`error` 态只留结论）、编辑态、Escape 关闭与焦点归位均正常；dev fixture 覆盖 ok / slow / stalled / refused / unreachable |
| `pnpm tauri dev` 真机（IPC / 事件 / settings / 剪贴板） | ⬜ 需在真机点一轮「全部检测」并复制结果（本轮只做了 `curl` 全链路冒烟与前端走查） |
| `scripts/check-windows.sh` | ⬜ 待跑（`-o NUL` 分支尚未过 Windows 交叉编译门禁） |
| Windows 真机 | ⬜ 并入下一批 Windows 走查 |

### 15.5 门禁实测数字

- `cargo test --workspace`：`prune-core` 428 passed / 2 ignored（含 sites 21 项 + report 3 项；ignored 之一是 `live_probe_smoke`），`prune-app` 49 passed，`prune-cli` 22 passed / 1 ignored → 合计 **499 passed**（改动前 475）
- 前端产物：`dist/assets/index-*.js` 634.77 kB（gzip 189.10 kB），构建 3.79 s
