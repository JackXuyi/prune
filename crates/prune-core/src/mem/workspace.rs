//! 工作区归属（M2.5）：把进程映射到它正在服务的项目目录。
//!
//! 目标问题：「Cursor 的这个进程对应桌面上哪个项目？」
//! 没有任何单一信号能稳定回答，所以按**置信度降序**融合三层：
//!
//! | 层 | 信号 | 置信度 | 局限 |
//! |---|---|---|---|
//! | 1 | `lsof` 打开的文件句柄集中在同一项目目录 | 精确（`Handle`） | 瞬态：只有正在读写文件的进程才持有句柄 |
//! | 2 | extension host 进程名里的窗口标签（`extension-host (user) AI [8-142]`） | 中（`Label`） | 只有标签，需用 workspaceStorage 解析成完整路径 |
//! | 3 | 子进程 `--clientProcessId=<exthost>` 继承父窗口 | 中（`Inherited`） | 依赖 argv 未被 setproctitle 覆盖 |
//!
//! 三条都对不上 → **不归属**（UI 显示「未归属」），绝不用历史记录冒充当前归属。
//!
//! 进程的 cwd 对 GUI 应用一律是 `/`（实测 Cursor 全部如此），因此不作为信号。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
// lsof 句柄扫描只在 unix 存在；Windows 走 cfg(windows) 的空表分支
#[cfg(unix)]
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::ProcMem;

/// 归属来源（置信度从高到低）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceSource {
    /// 打开的文件句柄落在同一项目目录下
    Handle,
    /// extension host 进程名里的窗口标签，经 workspaceStorage 解析
    Label,
    /// 子进程通过 `--clientProcessId` 继承父窗口
    Inherited,
}

/// 一个进程的工作区归属
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRef {
    /// 项目目录绝对路径
    pub path: String,
    /// 展示名（目录名）
    pub label: String,
    pub source: WorkspaceSource,
}

impl WorkspaceRef {
    pub fn new(path: impl Into<String>, source: WorkspaceSource) -> Self {
        let path = path.into();
        let label = Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        Self {
            path,
            label,
            source,
        }
    }
}

/// 只有这些家族谈得上「工作区」（决策 D-W1：不做全量通用启发式，避免错误归属）
pub fn workspace_capable(family: &str) -> bool {
    matches!(
        family,
        "Cursor" | "VS Code" | "JetBrains" | "Electron" | "Xcode"
    )
}

/// 单轮 lsof 扫描的进程上限（按 RSS 降序取，深探轮预算内）
pub const HANDLE_SCAN_MAX: usize = 24;
/// lsof 并发
pub const HANDLE_CONCURRENCY: usize = 12;

/// 从 extension host 的进程名里取窗口工作区标签。
///
/// 实测形态（Cursor 3.25 / VS Code）：
/// `Cursor Helper (Plugin): extension-host (user) AI [8-142]`
pub fn label_from_title(title: &str) -> Option<String> {
    let key = "extension-host";
    let idx = title.find(key)?;
    let rest = title[idx + key.len()..].trim_start();
    // 可能是 `(user)` / `(local)` / 直接接标签
    let rest = match rest.strip_prefix('(') {
        Some(r) => match r.find(')') {
            Some(p) => r[p + 1..].trim_start(),
            None => rest,
        },
        None => rest,
    };
    let rest = strip_trailing_counter(rest).trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// 去掉尾部形如 ` [8-142]` 的窗口计数（也可能紧贴标签：`[8-142]`）
fn strip_trailing_counter(s: &str) -> &str {
    let t = s.trim_end();
    let Some(open) = t.rfind('[') else {
        return t;
    };
    let Some(body) = t[open + 1..].strip_suffix(']') else {
        return t;
    };
    let looks_like_counter = !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == ':' || c == '.');
    if !looks_like_counter {
        return t;
    }
    // 只有前面是空白（或什么都没有）才当计数后缀，避免吃掉「项目[1]」这类真名字
    let before = &t[..open];
    if before.is_empty() || before.ends_with(' ') {
        before.trim_end()
    } else {
        t
    }
}

/// 从 argv 里取 `--clientProcessId=<pid>`
pub fn client_process_id(cmd: &[String]) -> Option<i32> {
    cmd.iter()
        .find_map(|a| a.strip_prefix("--clientProcessId="))
        .and_then(|v| v.parse::<i32>().ok())
}

/// workspaceStorage 里的一条静态映射
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownWorkspace {
    pub label: String,
    pub path: String,
    /// workspace.json 的修改时间（秒），用于「同名工作区优先取最近打开的」
    pub mtime: u64,
    /// 来源应用（Cursor / Code），仅用于排错
    pub app: String,
}

/// 解析 `workspace.json`：单根取 `folder`，多根工作区取 `workspace`
pub fn parse_workspace_json(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    for key in ["folder", "workspace"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            if let Some(p) = file_uri_to_path(s) {
                return Some(p);
            }
        }
    }
    None
}

/// `file:///Users/xuyi/Desktop/git/AI` → `/Users/xuyi/Desktop/git/AI`
///
/// Windows 的 VS Code 写的是 `file:///c%3A/Users/...`，解码后是 `/c:/Users/...`，
/// 多出来的前导斜杠会让 `Path::is_dir()` 判定失败，因此要把盘符前的斜杠去掉。
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // file://<host>/path —— 本机 host 为空
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) => &rest[i..],
        None => return None,
    };
    let path = percent_decode(path);
    #[cfg(windows)]
    {
        let b = path.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
            return Some(path[1..].to_string());
        }
    }
    Some(path)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 扫描已知的编辑器工作区（workspaceStorage/*/workspace.json）。
///
/// 只覆盖 Cursor 与 VS Code；读不到就当没有，不影响其它信号。
pub fn known_workspaces(home: &Path) -> Vec<KnownWorkspace> {
    let sups = [
        ("Cursor", "Cursor"),
        ("Code", "VS Code"),
        ("VSCodium", "VSCodium"),
    ];
    let mut out = Vec::new();
    let config_root = crate::paths::config_dir_in(home);
    for (dir, app) in sups {
        let root = config_root.join(dir).join("User/workspaceStorage");
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let wj = entry.path().join("workspace.json");
            let Ok(content) = std::fs::read_to_string(&wj) else {
                continue;
            };
            let Some(path) = parse_workspace_json(&content) else {
                continue;
            };
            let mtime = std::fs::metadata(&wj)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let label = Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            out.push(KnownWorkspace {
                label,
                path,
                mtime,
                app: app.to_string(),
            });
        }
    }
    // 同一路径可能被多次打开：保留最新的一条
    out.sort_by_key(|k| std::cmp::Reverse(k.mtime));
    let mut seen = std::collections::HashSet::new();
    out.retain(|k| seen.insert(k.path.clone()));
    out
}

/// 用标签解析出完整路径：先查 known 表，再退到几个常见根目录下按名字找。
pub fn resolve_label(label: &str, known: &[KnownWorkspace], home: &Path) -> Option<String> {
    // known 已按 mtime 降序，取第一个同名且目录仍存在的
    if let Some(k) = known
        .iter()
        .find(|k| k.label == label && Path::new(&k.path).is_dir())
    {
        return Some(k.path.clone());
    }
    let roots = [
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
        home.join("Projects"),
        home.join("git"),
        home.join("code"),
    ];
    for root in roots.iter().filter(|r| r.is_dir()) {
        if let Some(hit) = find_dir_named(root, label, 3) {
            return Some(hit);
        }
    }
    None
}

/// 在 root 下按目录名查找（深度受限，跳过隐藏目录与 node_modules）
fn find_dir_named(root: &Path, name: &str, depth: usize) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with('.') || n == "node_modules" {
            continue;
        }
        if n == name {
            return Some(p.to_string_lossy().to_string());
        }
        dirs.push(p);
    }
    for d in dirs {
        if let Some(hit) = find_dir_named(&d, name, depth - 1) {
            return Some(hit);
        }
    }
    None
}

/// 这些路径参与「项目目录」推断时会被忽略（系统/缓存/依赖）。
///
/// Windows 的路径分隔符是 `\`，因此匹配前统一归一化为 `/`，
/// 否则所有规则在 Windows 上都会静默失效（噪声目录会被当成项目目录）。
fn path_is_noise(p: &str, home: &str) -> bool {
    const NOISE: &[&str] = &[
        "/Library/",
        "/node_modules/",
        "/.git/",
        "/Caches/",
        "/Applications/",
        "/private/",
        "/System/",
        "/usr/",
        "/.Trash/",
        "/Workspace Storage/",
        "/.cursor/extensions/",
        "/.vscode/extensions/",
        // Windows 侧系统/缓存目录
        "/AppData/",
        "/Windows/",
        "/Program Files/",
        "/ProgramData/",
    ];
    let p = p.replace('\\', "/");
    let home = home.replace('\\', "/");
    if !p.starts_with(home.as_str()) {
        return true;
    }
    NOISE.iter().any(|n| p.contains(n))
}

/// 从一组文件句柄路径推断项目目录。
///
/// 返回 `None` 表示句柄跨了多个项目（无法归属）或落在噪声目录里。
pub fn common_project_root(paths: &[String], home: &Path) -> Option<String> {
    let home_str = home.to_string_lossy().to_string();
    let mut dirs: Vec<String> = paths
        .iter()
        .filter(|p| !path_is_noise(p, &home_str))
        .map(|p| {
            // 文件路径统一收敛到所在目录
            match Path::new(p).parent() {
                Some(d) => d.to_string_lossy().to_string(),
                None => p.clone(),
            }
        })
        .filter(|p| p.starts_with(&home_str) && p.len() > home_str.len() + 1)
        .collect();
    dirs.sort();
    dirs.dedup();
    if dirs.is_empty() {
        return None;
    }

    let home_depth = home.components().count();
    let split = |s: &str| -> Vec<String> {
        // 同时按 `/` 与 `\` 切分：Windows 路径是反斜杠，混用的字符串也能处理
        s.split(['/', '\\'])
            .filter(|x| !x.is_empty())
            .map(|x| x.to_string())
            .collect()
    };

    let mut common = split(&dirs[0]);
    for d in &dirs[1..] {
        let seg = split(d);
        let mut i = 0;
        while i < common.len().min(seg.len()) && common[i] == seg[i] {
            i += 1;
        }
        common.truncate(i);
    }

    // 公共祖先必须先落进 home 内部：home 本身 / home 下第一层一律不算项目
    if common.len() <= home_depth + 1 {
        return None;
    }
    let joined = common.join("/");
    // unix 需要补前导 `/`（绝对路径）；Windows 保留盘符形式（`C:/Users/...`）
    Some(if cfg!(windows) {
        joined
    } else {
        format!("/{joined}")
    })
}

/// 读某个进程打开的文件路径（`lsof -Fn` 的 `n` 行）。
///
/// macOS/Linux 走 `lsof`；Windows 没有 `lsof`，直接返回空表
/// （上层按「归属不可用」处理，见 W2 说明）。
pub fn open_file_paths(pid: i32) -> Vec<String> {
    #[cfg(not(windows))]
    {
        let out = Command::new("/usr/sbin/lsof")
            .args(["-w", "-n", "-P", "-p", &pid.to_string(), "-Fn"])
            .output();
        let Ok(out) = out else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.strip_prefix('n'))
            .filter(|p| Path::new(p).is_absolute())
            .map(|p| p.to_string())
            .collect()
    }
    #[cfg(windows)]
    {
        let _ = pid;
        Vec::new()
    }
}

/// 一批进程的工作区归属（深探轮调用，自建线程池并发跑 lsof）
///
/// 顺序：标签 → 继承 → 句柄。前两个免费，句柄最贵，所以只给尚未归属的进程跑，
/// 且按 RSS 降序限量 `HANDLE_SCAN_MAX` 个，保证深探轮预算不被拖垮。
pub fn resolve_many(procs: &[ProcMem], home: &Path) -> HashMap<i32, WorkspaceRef> {
    let known = known_workspaces(home);
    let mut out: HashMap<i32, WorkspaceRef> = HashMap::new();

    // 第 2 层：extension host 标签。
    // 注意：sysinfo 的 `name` 是 exec 名（"Cursor Helper (Plugin)"），
    // setproctitle 的窗口标题在 `cmd[0]`，必须读 argv 而不是 name。
    for p in procs.iter().filter(|p| workspace_capable(&p.family)) {
        let title = p.cmd.first().map(String::as_str).unwrap_or(p.name.as_str());
        if let Some(label) = label_from_title(title) {
            if let Some(path) = resolve_label(&label, &known, home) {
                out.insert(p.pid, WorkspaceRef::new(path, WorkspaceSource::Label));
            }
        }
    }

    // 第 3 层：子进程继承（--clientProcessId 指向已归属的 exthost）
    for p in procs.iter().filter(|p| workspace_capable(&p.family)) {
        if out.contains_key(&p.pid) {
            continue;
        }
        if let Some(parent) = client_process_id(&p.cmd) {
            if let Some(ws) = out.get(&parent) {
                out.insert(
                    p.pid,
                    WorkspaceRef {
                        source: WorkspaceSource::Inherited,
                        ..ws.clone()
                    },
                );
            }
        }
    }

    // 第 1 层：文件句柄（最贵，仅对仍未归属的重进程做）
    let mut pending: Vec<&ProcMem> = procs
        .iter()
        .filter(|p| workspace_capable(&p.family) && !out.contains_key(&p.pid))
        .collect();
    pending.sort_by_key(|p| std::cmp::Reverse(p.rss));
    pending.truncate(HANDLE_SCAN_MAX);
    let pids: Vec<i32> = pending.iter().map(|p| p.pid).collect();

    for (pid, paths) in scan_paths(&pids, HANDLE_CONCURRENCY) {
        if let Some(root) = common_project_root(&paths, home) {
            out.insert(pid, WorkspaceRef::new(root, WorkspaceSource::Handle));
        }
    }

    out
}

/// 并发跑 lsof（每批 HANDLE_CONCURRENCY 个）
fn scan_paths(pids: &[i32], concurrency: usize) -> Vec<(i32, Vec<String>)> {
    use std::sync::Mutex;

    if pids.is_empty() {
        return Vec::new();
    }
    let queue = Mutex::new(pids.iter().copied());
    let results: Mutex<Vec<(i32, Vec<String>)>> = Mutex::new(Vec::new());
    let workers = concurrency.clamp(1, pids.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.next()
                };
                let Some(pid) = next else { break };
                let paths = open_file_paths(pid);
                if !paths.is_empty() {
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((pid, paths));
                }
            });
        }
    });

    results.into_inner().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_parsing_cursor_and_vscode_titles() {
        assert_eq!(
            label_from_title("Cursor Helper (Plugin): extension-host (user) AI [8-142]"),
            Some("AI".to_string())
        );
        assert_eq!(
            label_from_title("Code Helper (Plugin): extension-host (user) prune [9-33]"),
            Some("prune".to_string())
        );
        // 没有标签的窗口（空窗口）不归属
        assert_eq!(
            label_from_title("Cursor Helper (Plugin): extension-host (user) [8-142]"),
            None
        );
        // 普通 helper 不参与
        assert_eq!(
            label_from_title("Cursor Helper: fileWatcher [8:abc]"),
            None
        );
        // 标签里带空格
        assert_eq!(
            label_from_title("Code Helper: extension-host (user) my project [8-1]"),
            Some("my project".to_string())
        );
    }

    #[test]
    fn client_pid_from_argv() {
        let cmd = vec![
            "/Applications/Cursor.app/helper/node".to_string(),
            "server.js".to_string(),
            "--node-ipc".to_string(),
            "--clientProcessId=173".to_string(),
        ];
        assert_eq!(client_process_id(&cmd), Some(173));
        assert_eq!(client_process_id(&["node".to_string()]), None);
    }

    #[test]
    fn workspace_json_folder_and_multicroot() {
        assert_eq!(
            parse_workspace_json(r#"{"folder":"file:///Users/xuyi/Desktop/git/AI"}"#),
            Some("/Users/xuyi/Desktop/git/AI".to_string())
        );
        assert_eq!(
            parse_workspace_json(r#"{"workspace":"file:///Users/xuyi/my%20repo/code-workspace"}"#),
            Some("/Users/xuyi/my repo/code-workspace".to_string())
        );
        assert_eq!(parse_workspace_json("{}"), None);
    }

    #[cfg(windows)]
    #[test]
    fn file_uri_strips_leading_slash_before_drive() {
        // VS Code 在 Windows 上写的是 file:///c%3A/...，解码后不能留前导斜杠
        assert_eq!(
            file_uri_to_path("file:///c%3A/Users/xuyi/git/AI").as_deref(),
            Some("c:/Users/xuyi/git/AI")
        );
    }

    #[test]
    fn common_root_picks_project_and_rejects_ambiguous() {
        // 用平台对应的 home 构造样本，Windows 上同样要能收敛到项目根
        let home = if cfg!(windows) {
            PathBuf::from("C:\\Users\\xuyi")
        } else {
            PathBuf::from("/Users/xuyi")
        };
        let norm = |p: PathBuf| p.to_string_lossy().replace('\\', "/");
        let proj = home.join("Desktop/git/AI");

        // 同一项目下的多个文件 → 收敛到项目根
        let paths = vec![
            norm(proj.join(".cursor/hooks.json")),
            norm(proj.join("docs/a.md")),
        ];
        assert_eq!(common_project_root(&paths, &home), Some(norm(proj.clone())));

        // 跨两个项目 → 公共祖先只是 git/，不能瞎猜
        let paths = vec![
            norm(proj.join("a.md")),
            norm(home.join("Desktop/git/prune/b.md")),
        ];
        assert_eq!(common_project_root(&paths, &home), None);

        // 只有缓存/依赖噪声 → 不归属
        let paths = vec![
            norm(home.join("Library/Application Support/Cursor/x.db")),
            norm(proj.join("node_modules/react/index.js")),
        ];
        assert_eq!(common_project_root(&paths, &home), None);
    }

    #[test]
    fn noise_paths_are_ignored() {
        let home = "/Users/xuyi";
        assert!(path_is_noise("/Applications/Cursor.app/x", home));
        assert!(path_is_noise("/Users/xuyi/Desktop/git/AI/node_modules/a.js", home));
        assert!(path_is_noise("/Users/xuyi/Desktop/git/AI/.git/FETCH_HEAD", home));
        assert!(!path_is_noise("/Users/xuyi/Desktop/git/AI/src/main.ts", home));
    }

    #[test]
    fn noise_paths_handle_windows_separators() {
        // 反斜杠路径必须同样命中规则，否则 Windows 上缓存目录会被当成项目
        let home = "C:\\Users\\xuyi";
        assert!(path_is_noise(
            "C:\\Users\\xuyi\\Desktop\\git\\AI\\node_modules\\a.js",
            home
        ));
        assert!(path_is_noise(
            "C:\\Users\\xuyi\\AppData\\Local\\Temp\\x.log",
            home
        ));
        assert!(path_is_noise("C:\\Windows\\System32\\drivers\\x.sys", home));
        assert!(!path_is_noise(
            "C:\\Users\\xuyi\\Desktop\\git\\AI\\src\\main.ts",
            home
        ));
    }

    fn proc(pid: i32, family: &str, name: &str, rss: u64, cmd: Vec<String>) -> ProcMem {
        ProcMem {
            pid,
            ppid: 1,
            name: name.to_string(),
            exe: None,
            family: family.to_string(),
            rss,
            footprint: None,
            swapped: None,
            source: super::super::MemSource::Rss,
            start_time: 0,
            cmd,
            workspace: None,
        }
    }

    #[test]
    fn resolve_inherits_client_process_workspace() {
        // 用临时 home 造一份 workspaceStorage，验证「标签 → 路径 → 子进程继承」全链路
        let tmp = std::env::temp_dir().join(format!("prune-ws-{}", std::process::id()));
        let proj = tmp.join("Desktop/git/AI");
        std::fs::create_dir_all(&proj).unwrap();
        let ws = crate::paths::config_dir_in(&tmp).join("Cursor/User/workspaceStorage/hash1");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("workspace.json"),
            format!(
                r#"{{"folder":"file://{}"}}"#,
                proj.to_string_lossy()
            ),
        )
        .unwrap();

        let procs = vec![
            proc(
                100,
                "Cursor",
                "Cursor Helper (Plugin): extension-host (user) AI [8-142]",
                500,
                vec![],
            ),
            proc(
                101,
                "Cursor",
                "Cursor Helper (Plugin)",
                100,
                vec!["--clientProcessId=100".to_string()],
            ),
        ];
        let resolved = resolve_many(&procs, &tmp);

        assert_eq!(
            resolved.get(&100).map(|w| (w.path.clone(), w.source)),
            Some((proj.to_string_lossy().to_string(), WorkspaceSource::Label))
        );
        assert_eq!(
            resolved.get(&101).map(|w| w.source),
            Some(WorkspaceSource::Inherited),
            "语言服务器应继承 exthost 的工作区"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
