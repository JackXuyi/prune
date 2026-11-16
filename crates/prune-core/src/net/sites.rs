//! 站点连通性 / 传输速率检测（docs/net-sites-plan.md，F15）。
//!
//! 与 [`super::diag`] 的分工：诊断回答「网络哪一层慢」（网关 / 出口 / DNS 三个固定目标），
//! 本模块回答「**这几个具体网站连得上吗、断在哪一步**」。两者不重叠，也不共用判定表。
//!
//! 四条口径纪律（全部对应方案 §12 的假事实清单）：
//!
//! 1. **五个阶段各自可为 None**：DNS / TCP / TLS / TTFB / 传输。None 就是「没走到这一步」，
//!    UI 文案化，**永不渲染成 0 ms**（沿用 diag 的铁律）。
//! 2. **只描述观测事实**：`stage` 说「TCP 无响应」，不说「被墙」。原因不可知，不猜。
//! 3. **速率 = bytes / (total − ttfb)**，且样本不足不给数（< 200 KB 或 < 300 ms）。
//!    curl 的 `%{speed_download}` 是 `bytes / total`，把 DNS+握手+TTFB 摊进分母，
//!    首页这种小文件会被系统性低估（实测 apple 256 KB：763 KB/s vs 传输阶段 1.22 MB/s）。
//! 4. **超时 ≠ 无读数**：`--max-time` 截断时退出码 28，但 curl 仍输出已收字节
//!    （实测 206 646 B）。有字节就按数据判「慢」，只有 0 字节才判「不通」。
//!
//! 探测执行在 `src-tauri/src/net_sites.rs`；本模块只有纯函数与参数拼装，单测不碰网络。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::i18n::err::err_text;
use crate::i18n::fill;
use crate::locale::Locale;

// ---------------------------------------------------------------- 阈值与上限（方案 §4.4）

/// TCP 握手累计耗时超过它 → 握手慢
pub const SITE_TCP_SLOW_MS: f64 = 300.0;
/// TLS 握手**增量**（tls − tcp）超过它 → 握手慢
pub const SITE_TLS_SLOW_MS: f64 = 400.0;
/// 首字节累计耗时超过它 → 首字节慢
pub const SITE_TTFB_SLOW_MS: f64 = 1_000.0;
/// 传输阶段速率低于它 → 传输慢
pub const SITE_SLOW_BPS: f64 = 100_000.0;
/// 速率样本下限：字节数
pub const SITE_MIN_SPEED_BYTES: u64 = 200_000;
/// 速率样本下限：传输时长（扣掉 TTFB 后）
pub const SITE_MIN_SPEED_MS: f64 = 300.0;
/// Range 上限：尽量只下 2 MiB（不尊重 Range 的站靠 max-time 兜底）
pub const SITE_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const SITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
pub const SITE_MAX_TIME: Duration = Duration::from_secs(8);
/// 并发 worker 数：10 站典型 3–8 s；再高就是自己把带宽打满
pub const SITE_CONCURRENCY: usize = 4;
/// 站表上限
pub const SITE_MAX_COUNT: usize = 20;
pub const SITE_NAME_MAX_CHARS: usize = 32;
pub const SITE_URL_MAX_CHARS: usize = 512;

// ---------------------------------------------------------------- 数据契约

/// 一个被检测的站点。`id` 要跨保存稳定（进度事件与单站重测按它定位）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// 站点状态。**`Refused` 不是失败**：403/429 证明链路全通，只是站点拒绝本次请求
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SiteVerdict {
    Ok,
    Slow,
    /// 握手全过但首字节不到（实测 github.com 就是这一态）
    Stalled,
    /// 站点返回 4xx/5xx
    Refused,
    /// 链路层面没打通（DNS / TCP / TLS）
    Unreachable,
    /// 探测机制本身出错（curl 缺失 / 参数非法），与站点无关
    Error,
}

/// 断在哪一步。只用于文案与着色，不参与汇总计数
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailStage {
    Dns,
    Tcp,
    Tls,
    Ttfb,
    /// 传输阶段（速率低 / 8 s 内没下完）
    Transfer,
    Http,
}

/// 单站一次探测的完整产出。`at` 为 Unix 秒
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteProbe {
    pub id: String,
    /// 探测当时的名称与 URL 快照：站表随后可能被改，报告必须记录「实际打了哪个地址」
    pub name: String,
    pub url: String,
    pub at: i64,
    /// 以下五个时刻都是**从进程启动算起的累计值**（ms）。None = 该阶段未到达
    pub dns_ms: Option<f64>,
    pub tcp_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub bytes: u64,
    pub http_code: Option<u16>,
    pub remote_ip: Option<String>,
    /// bytes / (total − ttfb)；样本不足时为 None（**不是 0**）
    pub transfer_bps: Option<f64>,
    pub verdict: SiteVerdict,
    pub stage: Option<FailStage>,
    /// curl 的原始说明（退出码 + stderr 首行）。技术细节，展开行里原样展示
    pub note: Option<String>,
    pub curl_exit: Option<i32>,
}

/// 一轮检测
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteRun {
    pub at: i64,
    pub results: Vec<SiteProbe>,
    pub cancelled: bool,
    pub elapsed_ms: f64,
}

/// 站表 + 是否为自定义配置（前端据此决定是否显示「恢复默认」）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteList {
    pub sites: Vec<Site>,
    pub customized: bool,
    /// true = 存储的站表读不出来（或校验不过），已回落默认。前端必须提示，
    /// 不能静默把用户的配置换掉（方案 §8）
    #[serde(default)]
    pub recovered: bool,
}

/// 汇总计数（UI 汇总条）。各项之和 = `total`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSummary {
    pub ok: usize,
    pub slow: usize,
    pub stalled: usize,
    pub refused: usize,
    pub unreachable: usize,
    pub error: usize,
    pub total: usize,
}

// ---------------------------------------------------------------- 默认站表（方案 §5）

/// 10 个国外常用站。加 `slug` 当 id 而不随机生成：站表要能跨版本、跨设备对齐。
pub fn default_sites() -> Vec<Site> {
    const DEFS: [(&str, &str); 10] = [
        ("google", "https://www.google.com/"),
        ("youtube", "https://www.youtube.com/"),
        ("github", "https://github.com/"),
        ("wikipedia", "https://www.wikipedia.org/"),
        ("x", "https://x.com/"),
        ("cloudflare", "https://www.cloudflare.com/"),
        ("apple", "https://www.apple.com/"),
        ("microsoft", "https://www.microsoft.com/"),
        ("amazon", "https://www.amazon.com/"),
        ("openai", "https://openai.com/"),
    ];
    DEFS.iter()
        .map(|(id, url)| Site {
            id: (*id).to_string(),
            name: display_name(url),
            url: (*url).to_string(),
        })
        .collect()
}

/// 由 URL 推一个可读名：去掉 `www.` 与协议，取 host
pub fn display_name(url: &str) -> String {
    let host = host_of(url).unwrap_or_else(|| url.to_string());
    host.strip_prefix("www.")
        .unwrap_or(host.as_str())
        .to_string()
}

// ---------------------------------------------------------------- URL 校验（方案 §8）

/// 极简 URL 校验：不引 url crate（只需要 scheme + host 两个事实）。
///
/// 拒绝 userinfo（`user:pass@`）是硬要求：站表写在 settings.json 里，
/// 凭据一旦落盘就成了「用户没意识到自己被存了密码」。
fn check_url(raw: &str) -> Result<(), UrlIssue> {
    if raw.chars().count() > SITE_URL_MAX_CHARS {
        return Err(UrlIssue::TooLong);
    }
    // 控制字符与空白会被 curl 当成多个 argv 的一部分，直接拒绝
    if raw
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
    {
        return Err(UrlIssue::BadChars);
    }
    let lower = raw.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        &raw[raw.len() - r.len()..]
    } else if let Some(r) = lower.strip_prefix("http://") {
        &raw[raw.len() - r.len()..]
    } else {
        return Err(UrlIssue::Scheme);
    };
    if rest.contains('@') {
        return Err(UrlIssue::UserInfo);
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(':');
    // host 里允许 `:port`，去掉端口后再判空
    let host = host.split(':').next().unwrap_or_default();
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return Err(UrlIssue::Host);
    }
    Ok(())
}

enum UrlIssue {
    Scheme,
    Host,
    UserInfo,
    BadChars,
    TooLong,
}

fn host_of(url: &str) -> Option<String> {
    let s = url.trim();
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.split(':').next()?.trim();
    (!host.is_empty()).then(|| host.to_string())
}

fn url_issue_text(issue: UrlIssue, locale: Locale) -> String {
    let t = err_text(locale);
    match issue {
        UrlIssue::Scheme => t.sites_url_scheme.into(),
        UrlIssue::Host => t.sites_url_host.into(),
        UrlIssue::UserInfo => t.sites_url_userinfo.into(),
        UrlIssue::BadChars => t.sites_url_chars.into(),
        UrlIssue::TooLong => fill(
            t.sites_url_too_long,
            &[("max", &SITE_URL_MAX_CHARS.to_string())],
        ),
    }
}

/// 校验并归一化站表：trim、空名回落 host、去重、补 id。
///
/// 返回 `Err` 时调用方**不得写盘**（方案 §8：不写半套配置）。
pub fn validate_sites(sites: &[Site], locale: Locale) -> Result<Vec<Site>, String> {
    let t = err_text(locale);
    if sites.len() > SITE_MAX_COUNT {
        return Err(fill(
            t.sites_too_many,
            &[("max", &SITE_MAX_COUNT.to_string())],
        ));
    }
    let mut out: Vec<Site> = Vec::with_capacity(sites.len());
    let mut seen: Vec<String> = Vec::new();
    for s in sites {
        let url = s.url.trim();
        if url.is_empty() {
            return Err(t.sites_url_empty.into());
        }
        if let Err(issue) = check_url(url) {
            return Err(fill(
                t.sites_url_invalid,
                &[("url", url), ("why", &url_issue_text(issue, locale))],
            ));
        }
        // 去重按 URL 归一化（scheme + host 小写，路径保留）：同一地址梳两遍没有意义
        let key = url.to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let name = s.name.trim();
        if name.chars().count() > SITE_NAME_MAX_CHARS {
            return Err(fill(
                t.sites_name_too_long,
                &[("name", name), ("max", &SITE_NAME_MAX_CHARS.to_string())],
            ));
        }
        out.push(Site {
            id: if s.id.trim().is_empty() {
                slug(url)
            } else {
                s.id.trim().to_string()
            },
            name: if name.is_empty() {
                display_name(url)
            } else {
                name.to_string()
            },
            url: url.to_string(),
        });
    }
    // id 重复：后出现的重算（保留用户的顺序，只修 id）
    for i in 0..out.len() {
        let dup = out[..i].iter().any(|p| p.id == out[i].id);
        if dup {
            out[i].id = slug(&out[i].url);
        }
    }
    Ok(out)
}

/// 稳定短 id：fnv1a-64 的低 8 位十六进制。不引 uuid（一个十进制哈希就够）
pub fn slug(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (h >> 32) as u32)
}

// ---------------------------------------------------------------- curl 参数（方案 §4.5）

/// `-w` 输出格式：8 段 tab 分隔。字段顺序与 [`parse_curl_output`] 一一对应
pub const CURL_WRITE_OUT: &str = "%{time_namelookup}\t%{time_connect}\t%{time_appconnect}\t%{time_starttransfer}\t%{time_total}\t%{size_download}\t%{http_code}\t%{remote_ip}\n";

/// 空设备：不落地临时文件
#[cfg(windows)]
pub const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
pub const NULL_DEVICE: &str = "/dev/null";

/// 拼 curl 参数（不含程序名）。
///
/// 三条纪律写死在这里并由单测锁住：
/// - `-q` 必须是**第一个**参数（否则用户的 `~/.curlrc` 会改掉代理、UA、`-L`）
/// - `--noproxy *`：不读 `http_proxy`/`https_proxy`，同一份配置在两台机器上结果一致
/// - 不加 `-L`（3xx 本身是读数）、不加 `--compressed`（字节数才可比）
pub fn build_curl_args(site: &Site, ua: &str) -> Vec<String> {
    vec![
        "-q".into(),
        "-sS".into(),
        "-o".into(),
        NULL_DEVICE.into(),
        "--noproxy".into(),
        "*".into(),
        "--connect-timeout".into(),
        SITE_CONNECT_TIMEOUT.as_secs().to_string(),
        "--max-time".into(),
        SITE_MAX_TIME.as_secs().to_string(),
        "--range".into(),
        format!("0-{}", SITE_MAX_BYTES - 1),
        "-A".into(),
        ua.into(),
        "-H".into(),
        "Accept: */*".into(),
        "-w".into(),
        CURL_WRITE_OUT.into(),
        site.url.clone(),
    ]
}

// ---------------------------------------------------------------- 输出解析

/// curl `-w` 的原始读数（秒已换算成毫秒；0 一律折成 None）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawTiming {
    pub dns_ms: Option<f64>,
    pub tcp_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub bytes: u64,
    pub http_code: Option<u16>,
    pub remote_ip: Option<String>,
}

fn sec_to_ms(field: &str) -> Option<f64> {
    let v: f64 = field.trim().parse().ok()?;
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    Some(v * 1000.0)
}

/// 解析 `-w` 输出。取**最后一个非空行**（前面可能混进 curl 的进度/告警行）。
///
/// 段数不足直接 Err：宁可报「解析失败」，也不按位置猜字段。
///
/// **不能 `trim_end()`**：TCP 超时时 `%{remote_ip}` 是空串，行尾就是 `...\t0\t000\t\n`，
/// 削掉行尾空白会把「空字段」连同分隔符一起吃掉 —— 8 段变 7 段，
/// 于是一个正常的「不可达」被误报成「解析失败」（真机 2026-11-15 复现：
/// google / youtube / github / wikipedia / x 五站全中）。`lines()` 已经处理了 `\r\n`。
pub fn parse_curl_output(stdout: &str) -> Result<RawTiming, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with(char::is_numeric))
        .ok_or_else(|| "no write-out line".to_string())?;
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 8 {
        return Err(format!("write-out has {} fields, expected 8", f.len()));
    }
    let bytes = f[5].trim().parse::<u64>().unwrap_or(0);
    let code = f[6].trim().parse::<u16>().unwrap_or(0);
    let ip = f[7].trim();
    Ok(RawTiming {
        dns_ms: sec_to_ms(f[0]),
        tcp_ms: sec_to_ms(f[1]),
        tls_ms: sec_to_ms(f[2]),
        ttfb_ms: sec_to_ms(f[3]),
        total_ms: sec_to_ms(f[4]),
        bytes,
        http_code: (code > 0).then_some(code),
        remote_ip: (!ip.is_empty() && ip != "-").then(|| ip.to_string()),
    })
}

// ---------------------------------------------------------------- 判定（方案 §4.2 / §4.3）

/// 传输速率：`bytes / (total − ttfb)`，样本不足返回 None。
///
/// 样本门槛不是洁癖：apple 首页 256 KB / 209 ms 算出 1.22 MB/s，
/// 那是 TCP 慢启动区间，报成「吞吐」就是假精确。
pub fn transfer_bps(bytes: u64, total_ms: Option<f64>, ttfb_ms: Option<f64>) -> Option<f64> {
    let (t, ttfb) = (total_ms?, ttfb_ms?);
    let transfer_ms = t - ttfb;
    if bytes < SITE_MIN_SPEED_BYTES || transfer_ms < SITE_MIN_SPEED_MS {
        return None;
    }
    Some(bytes as f64 / (transfer_ms / 1000.0))
}

/// 判定一站的形态（纯函数，全分支由单测覆盖）。
///
/// `exit` / `note` 只进 `note` 与「部分成功」判定，不直接决定 verdict——
/// verdict 一律由**实测时刻**推出，退出码只是旁证。
pub fn judge(site: &Site, raw: &RawTiming, exit: i32, note: Option<String>, at: i64) -> SiteProbe {
    let is_https = site
        .url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://");
    let bps = transfer_bps(raw.bytes, raw.total_ms, raw.ttfb_ms);

    let (verdict, stage) = if raw.dns_ms.is_none() {
        (SiteVerdict::Unreachable, Some(FailStage::Dns))
    } else if raw.tcp_ms.is_none() {
        (SiteVerdict::Unreachable, Some(FailStage::Tcp))
    } else if is_https && raw.tls_ms.is_none() {
        (SiteVerdict::Unreachable, Some(FailStage::Tls))
    } else if raw.ttfb_ms.is_none() {
        (SiteVerdict::Stalled, Some(FailStage::Ttfb))
    } else if raw.http_code.is_some_and(|c| c >= 400) {
        (SiteVerdict::Refused, Some(FailStage::Http))
    } else if exit == 28 && raw.bytes > 0 {
        // 8 s 内没下完：有数据就是「慢」，不是「不通」——只是这次取样被截断
        (SiteVerdict::Slow, Some(FailStage::Transfer))
    } else if raw.tcp_ms.is_some_and(|v| v > SITE_TCP_SLOW_MS) {
        (SiteVerdict::Slow, Some(FailStage::Tcp))
    } else if raw
        .tls_ms
        .zip(raw.tcp_ms)
        .is_some_and(|(tls, tcp)| tls - tcp > SITE_TLS_SLOW_MS)
    {
        (SiteVerdict::Slow, Some(FailStage::Tls))
    } else if raw.ttfb_ms.is_some_and(|v| v > SITE_TTFB_SLOW_MS) {
        (SiteVerdict::Slow, Some(FailStage::Ttfb))
    } else if bps.is_some_and(|v| v < SITE_SLOW_BPS) {
        (SiteVerdict::Slow, Some(FailStage::Transfer))
    } else {
        (SiteVerdict::Ok, None)
    };

    SiteProbe {
        id: site.id.clone(),
        name: site.name.clone(),
        url: site.url.clone(),
        at,
        dns_ms: raw.dns_ms,
        // HTTP 站没有 TLS 阶段，`time_appconnect` 是 0 —— 这里保持 None，
        // 「不适用」和「握手失败」必须在数据层就分开
        tls_ms: if is_https { raw.tls_ms } else { None },
        tcp_ms: raw.tcp_ms,
        ttfb_ms: raw.ttfb_ms,
        total_ms: raw.total_ms,
        bytes: raw.bytes,
        http_code: raw.http_code,
        remote_ip: raw.remote_ip.clone(),
        transfer_bps: bps,
        verdict,
        stage,
        note,
        curl_exit: Some(exit),
    }
}

/// 汇总计数
pub fn summarize(probes: &[SiteProbe]) -> SiteSummary {
    let mut s = SiteSummary {
        total: probes.len(),
        ..SiteSummary::default()
    };
    for p in probes {
        match p.verdict {
            SiteVerdict::Ok => s.ok += 1,
            SiteVerdict::Slow => s.slow += 1,
            SiteVerdict::Stalled => s.stalled += 1,
            SiteVerdict::Refused => s.refused += 1,
            SiteVerdict::Unreachable => s.unreachable += 1,
            SiteVerdict::Error => s.error += 1,
        }
    }
    s
}

/// 报告里的 URL 快照（站表改了也能对上当时打的地址）
pub fn report_markdown(run: &SiteRun, locale: Locale) -> String {
    crate::i18n::report::render_sites(run, locale)
}

// ---------------------------------------------------------------- 单测

// ---------------------------------------------------------------- 探测执行（W2/D-11）
//
// 探测原本只存在于 `src-tauri/src/net_sites.rs`（GUI 专属）。MCP `net_sites` 与
// `prune sites` 也需要同一套执行逻辑，所以下沉到 core：**同一份 build_curl_args +
// judge + 文案**，GUI / CLI / MCP 三方不可能量出不同结果。
// GUI 侧的 `run_all` 仍保留进度事件与取消（那是 UI 关注点），单站探测改为调用这里。

/// 默认 UA 前缀。实测：默认 curl UA 下 github 会呈 stalled（ttfb=0），
/// 明示身份后同分钟即正常返回——UA 是口径的一部分，不是可选装饰。
pub const UA_PREFIX: &str = "PruneSiteProbe/";

/// 本次构建的 UA（带版本号，便于对方站点识别来源）
pub fn user_agent() -> String {
    format!("{}{}", UA_PREFIX, crate::version())
}

/// 跑一次 curl 并判定。任何「机制层」失败都落成 `verdict = Error` + note，
/// 绝不编造读数（方案 §12）。
pub fn probe(site: &Site, at: i64, locale: Locale) -> SiteProbe {
    use std::process::{Command, Stdio};

    let t = err_text(locale);
    let args = build_curl_args(site, &user_agent());
    let out = match Command::new("curl")
        .args(&args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return error_probe(
                site,
                at,
                fill(t.sites_curl_spawn, &[("err", &e.to_string())]),
            )
        }
    };
    let exit = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let note = detail_note(exit, &stderr);
    match parse_curl_output(&stdout) {
        Ok(raw) => judge(site, &raw, exit, note, at),
        Err(e) => {
            // -w 行都没拿到：curl 连 URL 都没解析成功（如 exit 3）
            let why = match note {
                Some(n) => format!("{e}; {n}"),
                None => e,
            };
            error_probe(site, at, why)
        }
    }
}

/// 一整轮：4 个 worker 抢索引，结果按**配置顺序**回填（调用方不需要重排）。
///
/// 无进度事件、无取消——那是 GUI 的关注点（`src-tauri/src/net_sites.rs` 的
/// `run_all`）。CLI / MCP 要的是「一次调用一个结果」，中断由进程信号接管。
pub fn run(sites: &[Site], at: i64, locale: Locale) -> SiteRun {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<SiteProbe>>> = Mutex::new(vec![None; sites.len()]);
    let workers = SITE_CONCURRENCY.min(sites.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let slots = &slots;
            let next = &next;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= sites.len() {
                    return;
                }
                let probe = probe(&sites[i], at, locale);
                if let Ok(mut s) = slots.lock() {
                    s[i] = Some(probe);
                }
            });
        }
    });

    let results = slots
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect();
    SiteRun {
        at,
        results,
        cancelled: false,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

/// curl 的失败原文 → 一行短说明。note 是**技术细节**（英文原样），
/// 用户看到的结论来自 `verdict` + `stage` 的本地化文案
fn detail_note(exit: i32, stderr: &str) -> Option<String> {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(160).collect::<String>());
    match (exit, line) {
        (0, None) => None,
        (0, Some(l)) => Some(l),
        (e, Some(l)) => Some(format!("curl exit {e}: {l}")),
        (e, None) => Some(format!("curl exit {e}")),
    }
}

fn error_probe(site: &Site, at: i64, note: String) -> SiteProbe {
    SiteProbe {
        id: site.id.clone(),
        name: site.name.clone(),
        url: site.url.clone(),
        at,
        dns_ms: None,
        tcp_ms: None,
        tls_ms: None,
        ttfb_ms: None,
        total_ms: None,
        bytes: 0,
        http_code: None,
        remote_ip: None,
        transfer_bps: None,
        verdict: SiteVerdict::Error,
        stage: None,
        note: Some(note),
        curl_exit: None,
    }
}

/// 站点表来源：设置文件里的 `netSites`（用户自定义）→ 校验 → 失败回落默认。
///
/// 与 GUI 的 `load_sites` 同语义，返回值第二项 = 「是否回落了默认」，
/// 调用方必须把这件事说出来（静默换掉用户配置是最坏的失败方式）。
pub fn sites_from_settings(locale: Locale) -> (Vec<Site>, bool) {
    let custom = crate::config::get_value("netSites")
        .and_then(|v| serde_json::from_value::<Vec<Site>>(v).ok())
        .filter(|s| !s.is_empty());
    match custom {
        Some(list) => match validate_sites(&list, locale) {
            Ok(ok) => (ok, false),
            Err(_) => (default_sites(), true),
        },
        None => (default_sites(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(url: &str) -> Site {
        Site {
            id: slug(url),
            name: display_name(url),
            url: url.into(),
        }
    }

    fn raw(dns: Option<f64>, tcp: Option<f64>, tls: Option<f64>, ttfb: Option<f64>) -> RawTiming {
        RawTiming {
            dns_ms: dns,
            tcp_ms: tcp,
            tls_ms: tls,
            ttfb_ms: ttfb,
            total_ms: ttfb.map(|t| t + 500.0),
            bytes: 400_000,
            http_code: Some(200),
            remote_ip: Some("1.2.3.4".into()),
        }
    }

    // ---- 默认站表与校验

    #[test]
    fn default_sites_are_ten_and_valid() {
        let d = default_sites();
        assert_eq!(d.len(), 10);
        let ok = validate_sites(&d, Locale::Zh).expect("默认站表必须自洽");
        assert_eq!(ok.len(), 10);
        // id 不重复、name 非空（去 www. 之后是纯 host）
        let mut ids: Vec<&str> = ok.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 10);
        assert_eq!(ok[0].name, "google.com");
        assert!(ok.iter().all(|s| s.url.starts_with("https://")));
    }

    #[test]
    fn validate_rejects_bad_urls() {
        let bad = |url: &str| validate_sites(&[site(url)], Locale::Zh).unwrap_err();
        assert!(bad("ftp://example.com/").contains("ftp://example.com"));
        assert!(bad("file:///etc/passwd").contains("http"));
        assert!(bad("https://u:p@example.com/").contains("凭据"));
        assert!(bad("https://exa mple.com/").contains("空白"));
        assert!(bad("https:///path").contains("主机"));
        // 空列表合法（空态页），不是错误
        assert!(validate_sites(&[], Locale::Zh).unwrap().is_empty());
    }

    #[test]
    fn validate_normalizes_and_dedupes() {
        let a = Site {
            id: String::new(),
            name: "  ".into(),
            url: "  https://example.com/x  ".into(),
        };
        let b = Site {
            id: "dup".into(),
            name: "同址".into(),
            url: "https://EXAMPLE.com/x".into(),
        };
        let out = validate_sites(&[a, b], Locale::Zh).unwrap();
        assert_eq!(out.len(), 1, "同一 URL（大小写无关）只留一条");
        assert_eq!(out[0].url, "https://example.com/x");
        assert_eq!(out[0].name, "example.com", "空名回落 host");
        assert!(!out[0].id.is_empty(), "空 id 自动生成");
    }

    #[test]
    fn validate_caps_count_and_name() {
        let many: Vec<Site> = (0..=SITE_MAX_COUNT)
            .map(|i| site(&format!("https://s{i}.example.com/")))
            .collect();
        assert!(validate_sites(&many, Locale::Zh).is_err());
        let long = Site {
            id: "x".into(),
            name: "字".repeat(SITE_NAME_MAX_CHARS + 1),
            url: "https://example.com/".into(),
        };
        assert!(validate_sites(&[long], Locale::Zh)
            .unwrap_err()
            .contains("字符"));
    }

    #[test]
    fn slug_is_stable_and_short() {
        assert_eq!(slug("https://a.com/"), slug("https://a.com/"));
        assert_ne!(slug("https://a.com/"), slug("https://b.com/"));
        assert_eq!(slug("https://a.com/").len(), 8);
    }

    // ---- 参数纪律

    #[test]
    fn curl_args_follow_discipline() {
        let s = site("https://example.com/");
        let a = build_curl_args(&s, "PruneSiteProbe/0.11.0");
        assert_eq!(a[0], "-q", "-q 必须在首位，否则 .curlrc 会改掉一切");
        assert!(a.iter().any(|v| v == "--noproxy"));
        assert!(a.windows(2).any(|w| w[0] == "--noproxy" && w[1] == "*"));
        assert!(!a.iter().any(|v| v == "-L"), "不跟随重定向");
        assert!(!a.iter().any(|v| v == "--compressed"), "字节数才可比");
        assert!(a.windows(2).any(|w| w[0] == "--max-time" && w[1] == "8"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--range" && w[1] == "0-2097151"));
        assert_eq!(a.last().map(String::as_str), Some("https://example.com/"));
    }

    // ---- 输出解析

    #[test]
    fn parse_full_line() {
        let out = "0.014633\t0.029911\t0.078835\t0.125823\t0.335247\t256049\t200\t115.231.32.12\n";
        let r = parse_curl_output(out).unwrap();
        // 浮点换算有尾差（0.014633 * 1000 = 14.633000000000001），按容差比
        let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-6;
        assert!(near(r.dns_ms, 14.633));
        assert!(near(r.tcp_ms, 29.911));
        assert!(near(r.ttfb_ms, 125.823));
        assert_eq!(r.bytes, 256_049);
        assert_eq!(r.http_code, Some(200));
        assert_eq!(r.remote_ip.as_deref(), Some("115.231.32.12"));
    }

    /// 回归（2026-11-15 真机）：TCP 超时时 `%{remote_ip}` 是空串，行尾是 `\t\n`。
    /// 曾经的 `trim_end()` 把空字段和分隔符一起削掉 → 7 段 → 整站误报「解析失败」。
    /// 这条原文是 macOS `curl 8.7.1` 打 google.com 的真实输出（od 确认）。
    #[test]
    fn parse_keeps_empty_last_field() {
        let out = "0.014257\t0.000000\t0.000000\t0.000000\t4.002144\t0\t000\t\n";
        let r = parse_curl_output(out).expect("空 remote_ip 不是解析失败");
        assert!(r.dns_ms.is_some());
        assert_eq!(r.tcp_ms, None);
        assert!(r.total_ms.is_some());
        assert_eq!(r.bytes, 0);
        assert_eq!(r.http_code, None);
        assert_eq!(r.remote_ip, None, "空串 → None，不是 Some(\"\")");
        // 判出来必须是「TCP 不通」，而不是 error
        let p = judge(&site("https://www.google.com/"), &r, 28, None, 0);
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Unreachable, Some(FailStage::Tcp))
        );
    }

    #[test]
    fn parse_tolerates_noise_and_zero_fields() {
        // 前面混进一行非数字日志：取最后一个以数字开头的行
        let out =
            "curl: (28) timed out\n0.013775\t0.122243\t0.256874\t0.000000\t10.002012\t0\t000\t-\n";
        let r = parse_curl_output(out).unwrap();
        assert_eq!(r.ttfb_ms, None, "0 是「没走到」，不是 0 ms");
        assert_eq!(r.http_code, None, "000 不是状态码");
        assert_eq!(r.remote_ip, None);
        assert_eq!(r.bytes, 0);
    }

    #[test]
    fn parse_rejects_short_or_empty() {
        assert!(parse_curl_output("").is_err());
        assert!(
            parse_curl_output("1\t2\t3\n").is_err(),
            "段数不足必须报错，不猜"
        );
    }

    // ---- 判定：五种形态 + 四个慢阈值

    #[test]
    fn judge_unreachable_dns() {
        let p = judge(
            &site("https://x.com/"),
            &raw(None, None, None, None),
            6,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Unreachable, Some(FailStage::Dns))
        );
    }

    #[test]
    fn judge_unreachable_tcp() {
        // 实测 google.com：DNS 通、TCP 0
        let p = judge(
            &site("https://www.google.com/"),
            &raw(Some(15.7), None, None, None),
            28,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Unreachable, Some(FailStage::Tcp))
        );
    }

    #[test]
    fn judge_unreachable_tls() {
        // 实测 x.com：TCP 通、TLS 0
        let p = judge(
            &site("https://x.com/"),
            &raw(Some(14.5), Some(161.0), None, None),
            28,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Unreachable, Some(FailStage::Tls))
        );
    }

    #[test]
    fn judge_stalled_after_handshake() {
        // 实测 github.com：握手全过，首字节 0
        let p = judge(
            &site("https://github.com/"),
            &raw(Some(13.8), Some(122.2), Some(256.9), None),
            28,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Stalled, Some(FailStage::Ttfb))
        );
    }

    #[test]
    fn judge_refused_is_not_failure() {
        // 实测 openai.com：403 但链路全通
        let mut r = raw(Some(18.0), Some(86.2), Some(160.7), Some(249.5));
        r.http_code = Some(403);
        r.bytes = 9_626;
        let p = judge(&site("https://openai.com/"), &r, 0, None, 1);
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Refused, Some(FailStage::Http))
        );
        assert_eq!(p.transfer_bps, None, "9 KB 不够样本");
    }

    #[test]
    fn judge_ok_when_all_fast() {
        let p = judge(
            &site("https://www.apple.com/"),
            &raw(Some(14.6), Some(29.9), Some(78.8), Some(125.8)),
            0,
            None,
            1,
        );
        assert_eq!(p.verdict, SiteVerdict::Ok);
        assert_eq!(p.stage, None);
    }

    #[test]
    fn judge_slow_stages_in_order() {
        // TCP 慢优先于其它
        let p = judge(
            &site("https://a.com/"),
            &raw(Some(10.0), Some(500.0), Some(600.0), Some(700.0)),
            0,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Slow, Some(FailStage::Tcp))
        );

        // TLS 增量慢（600−200 = 400 不算超，401 才算）
        let p = judge(
            &site("https://a.com/"),
            &raw(Some(10.0), Some(200.0), Some(601.0), Some(700.0)),
            0,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Slow, Some(FailStage::Tls))
        );

        // 首字节慢
        let p = judge(
            &site("https://a.com/"),
            &raw(Some(10.0), Some(40.0), Some(90.0), Some(1_500.0)),
            0,
            None,
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Slow, Some(FailStage::Ttfb))
        );

        // 传输慢：250 KB / 3 s ≈ 83 KB/s（样本够、速率不足）
        let mut r = raw(Some(10.0), Some(40.0), Some(90.0), Some(100.0));
        r.total_ms = Some(3_100.0);
        r.bytes = 250_000;
        let p = judge(&site("https://a.com/"), &r, 0, None, 1);
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Slow, Some(FailStage::Transfer))
        );
    }

    #[test]
    fn judge_partial_timeout_with_bytes_is_slow() {
        // 实测 --limit-rate 2 s：退出码 28，但收下 206 646 B
        let mut r = raw(Some(10.0), Some(40.0), Some(90.0), Some(570.0));
        r.total_ms = Some(2_000.0);
        r.bytes = 206_646;
        let p = judge(
            &site("https://a.com/"),
            &r,
            28,
            Some("curl exit 28".into()),
            1,
        );
        assert_eq!(
            (p.verdict, p.stage),
            (SiteVerdict::Slow, Some(FailStage::Transfer))
        );
        assert!(p.transfer_bps.is_some_and(|v| v > 100_000.0));
    }

    #[test]
    fn http_site_needs_no_tls_stage() {
        let mut r = raw(Some(5.0), Some(20.0), None, Some(60.0));
        r.http_code = Some(200);
        let p = judge(&site("http://localhost:8080/"), &r, 0, None, 1);
        assert_eq!(
            p.verdict,
            SiteVerdict::Ok,
            "http 站没有 TLS 阶段，不能判 unreachable"
        );
        assert_eq!(p.tls_ms, None);
    }

    // ---- 速率门槛

    #[test]
    fn transfer_rate_requires_enough_sample() {
        // 256 KB / 209 ms → 数得出来，但门槛卡的是 300 ms
        assert_eq!(transfer_bps(256_049, Some(335.0), Some(126.0)), None);
        // 字节不足
        assert_eq!(transfer_bps(9_626, Some(1_000.0), Some(250.0)), None);
        // 样本够：5 MB / (6.7 s − 0.6 s)
        let bps = transfer_bps(5_000_000, Some(6_700.0), Some(600.0)).unwrap();
        assert!((bps - 819_672.0).abs() < 1.0, "{bps}");
        // 缺 total / ttfb 一律 None
        assert_eq!(transfer_bps(5_000_000, None, Some(600.0)), None);
        assert_eq!(transfer_bps(5_000_000, Some(6_700.0), None), None);
    }

    // ---- 汇总

    /// 真机冒烟（`cargo test -p prune-core -- --ignored live_probe`，需要外网）：
    /// 只断言「拿到了阶段读数、没落到 Error」，不依赖某个站是否可达。
    #[test]
    #[ignore]
    fn live_probe_smoke() {
        let s = Site {
            id: "apple".into(),
            name: "apple.com".into(),
            url: "https://www.apple.com/".into(),
        };
        let args = build_curl_args(&s, "PruneSiteProbe/test");
        let out = std::process::Command::new("curl")
            .args(&args)
            .output()
            .expect("curl 必须存在");
        let exit = out.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let raw = parse_curl_output(&stdout).expect("write-out line missing");
        let p = judge(&s, &raw, exit, None, 0);
        eprintln!("{p:?}");
        assert!(p.dns_ms.is_some(), "DNS 阶段应当有读数");
        assert_ne!(p.verdict, SiteVerdict::Error);
    }

    #[test]
    fn summarize_counts_every_verdict() {
        let mk = |v| SiteProbe {
            id: "x".into(),
            name: "x".into(),
            url: "https://x.com/".into(),
            at: 0,
            dns_ms: None,
            tcp_ms: None,
            tls_ms: None,
            ttfb_ms: None,
            total_ms: None,
            bytes: 0,
            http_code: None,
            remote_ip: None,
            transfer_bps: None,
            verdict: v,
            stage: None,
            note: None,
            curl_exit: None,
        };
        let s = summarize(&[
            mk(SiteVerdict::Ok),
            mk(SiteVerdict::Slow),
            mk(SiteVerdict::Stalled),
            mk(SiteVerdict::Refused),
            mk(SiteVerdict::Unreachable),
            mk(SiteVerdict::Error),
        ]);
        assert_eq!(
            (
                s.ok,
                s.slow,
                s.stalled,
                s.refused,
                s.unreachable,
                s.error,
                s.total
            ),
            (1, 1, 1, 1, 1, 1, 6)
        );
    }

    // ---------------------------------------------------------- 执行层（W2/D-11）

    /// 打一个**本机必然拒连**的端口：验证 `run` 真的调用 curl、按配置顺序回填、
    /// 且失败落成可解释的 verdict，而不是 panic 或编造读数。
    /// 不发外部网络请求（CI 与离线环境都能跑）。
    #[test]
    fn run_probes_local_refused_port_without_network() {
        let sites = vec![Site {
            id: "self".into(),
            name: "self".into(),
            url: "http://127.0.0.1:1/".into(),
        }];
        let run = super::run(&sites, 1_800_000_000, Locale::En);
        assert_eq!(run.results.len(), 1, "按配置顺序回填，一站一条");
        assert!(!run.cancelled);
        let p = &run.results[0];
        assert_eq!(p.id, "self");
        assert_eq!(p.at, 1_800_000_000);
        // 拒连 → 链路层没打通（TCP），不是 ok
        assert_ne!(p.verdict, SiteVerdict::Ok);
        assert!(p.curl_exit.is_some(), "必须记录 curl 退出码，便于解释");
    }

    #[test]
    fn user_agent_carries_version() {
        let ua = super::user_agent();
        assert!(ua.starts_with(UA_PREFIX));
        assert!(ua.contains(crate::version()));
    }
}
