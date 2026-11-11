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
pub fn render(diag: &NetDiag, speed: Option<&SpeedResult>, locale: Locale) -> String {
    render_with(diag, speed, table(locale))
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
    /// 漏一个词就是「界面英文、报告中文」的穿帮
    #[test]
    fn english_report_has_no_cjk() {
        let r = render(&sample_diag(), None, Locale::En);
        assert!(!has_cjk(&r), "英文报告残留中文：\n{r}");
        assert!(r.contains("# Prune network diagnostic report"));
        assert!(r.contains("## Layer probes"));
        assert!(r.contains("## Verdict"));
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
}
