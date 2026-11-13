//! 进程枚举（sysinfo）+ 家族归类（D-M2）。
//!
//! 归类策略：内置规则表优先（Cursor / Chrome / Electron / 微信…），
//! 未命中时从 exe 路径里找 `X.app` 取应用名，最后才退回进程名本身。
//! 这样「Cursor Helper (Renderer)」这类子进程不会各占一行。

use std::collections::HashSet;
use std::path::Path;

use sysinfo::{CpuRefreshKind, Pid, ProcessesToUpdate, System};

use super::{CpuLoad, MemSource, ProcMem};

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
        &[
            "intellij",
            "pycharm",
            "webstorm",
            "goland",
            "clion",
            "rustrover",
        ],
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

/// 一轮扫描的产出：进程表 + 整机 CPU 读数（M5）
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub procs: Vec<ProcMem>,
    /// 整机 CPU 使用率（0–100，每核平均）。None = 首轮（差分窗口未建立）或采集失败
    pub cpu_usage: Option<f32>,
    /// 逻辑核数
    pub cores: u32,
    /// 1/5/15 min 排队长度。None = 平台不支持（Windows）
    pub load: Option<CpuLoad>,
}

/// 持久的进程枚举器：复用 `System`，避免每轮重建开销。
pub struct Scanner {
    sys: System,
    /// 上一轮见过的 `(pid, start_time)`。sysinfo 的进程 CPU 是差分值，新进程
    /// 首次刷新恒为 0——必须区分「首见」（→ None）与「真的 0%」（→ Some(0.0)）。
    seen: HashSet<(i32, u64)>,
    /// 首轮不产出整机 CPU：`new_all` 的初值是初始化时刻的采样，不是 10 s 窗口均值。
    cpu_warmed: bool,
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
            seen: HashSet::new(),
            cpu_warmed: false,
        }
    }

    /// 枚举全量进程（RSS / CPU / ppid / exe / 启动时间），按 RSS 降序。
    ///
    /// 深探字段（footprint / swapped）此轮为 None，由 sampler 用上一轮 deep 结果合并。
    pub fn scan(&mut self) -> ScanResult {
        // 整机 CPU 先刷（0.1 ms 级，实测见 m5-cpu-plan.md §2.2），差分窗口与进程轮一致
        self.sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage());
        // sysinfo 0.39 的 refresh_processes 默认 ProcessRefreshKind 带 with_cpu()，
        // macOS 上 RSS 与 CPU 累计时间来自同一次 proc_pidinfo，不增加 syscall
        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        let prev_seen = std::mem::take(&mut self.seen);
        let mut out: Vec<ProcMem> = self
            .sys
            .processes()
            .values()
            .map(|p| {
                let name = p.name().to_string_lossy().to_string();
                let exe = p.exe().map(|e| e.to_string_lossy().to_string());
                let family = classify(&name, exe.as_deref());
                // argv 复制范围（M2.5 + 服务类型推断）：编辑器家族要给工作区归属，
                // 运行时解释器进程（node/python/java…）要给端口服务推断
                // （`node …/vite`、`python -m http.server`、`node …/mcp-server-x`）。
                // 两类之外不复制，避免 10 s 轮次做无用的字符串分配
                let cmd = if super::workspace::workspace_capable(&family)
                    || crate::net::service::is_runtime_comm(&name)
                {
                    p.cmd()
                        .iter()
                        .map(|c| c.to_string_lossy().to_string())
                        .collect()
                } else {
                    Vec::new()
                };
                let pid = p.pid().as_u32() as i32;
                let start_time = p.start_time();
                // 上轮见过 → 差分有效；首见 → None（它可能正在满核编译，0 是假事实）
                let cpu = if prev_seen.contains(&(pid, start_time)) {
                    Some(p.cpu_usage())
                } else {
                    None
                };
                ProcMem {
                    pid,
                    ppid: p.parent().map(|x: Pid| x.as_u32() as i32).unwrap_or(0),
                    family,
                    name,
                    exe,
                    rss: p.memory(),
                    footprint: None,
                    swapped: None,
                    source: MemSource::Rss,
                    start_time,
                    cmd,
                    workspace: None,
                    cpu,
                    cpu_time_ms: p.accumulated_cpu_time(),
                }
            })
            .collect();
        // 用本轮集合整体替换（而非累积）：退出进程不留在表里，集合大小有界
        self.seen = out.iter().map(|p| (p.pid, p.start_time)).collect();

        let cpu_usage = if self.cpu_warmed {
            Some(self.sys.global_cpu_usage())
        } else {
            None
        };
        self.cpu_warmed = true;
        let cores = self.sys.cpus().len().max(1) as u32;
        // Windows 的 load_average 恒为 0（假事实）→ Rust 侧直接转 None，UI 隐藏
        #[cfg(windows)]
        let load = None;
        #[cfg(not(windows))]
        let load = {
            // sysinfo 0.39 的 load_average 是关联函数（不依赖 System 实例）
            let la = System::load_average();
            Some(CpuLoad {
                one: la.one,
                five: la.five,
                fifteen: la.fifteen,
            })
        };

        out.sort_by_key(|p| std::cmp::Reverse(p.rss));
        ScanResult {
            procs: out,
            cpu_usage,
            cores,
            load,
        }
    }

    pub fn total_memory(&self) -> u64 {
        self.sys.total_memory()
    }
}

/// 结束进程（跨平台）。
///
/// - macOS / Linux：`force = false` 发 SIGTERM（给应用保存数据的机会），`true` 发 SIGKILL。
/// - Windows：没有 SIGTERM 语义（`TerminateProcess` 相当于强杀），两种模式走同一条路径；
///   GUI 应用的「优雅关闭」需要 WM_CLOSE，属于后续增强，不在 W1 范围。
///
/// 调用方负责护栏（不杀系统关键进程 / 不杀自身），这里只做平台分派与错误翻译。
pub fn kill(pid: i32, force: bool) -> Result<(), String> {
    if pid <= 0 {
        return Err(format!("无效 pid：{pid}"));
    }
    #[cfg(unix)]
    {
        let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
        let rc = unsafe { libc::kill(pid, sig) };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            return Err(match e.kind() {
                std::io::ErrorKind::PermissionDenied => "没有权限结束该进程".to_string(),
                std::io::ErrorKind::NotFound => "进程已退出".to_string(),
                _ => format!("结束进程失败: {e}"),
            });
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = force;
        let pid = Pid::from_u32(pid as u32);
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        match sys.process(pid) {
            Some(p) if p.kill() => Ok(()),
            Some(_) => Err("结束进程失败：权限不足（可能需要管理员身份）".to_string()),
            None => Err("进程已退出".to_string()),
        }
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
            classify(
                "Google Chrome Helper",
                Some("/Applications/Google Chrome.app")
            ),
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
            classify(
                "helper-tool",
                Some("/Applications/Acme工具.app/Contents/MacOS/helper-tool")
            ),
            "Acme工具"
        );
    }

    #[test]
    fn classify_prefers_specific_rule_over_generic() {
        // "Code Helper" 必须归到 VS Code，而不是被别的规则抢走
        assert_eq!(
            classify(
                "Code Helper (Plugin)",
                Some("/Applications/Visual Studio Code.app/x")
            ),
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
        let result = s.scan();
        assert!(!result.procs.is_empty(), "至少应枚举到当前进程");
        assert!(s.total_memory() > 0);
        // 当前测试进程应当在列表里
        let me = std::process::id() as i32;
        assert!(
            result.procs.iter().any(|p| p.pid == me),
            "枚举结果应包含自身 pid"
        );
        // RSS 降序
        assert!(result.procs.windows(2).all(|w| w[0].rss >= w[1].rss));
        // 整机核数
        assert!(result.cores >= 1);
    }

    #[test]
    fn cpu_semantics_first_round_null_then_value() {
        // P0 正确性：首轮整机/进程 CPU 必须是 None（差分窗口未建立），第二轮才有值
        let mut s = Scanner::new();
        let first = s.scan();
        assert_eq!(
            first.cpu_usage, None,
            "首轮整机 CPU 是初始化采样，必须标 None"
        );
        let me = std::process::id() as i32;
        let first_self = first.procs.iter().find(|p| p.pid == me).unwrap();
        assert_eq!(
            first_self.cpu, None,
            "首轮自身进程 CPU 差分无效，必须标 None"
        );

        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let second = s.scan();
        assert!(second.cpu_usage.is_some(), "第二轮整机 CPU 应有读数");
        let second_self = second.procs.iter().find(|p| p.pid == me).unwrap();
        assert!(
            second_self.cpu.is_some(),
            "第二轮起进程 CPU 差分有效；真实 0% 是 Some(0.0)，不是 None"
        );
        assert!(
            second_self.cpu_time_ms > 0 || second_self.cpu == Some(0.0),
            "累计 CPU 时间应为非负事实值"
        );
    }
}
