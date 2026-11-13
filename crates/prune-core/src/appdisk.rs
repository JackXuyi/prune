//! M8：应用维度磁盘占用（三层口径）。
//!
//! | 层 | 路径 | 语义 |
//! |---|---|---|
//! | 本体 `bundle` | 进程 exe 里的 `X.app` | 应用安装体积，只读 |
//! | 缓存 `cache`  | `~/Library/Caches/<名>` | 可再生，清理入口归「存储」域 |
//! | 数据 `data`   | `~/Library/Application Support/<名>` | 用户数据，**永不给清理入口** |
//!
//! 三条硬约束（对应方案 §4 的假事实）：
//! 1. `du` 是文件名义大小之和，APFS 克隆/共享框架会被重复计入——UI 必须带脚注，
//!    不能让它冒充「删了能省这么多」。
//! 2. 测不出就是 `None`，**不是 0**。系统进程（`/usr/sbin/*`）没有 .app，CLI 工具
//!    也没有稳定缓存归属，UI 要显示「—」而不是「0 B」。
//! 3. 一次测量 0.25–3 s，只能按需触发 + TTL 缓存，绝不能进任何定时采样轮。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 缓存有效期：bundle 与缓存目录的大小是慢变量，30 分钟足够，
/// 用户真要最新值可以手动重测（UI 提供）。
pub const TTL: Duration = Duration::from_secs(30 * 60);

/// 一层磁盘读数
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskLeaf {
    pub path: String,
    pub bytes: u64,
    /// 超出时间预算、只量了一部分。UI 必须显示「≥」而不是精确等于
    #[serde(default)]
    pub partial: bool,
}

impl DiskLeaf {
    fn new(path: PathBuf, bytes: u64, partial: bool) -> Self {
        Self {
            path: path.to_string_lossy().to_string(),
            bytes,
            partial,
        }
    }
}

/// 一个应用的磁盘画像。三层各自独立 Option——一层失败不拖垮其余。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDisk {
    pub family: String,
    pub bundle: Option<DiskLeaf>,
    pub cache: Option<DiskLeaf>,
    pub data: Option<DiskLeaf>,
    /// 测量时刻（unix 秒）
    pub at: i64,
    pub elapsed_ms: u64,
    /// 为什么没有数据（给 UI 一句人话，别让用户猜）
    pub note: Option<String>,
}

impl AppDisk {
    /// 三层合计（仅用于展示「合计」；不等于可释放空间）
    pub fn total(&self) -> u64 {
        [
            self.bundle.as_ref(),
            self.cache.as_ref(),
            self.data.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|l| l.bytes)
        .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.bundle.is_none() && self.cache.is_none() && self.data.is_none()
    }
}

/// 从 exe 路径里切出 `.app` 这一段（`/Applications/Foo.app/Contents/MacOS/Foo` → `/Applications/Foo.app`）。
///
/// 只认 `.app` 结尾的目录段：Linux/Windows 没有这个概念，自然返回 None。
pub fn bundle_of_exe(exe: &str) -> Option<PathBuf> {
    let mut prefix = PathBuf::new();
    for seg in exe.split('/') {
        if seg.is_empty() {
            continue;
        }
        prefix.push(seg);
        if seg.ends_with(".app") && seg.len() > 4 {
            // exe 是绝对路径时 prefix 以根开头；相对路径时保持原样（不会出现，防御）
            return Some(if exe.starts_with('/') {
                Path::new("/").join(&prefix)
            } else {
                prefix
            });
        }
    }
    None
}

/// 单个文件的**实际占用**字节数。
///
/// 关键坑（实测踩到）：Docker 的 `Docker.raw` 是 **1 TiB 稀疏文件**，`metadata.len()`
/// 会报 1099.5 GB，而它实占 3.1 GB。用逻辑长度做磁盘读数会得出荒谬结论，
/// 所以 unix 走 `st_blocks * 512`（已分配块），Windows 没有 blocks 概念，退回 len。
#[inline]
fn file_bytes(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // st_blocks 单位固定是 512 字节（POSIX），与 st_blksize 无关
        meta.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

/// 递归统计目录体积（单线程版，供测试与小目录使用）。
pub fn du(path: &Path) -> u64 {
    du_budget(path, None).0
}

/// 带时间预算的递归统计。
///
/// 返回 `(已统计字节, 是否超预算)`。超预算时**保留已量到的部分**：
/// Chrome 的 profile 有近百万小文件，实测单线程 48 s、系统 `du -sk` 96 s——
/// 为了一个读数让用户等一分半不可接受，所以宁可给「≥ 12 GB（未测完）」。
///
/// 顶层子项并行（macOS 的 APFS 对这种海量小文件并行 stat 收益明显）。
pub fn du_budget(path: &Path, budget: Option<Duration>) -> (u64, bool) {
    let deadline = budget.map(|b| Instant::now() + b);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, false);
    };
    if meta.file_type().is_symlink() {
        return (0, false);
    }
    if meta.is_file() {
        return (file_bytes(&meta), false);
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return (0, false);
    };
    let children: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    if children.is_empty() {
        return (0, false);
    }

    // 顶层并行：子项数很少时没必要开线程（线程启动比 stat 还贵）
    let workers = children.len().clamp(1, 8);
    let queue = std::sync::Mutex::new(children.into_iter());
    let total = std::sync::atomic::AtomicU64::new(0);
    let truncated = std::sync::atomic::AtomicBool::new(false);

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.next()
                };
                let Some(child) = next else { break };
                if deadline.map(|d| Instant::now() > d).unwrap_or(false) {
                    truncated.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let (bytes, cut) = walk(&child, deadline);
                if cut {
                    truncated.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                total.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
            });
        }
    });

    (
        total.into_inner(),
        truncated.into_inner() && deadline.is_some(),
    )
}

/// 单线程递归（栈式，避免深目录爆栈）。超预算时立刻返回已量到的部分。
fn walk(root: &Path, deadline: Option<Instant>) -> (u64, bool) {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0u32;
    while let Some(path) = stack.pop() {
        seen += 1;
        // 每 256 个条目查一次表，比每个都查 Instant::now() 便宜得多
        if seen.is_multiple_of(256) {
            if let Some(d) = deadline {
                if Instant::now() > d {
                    return (total, true);
                }
            }
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            total = total.saturating_add(file_bytes(&meta));
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&path) {
            for entry in rd.flatten() {
                stack.push(entry.path());
            }
        }
    }
    (total, false)
}

/// 从 `Info.plist` 取 bundle id（`defaults read` 是唯一不需要 plist 依赖的办法，
/// 系统自带、~15 ms，而且只在按名匹配失败时才调用一次）。
fn bundle_id_of(app: &Path) -> Option<String> {
    let info = app.join("Contents").join("Info");
    let out = std::process::Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(&info)
        .arg("CFBundleIdentifier")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if id.is_empty() || id.contains(' ') {
        None
    } else {
        Some(id)
    }
}

/// 单层测量的时间预算。
///
/// 实测标定：Cursor 的 35 GB 数据目录 1.6 s（并行后）内完成、Chrome 的 13 GB
/// 镜像目录远超预算（单线程 48 s）。4 s 是「用户还愿意等」与「能测完常见应用」
/// 的折中：超了就给下界（`partial`），不阻塞 UI。
pub const BUDGET: Duration = Duration::from_secs(4);

/// 目录存在且是目录才算命中（不存在 = 该应用没有这一层，返回 None）
fn probe(dir: Option<PathBuf>) -> Option<DiskLeaf> {
    let dir = dir?;
    let meta = std::fs::symlink_metadata(&dir).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let (bytes, partial) = du_budget(&dir, Some(BUDGET));
    Some(DiskLeaf::new(dir, bytes, partial))
}

/// 候选目录名（按优先级，去重）。
///
/// 只靠家族名会漏掉一大票真实应用——实测：
/// - Chrome 用 `Google/Chrome`（厂商名 + 产品名两层）
/// - Docker 用 `com.docker.docker`（bundle id 全名）
/// - Cursor 用 `Cursor`（家族名，能中）
///
/// 所以按 bundle id 拆出四种形态依次试。**只做精确目录名匹配**，
/// 不做模糊匹配：宁可显示「—」，也不能把别人的 30 GB 算到这个应用头上。
fn dir_candidates(family: &str, bundle_id: Option<&str>) -> Vec<String> {
    let mut out = vec![family.to_string()];
    if let Some(id) = bundle_id {
        let last = id.rsplit('.').next().unwrap_or(id);
        if last.len() > 1 && !last.eq_ignore_ascii_case(family) {
            out.push(last.to_string());
        }
        if id.len() > 1 && id != family {
            out.push(id.to_string());
        }
        // `com.google.Chrome` → `Google/Chrome`：macOS 上厂商目录 + 产品目录的经典布局
        let parts: Vec<&str> = id.split('.').collect();
        if parts.len() >= 3 {
            let vendor = parts[parts.len() - 2];
            if vendor.len() > 1 && last.len() > 1 {
                out.push(format!("{vendor}/{last}"));
            }
        }
    }
    out.dedup();
    out
}

/// 在若干根目录里按候选名找第一命中
fn first_hit(roots: &[PathBuf], names: &[String]) -> Option<DiskLeaf> {
    for root in roots {
        for name in names {
            if let Some(hit) = probe(Some(root.join(name))) {
                return Some(hit);
            }
        }
    }
    None
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 测量一个应用的三层磁盘占用。
///
/// `exe` 取家族里任一成员的 exe 路径（GUI 应用每个成员都在同一个 .app 内）。
/// 三层并发测量：最贵的 data 层（实测 35 GB / 3.2 s）单独跑，不与另两层排队。
pub fn measure(family: &str, exe: Option<&str>, home: &Path) -> AppDisk {
    let started = Instant::now();

    let bundle_path = exe.and_then(bundle_of_exe);
    let lib = home.join("Library");
    let id = bundle_path.as_ref().and_then(|app| bundle_id_of(app));
    let names = dir_candidates(family, id.as_deref());

    // 缓存：Caches（Electron 主战场）→ Containers（沙盒应用的缓存也在里面）
    let cache_roots = [lib.join("Caches"), lib.join("Containers")];
    // 数据：App Support（Electron 主战场）→ Containers / Group Containers（沙盒应用）
    let data_roots = [
        lib.join("Application Support"),
        lib.join("Containers"),
        lib.join("Group Containers"),
    ];

    let (bundle, cache, data) = std::thread::scope(|s| {
        let bp = bundle_path.clone();
        let b = s.spawn(move || {
            let p = bp?;
            let meta = std::fs::symlink_metadata(&p).ok()?;
            if !meta.is_dir() {
                return None;
            }
            let (bytes, partial) = du_budget(&p, Some(BUDGET));
            Some(DiskLeaf::new(p, bytes, partial))
        });
        let c = s.spawn(|| first_hit(&cache_roots, &names));
        let d = s.spawn(|| first_hit(&data_roots, &names));
        (
            b.join().ok().flatten(),
            c.join().ok().flatten(),
            d.join().ok().flatten(),
        )
    });

    // Containers 同时出现在缓存与数据两个候选根里：同一路径只保留在「数据」层，
    // 否则一个 10 GB 容器会被显示成 20 GB。
    let (cache, data) = match (cache, data) {
        (Some(c), Some(d)) if c.path == d.path => (None, Some(d)),
        pair => pair,
    };

    let note = if bundle.is_none() && cache.is_none() && data.is_none() {
        Some(if bundle_path.is_some() {
            "无法读取应用目录（权限不足）".to_string()
        } else {
            "无 .app bundle（系统进程或命令行工具）".to_string()
        })
    } else {
        None
    };

    AppDisk {
        family: family.to_string(),
        bundle,
        cache,
        data,
        at: now_secs(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        note,
    }
}

/// 按家族的测量缓存（TTL + bundle mtime 校验）。
///
/// 存在的理由很实在：用户反复展开同一个应用卡时不该反复 du 一个 35 GB 的目录。
#[derive(Default)]
pub struct AppDiskCache {
    entries: HashMap<String, (AppDisk, Instant)>,
}

impl AppDiskCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取缓存；过期或 bundle 目录 mtime 变化则重测。
    pub fn get_or_measure(&mut self, family: &str, exe: Option<&str>, home: &Path) -> AppDisk {
        if let Some((cached, at)) = self.entries.get(family) {
            if at.elapsed() < TTL && !bundle_changed(cached, exe) {
                return cached.clone();
            }
        }
        let fresh = measure(family, exe, home);
        self.entries
            .insert(family.to_string(), (fresh.clone(), Instant::now()));
        fresh
    }

    /// 强制重测（UI 上的「重新测量」）
    pub fn invalidate(&mut self, family: &str) {
        self.entries.remove(family);
    }
}

/// bundle 目录被替换/升级后，缓存的体积就不成立了。
/// 只 stat 一次目录，代价可忽略。
fn bundle_changed(cached: &AppDisk, exe: Option<&str>) -> bool {
    let Some(path) = exe.and_then(bundle_of_exe) else {
        return false;
    };
    match (
        std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
        cached.bundle.as_ref().map(|b| b.path.as_str()),
    ) {
        // 缓存的 bundle 路径与本轮解析出的不是同一个 → 应用被移动/重装
        (_, Some(cached_path)) if cached_path != path.to_string_lossy() => true,
        // mtime 晚于测量时刻 → 应用更新过，体积可能变了
        (Some(mtime), _) => mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64 > cached.at)
            .unwrap_or(false),
        _ => false,
    }
}

// ---------------------------------------------------------------- M10 批量本体测量

/// 一个「应用 → exe」查询。前端本来就有进程明细，直接给 exe，后端不再枚举进程。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleQuery {
    pub family: String,
    pub exe: Option<String>,
}

/// 只含本体的体积读数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSize {
    pub bytes: u64,
    /// 超预算只量了一部分 → UI 显示「≥」
    pub partial: bool,
}

/// 批量测得一批应用的**本体**体积（M10 卡片首屏用）。
///
/// 只测 `.app` 本体，不碰缓存/数据：卡片每张都要一个数，而缓存/数据层
/// 0.25–4 s/应用（Chrome 的 profile 能撞满 4 s 预算），进不了首屏。
///
/// 两个必须做的优化：
/// 1. **按 bundle 路径去重**——Chrome 的 42 个帮助进程指向同一个 `.app`，
///    不去重就会把同一个 1.5 GB 量 42 次；
/// 2. **8 路并行**——实测 20 个 `.app` 串行 1.96 s，并行后 ~0.5 s。
pub fn bundle_bytes_batch(
    items: &[BundleQuery],
    per_item: Duration,
) -> HashMap<String, BundleSize> {
    // 路径 → 家族列表（同一个 bundle 可能被多个家族共享）
    let mut by_path: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for item in items {
        if let Some(path) = item.exe.as_deref().and_then(bundle_of_exe) {
            by_path.entry(path).or_default().push(item.family.clone());
        }
    }

    let queue = std::sync::Mutex::new(by_path.into_iter());
    let out = std::sync::Mutex::new(HashMap::new());

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.next()
                };
                let Some((path, families)) = next else { break };
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if !meta.is_dir() {
                    continue;
                }
                let (bytes, partial) = du_budget(&path, Some(per_item));
                let mut guard = out.lock().unwrap_or_else(|e| e.into_inner());
                for family in families {
                    guard.insert(family, BundleSize { bytes, partial });
                }
            });
        }
    });

    out.into_inner().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_path_from_exe() {
        assert_eq!(
            bundle_of_exe("/Applications/Cursor.app/Contents/MacOS/Cursor"),
            Some(PathBuf::from("/Applications/Cursor.app"))
        );
        // 带空格的路径不能因为 split('/') 出问题
        assert_eq!(
            bundle_of_exe("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"),
            Some(PathBuf::from("/Applications/Visual Studio Code.app"))
        );
        // 系统进程 / CLI：没有 .app
        assert_eq!(bundle_of_exe("/usr/sbin/rapportd"), None);
        assert_eq!(bundle_of_exe("/opt/homebrew/bin/node"), None);
        // 名字里带 .app 但不是 bundle 目录（后缀必须是整段）
        assert_eq!(bundle_of_exe("/tmp/my.app.backup/bin/x"), None);
    }

    #[test]
    fn du_counts_files_not_symlinks() {
        let root = std::env::temp_dir().join(format!("prune-appdisk-{}", std::process::id()));
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join("a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(sub.join("b.bin"), vec![0u8; 2000]).unwrap();
        let total = du(&root);
        assert!(total >= 3000, "实际 {total}");

        // 符号链接不跟随：加一条指向自己的链接，体积不应翻倍
        #[cfg(unix)]
        {
            let link = root.join("loop");
            let _ = std::os::unix::fs::symlink(&root, &link);
            assert_eq!(du(&root), total);
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_dirs_are_none_not_zero() {
        let home = std::env::temp_dir().join(format!("prune-appdisk-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let d = measure("NoSuchApp", None, &home);
        assert!(d.bundle.is_none() && d.cache.is_none() && d.data.is_none());
        // 关键：没有数据必须有解释，UI 不能显示「0 B」
        assert_eq!(
            d.note.as_deref(),
            Some("无 .app bundle（系统进程或命令行工具）")
        );
        assert!(d.is_empty());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn cache_layers_resolve_by_family_name() {
        let home = std::env::temp_dir().join(format!("prune-appdisk-hit-{}", std::process::id()));
        let caches = home.join("Library").join("Caches").join("FakeApp");
        let data = home
            .join("Library")
            .join("Application Support")
            .join("FakeApp");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(caches.join("c.bin"), vec![0u8; 500]).unwrap();
        std::fs::write(data.join("d.bin"), vec![0u8; 700]).unwrap();

        let d = measure("FakeApp", None, &home);
        // unix 按「实占块」计算（st_blocks * 512），所以 500 字节的文件是 4096：
        // 这正是我们要的口径——稀疏文件的逻辑长度会骗人，块不会。
        let cache_bytes = d.cache.as_ref().map(|l| l.bytes).unwrap_or(0);
        let data_bytes = d.data.as_ref().map(|l| l.bytes).unwrap_or(0);
        assert!(cache_bytes >= 500, "cache 层应命中且不小于文件长度");
        assert!(data_bytes >= 700, "data 层应命中且不小于文件长度");
        assert_eq!(d.total(), cache_bytes + data_bytes);
        assert!(d.note.is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn cache_reuses_within_ttl() {
        let mut cache = AppDiskCache::new();
        let home = std::env::temp_dir().join(format!("prune-appdisk-ttl-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let first = cache.get_or_measure("X", None, &home);
        let second = cache.get_or_measure("X", None, &home);
        assert_eq!(first.at, second.at, "TTL 内应直接复用，不重测");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn batch_measures_shared_bundle_once() {
        // 同一 .app 被多个家族共享（Chrome 的 42 个帮助进程）：必须能对上同一个读数
        let root = std::env::temp_dir().join(format!("prune-batch-{}", std::process::id()));
        let app = root.join("Fake.app");
        std::fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        std::fs::write(
            app.join("Contents").join("MacOS").join("Fake"),
            vec![0u8; 4096],
        )
        .unwrap();

        let exe = app.join("Contents").join("MacOS").join("Fake");
        let exe_s = exe.to_string_lossy().to_string();
        let items = vec![
            BundleQuery {
                family: "Fake".into(),
                exe: Some(exe_s.clone()),
            },
            BundleQuery {
                family: "Fake Helper".into(),
                exe: Some(exe_s),
            },
        ];
        let got = bundle_bytes_batch(&items, Duration::from_secs(2));
        assert_eq!(got.len(), 2);
        assert_eq!(got["Fake"].bytes, got["Fake Helper"].bytes);
        assert!(got["Fake"].bytes >= 4096);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn batch_skips_apps_without_bundle() {
        // CLI 工具 / 系统进程没有 .app：没有条目 ≠ 0 字节，前端要显示「—」
        let items = vec![
            BundleQuery {
                family: "node".into(),
                exe: Some("/opt/homebrew/bin/node".into()),
            },
            BundleQuery {
                family: "unknown".into(),
                exe: None,
            },
        ];
        assert!(bundle_bytes_batch(&items, Duration::from_secs(1)).is_empty());
    }
}
