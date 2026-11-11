//! Rust 侧用户可见文案（docs/i18n-spec.md §4.5–4.8）。
//!
//! **为什么文案不放在 Rust 的一个大 catalog 里**：Rust 没有 TS 那种模板字面量
//! 类型，`t("diag.lan_slow")` 拼错只能运行时发现。这里一律用结构体常量 ——
//! 少写一个字段直接编译失败，这是 Rust 能给的、比前端更强的那道闸。
//!
//! **CLI 不在迁移范围**：`prune-cli` 是机器接口，冻结为中文（传 [`Locale::Zh`]）。
//! 这样 core 的函数都收 `locale` 参数，CLI 显式传中文即可，不必为它再建一套文本。

pub mod diag_text;
pub mod err;
pub mod report;

/// `{name}` 占位替换。
///
/// 只认花括号，不做转义 —— 模板全是我们自己写的常量，不存在用户输入。
/// 未知占位**原样保留**：宁可让 `{foo}` 露在界面上被发现，也不要静默吞掉内容。
pub fn fill(tpl: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut rest = tpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            // 有 `{` 没 `}`：尾部原样输出，不 panic
            out.push_str(&rest[open..]);
            return out;
        };
        let key = &rest[open + 1..open + close];
        match pairs.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => out.push_str(v),
            None => {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn fill_replaces_known_placeholders() {
        assert_eq!(fill("{a} 与 {b}", &[("a", "1"), ("b", "2")]), "1 与 2");
        assert_eq!(
            fill("路由器响应慢（{v} ms）", &[("v", "120")]),
            "路由器响应慢（120 ms）"
        );
    }

    /// 未知占位必须原样露出，不能变成空串 —— 空串会让「阈值缺失」看起来像
    /// 「阈值是 0」，这类静默错误比渲染难看严重得多
    #[test]
    fn fill_keeps_unknown_placeholder() {
        assert_eq!(fill("{a} {zz}", &[("a", "1")]), "1 {zz}");
        // 不配对的 `{` 不 panic
        assert_eq!(fill("a { b", &[]), "a { b");
    }

    /// 同一占位出现多次要全部替换（`WAN − LAN > {t} ms` 只出现一次，但别埋坑）
    #[test]
    fn fill_handles_repeats() {
        assert_eq!(fill("{v}-{v}", &[("v", "9")]), "9-9");
    }
}
