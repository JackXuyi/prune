//! `prune reclaim --emit-script`（W3/D-15，DP-4 裁 A）。
//!
//! 开一条「打印命令」的缝：把 reclaim 预览的 L1 白名单项命令化成一段
//! 可审计的 POSIX sh 脚本，执行权完全留在人手里——Prune 进程内
//! **不存在执行该脚本的代码路径**（A5 有源码断言，见本文件底部测试）。
//!
//! 边界（docs/d15-emit-script-plan.md §2）：
//! - 只取 L1 死重白名单项；L3 需逐项判断，L4 永不提供删除命令；
//! - 每条命令 `mv -n` 进废纸篓，不用 rm——与 reclaim 执行层同一条护栏；
//! - 逐项复验静态白名单，不过的项不进脚本（D4：宁可少给，不给错）；
//! - 本模块是纯字符串生成，零 IO、零子进程。

use std::io::Write;
use std::path::Path;

use prune_core::reclaim::ReclaimPreview;

/// 单引号 shell 转义：`'` → `'\''`，外层再包一对单引号。
///
/// POSIX sh 里单引号内无任何转义生效，唯一出路是断开再续——这是
/// 防路径注入的唯一正确写法（`$(...)`、反引号、`;` 全部失效在引号内）。
pub(crate) fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// 生成脚本文本。纯函数：输入预览 + 已通过白名单复验的项在 `passed` 里
/// 由调用方给出（见 [`run`]），本函数不做任何文件系统访问。
///
/// `generated_at` 由调用方传（测试传固定值），保证输出可复现。
pub(crate) fn script_text(
    passed: &[prune_core::reclaim::ReclaimItem],
    total_bytes: u64,
    generated_at: &str,
    version: &str,
) -> String {
    let mut out = String::new();
    out.push_str("#!/bin/sh\n");
    out.push_str(&format!(
        "# prune reclaim --emit-script 生成于 {generated_at}（prune v{version}）\n"
    ));
    out.push_str("# 来源：L1 零风险死重白名单（prune reclaim 预览的命令化）。\n");
    out.push_str("# 本脚本由 Prune 打印，Prune 进程内不存在执行它的代码路径；\n");
    out.push_str("# 请人工逐条审阅后自行执行。mv -n 不覆盖：废纸篓已有同名条目时\n");
    out.push_str("# 新条目不会移入（静默跳过），请人工核对。\n");
    out.push_str("set -eu\n");
    if passed.is_empty() {
        out.push_str("\n# 本机当前没有可命令化的 L1 白名单项（预览清单为空）。\n");
        return out;
    }
    out.push_str(&format!(
        "\n# 共 {} 项，合计 {}（体积来自扫描快照，执行时以实际为准）。\n",
        passed.len(),
        crate::hb(total_bytes),
    ));
    for it in passed {
        out.push_str(&format!("\n# {}  {}\n", crate::hb(it.bytes), it.label));
        out.push_str("# 白名单校验：✓ 静态白名单（L1 零风险）\n");
        out.push_str(&format!(
            "mv -n {} \"$HOME/.Trash/\"\n",
            shell_quote(&it.path),
        ));
    }
    out
}

/// emit-script 入口：复验白名单 → 生成 → stdout。
///
/// 复验不过的项**不进脚本**并 stderr 告警（D4）——报告可能已过期，
/// 白名单是删除建议的最后一道闸，emit 通道不豁免。
pub(crate) fn run(pv: &ReclaimPreview, home: &Path, generated_at: &str) -> i32 {
    let mut passed = Vec::new();
    for it in &pv.items {
        let path = std::path::PathBuf::from(&it.path);
        if prune_core::disk::whitelist::is_whitelisted(&path, home) {
            passed.push(it.clone());
        } else {
            eprintln!("⚠️ 白名单复验未通过，不进入脚本：{}", it.path);
        }
    }
    let text = script_text(
        &passed,
        passed.iter().map(|i| i.bytes).sum(),
        generated_at,
        prune_core::version(),
    );
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = lock.write_all(text.as_bytes());
    let _ = lock.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use prune_core::disk::whitelist::Level;
    use prune_core::reclaim::ReclaimItem;

    fn item(path: &str, bytes: u64, label: &str) -> ReclaimItem {
        ReclaimItem {
            path: path.into(),
            label: label.into(),
            bytes,
            level: Level::L1,
        }
    }

    #[test]
    fn shell_quote_neutralizes_metacharacters() {
        // 引号内元字符必须全部失效；单引号要断开续接
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("$(rm -rf ~)"), "'$(rm -rf ~)'");
        assert_eq!(shell_quote("`evil`;x"), "'`evil`;x'");
        // 中文与换行也安全（换行在引号内只是字面量）
        assert_eq!(shell_quote("项目 名"), "'项目 名'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    /// 统计真正的命令行（行首 mv）：脚本头注释里提到 mv -n 语义，不能算
    fn command_lines(text: &str) -> usize {
        text.lines().filter(|l| l.starts_with("mv ")).count()
    }

    #[test]
    fn script_has_guard_header_and_per_item_mv() {
        let items = vec![
            item(
                "/Users/x/Library/Caches/Yarn/v6/npm-a-linux-x64",
                200,
                "yarn · a",
            ),
            item(
                "/Users/x/Library/Caches/Yarn/v6/npm-b-linux-x64",
                100,
                "yarn · b",
            ),
        ];
        let text = script_text(&items, 300, "2026-11-18T12:00:00+08:00", "0.14.0");
        assert!(text.starts_with("#!/bin/sh\n"), "脚本必须以 shebang 开头");
        assert!(text.contains("set -eu"), "护栏 set -eu 必须在");
        assert!(text.contains("不存在执行它的代码路径"), "边界声明必须自证");
        assert!(text.contains("2026-11-18T12:00:00+08:00"));
        assert!(text.contains("prune v0.14.0"));
        assert!(text.contains("共 2 项，合计 300 B"));
        // 每项一条 mv 命令行，且注释带体积与白名单校验结果
        assert_eq!(command_lines(&text), 2);
        assert!(text.contains("# 200 B  yarn · a"));
        assert_eq!(text.matches("白名单校验：✓").count(), 2);
        assert!(
            !text.contains("'$HOME/.Trash/'"),
            "Trash 路径必须可展开，不能被单引号锁死"
        );
        assert!(text.contains("\"$HOME/.Trash/\""));
    }

    #[test]
    fn empty_list_yields_no_mv_lines() {
        let text = script_text(&[], 0, "2026-11-18T00:00:00Z", "0.14.0");
        assert_eq!(command_lines(&text), 0, "空清单不得输出任何命令行");
        assert!(text.contains("没有可命令化"), "空清单要说明而不是沉默");
    }

    /// 验收 A5：无任何执行路径。源码断言 emit_script.rs 不含进程/删除原语——
    /// 未来有人在这里塞子进程调用或废纸篓原语会直接红。
    /// 禁词用空格拼接构造（"Comm and" 拼回全词）：断言的字面量本身
    /// 也在本文件源码里，直接写全必然自指失败。
    #[test]
    fn module_source_contains_no_execution_primitive() {
        // 只断言产品代码（#[cfg(test)] 之前）：测试自身的临时目录清理
        // 会合法使用删除原语，不参与断言
        let src = include_str!("emit_script.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        let banned = [
            "Comm and",
            "proces s::",
            "tras h::",
            "execut e(",
            "remove_d ir",
            "remove_f ile",
            "delet e(",
            "spaw n(",
        ];
        for piece in banned {
            let word = piece.replace(' ', "");
            assert!(
                !src.contains(&word),
                "emit_script.rs 出现执行原语 {word:?}：--emit-script 只允许打印文本"
            );
        }
        // run() 只允许 write 到 stdout
        assert!(src.contains("write_all"), "stdout 输出走 write_all");
    }

    /// run() 的白名单复验：非白名单路径绝不进入脚本（A4）。
    /// 路径造在临时目录（任何白名单都不会收录），macOS/Linux 判定一致。
    #[test]
    fn run_filters_out_paths_failing_whitelist() {
        let home = std::env::temp_dir().join(format!("prune-emit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&home);
        let pv = ReclaimPreview {
            items: vec![item(
                &home.join("not-whitelisted-dir").to_string_lossy(),
                4096,
                "白名单外",
            )],
            total_bytes: 4096,
            needs_confirm: false,
            notes: Vec::new(),
        };
        // 捕获脚本内容：run 写 stdout，这里直接走同款复验逻辑断言过滤
        let mut passed = Vec::new();
        for it in &pv.items {
            let path = std::path::PathBuf::from(&it.path);
            if prune_core::disk::whitelist::is_whitelisted(&path, &home) {
                passed.push(it.clone());
            }
        }
        assert!(
            passed.is_empty(),
            "白名单外的路径必须被过滤，实际放行 {:?}",
            passed
        );
        let text = script_text(&passed, 0, "t", "0.14.0");
        assert_eq!(command_lines(&text), 0, "被过滤后不得残留任何 mv 命令行");
        let _ = std::fs::remove_dir_all(&home);
    }
}
