//! 界面语言（docs/i18n-spec.md §1.1）。
//!
//! 放在 core 而不是 src-tauri：`net::diag` 的结论与报告要按语言渲染，
//! core 若从 tauri 取 Locale 就形成反向依赖（CLI 也不敢引用 tauri）。
//!
//! **只有两个变体**，没有「自动」：`auto` 是设置层的概念，解析完就落到 Zh / En。
//! 第三个语言真要来时，先想清楚 `from_tag` 的前缀匹配怎么扩展 —— 现在
//! `zh-Hant` 也会落到 Zh（繁体用户看到简体），这是已知取舍。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    Zh,
    #[default]
    En,
}

impl Locale {
    /// BCP-47 标签 → Locale。只做前缀匹配，不引入 ICU。
    ///
    /// - `zh` / `zh-Hans` / `zh-CN` / `zh-Hant` → [`Locale::Zh`]（繁体看到简体，已知取舍）
    /// - 其余一律 [`Locale::En`]（含 `ja` / `de` / 空串 / 无法识别）
    pub fn from_tag(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
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

    #[test]
    fn tag_prefix_matching() {
        assert_eq!(Locale::from_tag("zh"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-Hans"), Locale::Zh);
        assert_eq!(Locale::from_tag("zh-Hant-TW"), Locale::Zh);
        assert_eq!(Locale::from_tag("ZH"), Locale::Zh);
        assert_eq!(Locale::from_tag(" zh-CN "), Locale::Zh);

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
