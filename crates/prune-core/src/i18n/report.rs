//! 诊断报告的 Markdown 渲染（docs/i18n-spec.md §4.6）。
//!
//! 报告是唯一会被**复制出去、离开应用**的文本：用户把它粘给 AI 或同事，
//! 那边看不到 UI，只有这段 Markdown。所以它必须自己说清楚口径（连接耗时
//! 不是 ping、DNS 含缓存、portal 不走系统代理），也必须随语言走 ——
//! 一份中文报告配英文界面，等于把「这 App 没做完」写在额头上。
//!
//! 布局只写一次（[`render_with`]），中英各给一张表。行序、空行、表格分隔
//! 线全在代码里，不在文案里 —— 翻译只改字，不许动结构。

use crate::locale::Locale;
use crate::net::diag::{CaptiveState, DiagStatus, LayerProbe, NetDiag, SpeedResult};
use crate::net::sites::{FailStage, SiteProbe, SiteRun, SiteVerdict};

use super::fill;

/// 一份语言的全部静态片段。字段全是 `&'static str`，漏一个编译失败
struct T {
    title: &'static str,
    time: &'static str,
    wifi: &'static str,
    platform: &'static str,
    h2_probes: &'static str,
    round_failed: &'static str,
    table_head: &'static str,
    dns_name: &'static str,
    dns_extra: &'static str,
    lan_name: &'static str,
    lan_extra: &'static str,
    wan_name: &'static str,
    jitter_prefix: &'static str,
    failed_cell: &'static str,
    not_probed: &'static str,
    /// 一格里拼「说明；后端 note」时的连接符（中文全角，英文分号 + 空格）
    join: &'static str,
    captive_line: &'static str,
    captive_none: &'static str,
    captive_suspected: &'static str,
    captive_unknown: &'static str,
    h2_verdict: &'static str,
    verdict_body: &'static str,
    h2_speed: &'static str,
    speed_body: &'static str,
    footnote: &'static str,
    other_platform: &'static str,
    /// `failed` 但 note 为空的兜底词（理论上不发生，落一句比落 `—` 可读）
    unknown_reason: &'static str,
}

const ZH: T = T {
    title: "# Prune 网络诊断报告\n\n",
    time: "- 时间：<t:{ts}>（Unix 秒）\n",
    wifi: "- Wi-Fi：{ssid}\n",
    platform: "- 平台：{p}\n",
    h2_probes: "\n## 分层探测\n\n",
    round_failed: "整轮失败：{note}\n（探测机制本身失败，与「连不上路由器」不同）\n",
    table_head: "| 层 | 连接耗时 | 说明 | 原始样本 |\n|---|---|---|---|\n",
    dns_name: "DNS（解析 www.apple.com）",
    dns_extra: "走系统解析器，含缓存",
    lan_name: "网关 LAN（{gw}:80）",
    lan_extra: "TCP connect",
    // 回退目标写进行名：目标本身就是事实，不能静默换
    wan_name: "出口 WAN（首选 1.1.1.1:443，被拦回退 223.5.5.5:443）",
    jitter_prefix: "抖动 {j}",
    failed_cell: "探测失败",
    not_probed: "未探测",
    join: "；",
    captive_line: "\n- captive portal：{v}\n",
    captive_none: "正常公网",
    captive_suspected: "疑似需要网页登录（酒店/机场 Wi-Fi）",
    captive_unknown: "探测失败，不下结论",
    h2_verdict: "\n## 结论\n\n",
    verdict_body: "**{label}**\n\n- 建议：{advice}\n- 规则：{rule}\n- 注：未含 Wi-Fi 信号层（v2 提供）\n",
    h2_speed: "\n## 测速（macOS networkQuality）\n\n",
    speed_body: "- 下行 {down} Mbps / 上行 {up} Mbps\n- 负载下响应度 RPM {rpm}（{level}）\n- base_rtt {rtt} ms\n",
    footnote: "\n> 口径：连接耗时（TCP connect）不是 ICMP ping；DNS 走系统解析器含缓存，低值不代表无 DNS 问题；portal 探测不经系统代理，结果可能与浏览器体验不一致。\n",
    other_platform: "其他",
    unknown_reason: "未知原因",
};

const EN: T = T {
    title: "# Prune network diagnostic report\n\n",
    time: "- Time: <t:{ts}> (Unix seconds)\n",
    wifi: "- Wi-Fi: {ssid}\n",
    platform: "- Platform: {p}\n",
    h2_probes: "\n## Layer probes\n\n",
    round_failed: "Round failed: {note}\n(the probe mechanism itself failed — this is not \"can't reach the router\")\n",
    table_head: "| Layer | Connect time | Notes | Raw samples |\n|---|---|---|---|\n",
    dns_name: "DNS (resolve www.apple.com)",
    dns_extra: "System resolver, cache included",
    lan_name: "Gateway LAN ({gw}:80)",
    lan_extra: "TCP connect",
    wan_name: "WAN (1.1.1.1:443 first, falls back to 223.5.5.5:443 if blocked)",
    jitter_prefix: "jitter {j}",
    failed_cell: "probe failed",
    not_probed: "not probed",
    join: "; ",
    captive_line: "\n- Captive portal: {v}\n",
    captive_none: "Public internet",
    captive_suspected: "Suspected sign-in page (hotel or airport Wi-Fi)",
    captive_unknown: "Probe failed — no conclusion",
    h2_verdict: "\n## Verdict\n\n",
    verdict_body: "**{label}**\n\n- Advice: {advice}\n- Rule: {rule}\n- Note: Wi-Fi signal layer not included (v2)\n",
    h2_speed: "\n## Speed test (macOS networkQuality)\n\n",
    speed_body: "- Down {down} Mbps / Up {up} Mbps\n- Responsiveness RPM {rpm} ({level})\n- base_rtt {rtt} ms\n",
    footnote: "\n> Scope: connect time (TCP connect), not ICMP ping; DNS goes through the system resolver including cache, so a low value does not rule out DNS problems; the portal probe bypasses the system proxy, so results may differ from browser behaviour.\n",
    other_platform: "Other",
    unknown_reason: "unknown reason",
};

fn table(l: Locale) -> &'static T {
    match l {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    }
}

/// 渲染一份完整报告。`speed` 为 None 时不输出测速段（没跑过 ≠ 0 Mbps）。
///
/// 结论文案在这里按**当前语言**重算，而不是直接用 `diag.verdict` 里存的那份：
/// 诊断要跑几秒，用户完全可能跑完中文诊断、切到英文再来复制报告 —— 存下来的
/// label/advice/rule 是中文，报告就成了中英混排。判定与语言无关
/// （[`crate::net::diag::verdict`] 的纯函数契约），同一份探测数据重算只换文案、
/// 不换结论，所以这样做不引入第二种判定。
///
/// `kind` 缺失（旧版本数据 / 手工构造）时无法重算，退回存储值 —— 宁可中英混排，
/// 也不要把整段结论删掉。
pub fn render(diag: &NetDiag, speed: Option<&SpeedResult>, locale: Locale) -> String {
    let d = with_localized_verdict(diag, locale);
    render_with(&d, speed, table(locale))
}

/// 用当前语言重算结论；算不出（无 verdict 或无 kind）时原样返回
fn with_localized_verdict(diag: &NetDiag, locale: Locale) -> NetDiag {
    if diag.verdict.as_ref().is_none_or(|v| v.kind.is_none()) {
        return diag.clone();
    }
    match crate::net::diag::verdict(
        diag.lan.as_ref(),
        diag.dns.as_ref(),
        diag.wan.as_ref(),
        diag.jitter_ms,
        diag.captive.unwrap_or(CaptiveState::None),
        locale,
    ) {
        Some(v) => NetDiag {
            verdict: Some(v),
            ..diag.clone()
        },
        None => diag.clone(),
    }
}

fn render_with(diag: &NetDiag, speed: Option<&SpeedResult>, t: &T) -> String {
    let mut r = String::from(t.title);
    r.push_str(&fill(t.time, &[("ts", &diag.at.to_string())]));
    if let Some(ssid) = &diag.ssid {
        r.push_str(&fill(t.wifi, &[("ssid", ssid)]));
    }
    let platform = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        t.other_platform
    };
    r.push_str(&fill(t.platform, &[("p", platform)]));
    r.push_str(t.h2_probes);

    if diag.status == DiagStatus::Failed {
        r.push_str(&fill(
            t.round_failed,
            &[("note", diag.note.as_deref().unwrap_or(t.unknown_reason))],
        ));
        return r;
    }

    // 一行的四种形态：有读数 / 探测失败 / 未探测。括号里是「后端 note」，
    // 语言无关（note 自己也已本地化）
    let row = |name: &str, p: &Option<LayerProbe>, extra: &str| -> String {
        match p {
            Some(p) => match p.median_ms {
                Some(ms) => {
                    let notes = match p.note.as_deref() {
                        Some(n) => format!("{extra}{}{n}", t.join),
                        None => extra.to_string(),
                    };
                    let samples = p
                        .samples
                        .iter()
                        .map(|v| format!("{v:.0}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("| {name} | {ms:.1} ms | {notes} | {samples} |\n")
                }
                None => format!(
                    "| {name} | {} | {} | — |\n",
                    t.failed_cell,
                    p.note.as_deref().unwrap_or("")
                ),
            },
            None => format!("| {name} | {} | {extra} | — |\n", t.not_probed),
        }
    };

    r.push_str(t.table_head);
    r.push_str(&row(t.dns_name, &diag.dns, t.dns_extra));
    r.push_str(&row(
        &fill(
            t.lan_name,
            &[("gw", diag.gateway.as_deref().unwrap_or("?"))],
        ),
        &diag.lan,
        t.lan_extra,
    ));
    let jitter = fill(
        t.jitter_prefix,
        &[(
            "j",
            &diag
                .jitter_ms
                .map(|j| format!("±{j:.1} ms"))
                .unwrap_or_else(|| "—".into()),
        )],
    );
    r.push_str(&row(t.wan_name, &diag.wan, &jitter));

    if let Some(c) = diag.captive {
        let v = match c {
            CaptiveState::None => t.captive_none,
            CaptiveState::Suspected => t.captive_suspected,
            CaptiveState::Unknown => t.captive_unknown,
        };
        r.push_str(&fill(t.captive_line, &[("v", v)]));
    }

    if let Some(v) = &diag.verdict {
        r.push_str(t.h2_verdict);
        r.push_str(&fill(
            t.verdict_body,
            &[
                ("label", &v.label),
                ("advice", &v.advice),
                ("rule", &v.rule),
            ],
        ));
    }

    if let Some(s) = speed {
        r.push_str(t.h2_speed);
        r.push_str(&fill(
            t.speed_body,
            &[
                ("down", &format!("{:.1}", s.down_bps / 1_000_000.0)),
                ("up", &format!("{:.1}", s.up_bps / 1_000_000.0)),
                ("rpm", &format!("{:.0}", s.rpm)),
                // rpm_level 是 Apple 原文（Poor/Low/Medium/Good），不译
                ("level", &s.rpm_level),
                ("rtt", &format!("{:.0}", s.base_rtt_ms)),
            ],
        ));
    }

    r.push_str(t.footnote);
    r
}

// ---------------------------------------------------------------- 站点检测报告（F15）

/// 站点报告的语言表。与诊断报告同一纪律：布局在代码里，翻译只改字
struct SiteT {
    title: &'static str,
    time: &'static str,
    platform: &'static str,
    scope: &'static str,
    h2_table: &'static str,
    /// 表头整行（列固定 10 个，翻译不动列数）
    table_head: &'static str,
    ok: &'static str,
    slow: &'static str,
    stalled: &'static str,
    refused: &'static str,
    unreachable: &'static str,
    error: &'static str,
    cancelled: &'static str,
    dash: &'static str,
    h2_notes: &'static str,
    notes: &'static str,
    footnote: &'static str,
}

const SITES_ZH: SiteT = SiteT {
    title: "# Prune 站点检测报告\n\n",
    time: "- 时间：<t:{ts}>（Unix 秒）\n",
    platform: "- 平台：{p}\n",
    scope: "- 范围：{n} 个站点 · 直连（不走系统代理）· 单次采样\n",
    h2_table: "\n## 站点读数\n\n",
    table_head: "| 站点 | 状态 | DNS | TCP | TLS | 首字节 | 速率 | 样本 | HTTP | 出口 IP |\n|---|---|---|---|---|---|---|---|---|---|\n",
    ok: "正常",
    slow: "偏慢",
    stalled: "连上但无响应",
    refused: "站点拒绝",
    unreachable: "不可达",
    error: "探测出错",
    cancelled: "本轮被取消，以下为已完成的站点",
    dash: "—",
    h2_notes: "\n## 说明\n\n",
    notes: "- 状态按断点判定：DNS / TCP / TLS / 首字节 / 传输，逐段短路；`无响应` 指握手完成但首字节始终不到。\n- `站点拒绝` 表示链路全通、站点返回 4xx/5xx（常见于反爬或区域限制），**不是连不上**。\n- 速率为「首字节之后」的传输速率，样本 < 200 KB 或传输 < 300 ms 时不给数字（只写样本量）。\n",
    footnote: "\n> 口径：直连、不走系统或环境代理；只请求首页文档（≤ 2 MiB，不含图片/脚本等子资源）；不跟随重定向。以上读数用于同站前后对比，不是浏览体验的完整代理。\n",
};

const SITES_EN: SiteT = SiteT {
    title: "# Prune site reachability report\n\n",
    time: "- Time: <t:{ts}> (Unix seconds)\n",
    platform: "- Platform: {p}\n",
    scope: "- Scope: {n} sites · direct connection (system proxy bypassed) · single sample\n",
    h2_table: "\n## Per-site readings\n\n",
    table_head: "| Site | State | DNS | TCP | TLS | First byte | Rate | Sample | HTTP | Remote IP |\n|---|---|---|---|---|---|---|---|---|---|\n",
    ok: "OK",
    slow: "Slow",
    stalled: "No response",
    refused: "Refused",
    unreachable: "Unreachable",
    error: "Probe error",
    cancelled: "This round was cancelled; only completed sites are listed",
    dash: "—",
    h2_notes: "\n## Notes\n\n",
    notes: "- The state follows the first failing stage: DNS / TCP / TLS / first byte / transfer. `No response` means the handshake completed but the first byte never arrived.\n- `Refused` means the path is fully open and the site answered 4xx/5xx (bot protection or regional limits) — it is **not** an unreachable host.\n- Rate is the transfer phase after the first byte; no number is given when the sample is under 200 KB or shorter than 300 ms (only the sample size is reported).\n",
    footnote: "\n> Scope: direct connection, system or environment proxies bypassed; only the home document is requested (≤ 2 MiB, no images/scripts); redirects are not followed. Use these numbers for before/after comparison of the same site, not as a complete proxy for browsing experience.\n",
};

fn site_table(l: Locale) -> &'static SiteT {
    match l {
        Locale::Zh => &SITES_ZH,
        Locale::En => &SITES_EN,
    }
}

fn fmt_ms(v: Option<f64>, dash: &str) -> String {
    match v {
        Some(v) => format!("{v:.0} ms"),
        None => dash.to_string(),
    }
}

/// 增量：`later − earlier`，任一缺失就 dash（不用 0 兜底）
fn fmt_delta(later: Option<f64>, earlier: Option<f64>, dash: &str) -> String {
    match (later, earlier) {
        (Some(l), Some(e)) if l - e > 0.0 => format!("{:.0} ms", l - e),
        _ => dash.to_string(),
    }
}

fn fmt_rate(bps: Option<f64>, dash: &str) -> String {
    match bps {
        Some(v) if v >= 1_000_000.0 => format!("{:.1} MB/s", v / 1_000_000.0),
        Some(v) => format!("{:.0} KB/s", v / 1_000.0),
        None => dash.to_string(),
    }
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0} KB", n as f64 / 1_000.0)
    } else {
        format!("{n} B")
    }
}

/// 站点的状态词（报告用；UI 的文案在 TS catalog，两处措辞刻意保持一致）
fn site_state_label(v: SiteVerdict, t: &SiteT) -> &'static str {
    match v {
        SiteVerdict::Ok => t.ok,
        SiteVerdict::Slow => t.slow,
        SiteVerdict::Stalled => t.stalled,
        SiteVerdict::Refused => t.refused,
        SiteVerdict::Unreachable => t.unreachable,
        SiteVerdict::Error => t.error,
    }
}

/// 站点检测报告的 Markdown。站点名 + URL 都写：站表随后可能被改，
/// 报告必须记录「当时打的到底是哪个地址」。
pub fn render_sites(run: &SiteRun, locale: Locale) -> String {
    let t = site_table(locale);
    let mut r = String::from(t.title);
    r.push_str(&fill(t.time, &[("ts", &run.at.to_string())]));
    let platform = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        site_platform_other(locale)
    };
    r.push_str(&fill(t.platform, &[("p", platform)]));
    r.push_str(&fill(t.scope, &[("n", &run.results.len().to_string())]));
    r.push_str(t.h2_table);
    r.push_str(t.table_head);
    for p in &run.results {
        r.push_str(&site_row(p, t));
        r.push('\n');
    }
    if run.cancelled {
        r.push_str(&format!("\n> {}\n", t.cancelled));
    }
    r.push_str(t.h2_notes);
    r.push_str(t.notes);
    r.push_str(t.footnote);
    r
}

fn site_platform_other(locale: Locale) -> &'static str {
    match locale {
        Locale::Zh => "其他",
        Locale::En => "Other",
    }
}

fn site_row(p: &SiteProbe, t: &SiteT) -> String {
    let d = t.dash;
    let mut state = site_state_label(p.verdict, t).to_string();
    // 断点写进状态列：报告读者看不到 UI 的着色
    if let Some(stage) = p.stage {
        let stage_word = match stage {
            FailStage::Dns => "DNS",
            FailStage::Tcp => "TCP",
            FailStage::Tls => "TLS",
            FailStage::Ttfb => "TTFB",
            FailStage::Transfer => "transfer",
            FailStage::Http => "HTTP",
        };
        state.push_str(&format!(" · {stage_word}"));
    }
    if let Some(note) = &p.note {
        // 括号用半角：note 是 curl 原文（英文），全角括号在英文报告里很扎眼
        state.push_str(&format!(" ({})", note.replace('|', "/")));
    }
    format!(
        "| {} `{}` | {state} | {} | {} | {} | {} | {} | {} | {} | {} |",
        p.name,
        p.url,
        fmt_ms(p.dns_ms, d),
        fmt_ms(p.tcp_ms, d),
        fmt_delta(p.tls_ms, p.tcp_ms, d),
        fmt_delta(p.ttfb_ms, p.tls_ms.or(p.tcp_ms), d),
        fmt_rate(p.transfer_bps, d),
        fmt_bytes(p.bytes),
        p.http_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| d.into()),
        p.remote_ip.as_deref().unwrap_or(d),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c))
    }

    fn sample_diag() -> NetDiag {
        NetDiag {
            at: 1_700_000_000,
            status: DiagStatus::Done,
            note: None,
            dns: Some(LayerProbe {
                median_ms: Some(28.4),
                samples: vec![30.0, 27.0],
                note: None,
            }),
            lan: Some(LayerProbe {
                median_ms: Some(3.2),
                samples: vec![3.0, 4.0],
                note: None,
            }),
            wan: Some(LayerProbe {
                median_ms: Some(31.0),
                samples: vec![30.0, 32.0],
                note: None,
            }),
            jitter_ms: Some(1.0),
            gateway: Some("192.168.1.1".into()),
            captive: Some(CaptiveState::None),
            verdict: Some(crate::net::diag::Verdict {
                label: "各层正常".into(),
                advice: "慢感可能来自对端服务本身".into(),
                rule: "所有层都在阈值内".into(),
                kind: Some(crate::net::diag::VerdictKind::AllGood),
            }),
            ssid: Some("HomeNet".into()),
        }
    }

    /// 英文报告里不允许出现一个汉字 —— 这份文本会被复制到别处，
    /// 漏一个词就是「界面英文、报告中文」的穿帮。
    ///
    /// fixture 里的 verdict 故意存**中文**：这模拟「中文下跑诊断、切英文后复制
    /// 报告」的真实路径。渲染时按当前语言重算结论，所以这条既验文案表，也验
    /// 重算这条链路本身。
    #[test]
    fn english_report_has_no_cjk() {
        let r = render(&sample_diag(), None, Locale::En);
        assert!(!has_cjk(&r), "英文报告残留中文：\n{r}");
        assert!(r.contains("# Prune network diagnostic report"));
        assert!(r.contains("## Layer probes"));
        assert!(r.contains("## Verdict"));
        assert!(r.contains("All layers normal"), "结论没跟着报告语言走");
    }

    /// 结论跟着**报告语言**走，而不是 diag 里存的那份：同一份数据，两种渲染
    #[test]
    fn verdict_follows_report_language() {
        let d = sample_diag(); // verdict 存的是中文（见上面 fixture 注释）
        assert!(render(&d, None, Locale::Zh).contains("各层正常"));
        assert!(render(&d, None, Locale::En).contains("All layers normal"));
        // 原始数据不被污染：重算只发生在渲染时，不改调用方的 diag
        assert_eq!(
            d.verdict.as_ref().map(|v| v.label.as_str()),
            Some("各层正常")
        );
    }

    /// 失败整轮的英文报告同样不能漏中文（这条分支容易忘，它提前 return）
    #[test]
    fn english_failed_report_has_no_cjk() {
        let mut d = sample_diag();
        d.status = DiagStatus::Failed;
        d.note = Some("No default gateway".into());
        d.verdict = None;
        let r = render(&d, None, Locale::En);
        assert!(!has_cjk(&r), "失败报告残留中文：\n{r}");
        assert!(r.contains("Round failed"));
    }

    /// 中文报告的行结构与历史输出保持一致：表格分隔线 + 三行探测
    #[test]
    fn chinese_report_keeps_table_shape() {
        let r = render(&sample_diag(), None, Locale::Zh);
        assert!(r.contains("| 层 | 连接耗时 | 说明 | 原始样本 |"));
        assert!(r.contains("DNS（解析 www.apple.com）"));
        assert!(r.contains("网关 LAN（192.168.1.1:80）"));
        assert!(r.contains("出口 WAN（首选 1.1.1.1:443，被拦回退 223.5.5.5:443）"));
    }

    /// 探测失败必须是「探测失败」而不是 0 ms
    #[test]
    fn failed_probe_renders_as_failed_not_zero() {
        let mut d = sample_diag();
        d.wan = Some(LayerProbe {
            median_ms: None,
            samples: vec![],
            note: Some("1.1.1.1:443 5 次全部失败".into()),
        });
        let zh = render(&d, None, Locale::Zh);
        assert!(zh.contains("探测失败"), "{zh}");
        let en = render(&d, None, Locale::En);
        assert!(en.contains("probe failed"), "{en}");
    }

    // ---- 站点检测报告（F15）

    fn probe(name: &str, url: &str, verdict: SiteVerdict, stage: Option<FailStage>) -> SiteProbe {
        SiteProbe {
            id: name.to_lowercase(),
            name: name.into(),
            url: url.into(),
            at: 1_700_000_000,
            dns_ms: Some(18.0),
            tcp_ms: Some(86.0),
            tls_ms: Some(160.0),
            ttfb_ms: Some(249.0),
            total_ms: Some(1_249.0),
            bytes: 900_000,
            http_code: Some(200),
            remote_ip: Some("1.2.3.4".into()),
            transfer_bps: Some(450_000.0),
            verdict,
            stage,
            note: None,
            curl_exit: Some(0),
        }
    }

    fn sample_sites() -> SiteRun {
        SiteRun {
            at: 1_700_000_000,
            results: vec![
                probe("apple.com", "https://www.apple.com/", SiteVerdict::Ok, None),
                probe(
                    "github.com",
                    "https://github.com/",
                    SiteVerdict::Stalled,
                    Some(FailStage::Ttfb),
                ),
                probe(
                    "openai.com",
                    "https://openai.com/",
                    SiteVerdict::Refused,
                    Some(FailStage::Http),
                ),
                probe(
                    "google.com",
                    "https://www.google.com/",
                    SiteVerdict::Unreachable,
                    Some(FailStage::Tcp),
                ),
            ],
            cancelled: false,
            elapsed_ms: 4_200.0,
        }
    }

    /// 站点报告同样不许漏中文（它也会被复制走）
    #[test]
    fn english_sites_report_has_no_cjk() {
        let r = render_sites(&sample_sites(), Locale::En);
        assert!(!has_cjk(&r), "英文站点报告残留中文：\n{r}");
        assert!(r.contains("# Prune site reachability report"));
        assert!(r.contains("No response · TTFB"));
        assert!(r.contains("Refused · HTTP"));
        assert!(r.contains("450 KB/s"));
        assert!(r.contains("900 KB"));
    }

    /// 中文报告：断点与技术细节都要落在状态列里，速率与样本量同时出现
    #[test]
    fn chinese_sites_report_keeps_shape() {
        let mut run = sample_sites();
        run.results[3].note = Some("curl exit 28".into());
        let r = render_sites(&run, Locale::Zh);
        assert!(r.contains("# Prune 站点检测报告"));
        assert!(
            r.contains("| 站点 | 状态 | DNS | TCP | TLS | 首字节 | 速率 | 样本 | HTTP | 出口 IP |")
        );
        assert!(r.contains("不可达 · TCP (curl exit 28)"), "{r}");
        assert!(r.contains("`https://www.google.com/`"), "URL 必须进报告");
    }

    /// 增量列：TLS 与首字节是差值，不是累计时刻（累计值会让人误读成「TLS 用了 160 ms」）
    #[test]
    fn sites_report_uses_deltas() {
        let r = render_sites(&sample_sites(), Locale::Zh);
        assert!(r.contains("| 74 ms |"), "TLS 增量应为 160−86：{r}");
        assert!(r.contains("| 89 ms |"), "首字节增量应为 249−160：{r}");
    }
}
