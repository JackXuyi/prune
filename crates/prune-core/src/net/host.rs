//! 端口 → 项目目录归属（M7 §8 P0）。
//!
//! 只有 CLI 类进程的 cwd 才谈得上「项目」：dev server 的 cwd 通常就是项目根
//! （实测：端口 3118 的 node → `~/Desktop/git/meetily/frontend`，端口 1430 → `~/Desktop/git/prune`）。
//! GUI 应用的 cwd 不可信——实测微信是 `~/Library/Containers/com.tencent.xinWeChat/Data`，
//! 若照单全收，每个应用都会多出一个假项目。
//!
//! 因此规则是**排除式**的：落在 `/`、`~/Library/`、`/System`、`/private` 下的一律不归属。
//! 这与 `mem/workspace.rs` 的「三条都对不上就不归属」同一条纪律：宁可不给，不给错的。

use std::path::Path;

/// 单进程 cwd 查询（macOS / Linux）。`lsof -a -d cwd` 实测 20 ms。
#[cfg(not(windows))]
pub fn project_dir(pid: i32) -> Option<String> {
    use std::process::Command;

    let out = Command::new(super::ports::lsof_path())
        .args(["-w", "-n", "-P", "-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text.lines().filter_map(|l| l.strip_prefix('n')).next()?;
    sanitize(path)
}

/// Windows 没有 `lsof`，也不提供 cwd 的等价查询（需读 PEB，代价与风险都不值）。
#[cfg(windows)]
pub fn project_dir(pid: i32) -> Option<String> {
    let _ = pid;
    None
}

/// cwd 是否值得当「项目」——排除式判定，见模块注释。
pub fn sanitize(path: &str) -> Option<String> {
    if !Path::new(path).is_absolute() {
        return None;
    }
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return None;
    }
    // 系统与应用容器目录：不是用户项目
    const DENY_PREFIX: &[&str] = &[
        "/System",
        "/private",
        "/usr",
        "/Library",
        "/Applications",
        "/cores",
        "/dev",
        "/Volumes",
    ];
    if DENY_PREFIX.iter().any(|d| p == *d || p.starts_with(&format!("{d}/"))) {
        return None;
    }
    // ~/Library/... （App Sandbox 容器、缓存、日志）
    if let Some(home) = home_dir() {
        let lib = format!("{home}/Library");
        if p == lib || p.starts_with(&format!("{lib}/")) {
            return None;
        }
        // 家目录本身不是项目
        if p == home {
            return None;
        }
    }
    Some(p.to_string())
}

/// 家目录：统一走 [`crate::paths::home_dir`]（Windows 上 USERPROFILE 优先，
/// 避免被 Git for Windows / MSYS 改写的 HOME 指错）。
///
/// 此前这里自己读 env 且 HOME 优先，与清理白名单用的 paths 口径相反：
/// Windows 上「项目归属」和「白名单根」可能不是同一个目录，sanitize 的
/// ~/Library 拒绝判断也会用错前缀。项目归属必须与白名单同源。
pub fn home_dir() -> Option<String> {
    crate::paths::home_dir().map(|p| p.to_string_lossy().into_owned())
}

/// 展示名：优先「父/子」两段，父目录是容器目录时只取最后一段。
///
/// - `~/Desktop/git/prune` → `prune`（`git` 是容器目录，没有信息量）
/// - `~/Desktop/git/meetily/frontend` → `meetily/frontend`（`frontend` 单独出现无法识别）
pub fn label_of(path: &str, home: Option<&str>) -> String {
    let rel = match home {
        Some(h) if path.starts_with(h) => path[h.len()..].trim_start_matches('/'),
        _ => path.trim_start_matches('/'),
    };
    let segs: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return path.to_string();
    }
    let last = segs[segs.len() - 1];
    if segs.len() >= 2 {
        let parent = segs[segs.len() - 2];
        if !is_container(parent) {
            return format!("{parent}/{last}");
        }
    }
    last.to_string()
}

/// 没有信息量的容器目录（项目放在它们里面，名字本身不说明是哪个项目）
const CONTAINER_DIRS: &[&str] = &[
    "git",
    "desktop",
    "documents",
    "downloads",
    "projects",
    "project",
    "code",
    "repos",
    "repo",
    "work",
    "workspace",
    "workspaces",
    "dev",
    "src",
    "www",
    "sites",
];

fn is_container(seg: &str) -> bool {
    CONTAINER_DIRS.iter().any(|c| c.eq_ignore_ascii_case(seg))
}

/// 一次查询一批 pid 的 cwd（自建线程池，模式同 `mem/workspace.rs::scan_paths`）。
pub fn resolve_many(pids: &[i32], concurrency: usize) -> std::collections::HashMap<i32, String> {
    use std::collections::HashMap;
    use std::sync::Mutex;

    if pids.is_empty() {
        return HashMap::new();
    }
    let queue = Mutex::new(pids.iter().copied());
    let results: Mutex<HashMap<i32, String>> = Mutex::new(HashMap::new());
    let workers = concurrency.clamp(1, pids.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.next()
                };
                let Some(pid) = next else { break };
                if let Some(dir) = project_dir(pid) {
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(pid, dir);
                }
            });
        }
    });

    results.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// cwd 归属的跨轮缓存。
///
/// 为什么要缓存：dev server 的 cwd 在它活着的整个生命周期里不变，而一次 cwd
/// 查询要 20 ms（fork + exec lsof）。本机实测冷扫 137 ms、热扫 ~50 ms 的差别
/// 就在这里——用户每点一次刷新都重查是纯浪费。
///
/// 失效规则（两条同时成立才可信）：
/// - `pid` 已不在本轮监听集合里 → 立即丢弃（端口换人/进程退出）；
/// - 超过 `TTL` → 重查（覆盖 pid 复用这种极端情况）。
pub struct ProjectCache {
    map: std::collections::HashMap<i32, (String, std::time::Instant)>,
}

impl Default for ProjectCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectCache {
    /// 缓存有效期：5 min。与深探轮同频，避免长时间只吃旧值。
    const TTL: std::time::Duration = std::time::Duration::from_secs(300);

    pub fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
        }
    }

    /// 返回 `pids` 里所有已知的 cwd（命中缓存 + 本轮新查的）。
    pub fn resolve(
        &mut self,
        pids: &[i32],
        concurrency: usize,
    ) -> std::collections::HashMap<i32, String> {
        // 丢掉已经不在监听集合里的 pid，缓存不会随运行时间无限增长
        self.map.retain(|p, _| pids.contains(p));

        let now = std::time::Instant::now();
        let missing: Vec<i32> = pids
            .iter()
            .copied()
            .filter(|p| match self.map.get(p) {
                Some((_, at)) => now.duration_since(*at) > Self::TTL,
                None => true,
            })
            .collect();

        for (pid, dir) in resolve_many(&missing, concurrency) {
            self.map.insert(pid, (dir, now));
        }
        // 查不到 cwd 的进程也记一笔空值，避免每轮反复重试（TTL 到期再试）
        for pid in &missing {
            self.map
                .entry(*pid)
                .or_insert_with(|| (String::new(), now));
        }

        pids.iter()
            .filter_map(|p| {
                self.map
                    .get(p)
                    .filter(|(dir, _)| !dir.is_empty())
                    .map(|(dir, _)| (*p, dir.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_project_cwds() {
        assert_eq!(sanitize("/"), None);
        assert_eq!(sanitize("/System/Library/CoreServices"), None);
        assert_eq!(sanitize("/private/var/db"), None);
        assert_eq!(sanitize("relative/path"), None);
        assert_eq!(sanitize("/Users/xuyi/Library/Containers/com.tencent.xinWeChat"), None);
    }

    #[test]
    fn keeps_real_project_cwds() {
        assert_eq!(
            sanitize("/Users/xuyi/Desktop/git/prune"),
            Some("/Users/xuyi/Desktop/git/prune".to_string())
        );
    }

    #[test]
    fn label_uses_parent_when_informative() {
        let home = Some("/Users/xuyi");
        assert_eq!(label_of("/Users/xuyi/Desktop/git/prune", home), "prune");
        assert_eq!(
            label_of("/Users/xuyi/Desktop/git/meetily/frontend", home),
            "meetily/frontend"
        );
        // 非家目录路径也能给出可读名
        assert_eq!(label_of("/opt/services/api", None), "services/api");
    }
}
