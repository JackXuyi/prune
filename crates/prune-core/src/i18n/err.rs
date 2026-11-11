//! 用户可见的错误串（docs/i18n-spec.md §4.7）。
//!
//! 判断标准只有一条：**这句话会不会出现在 toast / 空态 / 错误面板里**。
//! `eprintln!` 的日志一律留在代码里用中文 —— 它们是给开发者看的，翻译它们
//! 只会让 grep 日志变难。
//!
//! 用结构体而不是 `t("err.self_kill")`：字段名拼错在 Rust 里是编译错误，
//! 字符串 key 拼错是线上空串。这是 Rust 侧唯一比 TS 强的地方，要用上。

use crate::locale::Locale;

pub struct ErrText {
    /// 结束进程：pid ≤ 1
    pub pid_critical: &'static str,
    /// 结束进程：自己
    pub self_kill: &'static str,
    pub path_missing: &'static str,
    pub reveal_failed: &'static str,
    pub disk_measure_failed: &'static str,
    pub disk_measure_batch_failed: &'static str,
    pub not_a_dir: &'static str,
    pub detail_thread: &'static str,
    pub reclaim_thread: &'static str,
    pub cuts_read: &'static str,
    pub cuts_top_read: &'static str,
    pub icon_thread: &'static str,
    pub app_history_read: &'static str,
    pub app_day_history_read: &'static str,
    // ---- 诊断 / 测速 / 剪贴板
    pub diag_dispatch: &'static str,
    pub diag_busy: &'static str,
    pub diag_none: &'static str,
    pub speed_dispatch: &'static str,
    pub speed_macos_only: &'static str,
    pub clip_spawn: &'static str,
    pub clip_no_stdin: &'static str,
    pub clip_write: &'static str,
    pub clip_wait: &'static str,
    pub clip_denied: &'static str,
    pub clip_unsupported: &'static str,
    // ---- networkQuality（外部工具，工具名不译）
    pub nq_spawn: &'static str,
    pub nq_no_stdout: &'static str,
    pub nq_timeout: &'static str,
    pub nq_read: &'static str,
    pub nq_exit: &'static str,
    pub nq_no_json: &'static str,
    pub nq_json_parse: &'static str,
    pub nq_missing_field: &'static str,
    // ---- 更新 / 语言设置
    pub updater_unavailable: &'static str,
    pub update_none_pending: &'static str,
    pub update_install_failed: &'static str,
    pub locale_write_failed: &'static str,
    pub main_thread_dispatch: &'static str,
    // ---- 清理错误（`ReclaimError` 的 Display 走这些）
    pub reclaim_not_whitelisted: &'static str,
    pub reclaim_not_cleanable: &'static str,
    pub reclaim_needs_confirm: &'static str,
    pub reclaim_nothing: &'static str,
    pub reclaim_all_failed: &'static str,
}

pub const ZH: ErrText = ErrText {
    pid_critical: "pid {pid} 是系统关键进程，不允许结束",
    self_kill: "不能结束 Prune 自身",
    path_missing: "路径不存在：{path}",
    reveal_failed: "打开文件管理器失败：{err}",
    disk_measure_failed: "磁盘测量失败：{err}",
    disk_measure_batch_failed: "批量磁盘测量失败：{err}",
    not_a_dir: "不是目录：{path}",
    detail_thread: "明细线程失败: {err}",
    reclaim_thread: "清理线程失败: {err}",
    cuts_read: "读取剪除记录失败：{err}",
    cuts_top_read: "读取剪除 Top 榜失败：{err}",
    icon_thread: "图标线程失败: {err}",
    app_history_read: "读取应用历史失败：{err}",
    app_day_history_read: "读取应用天历史失败：{err}",
    diag_dispatch: "诊断任务调度失败: {err}",
    diag_busy: "诊断状态被占用",
    diag_none: "还没有诊断结果，请先运行诊断",
    speed_dispatch: "测速任务调度失败: {err}",
    speed_macos_only: "测速仅 macOS 提供（系统 networkQuality）",
    clip_spawn: "剪贴板进程启动失败: {err}",
    clip_no_stdin: "剪贴板进程无 stdin",
    clip_write: "写入剪贴板失败: {err}",
    clip_wait: "剪贴板进程等待失败: {err}",
    clip_denied: "剪贴板写入被拒绝（{code}）：{err}",
    clip_unsupported: "当前平台未实现剪贴板写入（仅 macOS / Windows）",
    nq_spawn: "networkQuality 启动失败: {err}",
    nq_no_stdout: "networkQuality 无 stdout",
    nq_timeout: "networkQuality 超时（90 s），已终止",
    nq_read: "读取 networkQuality 输出: {err}",
    nq_exit: "networkQuality 异常退出（{code}）",
    nq_no_json: "networkQuality 没有输出 JSON",
    nq_json_parse: "networkQuality JSON 解析失败: {err}",
    nq_missing_field: "networkQuality 缺字段 {k}",
    updater_unavailable: "updater 不可用（未配置 plugins.updater?）: {err}",
    update_none_pending: "没有待安装的更新，请先检查更新",
    update_install_failed: "下载安装失败: {err}",
    locale_write_failed: "保存语言设置失败: {err}",
    main_thread_dispatch: "调度主线程失败: {err}",
    reclaim_not_whitelisted: "路径不在白名单内，拒绝清理: {path}",
    reclaim_not_cleanable: "该级别只给命令、不执行: {path}",
    reclaim_needs_confirm: "单次清理 {n} 字节超过熔断阈值，需二次确认",
    reclaim_nothing: "没有可清理的内容",
    reclaim_all_failed: "全部清理失败（{n} 项）",
};

pub const EN: ErrText = ErrText {
    pid_critical: "PID {pid} is a critical system process and cannot be killed",
    self_kill: "Prune can't kill itself",
    path_missing: "Path does not exist: {path}",
    reveal_failed: "Could not open the file manager: {err}",
    disk_measure_failed: "Disk measurement failed: {err}",
    disk_measure_batch_failed: "Batch disk measurement failed: {err}",
    not_a_dir: "Not a directory: {path}",
    detail_thread: "Detail worker failed: {err}",
    reclaim_thread: "Cleanup worker failed: {err}",
    cuts_read: "Could not read the cut log: {err}",
    cuts_top_read: "Could not read the cut leaderboard: {err}",
    icon_thread: "Icon worker failed: {err}",
    app_history_read: "Could not read app history: {err}",
    app_day_history_read: "Could not read app daily history: {err}",
    diag_dispatch: "Could not start the diagnostic: {err}",
    diag_busy: "Diagnostic state is busy",
    diag_none: "No diagnostic result yet — run a diagnosis first",
    speed_dispatch: "Could not start the speed test: {err}",
    speed_macos_only: "The speed test is only available on macOS (system networkQuality)",
    clip_spawn: "Could not start the clipboard process: {err}",
    clip_no_stdin: "The clipboard process has no stdin",
    clip_write: "Could not write to the clipboard: {err}",
    clip_wait: "Could not wait for the clipboard process: {err}",
    clip_denied: "The clipboard refused the write ({code}): {err}",
    clip_unsupported: "Clipboard writing is not implemented on this platform (macOS / Windows only)",
    nq_spawn: "Could not start networkQuality: {err}",
    nq_no_stdout: "networkQuality produced no stdout",
    nq_timeout: "networkQuality timed out (90 s) and was terminated",
    nq_read: "Could not read networkQuality output: {err}",
    nq_exit: "networkQuality exited abnormally ({code})",
    nq_no_json: "networkQuality produced no JSON",
    nq_json_parse: "Could not parse networkQuality JSON: {err}",
    nq_missing_field: "networkQuality is missing field {k}",
    updater_unavailable: "Updater unavailable (plugins.updater not configured?): {err}",
    update_none_pending: "No update is pending — check for updates first",
    update_install_failed: "Download and install failed: {err}",
    locale_write_failed: "Could not save the language setting: {err}",
    main_thread_dispatch: "Could not dispatch to the main thread: {err}",
    reclaim_not_whitelisted: "Path is not on the whitelist — refusing to clean: {path}",
    reclaim_not_cleanable: "This level only provides commands, nothing is executed: {path}",
    reclaim_needs_confirm: "Cleaning {n} bytes in one pass exceeds the breaker threshold and needs a second confirmation",
    reclaim_nothing: "Nothing to clean",
    reclaim_all_failed: "Every item failed to clean ({n})",
};

pub fn err_text(l: Locale) -> &'static ErrText {
    match l {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c))
    }

    /// 英文表里漏一个汉字翻译，用户就会在英文界面里看到中文错误 —— 逐条扫
    #[test]
    fn english_errors_have_no_cjk() {
        let t = err_text(Locale::En);
        let all = [
            t.pid_critical,
            t.self_kill,
            t.path_missing,
            t.reveal_failed,
            t.disk_measure_failed,
            t.disk_measure_batch_failed,
            t.not_a_dir,
            t.detail_thread,
            t.reclaim_thread,
            t.cuts_read,
            t.cuts_top_read,
            t.icon_thread,
            t.app_history_read,
            t.app_day_history_read,
            t.diag_dispatch,
            t.diag_busy,
            t.diag_none,
            t.speed_dispatch,
            t.speed_macos_only,
            t.clip_spawn,
            t.clip_no_stdin,
            t.clip_write,
            t.clip_wait,
            t.clip_denied,
            t.clip_unsupported,
            t.nq_spawn,
            t.nq_no_stdout,
            t.nq_timeout,
            t.nq_read,
            t.nq_exit,
            t.nq_no_json,
            t.nq_json_parse,
            t.nq_missing_field,
            t.updater_unavailable,
            t.update_none_pending,
            t.update_install_failed,
            t.locale_write_failed,
            t.main_thread_dispatch,
            t.reclaim_not_whitelisted,
            t.reclaim_not_cleanable,
            t.reclaim_needs_confirm,
            t.reclaim_nothing,
            t.reclaim_all_failed,
        ];
        for s in all {
            assert!(!has_cjk(s), "英文错误串里漏了中文：{s}");
        }
    }
}
