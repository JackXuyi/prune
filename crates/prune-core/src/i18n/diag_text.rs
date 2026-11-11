//! 诊断结论与探测说明的文案表（docs/i18n-spec.md §4.5）。
//!
//! 拆分理由：`net::diag` 是纯逻辑（判阈值、串探测），文案是另一件事。
//! 混在一起会让「加一个阈值」的 diff 里夹着两句英文翻译。
//!
//! 占位约定：`{v}` = 实测值（0 位小数），`{t}` = 阈值（0 位小数）。
//! 阈值数字由调用方从常量 format 出来 —— 文案里不写死 50 / 100，否则改了
//! `LAN_SLOW_MS` 而忘记改文案，界面上就会出现「阈值说 50、判定按 80」。

use crate::locale::Locale;

/// 无参数的三行文案（主因 / 建议 / 规则）
pub struct Line {
    pub label: &'static str,
    pub advice: &'static str,
    pub rule: &'static str,
}

/// 带占位的三行文案。`advice` 通常无占位，仍走同一个填充函数（无占位即原样）
pub struct ParamLine {
    pub label: &'static str,
    pub advice: &'static str,
    pub rule: &'static str,
}

/// 8 条结论（§3.3 优先级短路表）。
///
/// 字段与 [`crate::net::diag::VerdictKind`] 一一对应 —— 那条枚举多一个变体而
/// 这里没补字段时，`render` 的 match 会缺分支，编译期就能发现。
pub struct VerdictText {
    pub lan_down: Line,
    pub dns_fail: Line,
    pub captive: Line,
    pub lan_slow: ParamLine,
    pub wan_gap: ParamLine,
    pub jitter: ParamLine,
    pub dns_slow: ParamLine,
    pub all_good: Line,
}

pub const VERDICT_ZH: VerdictText = VerdictText {
    // 措辞是产品口径：「连不上路由器」比「网关不可达」更像人话
    lan_down: Line {
        label: "连不上路由器",
        advice: "检查 Wi-Fi / 网线是否连接",
        rule: "网关连接探测全部失败",
    },
    dns_fail: Line {
        label: "DNS 解析失败",
        advice: "网关通但域名解析不了，多为 DNS 配置或运营商 DNS 故障",
        rule: "DNS 探测全部失败",
    },
    captive: Line {
        label: "需要网页登录（酒店/机场 Wi-Fi）",
        advice: "打开浏览器完成认证",
        rule: "portal 探测未返回 Success",
    },
    lan_slow: ParamLine {
        label: "路由器响应慢（{v} ms）",
        advice: "重启路由器试试",
        rule: "连接耗时 > {t} ms",
    },
    wan_gap: ParamLine {
        label: "出口/运营商延迟高（差值 {v} ms）",
        advice: "你的宽带问题，Prune 无能为力，找 ISP",
        rule: "WAN − LAN > {t} ms",
    },
    jitter: ParamLine {
        label: "链路抖动大（±{v} ms）",
        advice: "信号弱 / 拥挤信道 / 运营商波动，多为 Wi-Fi 环境问题",
        rule: "抖动（MAD）> {t} ms",
    },
    dns_slow: ParamLine {
        label: "DNS 偏慢（{v} ms）",
        advice: "可在系统设置换 DNS 试试",
        rule: "解析耗时 > {t} ms",
    },
    all_good: Line {
        label: "各层正常",
        advice: "慢感可能来自对端服务本身",
        rule: "所有层都在阈值内",
    },
};

pub const VERDICT_EN: VerdictText = VerdictText {
    lan_down: Line {
        label: "Can't reach the router",
        advice: "Check the Wi-Fi or Ethernet connection",
        rule: "All gateway probes failed",
    },
    dns_fail: Line {
        label: "DNS resolution failed",
        advice:
            "The gateway responds but names don't resolve — usually DNS config or the ISP resolver",
        rule: "All DNS probes failed",
    },
    captive: Line {
        label: "Sign-in required (hotel or airport Wi-Fi)",
        advice: "Open a browser and complete sign-in",
        rule: "Portal probe did not return Success",
    },
    lan_slow: ParamLine {
        label: "Router responds slowly ({v} ms)",
        advice: "Try restarting the router",
        rule: "Connect time > {t} ms",
    },
    wan_gap: ParamLine {
        label: "High WAN/ISP latency (gap {v} ms)",
        advice: "This is your broadband, not Prune — contact your ISP",
        rule: "WAN − LAN > {t} ms",
    },
    jitter: ParamLine {
        label: "Link jitter is high (±{v} ms)",
        advice:
            "Weak signal, congested channel, or ISP fluctuation — usually the Wi-Fi environment",
        rule: "Jitter (MAD) > {t} ms",
    },
    dns_slow: ParamLine {
        label: "DNS is slow ({v} ms)",
        advice: "Try a different DNS in system settings",
        rule: "Resolve time > {t} ms",
    },
    all_good: Line {
        label: "All layers normal",
        advice: "The slowness may be the remote service itself",
        rule: "All layers within thresholds",
    },
};

pub fn verdict_text(l: Locale) -> &'static VerdictText {
    match l {
        Locale::Zh => &VERDICT_ZH,
        Locale::En => &VERDICT_EN,
    }
}

/// 探测失败 / 降级时写进 `LayerProbe.note` 与 `NetDiag.note` 的句子。
///
/// 这些 note 会被 UI 原样显示（LayerChart 的 warn 行、NetBoard 顶部的
/// 「不可读：…」），所以同样是用户可见文案，不能只译结论。
pub struct NoteText {
    /// 整轮 failed：拿不到默认网关
    pub no_gateway: &'static str,
    /// 单层失败：`{addr}` `{err}`
    pub lan_failed: &'static str,
    /// WAN 首选目标全失败，已回退
    pub wan_all_failed: &'static str,
    /// WAN 首选与回退都全失败
    pub wan_both_failed: &'static str,
    /// DNS 全失败
    pub dns_failed: &'static str,
    /// 兜底词：拿不到具体错误时
    pub unknown_err: &'static str,
    /// 兜底词：WAN 全失败且没有具体错误时（实测多为超时）
    pub timeout_err: &'static str,
    /// `TcpStream` 地址字符串解析失败（不是 DNS 失败）
    pub addr_parse: &'static str,
    /// DNS 探测总超时（getaddrinfo 不可中断，用手表兜）
    pub dns_timeout: &'static str,
}

pub const NOTE_ZH: NoteText = NoteText {
    no_gateway: "找不到默认网关，无法做分层探测",
    lan_failed: "{addr} 连接失败：{err}",
    wan_all_failed: "{addr} 5 次全部失败（{err}），疑似直连被网络拦截，回退 {fallback}",
    wan_both_failed:
        "{addr} 与回退 {fallback} 均 5 次全部失败：{err}（出口不通或被强拦，对照 LAN/DNS 读数定位）",
    dns_failed: "解析 {host} 全部失败：{err}",
    unknown_err: "未知错误",
    timeout_err: "连接超时",
    addr_parse: "地址解析失败 {addr}: {err}",
    dns_timeout: "{host}: DNS 探测超时（{dur}）",
};

pub const NOTE_EN: NoteText = NoteText {
    no_gateway: "No default gateway — layer probes can't run",
    lan_failed: "Connect to {addr} failed: {err}",
    wan_all_failed: "All 5 attempts to {addr} failed ({err}); direct access looks blocked, falling back to {fallback}",
    wan_both_failed: "All 5 attempts to both {addr} and the fallback {fallback} failed: {err} (WAN is down or blocked — compare the LAN/DNS readings)",
    dns_failed: "Resolving {host} failed on every attempt: {err}",
    unknown_err: "unknown error",
    timeout_err: "connect timeout",
    addr_parse: "Address parse failed for {addr}: {err}",
    dns_timeout: "{host}: DNS probe timed out ({dur})",
};

pub fn note_text(l: Locale) -> &'static NoteText {
    match l {
        Locale::Zh => &NOTE_ZH,
        Locale::En => &NOTE_EN,
    }
}

/// 8 条结论的中英对照都要能通过 `fill` 填出来，且英文侧不得残留中文。
/// 这类断言放在这里而不是 diag.rs：文案表自己出了问题，不该去改判定逻辑的测试。
#[cfg(test)]
mod tests {
    use super::{note_text, verdict_text};
    use crate::locale::Locale;

    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c))
    }

    #[test]
    fn english_verdicts_have_no_cjk() {
        let t = verdict_text(Locale::En);
        let all = [
            t.lan_down.label,
            t.lan_down.advice,
            t.lan_down.rule,
            t.dns_fail.label,
            t.dns_fail.advice,
            t.dns_fail.rule,
            t.captive.label,
            t.captive.advice,
            t.captive.rule,
            t.lan_slow.label,
            t.lan_slow.advice,
            t.lan_slow.rule,
            t.wan_gap.label,
            t.wan_gap.advice,
            t.wan_gap.rule,
            t.jitter.label,
            t.jitter.advice,
            t.jitter.rule,
            t.dns_slow.label,
            t.dns_slow.advice,
            t.dns_slow.rule,
            t.all_good.label,
            t.all_good.advice,
            t.all_good.rule,
        ];
        for s in all {
            assert!(!has_cjk(s), "英文结论里漏了中文：{s}");
        }
    }

    #[test]
    fn english_notes_have_no_cjk() {
        let t = note_text(Locale::En);
        let all = [
            t.no_gateway,
            t.lan_failed,
            t.wan_all_failed,
            t.wan_both_failed,
            t.dns_failed,
            t.unknown_err,
            t.timeout_err,
            t.addr_parse,
            t.dns_timeout,
        ];
        for s in all {
            assert!(!has_cjk(s), "英文探测说明里漏了中文：{s}");
        }
    }

    /// 中文侧必须真的有中文 —— 复制英文行过来忘改，是这类表最常见的错
    #[test]
    fn chinese_verdicts_are_chinese() {
        let t = verdict_text(Locale::Zh);
        assert!(has_cjk(t.lan_down.label));
        assert!(has_cjk(t.all_good.advice));
    }
}
