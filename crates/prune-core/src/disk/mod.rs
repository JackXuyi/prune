//! 定点白名单扫描与可回收报告装配（M1/T-302 + T-304）。
//!
//! 扫描对象只有 `whitelist::ENTRIES` 登记的目录（加上按项目根发现的 node_modules），
//! **不是全盘扫描**——这既是性能选择，也是安全边界：报告里的每一条都可追溯到白名单根。
//!
//! 体积口径：**物理分配块**（`st_blocks × 512`，与 `du` 一致）。
//! 不用 `metadata.len()`：Docker VM 的 `Docker.raw` 是稀疏文件，apparent size 会算出
//! 1099 GB（实测），而真实占用约 3 GB —— 虚高的数字会直接毁掉首屏可信度。

pub mod incremental;
pub mod project;
pub mod whitelist;

pub use incremental::{EntryFingerprint, ScanSnapshot};
pub use project::{ProjectHealth, ProjectVerdict};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::classify::{self, Arch, Verdict};
use whitelist::Level;

/// 扫描并发（元数据密集，12 路实测优于 8 路；再高收益递减）
pub const SCAN_CONCURRENCY: usize = 12;
/// 项目根发现 node_modules 的深度上限
pub const NODE_MODULES_MAX_DEPTH: usize = 6;

// ---------------------------------------------------------------- 数据模型

/// 一条可回收条目（A1 条目级判定的最小单位）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadweightItem {
    /// 目录名（Yarn 条目名）
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub platform: Option<String>,
    pub verdict: Verdict,
}

/// 平台分布
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStat {
    pub platform: String,
    pub entry_count: usize,
    pub bytes: u64,
}

/// A1 统计（仅对有平台元数据的组存在）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadweightStats {
    /// 🟢 本机永远加载不了
    pub bytes: u64,
    /// 本机原生可加载的同类包（darwin-arm64）——浪费比的分母只算这一部分
    pub native_bytes: u64,
    /// 其余条目（无平台词的普通包 + 🟡 待确认的 darwin-x64）
    pub other_bytes: u64,
    pub entry_count: usize,
    pub by_platform: Vec<PlatformStat>,
    /// 按体积降序，前端取前 10 做「最大的一个」展示
    pub items: Vec<DeadweightItem>,
    /// 浪费比 = 死重 / 原生可用（原生为 0 时 None，不编造比例）
    pub ratio: Option<f64>,
}

/// A4/A5 明细条目：报告里逐项展示的实体（node_modules 目录 / 模拟器设备 / 浏览器二进制）。
///
/// `path` 同时是清理时的「报告派生白名单」成员（见 `reclaim::is_report_detail_path`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailItem {
    /// 展示名（相对 home 的项目路径 / 设备名 / chromium-1155 等）
    pub name: String,
    pub path: String,
    pub bytes: u64,
    /// 距上次改动的天数（mtime；UI 对 >180 天标灰）
    pub age_days: u64,
    /// 附加信息（Simulator：机型 · 运行时）
    pub note: Option<String>,
}

/// 一组缓存
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheGroup {
    pub key: String,
    pub label: String,
    pub root: String,
    pub level: Level,
    pub total_bytes: u64,
    pub entry_count: usize,
    /// 是否能作为一个整体被清理。
    ///
    /// 聚合出来的分组（如「55 个项目的 node_modules」）没有单一可删除的根，
    /// 置 false —— 否则 `preview` 会把 `root`（= home）当成待删项，
    /// 既误导用户，执行时也必然被白名单拒绝。
    #[serde(default = "default_true")]
    pub cleanable: bool,
    /// A4/A5 明细（serde default：旧报告反序列化为空）
    #[serde(default)]
    pub details: Vec<DetailItem>,
    pub deadweight: Option<DeadweightStats>,
}

fn default_true() -> bool {
    true
}

/// 三管理器重复（A3）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DupSummary {
    /// (展示名, 体积)，按体积降序
    pub managers: Vec<(String, u64)>,
    pub total_bytes: u64,
    /// 合并建议文案（不用「删除」，用「保留一个即可」）
    pub advice: String,
}

/// 分层合计
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    /// L1 永远加载不了
    pub deadweight: u64,
    pub l2: u64,
    pub l3: u64,
    /// L4 只读（不含在主数字里）
    pub l4: u64,
    /// 主数字 = L1 + L2 + L3
    pub reclaimable: u64,
    /// 所有组的总和（含 L4，用于「缓存合计」展示）
    pub cache_total: u64,
}

/// 卷容量（M4/S0）：存储域的分母。展示用 `user_available`——
/// APFS 的 purgeable 空间会让「内核视角可释放」大于「用户可写」，两者分开留字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    /// 卷总容量
    pub total_bytes: u64,
    /// 剩余空间（含系统可回收的 purgeable，偏乐观）
    pub available_bytes: u64,
    /// 用户视角剩余（statvfs f_bavail，UI 主读数）
    pub user_available_bytes: u64,
}

/// 取 `path` 所在卷的容量。
///
/// - unix：`statvfs`（`user_available` 用 `f_bavail`，区别于含 purgeable 的 `f_bfree`）
/// - Windows：`sysinfo::Disks`，没有 bfree/bavail 之分，两者取同一个值
/// - 失败返回 None（调用方降级为无容量上下文）
pub fn volume_info(path: &Path) -> Option<VolumeInfo> {
    #[cfg(unix)]
    {
        let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
        if rc != 0 || st.f_frsize == 0 {
            return None;
        }
        let fr = st.f_frsize as u64;
        Some(VolumeInfo {
            total_bytes: (st.f_blocks as u64).saturating_mul(fr),
            available_bytes: (st.f_bfree as u64).saturating_mul(fr),
            user_available_bytes: (st.f_bavail as u64).saturating_mul(fr),
        })
    }
    #[cfg(not(unix))]
    {
        // 取挂载点最长（最具体）的那块盘：C:\ 与 D:\ 都可能匹配，需选最贴近 path 的
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let best = disks
            .list()
            .iter()
            .filter(|d| path.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())?;
        let available = best.available_space();
        Some(VolumeInfo {
            total_bytes: best.total_space(),
            available_bytes: available,
            user_available_bytes: available,
        })
    }
}

/// 一次扫描的完整结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskReport {
    pub scanned_at: i64,
    pub scan_elapsed_ms: u64,
    /// A7：本轮是否增量扫描（false = 全量；旧报告反序列化缺省为 false）
    #[serde(default)]
    pub incremental: bool,
    /// A1 组在前，随后按级别汇总
    pub groups: Vec<CacheGroup>,
    pub dup: Option<DupSummary>,
    pub totals: Totals,
    /// M4/S0：home 所在卷容量（statvfs 失败为 None；旧报告反序列化缺省为 None）
    #[serde(default)]
    pub volume: Option<VolumeInfo>,
    /// F9/M24：按项目的依赖账本（旧报告反序列化为空）
    #[serde(default)]
    pub projects: Vec<ProjectHealth>,
}

/// 扫描进度（前端进度条 + 心跳日志）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub bytes_so_far: u64,
    /// 从扫描开始到此刻的耗时（用于定位慢阶段，不参与 UI 逻辑）
    pub elapsed_ms: u64,
}

pub type ProgressFn<'a> = dyn Fn(ScanProgress) + Sync + 'a;

// ---------------------------------------------------------------- 扫描原语

/// 单个文件的占用字节：优先物理块（与 `du` 同口径，抗稀疏文件），拿不到退回逻辑大小。
fn file_bytes(md: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let blocks = md.blocks();
        if blocks > 0 {
            return blocks.saturating_mul(512);
        }
    }
    md.len()
}

/// 目录树体积（跳过符号链接，避免环与逃逸）
pub fn tree_size(root: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return file_bytes(&meta);
    }
    if !meta.is_dir() {
        return 0;
    }

    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                if let Ok(m) = entry.metadata() {
                    total = total.saturating_add(file_bytes(&m));
                }
            }
        }
    }
    total
}

/// 简单的有界并发 map（保持输入顺序）。用于 6155 个 Yarn 条目的体积归因。
pub fn parallel_map<T, R, F>(items: Vec<T>, concurrency: usize, f: F) -> Vec<R>
where
    T: Send + Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = concurrency.clamp(1, n);
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(n));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = f(&items[i]);
                out.lock().unwrap_or_else(|e| e.into_inner()).push((i, r));
            });
        }
    });

    let mut pairs = out.into_inner().unwrap_or_else(|e| e.into_inner());
    pairs.sort_by_key(|(i, _)| *i);
    pairs.into_iter().map(|(_, r)| r).collect()
}

/// 首层子目录各自体积（Yarn v6 条目级归因，一次遍历 + 并发）
pub fn first_level_sizes(root: &Path) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let entries: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect();
    parallel_map(entries, SCAN_CONCURRENCY, |(name, path)| {
        (name.clone(), tree_size(path))
    })
}

/// 在若干项目根下查找指定目录名（不进入已命中的目录，跳过隐藏目录与 Library）
pub fn find_dirs_named(roots: &[PathBuf], name: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname == name {
                found.push(entry.path()); // 命中即不再下钻
                continue;
            }
            if fname.starts_with('.') || fname == "Library" || fname == "node_modules" {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    found
}

/// 项目根候选（存在才扫）
pub fn project_roots(home: &Path) -> Vec<PathBuf> {
    [
        "Desktop",
        "Documents",
        "Downloads",
        "Projects",
        "code",
        "git",
        "dev",
        "work",
    ]
    .iter()
    .map(|r| home.join(r))
    .filter(|p| p.is_dir())
    .collect()
}

// ---------------------------------------------------------------- 报告装配

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn dir_entry_count(root: &Path) -> usize {
    std::fs::read_dir(root)
        .map(|rd| rd.flatten().count())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- A4/A5 明细

/// 生成明细的分组（A5：模拟器 / Playwright / Cypress）
const DETAIL_KEYS: &[&str] = &["simulator", "playwright", "cypress"];
/// 活跃度分界（功能方案 A4 口径）：>180 天在前端标灰
pub const STALE_DAYS: u64 = 180;

fn age_days_of(mtime_nanos: i64, now_nanos: i64) -> u64 {
    ((now_nanos - mtime_nanos).max(0) / 86_400_000_000_000) as u64
}

/// A5 组明细：首层指纹 → DetailItem（按体积降序）
///
/// 指纹里 mtime/bytes 都是扫描副产品，这里唯一的额外 IO 是 simulator 的
/// `device.plist` 读取（几十个小文件，忽略不计）。
fn build_details(
    entry_key: &str,
    root: &Path,
    fps: &[EntryFingerprint],
    now_nanos: i64,
) -> Vec<DetailItem> {
    if !DETAIL_KEYS.contains(&entry_key) {
        return Vec::new();
    }
    let mut items: Vec<DetailItem> = fps
        .iter()
        .map(|f| {
            let note = if entry_key == "simulator" {
                simulator_note(&root.join(&f.name))
            } else {
                None
            };
            DetailItem {
                name: f.name.clone(),
                path: root.join(&f.name).to_string_lossy().to_string(),
                bytes: f.bytes,
                age_days: age_days_of(f.mtime_nanos, now_nanos),
                note,
            }
        })
        .collect();
    items.sort_by_key(|d| std::cmp::Reverse(d.bytes));
    items
}

/// A4 明细：node_modules 聚合表（path → (mtime, bytes)）→ DetailItem（按体积降序）
fn node_module_details(
    map: &BTreeMap<String, (i64, u64)>,
    home: &Path,
    now_nanos: i64,
) -> Vec<DetailItem> {
    let mut items: Vec<DetailItem> = map
        .iter()
        .map(|(path, (mtime, bytes))| {
            let p = Path::new(path);
            let name = p
                .strip_prefix(home)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string();
            DetailItem {
                name,
                path: path.clone(),
                bytes: *bytes,
                age_days: age_days_of(*mtime, now_nanos),
                note: None,
            }
        })
        .collect();
    items.sort_by_key(|d| std::cmp::Reverse(d.bytes));
    items
}

/// 从 device.plist（XML）抽「设备名 · 运行时」；任何失败回退 None（UI 显示 UDID）
fn simulator_note(device_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(device_dir.join("device.plist")).ok()?;
    let name = xml_string_after(&text, "name");
    let runtime = xml_string_after(&text, "runtime").map(pretty_runtime);
    match (name, runtime) {
        (Some(n), Some(r)) => Some(format!("{n} · {r}")),
        (Some(n), None) => Some(n),
        _ => None,
    }
}

/// `<key>name</key>` 后第一个 `<string>…</string>` 的值
fn xml_string_after(text: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = &text[text.find(&marker)? + marker.len()..];
    let s = rest.find("<string>")? + "<string>".len();
    let e = rest[s..].find("</string>")? + s;
    Some(rest[s..e].trim().to_string())
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-17-5` → `iOS 17.5`
fn pretty_runtime(runtime: String) -> String {
    let tail = runtime.rsplit('.').next().unwrap_or(&runtime);
    match tail.split_once('-') {
        Some((os, ver)) => format!("{os} {}", ver.replace('-', ".")),
        None => tail.to_string(),
    }
}

/// 由条目体积表构建 A1 统计
pub fn deadweight_stats(
    sizes: Vec<(String, u64)>,
    arch: Arch,
    has_rosetta: bool,
    root: &Path,
) -> DeadweightStats {
    let mut bytes = 0u64;
    let mut native_bytes = 0u64;
    let mut other_bytes = 0u64;
    let mut entry_count = 0usize;
    let mut by_platform: BTreeMap<String, PlatformStat> = BTreeMap::new();
    let mut items: Vec<DeadweightItem> = Vec::new();

    for (name, size) in sizes {
        let platform = classify::platform_of(&name);
        let verdict = classify::classify(platform, arch, has_rosetta);
        match verdict {
            Verdict::Deadweight => {
                bytes = bytes.saturating_add(size);
                entry_count += 1;
                let key = platform
                    .map(|p| p.as_str().to_string())
                    .unwrap_or_else(|| "unknown".into());
                let stat = by_platform.entry(key.clone()).or_insert(PlatformStat {
                    platform: key,
                    entry_count: 0,
                    bytes: 0,
                });
                stat.entry_count += 1;
                stat.bytes = stat.bytes.saturating_add(size);
                items.push(DeadweightItem {
                    path: root.join(&name).to_string_lossy().to_string(),
                    name,
                    bytes: size,
                    platform: platform.map(|p| p.as_str().to_string()),
                    verdict,
                });
            }
            // 浪费比的分母只算「本机原生可用」的同类包，不能把所有普通包都算进去
            Verdict::Keep if platform == Some(classify::Platform::DarwinArm64) => {
                native_bytes = native_bytes.saturating_add(size)
            }
            _ => other_bytes = other_bytes.saturating_add(size),
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    let mut by_platform: Vec<PlatformStat> = by_platform.into_values().collect();
    by_platform.sort_by_key(|s| std::cmp::Reverse(s.bytes));

    DeadweightStats {
        bytes,
        native_bytes,
        other_bytes,
        entry_count,
        by_platform,
        ratio: if native_bytes > 0 {
            Some(bytes as f64 / native_bytes as f64)
        } else {
            None
        },
        items,
    }
}

/// A3：三（多）包管理器并存提示
pub fn dup_summary(groups: &[CacheGroup]) -> Option<DupSummary> {
    let keys = ["npm", "yarn", "pnpm", "bun"];
    let managers: Vec<(String, u64)> = groups
        .iter()
        .filter(|g| keys.contains(&g.key.as_str()))
        .map(|g| (g.label.clone(), g.total_bytes))
        .collect();
    if managers.len() < 2 {
        return None;
    }
    let total: u64 = managers.iter().map(|(_, b)| *b).sum();
    let names: Vec<&str> = managers.iter().map(|(n, _)| n.as_str()).collect();
    Some(DupSummary {
        total_bytes: total,
        advice: format!(
            "{} 个包管理器并存（{}），同一批依赖各存一份；保留正在用的一个即可，其余可整体移入废纸篓",
            managers.len(),
            names.join(" / ")
        ),
        managers,
    })
}

fn totals_of(groups: &[CacheGroup]) -> Totals {
    let mut deadweight = 0u64;
    let mut l2 = 0u64;
    let mut l3 = 0u64;
    let mut l4 = 0u64;
    let mut cache_total = 0u64;

    for g in groups {
        cache_total = cache_total.saturating_add(g.total_bytes);
        let dw = g.deadweight.as_ref().map(|d| d.bytes).unwrap_or(0);
        deadweight = deadweight.saturating_add(dw);
        match g.level {
            // A1 组整体登记为 L2，其中已判死的部分归入 L1，避免重复计数
            Level::L1 | Level::L2 => l2 = l2.saturating_add(g.total_bytes.saturating_sub(dw)),
            Level::L3 => l3 = l3.saturating_add(g.total_bytes),
            Level::L4 => l4 = l4.saturating_add(g.total_bytes),
        }
    }

    Totals {
        deadweight,
        l2,
        l3,
        l4,
        reclaimable: deadweight.saturating_add(l2).saturating_add(l3),
        cache_total,
    }
}

/// 全量扫描（保留旧签名：CLI / MCP / 探针 / 测试的直接入口）
pub fn scan(home: &Path, progress: Option<&ProgressFn>) -> DiskReport {
    scan_with(home, None, progress).0
}

/// 全量 / 增量统一入口（A7）。
///
/// `prev = None` → 全量；`Some(&snapshot)` → 与快照按首层条目 mtime（纳秒）diff，
/// 只重算变化的条目。两条路径共用同一套装配逻辑，增量报告与全量报告结构完全一致；
/// `DiskReport.incremental` 标记本轮模式。返回值携带新快照供调用方保存。
///
/// 已知近似（D-A7-1）：深层新增文件不改变浅层 mtime，增量扫不出来 —— 每周全量校准。
pub fn scan_with(
    home: &Path,
    prev: Option<&ScanSnapshot>,
    progress: Option<&ProgressFn>,
) -> (DiskReport, ScanSnapshot) {
    let started = Instant::now();
    let arch = classify::host_arch();
    let has_rosetta = classify::has_rosetta();
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);

    let entries = whitelist::resolve(home);
    let total_steps = entries.len() + 1; // +1 = node_modules
    let done = AtomicUsize::new(0);
    let bytes_done = std::sync::atomic::AtomicU64::new(0);

    // 进度回调：done 自增 + 携带当前累计体积与耗时（并行阶段也可安全调用）
    let report = |phase: &str| {
        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(cb) = progress {
            cb(ScanProgress {
                phase: phase.to_string(),
                done: d,
                total: total_steps,
                bytes_so_far: bytes_done.load(Ordering::Relaxed),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    };

    let mut groups: Vec<CacheGroup> = Vec::new();
    let mut fingerprints: BTreeMap<String, Vec<EntryFingerprint>> = BTreeMap::new();

    // node_modules 发现与定点目录扫描互相独立 → 并行跑，墙钟取 max 而不是 sum
    let (nm_count, nm_bytes, nm_map) = std::thread::scope(|scope| {
        let nm = scope.spawn(|| -> (usize, u64, BTreeMap<String, (i64, u64)>) {
            let dirs =
                find_dirs_named(&project_roots(home), "node_modules", NODE_MODULES_MAX_DEPTH);
            let count = dirs.len();
            let cached_nm = prev.map(|p| &p.node_modules);
            let dir_refs: Vec<&PathBuf> = dirs.iter().collect();
            let mtimes: Vec<i64> = parallel_map(dir_refs, SCAN_CONCURRENCY, |p| {
                std::fs::symlink_metadata(*p)
                    .map(|m| incremental::mtime_nanos(&m))
                    .unwrap_or(0)
            });

            // mtime 没变的目录复用快照体积，只重算新增 / 变化的
            let mut sizes = vec![0u64; dirs.len()];
            let mut todo: Vec<usize> = Vec::new();
            for (i, dir) in dirs.iter().enumerate() {
                let key = dir.to_string_lossy().to_string();
                let hit = cached_nm
                    .and_then(|m| m.get(&key))
                    .filter(|(cm, _)| *cm == mtimes[i])
                    .map(|(_, b)| *b);
                match hit {
                    Some(b) => sizes[i] = b,
                    None => todo.push(i),
                }
            }
            let recomputed = parallel_map(todo.clone(), SCAN_CONCURRENCY, |&i| tree_size(&dirs[i]));
            for (k, &i) in todo.iter().enumerate() {
                sizes[i] = recomputed[k];
            }

            let mut map = BTreeMap::new();
            for (i, dir) in dirs.iter().enumerate() {
                map.insert(dir.to_string_lossy().to_string(), (mtimes[i], sizes[i]));
            }
            (count, sizes.iter().sum(), map)
        });

        // 1) A1：Yarn 条目级归因（先跑，让首屏杀手数字尽早可用）
        for (entry, path) in entries.iter().filter(|(e, _)| e.a1_scope) {
            let cached = prev
                .and_then(|s| s.groups.get(entry.key))
                .map(|v| v.as_slice());
            let (pairs, fps) = incremental::resolve_group(path, true, cached);
            let dw = deadweight_stats(pairs, arch, has_rosetta, path);
            let group_total = dw
                .bytes
                .saturating_add(dw.native_bytes)
                .saturating_add(dw.other_bytes);
            bytes_done.fetch_add(group_total, Ordering::Relaxed);
            report("yarn");
            fingerprints.insert(entry.key.to_string(), fps);
            groups.push(CacheGroup {
                key: entry.key.to_string(),
                label: entry.label.to_string(),
                root: path.to_string_lossy().to_string(),
                level: entry.level,
                total_bytes: group_total,
                entry_count: dir_entry_count(path),
                cleanable: true,
                // A1 组的条目视图走 deadweight.items（平台判定），明细表留给 A4/A5
                details: Vec::new(),
                deadweight: Some(dw),
            });
        }

        // 2) 其余定点目录（逐个处理：`resolve_group` 内部工作队列已并行，外层再并行会线程爆炸）
        for (entry, path) in entries.iter().filter(|(e, _)| !e.a1_scope) {
            let cached = prev
                .and_then(|s| s.groups.get(entry.key))
                .map(|v| v.as_slice());
            let (pairs, fps) = incremental::resolve_group(path, false, cached);
            let size = pairs
                .iter()
                .fold(0u64, |acc, (_, b)| acc.saturating_add(*b));
            bytes_done.fetch_add(size, Ordering::Relaxed);
            report(entry.key);
            let details = build_details(entry.key, path, &fps, now_nanos);
            fingerprints.insert(entry.key.to_string(), fps);
            groups.push(CacheGroup {
                key: entry.key.to_string(),
                label: entry.label.to_string(),
                root: path.to_string_lossy().to_string(),
                level: entry.level,
                total_bytes: size,
                entry_count: if size > 0 { dir_entry_count(path) } else { 0 },
                cleanable: true,
                details,
                deadweight: None,
            });
        }

        nm.join().expect("node_modules 扫描线程失败")
    });

    // 3) 项目 node_modules（L3：只统计，不逐项清理；明细供 A4 逐项确认）
    bytes_done.fetch_add(nm_bytes, Ordering::Relaxed);
    report("node-modules");
    if nm_count > 0 {
        groups.push(CacheGroup {
            key: "node-modules".into(),
            label: format!("项目依赖（{nm_count} 个 node_modules）"),
            root: home.to_string_lossy().to_string(),
            level: Level::L3,
            total_bytes: nm_bytes,
            entry_count: nm_count,
            // 聚合项：没有单一可删根（root 是 home），整项不可清；明细逐项走 L3 确认通道
            cleanable: false,
            details: node_module_details(&nm_map, home, now_nanos),
            deadweight: None,
        });
    }

    // A1 组置顶
    groups.sort_by_key(|g| {
        let rank = if g.deadweight.is_some() {
            0
        } else {
            match g.level {
                Level::L1 | Level::L2 => 1,
                Level::L3 => 2,
                Level::L4 => 3,
            }
        };
        (rank, std::cmp::Reverse(g.total_bytes))
    });

    let dup = dup_summary(&groups);
    let totals = totals_of(&groups);

    let report = DiskReport {
        scanned_at: now_ts(),
        scan_elapsed_ms: started.elapsed().as_millis() as u64,
        incremental: prev.is_some(),
        groups,
        dup,
        totals,
        // 扫描入口实时取：容量是「现在」的读数，不跟随报告缓存
        volume: volume_info(home),
        // F9 项目体检：纯聚合 + 每项目几次 stat，跟随本轮 mtime，不额外扫描
        projects: project::build_projects(&nm_map, home, now_nanos),
    };
    let snapshot = ScanSnapshot {
        taken_at: now_ts(),
        groups: fingerprints,
        node_modules: nm_map,
    };
    (report, snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("prune-disk-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(p: &Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn tree_size_uses_allocated_blocks_and_skips_symlinks() {
        let root = tmp("size");
        write(&root.join("a/1.bin"), 4096);
        write(&root.join("a/b/2.bin"), 8192);
        write(&root.join("3.bin"), 4096);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("a"), root.join("link")).unwrap();
        let total = tree_size(&root);
        #[cfg(unix)]
        assert_eq!(total, 16384, "按物理块计（16 KB = 4 块），且不走 symlink");
        #[cfg(not(unix))]
        assert_eq!(total, 16384);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn volume_info_reports_consistent_capacity() {
        let info = volume_info(&PathBuf::from("/tmp")).expect("statvfs on /tmp should work");
        assert!(info.total_bytes > 0, "total 必须 > 0");
        assert!(
            info.available_bytes <= info.total_bytes,
            "available ≤ total，实际 available={} total={}",
            info.available_bytes,
            info.total_bytes
        );
        assert!(
            info.user_available_bytes <= info.available_bytes,
            "user_available ≤ available（bavail ≤ bfree）"
        );
    }

    #[test]
    fn old_report_json_without_volume_deserializes_to_none() {
        // M3 时代落盘的报告没有 volume 字段，必须照常反序列化（serde default）
        let json = r#"{
            "scannedAt": 0, "scanElapsedMs": 1, "groups": [], "dup": null,
            "totals": { "deadweight": 0, "l2": 0, "l3": 0, "l4": 0, "reclaimable": 0, "cacheTotal": 0 }
        }"#;
        let report: DiskReport = serde_json::from_str(json).expect("旧报告应可反序列化");
        assert!(report.volume.is_none());
        assert!(!report.incremental);
    }

    #[test]
    fn first_level_attribution_is_per_entry() {
        let root = tmp("first");
        write(&root.join("npm-a-1.0.0-x/payload.bin"), 4096);
        write(&root.join("npm-b-2.0.0-y/sub/deep.bin"), 8192);
        let mut got = first_level_sizes(&root);
        got.sort();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "npm-a-1.0.0-x");
        assert_eq!(got[0].1, 4096);
        assert_eq!(got[1].1, 8192);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_node_modules_prunes_hit_and_hidden() {
        let root = tmp("nm");
        write(&root.join("p1/node_modules/x/1.js"), 10);
        write(&root.join("p1/node_modules/y/node_modules/z/2.js"), 20); // 命中后不再下钻
        write(&root.join(".cache/node_modules/hidden/3.js"), 30); // 隐藏目录跳过
        let got = find_dirs_named(&[root.clone()], "node_modules", 6);
        assert_eq!(got.len(), 1, "只算最外层命中，且不进隐藏目录");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deadweight_stats_groups_by_platform_and_skips_keep() {
        let root = Path::new("/tmp/yarn");
        let sizes = vec![
            ("npm-@next-swc-linux-x64-1.0.0-a".to_string(), 400),
            ("npm-@esbuild-android-arm64-1.0.0-b".to_string(), 100),
            ("npm-@next-swc-darwin-arm64-1.0.0-c".to_string(), 1000),
            ("npm-@next-swc-darwin-x64-1.0.0-d".to_string(), 50),
        ];
        let dw = deadweight_stats(sizes, Arch::Arm64, false, root);
        assert_eq!(dw.bytes, 550, "linux + android + darwin-x64(无 Rosetta)");
        assert_eq!(dw.native_bytes, 1000, "浪费比分母只算 darwin-arm64");
        assert_eq!(dw.other_bytes, 0);
        assert_eq!(dw.entry_count, 3);
        assert_eq!(dw.items[0].bytes, 400, "items 按体积降序");
        assert_eq!(dw.ratio, Some(0.55));
        // 有 Rosetta 时 darwin-x64 降级为待确认，不再算死重
        let dw2 = deadweight_stats(
            vec![("npm-@next-swc-darwin-x64-1.0.0-d".to_string(), 50)],
            Arch::Arm64,
            true,
            root,
        );
        assert_eq!(dw2.bytes, 0);
        assert_eq!(
            dw2.other_bytes, 50,
            "待确认条目归入 other，不进死重也不充作原生可用"
        );
        assert_eq!(dw2.ratio, None, "没有原生可用体积就不编造比例");
    }

    #[test]
    fn totals_do_not_double_count_deadweight() {
        let yarn = CacheGroup {
            key: "yarn".into(),
            label: "Yarn 缓存".into(),
            root: "/tmp".into(),
            level: Level::L2,
            total_bytes: 1000,
            entry_count: 10,
            cleanable: true,
            details: Vec::new(),
            deadweight: Some(DeadweightStats {
                bytes: 700,
                native_bytes: 300,
                other_bytes: 0,
                entry_count: 4,
                by_platform: vec![],
                items: vec![],
                ratio: Some(2.33),
            }),
        };
        let npm = CacheGroup {
            key: "npm".into(),
            label: "npm 缓存".into(),
            root: "/tmp".into(),
            level: Level::L2,
            total_bytes: 500,
            entry_count: 1,
            cleanable: true,
            details: Vec::new(),
            deadweight: None,
        };
        let docker = CacheGroup {
            key: "docker-vm".into(),
            label: "Docker".into(),
            root: "/tmp".into(),
            level: Level::L4,
            total_bytes: 900,
            entry_count: 1,
            cleanable: true,
            details: Vec::new(),
            deadweight: None,
        };
        let t = totals_of(&[yarn, npm, docker]);
        assert_eq!(t.deadweight, 700);
        assert_eq!(t.l2, 300 + 500, "Yarn 的 L2 部分要扣掉死重");
        assert_eq!(t.l4, 900);
        assert_eq!(t.reclaimable, 1500, "主数字不含 L4");
        assert_eq!(t.cache_total, 2400);
    }

    #[test]
    fn dup_summary_needs_two_managers() {
        let g = |key: &str, bytes: u64| CacheGroup {
            key: key.into(),
            label: format!("{key} 缓存"),
            root: "/tmp".into(),
            level: Level::L2,
            total_bytes: bytes,
            entry_count: 1,
            cleanable: true,
            details: Vec::new(),
            deadweight: None,
        };
        assert!(dup_summary(&[g("npm", 10)]).is_none());
        let d = dup_summary(&[g("npm", 10), g("yarn", 20), g("cargo", 99)]).unwrap();
        assert_eq!(d.managers.len(), 2, "只统计包管理器，Cargo 不算");
        assert_eq!(d.total_bytes, 30);
        assert!(d.advice.contains("保留"));
    }

    // ------------------------------------------------------------ A4/A5

    const NOW_NANOS: i64 = 1_800_000_000_000_000_000; // 某个固定「现在」

    fn fp(name: &str, age_days: i64, bytes: u64) -> EntryFingerprint {
        EntryFingerprint {
            name: name.into(),
            mtime_nanos: NOW_NANOS - age_days * 86_400_000_000_000,
            bytes,
        }
    }

    #[test]
    fn details_only_for_a5_keys_and_sorted_by_bytes() {
        let root = Path::new("/tmp/sim");
        let fps = vec![fp("small-device", 10, 100), fp("big-device", 400, 900)];
        assert!(
            build_details("yarn", root, &fps, NOW_NANOS).is_empty(),
            "A1 组不出明细"
        );
        let got = build_details("simulator", root, &fps, NOW_NANOS);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "big-device", "按体积降序");
        assert_eq!(got[0].age_days, 400, "age 来自指纹 mtime");
        assert_eq!(got[0].note, None, "plist 不存在 → 无备注（UI 回退 UDID）");
    }

    #[test]
    fn simulator_note_parses_device_plist_and_falls_back() {
        let home = tmp("plist");
        let dev = home.join("DEV1");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(
            dev.join("device.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
	<key>UDID</key>
	<string>DEV1</string>
	<key>name</key>
	<string>iPad mini (6th generation)</string>
	<key>runtime</key>
	<string>com.apple.CoreSimulator.SimRuntime.iOS-17-5</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            simulator_note(&dev).as_deref(),
            Some("iPad mini (6th generation) · iOS 17.5")
        );

        // 损坏 / 缺失 plist → 回退 None，不报错
        let broken = home.join("DEV2");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("device.plist"), "not a plist at all").unwrap();
        assert_eq!(simulator_note(&broken), None);
        assert_eq!(simulator_note(&home.join("DEV3")), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn runtime_is_prettified() {
        assert_eq!(
            pretty_runtime("com.apple.CoreSimulator.SimRuntime.iOS-17-5".into()),
            "iOS 17.5"
        );
        assert_eq!(
            pretty_runtime("com.apple.CoreSimulator.SimRuntime.tvOS-18".into()),
            "tvOS 18"
        );
        assert_eq!(pretty_runtime("weird".into()), "weird", "不认识的原样返回");
    }

    #[test]
    fn node_module_details_names_are_home_relative() {
        let home = tmp("nm-details");
        let nm1 = home.join("Desktop/git/prune/node_modules");
        std::fs::create_dir_all(&nm1).unwrap();
        let mtime = NOW_NANOS - 200 * 86_400_000_000_000;
        let mut map = BTreeMap::new();
        map.insert(nm1.to_string_lossy().to_string(), (mtime, 4096));
        map.insert("/outside/node_modules".to_string(), (NOW_NANOS, 8192));

        let got = node_module_details(&map, &home, NOW_NANOS);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "/outside/node_modules", "体积降序：8192 在前");
        assert_eq!(
            got[1].name, "Desktop/git/prune/node_modules",
            "home 内路径转相对"
        );
        assert_eq!(got[1].age_days, 200);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 旧报告 JSON（无 details 字段）反序列化不炸
    #[test]
    fn cache_group_deserializes_without_details() {
        let json = r#"{"key":"npm","label":"npm 缓存","root":"/tmp","level":"l2","totalBytes":1,"entryCount":1,"cleanable":true,"deadweight":null}"#;
        let g: CacheGroup = serde_json::from_str(json).unwrap();
        assert!(g.details.is_empty());
        assert!(g.cleanable);
    }
}
