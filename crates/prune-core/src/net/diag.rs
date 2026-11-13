//! 网络诊断（参考 docs/net-diag-plan.md）：分层定位「慢在哪」。
//!
//! WhyFi 的核心价值是分层：DNS / 网关(LAN) / 出口(WAN) 各探一层，谁异常指谁。
//! Prune 的口径纪律全部延续：
//!
//! 1. **连接耗时 ≠ ICMP ping**：全部用 `TcpStream::connect_timeout`（零依赖、
//!    零权限、零提权），措辞只写「连接耗时」。ICMP 要 raw socket，不做。
//! 2. **探测失败 ≠ 0 ms**：`LayerProbe::median_ms = None` 就是失败，UI 显示
//!    「探测失败」，绝不出现编造的 0。
//! 3. **判定阈值透明**：[`LAN_SLOW_MS`] 等常量是经验值，写死 + 显示在 UI，
//!    不做动态基线。判定规则双平台同一套（§3.6）。
//! 4. **平台差异只暴露两个口**：[`default_gateway()`] 与 [`wifi_ssid()`]，
//!    其余探测与判定纯函数双平台同源。
//!
//! 诊断是**手动触发**的短任务（一轮 ≤ 5 s 预算），不进常驻采样循环；
//! 结果是快照，不落库（回答「现在」，不回答「昨天」）。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::i18n::diag_text::{self, Line, ParamLine};
use crate::i18n::fill;
use crate::locale::Locale;

// ---------------------------------------------------------------- 阈值（§4.3）

/// 路由器连接耗时超过它 → 「路由器响应慢」
pub const LAN_SLOW_MS: f64 = 50.0;
/// 出口与网关的耗时差超过它 → 「出口/运营商延迟高」
pub const WAN_LAN_GAP_MS: f64 = 100.0;
/// 出口抖动（MAD）超过它 → 「链路抖动大」
pub const JITTER_HIGH_MS: f64 = 30.0;
/// DNS 解析耗时超过它 → 「DNS 偏慢」
pub const DNS_SLOW_MS: f64 = 300.0;

/// 每个探测的单次超时
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(2000);
/// DNS 探测的总超时（getaddrinfo 本身不可中断，靠旁路线程放弃等待）
pub const DNS_TIMEOUT: Duration = Duration::from_millis(2000);

// 探测目标（§9 待拍板 #3：固定公网目标，换私有域名会引入自家缓存偏差）
/// DNS 层目标：走系统解析器（含缓存），首样本丢弃
pub const DNS_PROBE_HOST: &str = "www.apple.com:443";
/// WAN 层目标：固定 IP 免 DNS 干扰；443 几乎不被路由器拦。
/// 大陆网络对 1.1.1.1/8.8.8.8 的 443 直连 TCP 有静默丢弃（实测 2026-11-09，
/// ICMP 可达但 TCP 全超时——ping 通不代表 443 通），全部失败时回退国内固定 IP。
pub const WAN_PROBE_ADDR: &str = "1.1.1.1:443";
pub const WAN_FALLBACK_ADDR: &str = "223.5.5.5:443";
/// captive portal 探测（Apple 官方检测端点，WhyFi 同款）
pub const PORTAL_HOST: &str = "captive.apple.com";
pub const PORTAL_PATH: &str = "/hotspot-detect.txt";
/// 正常网络的响应 body；不是它 = 疑似 portal
pub const PORTAL_OK_BODY: &str = "Success";

// ---------------------------------------------------------------- 数据契约

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagStatus {
    /// 没跑过（UI：点击开始诊断）
    Idle,
    Running,
    Done,
    /// 探测机制本身失败（如拿不到网关）。与「探测到全断网」（Done + 结论）严格区分
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptiveState {
    /// 正常公网（拿到 Success）
    None,
    /// 重定向 / body 异常 → 疑似酒店/机场登录页
    Suspected,
    /// 探测本身出错，不下结论
    Unknown,
}

/// 单层探测结果。`median_ms = None` = 探测失败（**不是 0**）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerProbe {
    pub median_ms: Option<f64>,
    /// 原始样本，UI 可展开
    pub samples: Vec<f64>,
    pub note: Option<String>,
}

impl LayerProbe {
    fn failed(note: impl Into<String>) -> Self {
        Self {
            median_ms: None,
            samples: Vec::new(),
            note: Some(note.into()),
        }
    }

    fn from_samples(samples: Vec<f64>) -> Self {
        let median_ms = median(&samples);
        Self {
            median_ms,
            samples,
            note: None,
        }
    }

    /// 全部探测都失败（用于「连不上路由器」判定）
    fn is_failed(&self) -> bool {
        self.median_ms.is_none()
    }
}

/// 命中的规则类型。
///
/// 加它的直接原因：原来的单测断言中文整句（`assert_eq!(v.label, "各层正常")`），
/// 改一个字的文案就挂一片测试，而这些测试真正想验证的是**优先级短路顺序**。
/// 有了 `kind`，测试断言「命中了哪条规则」，文案怎么改都不影响。
///
/// 顺序即 §3.3 的短路顺序，前端不消费（`skip_serializing_if`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerdictKind {
    LanDown,
    DnsFail,
    Captive,
    LanSlow,
    WanGap,
    Jitter,
    DnsSlow,
    AllGood,
}

/// 渲染所需的数字。缺项按 0 处理（不 panic）
#[derive(Debug, Clone, Copy, Default)]
pub struct VerdictParams {
    /// 实测值（LanSlow / Jitter / DnsSlow 用它，WanGap 用 gap）
    pub ms: f64,
    /// WanGap 的差值（WAN − LAN）
    pub gap_ms: f64,
    /// 该条规则的阈值，由调用方从常量传入 —— 文案里不写死数字，避免
    /// 「界面说 50 ms、代码按 80 ms 判」这种漂移
    pub threshold: f64,
}

/// 分层结论。规则与阈值一并给 UI，不搞黑盒（§3.3）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// 一句话主因，如「出口/运营商延迟高（差值 210 ms）」
    pub label: String,
    /// 建议
    pub advice: String,
    /// 命中的规则（含阈值原文），UI 逐行展示阈值时引用
    pub rule: String,
    /// 命中的规则类型。只用于测试与日志排查，前端不读
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<VerdictKind>,
}

/// 一轮诊断的完整产出。serde camelCase ↔ `src/types/net.ts`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetDiag {
    pub at: i64,
    pub status: DiagStatus,
    /// 非空 = 必须显示给用户的原因（failed 时必填）
    pub note: Option<String>,
    pub dns: Option<LayerProbe>,
    pub lan: Option<LayerProbe>,
    pub wan: Option<LayerProbe>,
    /// 出口抖动 = WAN 样本的平均绝对偏差（MAD）。样本少（5 个），MAD 比标准差稳
    pub jitter_ms: Option<f64>,
    pub gateway: Option<String>,
    pub captive: Option<CaptiveState>,
    pub verdict: Option<Verdict>,
    /// 报告用。读不到（网线 / netsh locale）就 None，**不下假结论**
    pub ssid: Option<String>,
}

/// 手动测速结果（macOS `networkQuality`，Windows 上整卡隐藏，见 IPC capability）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    pub at: i64,
    pub down_bps: f64,
    pub up_bps: f64,
    /// Apple「responsiveness」：负载下响应度，bufferbloat 的官方口径。不做自造公式
    pub rpm: f64,
    /// Apple 原文分级（Poor/Low/Medium/Good），前端映射中文
    pub rpm_level: String,
    pub base_rtt_ms: f64,
}

// ---------------------------------------------------------------- 纯函数（可测核心）

/// 中位数。空集 = None（失败语义，不是 0）
pub fn median(samples: &[f64]) -> Option<f64> {
    let mut s: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
    if s.is_empty() {
        return None;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = s.len() / 2;
    Some(if s.len() % 2 == 1 {
        s[mid]
    } else {
        (s[mid - 1] + s[mid]) / 2.0
    })
}

/// 平均绝对偏差（MAD around mean）。样本 < 2 时 None——单个样本谈不上「抖」
pub fn mean_abs_deviation(samples: &[f64]) -> Option<f64> {
    let n = samples.iter().filter(|v| v.is_finite()).count();
    if n < 2 {
        return None;
    }
    let sum: f64 = samples.iter().sum();
    let mean = sum / n as f64;
    Some(samples.iter().map(|v| (v - mean).abs()).sum::<f64>() / n as f64)
}

/// portal 响应判定（纯函数，网络层只负责取回这两段文本）。
/// 只有「网络可达但 body ≠ Success」才报疑似；连接失败上层给 Unknown。
pub fn parse_portal_response(status_line: &str, body: &str) -> CaptiveState {
    // "HTTP/1.1 200 OK" → 取第二段的数字
    let Some(code) = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
    else {
        return CaptiveState::Unknown;
    };
    if (300..400).contains(&code) {
        // portal 拦截的标准姿势：302/307 到登录页
        return CaptiveState::Suspected;
    }
    if code == 200 && body.contains(PORTAL_OK_BODY) {
        CaptiveState::None
    } else {
        CaptiveState::Suspected
    }
}

/// macOS `route -n get default` 输出解析（纯函数）。
/// 返回 (gateway, interface)。实测样例：
/// `    gateway: 192.168.1.1` / `  interface: en0`
pub fn parse_route_get(output: &str) -> Option<(String, Option<String>)> {
    let mut gateway = None;
    let mut iface = None;
    for line in output.lines() {
        let t = line.trim_start();
        if let Some(v) = t.strip_prefix("gateway:") {
            let v = v.trim();
            if !v.is_empty() {
                gateway = Some(v.to_string());
            }
        } else if let Some(v) = t.strip_prefix("interface:") {
            let v = v.trim();
            if !v.is_empty() {
                iface = Some(v.to_string());
            }
        }
    }
    gateway.map(|g| (g, iface))
}

/// macOS `ipconfig getsummary <iface>` 的 SSID 解析（纯函数）。
/// 实测行格式为 `  SSID : <value>`（冒号前有空格），非 ASCII SSID 会 hex 编码；
/// 精确匹配 key（`SSID` / `SSIDSTR`），`BSSID` / `_SSID` / `SSIDTYPE` 一律忽略。
pub fn parse_ipconfig_ssid(output: &str) -> Option<String> {
    let mut raw = None;
    for line in output.lines() {
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim();
        let value = line[colon + 1..].trim();
        match key {
            "SSIDSTR" => {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            "SSID" if !value.is_empty() && raw.is_none() => {
                raw = Some(value.to_string());
            }
            _ => {}
        }
    }
    let raw = raw?;
    // 全 hex 且偶数长度 → 大概率是 hex 编码；解码出的必须是合法 UTF-8 才采用，
    // 否则（如纯数字的「123456」SSIDs）按原文返回，不制造乱码
    if raw.len() % 2 == 0 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(bytes) = (0..raw.len() / 2)
            .map(|i| u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
        {
            if let Ok(s) = String::from_utf8(bytes) {
                return Some(s);
            }
        }
    }
    Some(raw)
}

/// Windows `netsh wlan show interfaces` 的 SSID 解析（纯函数）。
/// 输出 locale 敏感（中英文关键字不同），只认 "SSID" 子串并排除 "BSSID"；
/// 值为空 / 解析失败一律 None——绝不用猜的关键字兜底。
pub fn parse_netsh_ssid(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("BSSID") {
            continue;
        }
        if let Some(idx) = line.find("SSID") {
            let after = &line[idx + "SSID".len()..];
            if let Some(colon) = after.find(':') {
                let v = after[colon + 1..].trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
                // SSID 行存在但值为空 = 未连接，直接结束
                return None;
            }
        }
    }
    None
}

/// 无占位文案 → `Verdict`
fn simple(kind: VerdictKind, t: &Line) -> Verdict {
    Verdict {
        label: t.label.into(),
        advice: t.advice.into(),
        rule: t.rule.into(),
        kind: Some(kind),
    }
}

/// 带实测值 + 阈值的文案 → `Verdict`。数字一律 0 位小数（UI 与报告同口径）
fn measured(kind: VerdictKind, t: &ParamLine, v: f64, threshold: f64) -> Verdict {
    let v = format!("{v:.0}");
    let thr = format!("{threshold:.0}");
    let pairs = [("v", v.as_str()), ("t", thr.as_str())];
    Verdict {
        label: fill(t.label, &pairs),
        advice: fill(t.advice, &pairs),
        rule: fill(t.rule, &pairs),
        kind: Some(kind),
    }
}

/// 分层判定（§3.3 优先级短路，纯函数）。全分支由单测覆盖。
///
/// `locale` 只影响文案，不影响判定 —— 判定顺序与阈值对所有语言完全一致，
/// 否则「中文说正常、英文说抖动」这种事故只是时间问题。
pub fn verdict(
    lan: Option<&LayerProbe>,
    dns: Option<&LayerProbe>,
    wan: Option<&LayerProbe>,
    jitter_ms: Option<f64>,
    captive: CaptiveState,
    locale: Locale,
) -> Option<Verdict> {
    let t = diag_text::verdict_text(locale);
    // 1. 网关探测全失败 → 连不上路由器
    if lan.is_some_and(LayerProbe::is_failed) {
        return Some(simple(VerdictKind::LanDown, &t.lan_down));
    }
    // 2. DNS 全失败 → 解析故障
    if dns.is_some_and(LayerProbe::is_failed) {
        return Some(simple(VerdictKind::DnsFail, &t.dns_fail));
    }
    // 3. portal
    if captive == CaptiveState::Suspected {
        return Some(simple(VerdictKind::Captive, &t.captive));
    }
    let lan_ms = lan.and_then(|p| p.median_ms);
    let dns_ms = dns.and_then(|p| p.median_ms);
    let wan_ms = wan.and_then(|p| p.median_ms);
    // 4. 路由器慢
    if let Some(v) = lan_ms.filter(|v| *v > LAN_SLOW_MS) {
        return Some(measured(VerdictKind::LanSlow, &t.lan_slow, v, LAN_SLOW_MS));
    }
    // 5. 出口慢（WAN − LAN）
    if let (Some(w), Some(l)) = (wan_ms, lan_ms) {
        let gap = w - l;
        if gap > WAN_LAN_GAP_MS {
            return Some(measured(
                VerdictKind::WanGap,
                &t.wan_gap,
                gap,
                WAN_LAN_GAP_MS,
            ));
        }
    }
    // 6. 抖动
    if let Some(j) = jitter_ms.filter(|j| *j > JITTER_HIGH_MS) {
        return Some(measured(VerdictKind::Jitter, &t.jitter, j, JITTER_HIGH_MS));
    }
    // 7. DNS 慢
    if let Some(v) = dns_ms.filter(|v| *v > DNS_SLOW_MS) {
        return Some(measured(VerdictKind::DnsSlow, &t.dns_slow, v, DNS_SLOW_MS));
    }
    // 8. 正常
    Some(simple(VerdictKind::AllGood, &t.all_good))
}

// ---------------------------------------------------------------- 探测器（薄壳）

/// TCP connect 耗时（ms）。连接建立即停表——我们量的是「握上手」，不是「传完数据」
fn probe_tcp(addr: &str, timeout: Duration, locale: Locale) -> Result<f64, String> {
    let t = diag_text::note_text(locale);
    let sock = addr.parse().map_err(|e: std::net::AddrParseError| {
        fill(t.addr_parse, &[("addr", addr), ("err", &e.to_string())])
    })?;
    let start = Instant::now();
    TcpStream::connect_timeout(&sock, timeout)
        .map(|_| start.elapsed().as_secs_f64() * 1000.0)
        .map_err(|e| format!("{addr}: {e}"))
}

/// DNS 解析耗时（ms）。getaddrinfo 不可中断，套旁路线程实现总超时；
/// 超时后线程在后台自行结束（一次性泄漏 ≤ 系统解析器超时上限，可接受）
fn probe_dns(host_port: &str, timeout: Duration, locale: Locale) -> Result<f64, String> {
    let (tx, rx) = mpsc::channel();
    let target = host_port.to_string();
    std::thread::spawn(move || {
        let start = Instant::now();
        let r = target
            .to_socket_addrs()
            .map(|_| ())
            .map_err(|e| format!("{target}: {e}"));
        let _ = tx.send((r, start.elapsed().as_secs_f64() * 1000.0));
    });
    match rx.recv_timeout(timeout) {
        Ok((Ok(()), ms)) => Ok(ms),
        Ok((Err(e), _)) => Err(e),
        Err(_) => Err(fill(
            diag_text::note_text(locale).dns_timeout,
            &[("host", host_port), ("dur", &format!("{timeout:?}"))],
        )),
    }
}

/// captive portal 探测：裸 TcpStream 手写 HTTP GET（项目无 HTTP 客户端依赖，
/// 明文一个请求足够，不为它引 reqwest/ureq）。
/// 口径：**不读系统代理**——浏览器走代理而探测不走，Windows 企业网下结果
/// 可能与浏览器体验不一致（脚注必须写明）。
///
/// 这里的 Err 一律是内部诊断信息：调用方只用 `Ok` 或落到
/// `CaptiveState::Unknown`，错误串永不显示给用户 —— 所以保持英文原样，
/// 不进文案表（翻译一句没人看的话只是多一份要维护的东西）。
fn probe_portal(timeout: Duration) -> Result<CaptiveState, String> {
    let addr = format!("{PORTAL_HOST}:80");
    let mut stream = TcpStream::connect_timeout(
        &addr
            .to_socket_addrs()
            .map_err(|e| format!("portal DNS: {e}"))?
            .next()
            .ok_or("portal DNS: no address")?,
        timeout,
    )
    .map_err(|e| format!("{addr}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    let req = format!(
        "GET {PORTAL_PATH} HTTP/1.1\r\nHost: {PORTAL_HOST}\r\nUser-Agent: Prune\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("portal request write: {e}"))?;
    let mut buf = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("portal response read: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let body = match text.find("\r\n\r\n") {
        Some(i) => &text[i + 4..],
        None => "",
    };
    Ok(parse_portal_response(status_line, body))
}

// ---------------------------------------------------------------- 平台口

/// 平台信息（网关 + 出口网卡名）
pub struct GatewayInfo {
    pub gateway: String,
    pub iface: Option<String>,
}

/// 默认网关。macOS 解析 `route -n get default`（实测可用）；
/// Windows 走 [`crate::win::default_gateway`]（GetIpForwardTable2）。
pub fn default_gateway() -> Option<GatewayInfo> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("route")
            .args(["-n", "get", "default"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        parse_route_get(&text).map(|(gateway, iface)| GatewayInfo { gateway, iface })
    }
    #[cfg(windows)]
    {
        crate::win::default_gateway().map(|g| GatewayInfo {
            gateway: g,
            iface: None,
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

/// 当前 Wi-Fi SSID（报告用，双平台均 optional）。读不到 → None，不下假结论。
pub fn wifi_ssid(iface: Option<&str>) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let iface = iface.unwrap_or("en0");
        let out = std::process::Command::new("ipconfig")
            .args(["getsummary", iface])
            .output()
            .ok()?;
        parse_ipconfig_ssid(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .ok()?;
        parse_netsh_ssid(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = iface;
        None
    }
}

// ---------------------------------------------------------------- 整轮编排

/// 跑一轮诊断（串行，预算 ≤ 5 s：DNS 3 + LAN 3 + WAN 5 + portal 1，各 2 s 超时）。
/// 全部探测失败不等于整轮 failed——那是「连不上路由器」的 Done；
/// failed 只留给「探测机制本身失败」（拿不到网关）。
///
/// `locale` 决定 note 与结论的语言。诊断本身要跑几秒，用户在等待期间切语言
/// 不会中断这一轮 —— 语言在开跑那一刻定下，结果自洽即可。
pub fn run_diag(at: i64, locale: Locale) -> NetDiag {
    let nt = diag_text::note_text(locale);
    let Some(gw) = default_gateway() else {
        return NetDiag {
            at,
            status: DiagStatus::Failed,
            note: Some(nt.no_gateway.into()),
            dns: None,
            lan: None,
            wan: None,
            jitter_ms: None,
            gateway: None,
            captive: None,
            verdict: None,
            ssid: None,
        };
    };

    // LAN：网关 :80（3 次）。失败原因带出（不然 UI 只有「探测失败」，没法定位）
    let lan_addr = format!("{}:80", gw.gateway);
    let mut lan_err: Option<String> = None;
    let lan_samples: Vec<f64> = (0..3)
        .filter_map(|_| match probe_tcp(&lan_addr, PROBE_TIMEOUT, locale) {
            Ok(ms) => Some(ms),
            Err(e) => {
                lan_err = Some(e);
                None
            }
        })
        .collect();
    let mut lan = LayerProbe::from_samples(lan_samples);
    if lan.is_failed() {
        lan.note = Some(fill(
            nt.lan_failed,
            &[
                ("addr", &lan_addr),
                ("err", lan_err.as_deref().unwrap_or(nt.unknown_err)),
            ],
        ));
    }

    // WAN：1.1.1.1:443 优先（5 次，取中位数 + MAD 抖动）；全失败回退国内固定 IP。
    // 回退目标仍是固定 IP（免 DNS），判定规则（WAN−LAN 差值）与量纲不变；
    // 实际用了哪个目标写进 note——目标本身就是事实，不能静默换。
    let mut wan_err: Option<String> = None;
    let mut wan_samples: Vec<f64> = (0..5)
        .filter_map(|_| match probe_tcp(WAN_PROBE_ADDR, PROBE_TIMEOUT, locale) {
            Ok(ms) => Some(ms),
            Err(e) => {
                wan_err = Some(e);
                None
            }
        })
        .collect();
    let mut wan_note: Option<String> = None;
    if wan_samples.is_empty() {
        wan_note = Some(fill(
            nt.wan_all_failed,
            &[
                ("addr", WAN_PROBE_ADDR),
                ("err", wan_err.as_deref().unwrap_or(nt.timeout_err)),
                ("fallback", WAN_FALLBACK_ADDR),
            ],
        ));
        wan_samples = (0..5)
            .filter_map(|_| probe_tcp(WAN_FALLBACK_ADDR, PROBE_TIMEOUT, locale).ok())
            .collect();
        if wan_samples.is_empty() {
            wan_note = Some(fill(
                nt.wan_both_failed,
                &[
                    ("addr", WAN_PROBE_ADDR),
                    ("fallback", WAN_FALLBACK_ADDR),
                    ("err", wan_err.as_deref().unwrap_or(nt.timeout_err)),
                ],
            ));
        }
    }
    let jitter_ms = mean_abs_deviation(&wan_samples);
    let mut wan = LayerProbe::from_samples(wan_samples);
    wan.note = wan_note;

    // DNS：3 次，首样本丢弃（含冷启动偏差）。全部失败 → failed 语义（不是 0），原因带出
    let mut dns_samples: Vec<f64> = Vec::new();
    let mut dns_err: Option<String> = None;
    for i in 0..3 {
        match probe_dns(DNS_PROBE_HOST, DNS_TIMEOUT, locale) {
            Ok(ms) if i > 0 => dns_samples.push(ms),
            Ok(_) => {}
            Err(e) => {
                dns_err = Some(e);
            }
        }
    }
    let dns = if dns_samples.is_empty() {
        LayerProbe::failed(fill(
            nt.dns_failed,
            &[
                ("host", DNS_PROBE_HOST),
                ("err", dns_err.as_deref().unwrap_or(nt.unknown_err)),
            ],
        ))
    } else {
        LayerProbe::from_samples(dns_samples)
    };

    let captive = probe_portal(PROBE_TIMEOUT).unwrap_or(CaptiveState::Unknown);
    let ssid = wifi_ssid(gw.iface.as_deref());
    let v = verdict(
        Some(&lan),
        Some(&dns),
        Some(&wan),
        jitter_ms,
        captive,
        locale,
    );

    NetDiag {
        at,
        status: DiagStatus::Done,
        note: None,
        dns: Some(dns),
        lan: Some(lan),
        wan: Some(wan),
        jitter_ms,
        gateway: Some(gw.gateway),
        captive: Some(captive),
        verdict: v,
        ssid,
    }
}

// ---------------------------------------------------------------- 报告导出（§3.5）

/// Markdown 诊断报告（白名单内容：时间 / SSID / 三层 / 结论 / 测速 / 口径）。
/// 粘贴给 AI 可独立读懂；不含进程列表与流量明细。
///
/// 渲染在 [`crate::i18n::report`]：那里按语言给整套静态片段，布局只写一次
/// —— 两种语言各写一遍表格拼装，迟早有一边多一个空行。
pub fn report_markdown(diag: &NetDiag, speed: Option<&SpeedResult>, locale: Locale) -> String {
    crate::i18n::report::render(diag, speed, locale)
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(ms: Option<f64>, samples: Vec<f64>) -> LayerProbe {
        LayerProbe {
            median_ms: ms,
            samples,
            note: None,
        }
    }

    // ---- 纯函数：median / MAD

    #[test]
    fn median_basics() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0]), Some(3.0));
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        // 非有限值剔除，不产生 None
        assert_eq!(median(&[f64::NAN, 2.0, 4.0]), Some(3.0));
    }

    #[test]
    fn mad_needs_two_samples() {
        assert_eq!(mean_abs_deviation(&[5.0]), None);
        assert_eq!(mean_abs_deviation(&[]), None);
        assert!((mean_abs_deviation(&[10.0, 20.0]).unwrap() - 5.0).abs() < 1e-9);
        let s = [190.0, 210.0, 200.0, 205.0, 195.0];
        let m = mean_abs_deviation(&s).unwrap();
        assert!(m > 0.0 && m < 15.0, "m={m}");
    }

    // ---- 纯函数：判定表全分支（§3.3 8 条，按优先级短路）
    //
    // 断言 `kind` 而不是中文整句：这些用例验的是「谁优先命中」，不是文案。
    // 加一个 `Locale::En` 的用例足以证明文案真的换了一整套。

    #[test]
    fn verdict_all_layers_ok() {
        let lan = probe(Some(3.0), vec![3.0]);
        let dns = probe(Some(28.0), vec![28.0]);
        let wan = probe(Some(30.0), vec![30.0]);
        let v = verdict(
            Some(&lan),
            Some(&dns),
            Some(&wan),
            Some(8.0),
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::AllGood));
        assert_eq!(v.label, "各层正常");
    }

    #[test]
    fn verdict_gateway_down_wins() {
        let lan = LayerProbe::failed("连不上");
        let v = verdict(
            Some(&lan),
            None,
            None,
            None,
            CaptiveState::Unknown,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::LanDown));
    }

    #[test]
    fn verdict_dns_failure_wins_over_slow_lan() {
        let lan = probe(Some(80.0), vec![80.0]); // 也超阈值，但 DNS 全失败优先级更高
        let dns = LayerProbe::failed("解析不了");
        let v = verdict(
            Some(&lan),
            Some(&dns),
            None,
            None,
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::DnsFail));
    }

    #[test]
    fn verdict_portal_after_dns_ok() {
        let lan = probe(Some(3.0), vec![3.0]);
        let dns = probe(Some(28.0), vec![28.0]);
        let v = verdict(
            Some(&lan),
            Some(&dns),
            None,
            None,
            CaptiveState::Suspected,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::Captive));
    }

    #[test]
    fn verdict_slow_lan_beats_wan_gap() {
        let lan = probe(Some(90.0), vec![90.0]);
        let dns = probe(Some(28.0), vec![28.0]);
        let wan = probe(Some(500.0), vec![500.0]); // 差值 410 也超，但 LAN 慢优先
        let v = verdict(
            Some(&lan),
            Some(&dns),
            Some(&wan),
            None,
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::LanSlow));
    }

    #[test]
    fn verdict_wan_gap_reported() {
        let lan = probe(Some(5.0), vec![5.0]);
        let dns = probe(Some(28.0), vec![28.0]);
        let wan = probe(Some(210.0), vec![210.0]);
        let v = verdict(
            Some(&lan),
            Some(&dns),
            Some(&wan),
            Some(8.0),
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::WanGap));
    }

    #[test]
    fn verdict_jitter_reported() {
        let lan = probe(Some(5.0), vec![5.0]);
        let dns = probe(Some(28.0), vec![28.0]);
        let wan = probe(Some(60.0), vec![60.0]); // 差值 55 不超 100；抖动 35 超 30
        let v = verdict(
            Some(&lan),
            Some(&dns),
            Some(&wan),
            Some(35.0),
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::Jitter));
    }

    #[test]
    fn verdict_slow_dns_last_of_failures() {
        let lan = probe(Some(5.0), vec![5.0]);
        let dns = probe(Some(320.0), vec![320.0]);
        let wan = probe(Some(60.0), vec![60.0]);
        let v = verdict(
            Some(&lan),
            Some(&dns),
            Some(&wan),
            Some(8.0),
            CaptiveState::None,
            Locale::Zh,
        )
        .unwrap();
        assert_eq!(v.kind, Some(VerdictKind::DnsSlow));
    }

    /// 同一组读数在两种语言下必须命中同一条规则：判定与语言无关，只有文案变
    #[test]
    fn verdict_kind_is_locale_independent() {
        let lan = probe(Some(90.0), vec![90.0]);
        let zh = verdict(Some(&lan), None, None, None, CaptiveState::None, Locale::Zh).unwrap();
        let en = verdict(Some(&lan), None, None, None, CaptiveState::None, Locale::En).unwrap();
        assert_eq!(zh.kind, en.kind);
        assert!(zh.label.contains("路由器响应慢"), "{}", zh.label);
        assert!(en.label.contains("Router responds slowly"), "{}", en.label);
        // 阈值来自常量，不是文案里写死的数字
        let thr = format!("{LAN_SLOW_MS:.0}");
        assert!(zh.rule.contains(&thr), "{}", zh.rule);
        assert!(en.rule.contains(&thr), "{}", en.rule);
    }

    /// 英文结论不能漏中文（文案表改了但忘了译这一条，会在这里挂）
    #[test]
    fn english_verdict_has_no_cjk() {
        let lan = LayerProbe::failed("x");
        let v = verdict(
            Some(&lan),
            None,
            None,
            None,
            CaptiveState::Unknown,
            Locale::En,
        )
        .unwrap();
        assert!(!v
            .label
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c)));
    }

    // ---- 纯函数：portal 解析

    #[test]
    fn portal_parse_branches() {
        assert_eq!(
            parse_portal_response("HTTP/1.1 200 OK", "Success\n"),
            CaptiveState::None
        );
        assert_eq!(
            parse_portal_response("HTTP/1.1 302 Found", "Redirecting..."),
            CaptiveState::Suspected
        );
        assert_eq!(
            parse_portal_response("HTTP/1.1 307 Redirect", ""),
            CaptiveState::Suspected
        );
        // 200 但 body 不对 → 疑似（有的 portal 返回 200 + 登录页 HTML）
        assert_eq!(
            parse_portal_response("HTTP/1.1 200 OK", "<html>login</html>"),
            CaptiveState::Suspected
        );
        // 响应不完整 → unknown，不下结论
        assert_eq!(parse_portal_response("garbage", ""), CaptiveState::Unknown);
    }

    // ---- 纯函数：平台输出解析（双平台同测，不依赖目标系统）

    #[test]
    fn route_get_parse() {
        let out = "   route to: default\n   destination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n      flags: <UP,GATEWAY,HOST,DONE,STATIC>\n";
        let (gw, iface) = parse_route_get(out).unwrap();
        assert_eq!(gw, "192.168.1.1");
        assert_eq!(iface.as_deref(), Some("en0"));
        assert!(parse_route_get("gateway: \n").is_none());
        assert!(parse_route_get("no match").is_none());
    }

    #[test]
    fn ipconfig_ssid_parse() {
        // ASCII SSID 直出
        let out = "  BSSID : aa:bb\n  SSID : MyHome\n  SSIDTYPE : (not set)\n";
        assert_eq!(parse_ipconfig_ssid(out).as_deref(), Some("MyHome"));
        // hex 编码（e4b8ad 是 UTF-8 的某段）——"414243" = ABC
        let out2 = "  BSSID : aa:bb\n  SSID : 414243\n";
        assert_eq!(parse_ipconfig_ssid(out2).as_deref(), Some("ABC"));
        // SSIDSTR 优先
        let out3 = "  SSID : 414243\n  SSIDSTR: 花园\n";
        assert_eq!(parse_ipconfig_ssid(out3).as_deref(), Some("花园"));
        assert_eq!(parse_ipconfig_ssid("BSSID : aa"), None);
    }

    #[test]
    fn netsh_ssid_parse() {
        // 英文
        let en = "    Name                   : Wi-Fi\n    SSID                   : MyHome\n    BSSID                  : aa:bb\n";
        assert_eq!(parse_netsh_ssid(en).as_deref(), Some("MyHome"));
        // 中文 locale（SSID 关键字不变）
        let zh = "    名称                   : WLAN\n    SSID                   : 花园wifi\n    BSSID                  : aa:bb\n";
        assert_eq!(parse_netsh_ssid(zh).as_deref(), Some("花园wifi"));
        // 未连接（SSID 后为空）
        let none = "    名称                   : WLAN\n    SSID                   :\n    状态                   : 未连接\n";
        assert_eq!(parse_netsh_ssid(none), None);
        // 没有任何 SSID 行
        assert_eq!(parse_netsh_ssid("nothing here"), None);
    }

    // ---- LayerProbe 语义

    #[test]
    fn failed_probe_is_not_zero() {
        let p = LayerProbe::failed("连不上");
        assert!(p.is_failed());
        assert_eq!(p.median_ms, None);
        // from_samples：空样本 = 失败语义
        let p2 = LayerProbe::from_samples(vec![]);
        assert!(p2.is_failed());
    }
}
