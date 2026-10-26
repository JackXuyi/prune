//! 进程枚举（sysinfo）+ 家族归类（D-M2）。
//!
//! 归类策略：内置规则表优先（Cursor / Chrome / Electron / 微信…），
//! 未命中时从 exe 路径里找 `X.app` 取应用名，最后才退回进程名本身。
//! 这样「Cursor Helper (Renderer)」这类子进程不会各占一行。

use std::path::Path;

use sysinfo::{Pid, ProcessesToUpdate, System};

use super::{MemSource, ProcMem};

/// 内置规则表：`(家族名, 关键词)`，按顺序匹配，命中即停。
///
/// 关键词同时比对规范化后的进程名与 exe 路径（小写）。顺序敏感：
/// 更具体的规则要放在更前面（例如 "visual studio code" 必须先于泛化项）。
const RULES: &[(&str, &[&str])] = &[
    ("Cursor", &["cursor"]),
    (
        "VS Code",
        &["visual studio code", "code helper", "vscode", "/code.app/"],
    ),
    ("Chrome", &["google chrome", "chrome", "chromium"]),
    ("Safari", &["safari", "com.apple.webkit"]),
    ("Firefox", &["firefox"]),
    ("Arc", &["/arc.app/", "arc helper"]),
    ("Electron", &["electron"]),
    ("Node", &["/bin/node", "/node "]),
    ("Docker", &["docker", "com.docker", "vpnkit"]),
    ("Xcode", &["xcode", "swift-frontend", "sourcekit", "ibtool"]),
    ("WeChat", &["wechat", "微信"]),
    ("Slack", &["slack"]),
    ("Notion", &["notion"]),
    ("Figma", &["figma"]),
    (
        "JetBrains",
        &["intellij", "pycharm", "webstorm", "goland", "clion", "rustrover"],
    ),
    ("Zoom", &["zoom.us", "caphost", "aomhost"]),
    ("Spotify", &["spotify"]),
    (
        "Terminal",
        &["iterm", "warp", "alacritty", "kitty", "ghostty", "terminal"],
    ),
    ("Prune", &["prune-app", "/prune.app/"]),
];

/// 归类单个进程。`exe` 为可执行文件绝对路径（可能拿不到）。
pub fn classify(name: &str, exe: Option<&str>) -> String {
    let lower_name = name.to_lowercase();
    let lower_exe = exe.map(|e| e.to_lowercase()).unwrap_or_default();

    for (family, keywords) in RULES {
        for kw in *keywords {
            if lower_name.contains(kw) || lower_exe.contains(kw) {
                return (*family).to_string();
            }
        }
    }

    // 兜底 1：从 exe 路径里取 X.app 的应用名
    if let Some(app) = exe.and_then(app_bundle_name) {
        return app;
    }
    // 兜底 2：进程名本身（系统进程走这条，如 kernel_task / launchd）
    name.to_string()
}

/// `/Applications/Foo.app/Contents/MacOS/Foo` → `Foo`
fn app_bundle_name(exe: &str) -> Option<String> {
    let seg = exe
        .split('/')
        .find(|s| s.ends_with(".app") && s.len() > 4)?;
    let name = seg.trim_end_matches(".app");
    // 路径兜底，避免返回空串
    let name = if name.is_empty() {
        Path::new(exe).file_name()?.to_string_lossy().to_string()
    } else {
        name.to_string()
    };
    Some(name)
}

/// 持久的进程枚举器：复用 `System`，避免每轮重建开销。
pub struct Scanner {
    sys: System,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        // new_all 同时填充内存总量等信息（sysinfo 0.39 没有单独的 refresh_memory）；
        // 只在启动时执行一次，后续轮次走 refresh_processes
        Self {
            sys: System::new_all(),
        }
    }

    /// 枚举全量进程（RSS / ppid / exe / 启动时间），按 RSS 降序。
    ///
    /// 深探字段（footprint / swapped）此轮为 None，由 sampler 用上一轮 deep 结果合并。
    pub fn scan(&mut self) -> Vec<ProcMem> {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        let mut out: Vec<ProcMem> = self
            .sys
            .processes()
            .values()
            .map(|p| {
                let name = p.name().to_string_lossy().to_string();
                let exe = p.exe().map(|e| e.to_string_lossy().to_string());
                let family = classify(&name, exe.as_deref());
                // argv 只为工作区归属保留（M2.5）：只有编辑器类家族需要，
                // 其余进程不复制 argv，避免 10 s 轮次做无用的字符串分配
                let cmd = if super::workspace::workspace_capable(&family) {
                    p.cmd()
                        .iter()
                        .map(|c| c.to_string_lossy().to_string())
                        .collect()
                } else {
                    Vec::new()
                };
                ProcMem {
                    pid: p.pid().as_u32() as i32,
                    ppid: p.parent().map(|x: Pid| x.as_u32() as i32).unwrap_or(0),
                    family,
                    name,
                    exe,
                    rss: p.memory(),
                    footprint: None,
                    swapped: None,
                    source: MemSource::Rss,
                    start_time: p.start_time(),
                    cmd,
                    workspace: None,
                }
            })
            .collect();

        out.sort_by_key(|p| std::cmp::Reverse(p.rss));
        out
    }

    pub fn total_memory(&self) -> u64 {
        self.sys.total_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known_families() {
        assert_eq!(
            classify(
                "Cursor Helper (Renderer)",
                Some("/Applications/Cursor.app/Contents/Frameworks/Cursor Helper.app")
            ),
            "Cursor"
        );
        assert_eq!(
            classify("Google Chrome Helper", Some("/Applications/Google Chrome.app")),
            "Chrome"
        );
        assert_eq!(classify("node", Some("/opt/homebrew/bin/node")), "Node");
        assert_eq!(classify("tsserver", None), "tsserver");
        assert_eq!(classify("kernel_task", None), "kernel_task");
    }

    #[test]
    fn classify_falls_back_to_app_bundle() {
        // 无内置规则命中 → 从 .app 取应用名
        assert_eq!(
            classify("helper-tool", Some("/Applications/Acme工具.app/Contents/MacOS/helper-tool")),
            "Acme工具"
        );
    }

    #[test]
    fn classify_prefers_specific_rule_over_generic() {
        // "Code Helper" 必须归到 VS Code，而不是被别的规则抢走
        assert_eq!(
            classify("Code Helper (Plugin)", Some("/Applications/Visual Studio Code.app/x")),
            "VS Code"
        );
    }

    #[test]
    fn app_bundle_name_variants() {
        assert_eq!(
            app_bundle_name("/Applications/Foo.app/Contents/MacOS/Foo"),
            Some("Foo".to_string())
        );
        assert_eq!(app_bundle_name("/usr/bin/ssh"), None);
    }

    #[test]
    fn scanner_enumerates_self() {
        let mut s = Scanner::new();
        let procs = s.scan();
        assert!(!procs.is_empty(), "至少应枚举到当前进程");
        assert!(s.total_memory() > 0);
        // 当前测试进程应当在列表里
        let me = std::process::id() as i32;
        assert!(procs.iter().any(|p| p.pid == me), "枚举结果应包含自身 pid");
        // RSS 降序
        assert!(procs.windows(2).all(|w| w[0].rss >= w[1].rss));
    }
}
