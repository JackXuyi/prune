//! A7 增量扫描（`docs/a7-incremental-plan.md`）。
//!
//! 指纹 = 首层条目的 `name + mtime(纳秒)`：mtime 没变就复用上轮体积，变了（或新增）才重算。
//! **写文件只改直接父目录的 mtime**，因此深层新增（如 npm `_cacache` 的 content 深处）
//! 增量扫不出来 —— 这是已知近似（D-A7-1），由每周全量校准兜底；用户主动求证的场景
//! （启动首扫 / 手动刷新）直接走全量，见 `scan_with` 的调用方约定。
//!
//! 快照只存内存：应用重启即失，下一轮自动退化为全量，符合「首次全量」语义。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

use super::SCAN_CONCURRENCY;

/// 首层条目指纹
#[derive(Debug, Clone, PartialEq)]
pub struct EntryFingerprint {
    pub name: String,
    /// mtime，纳秒（APFS 精度；秒级会把同秒内的两次写入判成「没变」）
    pub mtime_nanos: i64,
    pub bytes: u64,
}

/// 增量快照（内存态，不持久化、不跨进程）
#[derive(Debug, Clone, Default)]
pub struct ScanSnapshot {
    pub taken_at: i64,
    /// 组 key → 首层条目指纹（与报告 groups 的 key 一一对应）
    pub groups: std::collections::BTreeMap<String, Vec<EntryFingerprint>>,
    /// node_modules 聚合：目录绝对路径 → (mtime_nanos, bytes)
    pub node_modules: std::collections::BTreeMap<String, (i64, u64)>,
}

/// mtime 纳秒（epoch 前 / 取不到时归 0，只影响指纹比较，不影响正确性）
pub(crate) fn mtime_nanos(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// 首层枚举出的一个条目
pub(crate) struct FirstEntry {
    pub name: String,
    pub mtime_nanos: i64,
    pub path: PathBuf,
    /// 目录：命中快照指纹时 Some(复用体积)，否则 None（待重算）；文件：恒 Some
    pub size: Option<u64>,
}

/// 枚举首层条目并与快照 diff（跳过 symlink；拿不到元数据的条目跳过，与旧遍历一致）。
///
/// `dirs_only = true` 用于 A1 组（Yarn 条目级归因只认目录）；
/// 文件条目当场重读体积（一次 stat，永远新鲜，不值得进指纹）。
pub(crate) fn diff_first_level(
    root: &Path,
    dirs_only: bool,
    cached: Option<&[EntryFingerprint]>,
) -> Vec<FirstEntry> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let is_dir = ft.is_dir();
        if dirs_only && !is_dir {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let mtime = mtime_nanos(&md);
        let name = e.file_name().to_string_lossy().to_string();
        let size = if is_dir {
            // 目录：同 name 且同 mtime（纳秒）才复用，宁可多算不可少算
            cached.and_then(|c| {
                c.iter()
                    .find(|f| f.name == name && f.mtime_nanos == mtime)
                    .map(|f| f.bytes)
            })
        } else {
            Some(super::file_bytes(&md))
        };
        out.push(FirstEntry {
            name,
            mtime_nanos: mtime,
            path: e.path(),
            size,
        });
    }
    out
}

/// 工作队列并行计算若干首层条目的子树体积，**按条目归账**（A7 核心）。
///
/// 与旧 `tree_size_fast` 同一套队列结构（共享队列，树形多扁都能吃满并发），
/// 区别是队列元素带条目下标（tag），文件字节记到对应条目头上 ——
/// 「单个首层条目下挂 44 万文件」（pnpm store）不会退化成单线程。
///
/// 返回值按下标对齐 `tag_count`；未出现在 seeds 里的下标为 0。
pub(crate) fn subtree_bytes(seeds: &[(usize, PathBuf)], tag_count: usize) -> Vec<u64> {
    let totals: Vec<AtomicU64> = (0..tag_count).map(|_| AtomicU64::new(0)).collect();
    if seeds.is_empty() {
        return totals.into_iter().map(|t| t.into_inner()).collect();
    }

    let mutex = Mutex::new((VecDeque::from(seeds.to_vec()), 0usize));
    let cv = Condvar::new();

    std::thread::scope(|scope| {
        for _ in 0..SCAN_CONCURRENCY {
            scope.spawn(|| loop {
                let next = {
                    let mut g = mutex.lock().unwrap_or_else(|e| e.into_inner());
                    loop {
                        if let Some(d) = g.0.pop_front() {
                            g.1 += 1;
                            break Some(d);
                        }
                        if g.1 == 0 {
                            break None; // 队列空且无人处理 → 整体结束
                        }
                        g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                };
                let Some((tag, dir)) = next else { return };

                let mut subdirs: Vec<(usize, PathBuf)> = Vec::new();
                let mut bytes = 0u64;
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for entry in rd.flatten() {
                        let Ok(ft) = entry.file_type() else { continue };
                        if ft.is_symlink() {
                            continue;
                        }
                        if ft.is_dir() {
                            subdirs.push((tag, entry.path()));
                        } else if ft.is_file() {
                            if let Ok(m) = entry.metadata() {
                                bytes = bytes.saturating_add(super::file_bytes(&m));
                            }
                        }
                    }
                }
                totals[tag].fetch_add(bytes, Ordering::Relaxed);

                let notify = {
                    let mut g = mutex.lock().unwrap_or_else(|e| e.into_inner());
                    g.1 -= 1;
                    let added = !subdirs.is_empty();
                    g.0.extend(subdirs);
                    added || g.1 == 0
                };
                if notify {
                    cv.notify_all();
                }
            });
        }
    });

    totals.into_iter().map(|t| t.into_inner()).collect()
}

/// 一个白名单根的完整体积解析：diff → 只重算变化条目 → 产出 (体积对, 新指纹)。
///
/// 全量（`cached = None`）时所有条目都进重算集合，等价旧的 `first_level_sizes` /
/// `tree_size_fast` 语义；这是全量与增量共用同一条路径的关键。
pub(crate) fn resolve_group(
    root: &Path,
    dirs_only: bool,
    cached: Option<&[EntryFingerprint]>,
) -> (Vec<(String, u64)>, Vec<EntryFingerprint>) {
    let entries = diff_first_level(root, dirs_only, cached);
    // tag = 条目下标，重算结果按 tag 对齐回填
    let seeds: Vec<(usize, PathBuf)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.size.is_none())
        .map(|(i, e)| (i, e.path.clone()))
        .collect();
    let computed = subtree_bytes(&seeds, entries.len());

    let mut pairs = Vec::with_capacity(entries.len());
    let mut fps = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let bytes = e.size.unwrap_or(computed[i]);
        pairs.push((e.name.clone(), bytes));
        fps.push(EntryFingerprint {
            name: e.name.clone(),
            mtime_nanos: e.mtime_nanos,
            bytes,
        });
    }
    (pairs, fps)
}

#[cfg(test)]
mod tests {
    use super::super::{scan_with, DiskReport};
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("prune-incr-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(p: &Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; bytes]).unwrap();
    }

    /// 最小可扫 home：Yarn（A1）+ npm cacache，各含可判定的条目
    fn seed_home(home: &Path) {
        let v6 = home.join("Library/Caches/Yarn/v6");
        write(
            &v6.join("npm-@next-swc-linux-x64-1.0.0-a/payload.bin"),
            4096,
        );
        write(
            &v6.join("npm-@next-swc-darwin-arm64-1.0.0-b/payload.bin"),
            4096,
        );
        write(&v6.join("npm-lodash-4.17.21-c/payload.bin"), 4096);
        write(&home.join(".npm/_cacache/content-v2/f1.bin"), 4096);
        write(&home.join(".npm/_cacache/index-v5/f2.bin"), 4096);
    }

    fn group<'a>(r: &'a DiskReport, key: &str) -> &'a super::super::CacheGroup {
        r.groups
            .iter()
            .find(|g| g.key == key)
            .unwrap_or_else(|| panic!("报告缺少组 {key}"))
    }

    fn assert_groups_match(a: &DiskReport, b: &DiskReport) {
        assert_eq!(a.groups.len(), b.groups.len(), "组数量必须一致");
        for ga in &a.groups {
            let gb = group(b, &ga.key);
            assert_eq!(ga.total_bytes, gb.total_bytes, "组 {} 字节不一致", ga.key);
            assert_eq!(ga.entry_count, gb.entry_count, "组 {} 条目数不一致", ga.key);
            assert_eq!(
                ga.deadweight.as_ref().map(|d| d.bytes),
                gb.deadweight.as_ref().map(|d| d.bytes),
                "组 {} 死重不一致",
                ga.key
            );
        }
        assert_eq!(a.totals, b.totals, "totals 必须一致");
    }

    /// 增量 vs 全量一致性。用例构造的是 Yarn（MacOnly）白名单布局，
    /// Linux 上 yarn 组为空，断言不成立——只在 macOS 上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn incremental_matches_full_after_changes() {
        let home = tmp("match");
        seed_home(&home);

        let (r1, s1) = scan_with(&home, None, None);
        assert!(!r1.incremental);
        assert_eq!(group(&r1, "yarn").deadweight.as_ref().unwrap().bytes, 4096);
        assert_eq!(group(&r1, "yarn").entry_count, 3);

        // 变化：新增死重条目 + 删除普通条目 + npm 首层目录内容变化（直接子项）
        let v6 = home.join("Library/Caches/Yarn/v6");
        write(
            &v6.join("npm-@esbuild-linux-arm64-1.0.0-d/payload.bin"),
            8192,
        );
        std::fs::remove_dir_all(v6.join("npm-lodash-4.17.21-c")).unwrap();
        write(&home.join(".npm/_cacache/content-v2/f3.bin"), 4096);

        let (r2, _s2) = scan_with(&home, Some(&s1), None);
        assert!(r2.incremental, "带快照的扫描必须标记为增量");
        let (rf, _sf) = scan_with(&home, None, None);

        assert_eq!(group(&r2, "yarn").entry_count, 3, "a/b/d 三个条目");
        assert_eq!(
            group(&r2, "yarn").deadweight.as_ref().unwrap().bytes,
            4096 + 8192,
            "linux-x64 + linux-arm64 都算死重"
        );
        assert_eq!(group(&r2, "npm").total_bytes, 4096 * 3);
        assert_groups_match(&r2, &rf);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn incremental_is_noop_when_nothing_changed() {
        let home = tmp("noop");
        seed_home(&home);
        let (r1, s1) = scan_with(&home, None, None);
        let (r2, _s2) = scan_with(&home, Some(&s1), None);
        assert_groups_match(&r1, &r2);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// D-A7-1 文档化近似：深层新增文件不改变浅层 mtime，增量保持旧值，全量校准
    #[test]
    fn deep_change_waits_for_full_scan() {
        let home = tmp("deep");
        write(
            &home.join(".npm/_cacache/content-v2/sha512/ab/deep.bin"),
            4096,
        );
        let (r1, s1) = scan_with(&home, None, None);
        assert_eq!(group(&r1, "npm").total_bytes, 4096);

        // 深层写入：只动 ab/cd 的 mtime，content-v2 的指纹不变
        write(
            &home.join(".npm/_cacache/content-v2/sha512/ab/cd/deeper.bin"),
            4096,
        );

        let (r_inc, _s) = scan_with(&home, Some(&s1), None);
        assert_eq!(
            group(&r_inc, "npm").total_bytes,
            4096,
            "增量扫不到深层新增（已知近似，不算 bug）"
        );
        let (r_full, _sf) = scan_with(&home, None, None);
        assert_eq!(group(&r_full, "npm").total_bytes, 4096 * 2, "全量校准");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 根组移除后增量落盘。同样依赖 Yarn（MacOnly）白名单——只在 macOS 上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn removed_root_group_dropped_incrementally() {
        let home = tmp("dropped");
        seed_home(&home);
        let (_r1, s1) = scan_with(&home, None, None);

        std::fs::remove_dir_all(home.join(".npm/_cacache")).unwrap();
        let (r2, s2) = scan_with(&home, Some(&s1), None);
        assert!(
            !r2.groups.iter().any(|g| g.key == "npm"),
            "根消失后增量报告不得残留旧组"
        );
        assert!(!s2.groups.contains_key("npm"), "快照同步收敛");
        assert!(group(&r2, "yarn").total_bytes > 0, "其余组不受影响");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn node_modules_incremental_tracks_add_and_remove() {
        let home = tmp("nm");
        write(&home.join("Desktop/p1/node_modules/x/1.js"), 1024);
        let (r1, s1) = scan_with(&home, None, None);
        assert_eq!(group(&r1, "node-modules").entry_count, 1);

        // 新增项目的 node_modules：find 重新发现 + mtime 全新 → 重算
        write(&home.join("Desktop/p2/node_modules/y/2.js"), 1024);
        let (r2, s2) = scan_with(&home, Some(&s1), None);
        assert_eq!(group(&r2, "node-modules").entry_count, 2);
        assert_eq!(s2.node_modules.len(), 2);

        // 删除后回落，且 p1 的体积来自快照复用（不重算）
        std::fs::remove_dir_all(home.join("Desktop/p2/node_modules")).unwrap();
        let (r3, s3) = scan_with(&home, Some(&s2), None);
        assert_eq!(group(&r3, "node-modules").entry_count, 1);
        assert_eq!(s3.node_modules.len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn subtree_bytes_accounts_per_entry() {
        let root = tmp("subtree");
        write(&root.join("e1/a.bin"), 4096);
        write(&root.join("e1/sub/b.bin"), 4096);
        write(&root.join("e2/c.bin"), 4096);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("e1"), root.join("e3")).unwrap();

        for run in 0..30 {
            let entries = diff_first_level(&root, true, None);
            let seeds: Vec<(usize, PathBuf)> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e.path.clone()))
                .collect();
            let sizes = subtree_bytes(&seeds, entries.len());
            // read_dir 顺序不保证，按 name 归位断言
            let by_name: std::collections::BTreeMap<&str, u64> = entries
                .iter()
                .zip(&sizes)
                .map(|(e, s)| (e.name.as_str(), *s))
                .collect();
            eprintln!("run {run}: {by_name:?}");
            assert_eq!(by_name["e1"], 8192, "run {run}: e1 = 直属 + 子目录");
            assert_eq!(by_name["e2"], 4096);
        }
        // symlink 条目被跳过（dirs_only 枚举不含 e3）
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_group_reuses_cached_fingerprints() {
        let root = tmp("reuse");
        write(&root.join("pkg-a/payload.bin"), 4096);
        let (pairs1, fps) = resolve_group(&root, true, None);
        assert_eq!(pairs1.len(), 1);

        // 同 mtime：命中指纹复用（不触发重算也能拿到一致体积）
        let (pairs2, fps2) = resolve_group(&root, true, Some(&fps));
        assert_eq!(pairs1, pairs2);
        assert_eq!(fps, fps2, "无变化时指纹应原样回写");

        // 写入条目内新文件 → 目录 mtime 变 → 重算
        write(&root.join("pkg-a/extra.bin"), 4096);
        let (pairs3, fps3) = resolve_group(&root, true, Some(&fps));
        assert_ne!(pairs3[0].1, pairs1[0].1, "mtime 变化必须重算");
        assert_ne!(fps3[0].mtime_nanos, fps[0].mtime_nanos);
        let _ = std::fs::remove_dir_all(&root);
    }
}
