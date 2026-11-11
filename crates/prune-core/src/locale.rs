//! 界面语言（docs/i18n-spec.md §1.1）。
//!
//! 放在 core 而不是 src-tauri：`net::diag` 的结论与报告要按语言渲染，
//! core 若从 tauri 取 Locale 就形成反向依赖（CLI 也不敢引用 tauri）。
//!
//! **只有两个变体**，没有「自动」：`auto` 是设置层的概念，解析完就落到 Zh / En。
//! 第三个语言真要来时，先想清楚 `from_tag` 的前缀匹配怎么扩展 —— 现在
//! 繁体标签落到 **En**（catalog 没有繁体，与其给简体不如给英文，
//! 见 `docs/i18n-language-switch-spec.md` §4.5）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    Zh,
    #[default]
    En,
}

/// 繁体标签：`hant` / `-TW` / `-HK` / `-MO`。
///
/// `hans` 优先：`zh-Hans-HK` 这种罕见混写是「简体 + 香港地区」，不能被
/// 地区码误判成繁体。
///
/// **改这条规则必须同步改 `src/i18n/detect.ts::isTraditionalTag`** —— 浏览器
/// dev 走 TS、Tauri 走这里，两边不一致会让 dev 与真机的 auto 结果打架。
fn is_traditional(low: &str) -> bool {
    if low.contains("hans") {
        return false;
    }
    low.contains("hant") || low.contains("-tw") || low.contains("-hk") || low.contains("-mo")
}

impl Locale {
    /// BCP-47 标签 → Locale。只做前缀匹配，不引入 ICU。
    ///
    /// - `zh` / `zh-Hans` / `zh-CN` / `zh-SG` → [`Locale::Zh`]
    /// - `zh-Hant*` / `zh-TW` / `zh-HK` / `zh-MO` → [`Locale::En`]（繁体无 catalog）
    /// - 其余一律 [`Locale::En`]（含 `ja` / `de` / 空串 / 无法识别）
    pub fn from_tag(tag: &str) -> Self {
        let low = tag.trim().to_ascii_lowercase();
        if !low.starts_with("zh") {
            return Self::En;
        }
        if is_traditional(&low) {
            return Self::En;
        }
        Self::Zh
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    /// 设置层字符串（`auto` / `zh` / `en`）→ 偏好。
    ///
    /// 非法值**不报错**，按 `auto` 处理（spec §1.2：其它值视为 auto）；
    /// 手改过 settings.json 的用户不该看到「语言设置损坏」这种弹窗。
    pub fn pref_from_str(raw: &str) -> LocalePref {
        match raw.trim().to_ascii_lowercase().as_str() {
            "zh" => LocalePref::Zh,
            "en" => LocalePref::En,
            _ => LocalePref::Auto,
        }
    }
}

/// 设置里的语言偏好：跟随系统 / 锁死某种语言
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalePref {
    #[default]
    Auto,
    Zh,
    En,
}

impl LocalePref {
    /// 落盘用的字符串
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Zh => "zh",
            Self::En => "en",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Locale, LocalePref};

    /// 用例表与 `docs/i18n-language-switch-spec.md` §4.5 同一份；
    /// TS 侧 `detect.ts::isTraditionalTag` 必须一字不差，否则 dev 与真机结果不同。
    #[test]
    fn tag_prefix_matching() {
        // 明确简体
        assert_eq!(Locale::from_tag("zh"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-Hans"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-Hans-CN"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-CN"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-SG"), Locale::Zh);
        assert_eq!(Locale::from_tag("ZH"), Locale::Zh);
        assert_eq!(Locale::from_tag(" zh-CN "), Locale::Zh);
        // hans 优先：简体 + 香港地区仍是简体
        assert_eq!(Locale::from_tag("zh-Hans-HK"), Locale::Zh);

        // 繁体 → 英文（catalog 无繁体）
        assert_eq!(Locale::from_tag("zh-Hant"), Locale::En);
        assert_eq!(Locale::from_tag("zh-Hant-TW"), Locale::En);
        assert_eq!(Locale::from_tag("zh-TW"), Locale::En);
        assert_eq!(Locale::from_tag("zh-HK"), Locale::En);
        assert_eq!(Locale::from_tag("zh-MO"), Locale::En);

        // 非中文
        assert_eq!(Locale::from_tag("en-US"), Locale::En);
        assert_eq!(Locale::from_tag("ja"), Locale::En);
        assert_eq!(Locale::from_tag(""), Locale::En);
    }

    #[test]
    fn round_trip_tag() {
        assert_eq!(Locale::from_tag(Locale::Zh.tag()), Locale::Zh);
        assert_eq!(Locale::from_tag(Locale::En.tag()), Locale::En);
    }

    /// 设置里的非法值必须静默归到 auto —— 手改坏 JSON 不该让应用切不了语言
    #[test]
    fn pref_parsing_tolerates_garbage() {
        assert_eq!(Locale::pref_from_str("auto"), LocalePref::Auto);
        assert_eq!(Locale::pref_from_str("ZH"), LocalePref::Zh);
        assert_eq!(Locale::pref_from_str("en"), LocalePref::En);
        assert_eq!(Locale::pref_from_str("fr"), LocalePref::Auto);
        assert_eq!(Locale::pref_from_str(""), LocalePref::Auto);
        assert_eq!(LocalePref::Auto.as_str(), "auto");
    }

    /// 默认必须是 En：系统语言读不到时 spec §1.5 要求落到英文
    #[test]
    fn default_is_en() {
        assert_eq!(Locale::default(), Locale::En);
    }
}
