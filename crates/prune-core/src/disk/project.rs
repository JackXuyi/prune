//! 项目体检（F9 / M24）：把 node_modules 聚合表拆成**按项目**的依赖账本。
//!
//! 只做体检：不产生清理项，也不判断「项目是否废弃」——结论只陈述
//! 「依赖多久没重装 / 项目多久没动」的事实，怎么处理由用户决定。
//!
//! 数据来源是扫描已产出的 node_modules 表（路径 + mtime + 体积），
//! 每个项目额外 6 次 `stat`（4 个锁文件 + package.json + `.git/index`）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 依赖「久未重装」阈值：install 之后 60 天没再碰过
pub const PROJECT_STALE_DAYS: u64 = 60;
/// 依赖「偏胖」阈值：1.5 GiB
pub const PROJECT_HEAVY_BYTES: u64 = 3 * 1024 * 1024 * 1024 / 2;

const DAY_NANOS: i64 = 86_400_000_000_000;

/// 锁文件 → 包管理器名（同一项目出现 ≥2 个 = 多管理器并存）
const LOCKFILES: &[(&str, &str)] = &[
    ("package-lock.json", "npm"),
    ("yarn.lock", "yarn"),
    ("pnpm-lock.yaml", "pnpm"),
    ("bun.lockb", "bun"),
];

/// 体检结论。优先级：僵尸 > 偏胖 > 久未重装 > 健康
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectVerdict {
    Healthy,
    /// 依赖 ≥60 天没重装，但项目本身近期还在动
    Stale,
    /// 依赖体积 ≥1.5 GiB
    Heavy,
    /// 依赖 ≥60 天没重装，且项目最后活动也 ≥60 天前（或完全查不到活动痕迹）
    Zombie,
}

/// 一个项目的依赖账本
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHealth {
    /// 展示名：相对 home 的项目路径（不在 home 下则回退绝对路径）
    pub name: String,
    /// 项目根（node_modules 的父目录），右键/点击定位目标
    pub root: String,
    /// node_modules 绝对路径
    pub deps_path: String,
    pub deps_bytes: u64,
    /// 依赖上次安装距今天数（node_modules 目录 mtime）
    pub deps_age_days: u64,
    /// 项目最后活动距今天数（package.json / 锁文件 / .git/index 里最新的那个）；
    /// None = 一个标记文件都没找到——此时僵尸判定只看依赖年龄
    pub activity_age_days: Option<u64>,
    /// 同时存在的包管理器（按 LOCKFILES 顺序）
    pub lockfiles: Vec<String>,
    /// ≥2 个锁文件并存：依赖树由多个管理器接管过，最容易漂移
    pub multi_manager: bool,
    pub verdict: ProjectVerdict,
}

/// node_modules 聚合表 → 按项目的账本（按依赖体积降序）。
///
/// 聚合键是 node_modules 的父目录：`find_dirs_named` 命中即停止下钻，
/// 所以同一项目的嵌套依赖不会重复计数（体积已含在内层目录里）。
pub fn build_projects(
    node_modules: &BTreeMap<String, (i64, u64)>,
    home: &Path,
    now_nanos: i64,
) -> Vec<ProjectHealth> {
    // 项目根 → (体积合计, 最近一次安装的 mtime, 最大的 node_modules 路径)
    let mut acc: BTreeMap<PathBuf, (u64, i64, PathBuf)> = BTreeMap::new();
    for (path, (mtime, bytes)) in node_modules {
        let deps = PathBuf::from(path);
        let Some(root) = deps.parent() else { continue };
        let entry = acc
            .entry(root.to_path_buf())
            .or_insert((0, i64::MAX, deps.clone()));
        entry.0 = entry.0.saturating_add(*bytes);
        // 依赖年龄取**最近**的一次安装（最小 mtime → 最小 age）：
        // 判「僵尸」时偏保守，宁可漏判也不冤枉刚装过的项目
        entry.1 = entry.1.min(*mtime);
        if *bytes > 0 {
            entry.2 = deps;
        }
    }

    let mut out: Vec<ProjectHealth> = acc
        .into_iter()
        .map(|(root, (bytes, deps_mtime, deps_path))| {
            let (activity_mtime, lockfiles) = probe_markers(&root);
            let name = root
                .strip_prefix(home)
                .unwrap_or(&root)
                .to_string_lossy()
                .to_string();
            assemble(
                name,
                root,
                deps_path,
                bytes,
                deps_mtime,
                activity_mtime,
                lockfiles,
                now_nanos,
            )
        })
        .collect();

    out.sort_by_key(|p| std::cmp::Reverse(p.deps_bytes));
    out
}

/// 项目标记文件探测：返回（最新 mtime, 存在的包管理器）
fn probe_markers(root: &Path) -> (Option<i64>, Vec<String>) {
    let mut times: Vec<i64> = Vec::new();
    let mut locks: Vec<String> = Vec::new();
    for (file, manager) in LOCKFILES {
        if let Ok(md) = std::fs::metadata(root.join(file)) {
            locks.push((*manager).to_string());
            let t = super::incremental::mtime_nanos(&md);
            if t > 0 {
                times.push(t);
            }
        }
    }
    for marker in ["package.json", ".git/index"] {
        if let Ok(md) = std::fs::metadata(root.join(marker)) {
            let t = super::incremental::mtime_nanos(&md);
            if t > 0 {
                times.push(t);
            }
        }
    }
    (times.into_iter().max(), locks)
}

fn age_days(mtime_nanos: i64, now_nanos: i64) -> u64 {
    // mtime 读不到时记 0：不拿「1970 年」去凑一个天文数字的年龄
    if mtime_nanos <= 0 {
        return 0;
    }
    ((now_nanos - mtime_nanos).max(0) / DAY_NANOS) as u64
}

/// 单个项目的结论装配（纯函数，单测直接喂合成时间戳）
#[allow(clippy::too_many_arguments)]
fn assemble(
    name: String,
    root: PathBuf,
    deps_path: PathBuf,
    deps_bytes: u64,
    deps_mtime: i64,
    activity_mtime: Option<i64>,
    lockfiles: Vec<String>,
    now_nanos: i64,
) -> ProjectHealth {
    let deps_age_days = age_days(deps_mtime, now_nanos);
    let activity_age_days = activity_mtime.map(|t| age_days(t, now_nanos));

    let stale = deps_age_days >= PROJECT_STALE_DAYS;
    let verdict = if stale
        && activity_age_days
            .map(|a| a >= PROJECT_STALE_DAYS)
            .unwrap_or(true)
    {
        ProjectVerdict::Zombie
    } else if deps_bytes >= PROJECT_HEAVY_BYTES {
        ProjectVerdict::Heavy
    } else if stale {
        ProjectVerdict::Stale
    } else {
        ProjectVerdict::Healthy
    };

    ProjectHealth {
        name,
        root: root.to_string_lossy().to_string(),
        deps_path: deps_path.to_string_lossy().to_string(),
        deps_bytes,
        deps_age_days,
        activity_age_days,
        multi_manager: lockfiles.len() >= 2,
        lockfiles,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000_000_000; // 任意固定「现在」

    fn days_ago(days: i64) -> i64 {
        NOW - days * DAY_NANOS
    }

    fn build(
        bytes: u64,
        deps_days: i64,
        activity_days: Option<i64>,
        locks: &[&str],
    ) -> ProjectHealth {
        assemble(
            "p".into(),
            PathBuf::from("/h/p"),
            PathBuf::from("/h/p/node_modules"),
            bytes,
            days_ago(deps_days),
            activity_days.map(days_ago),
            locks.iter().map(|s| s.to_string()).collect(),
            NOW,
        )
    }

    #[test]
    fn fresh_and_small_is_healthy() {
        let p = build(200 << 20, 3, Some(0), &["pnpm"]);
        assert_eq!(p.verdict, ProjectVerdict::Healthy);
        assert!(!p.multi_manager);
        assert_eq!(p.deps_age_days, 3);
        assert_eq!(p.activity_age_days, Some(0));
    }

    #[test]
    fn old_deps_with_active_project_is_stale_not_zombie() {
        let p = build(500 << 20, 90, Some(2), &[]);
        assert_eq!(p.verdict, ProjectVerdict::Stale);
    }

    #[test]
    fn old_deps_and_old_activity_is_zombie() {
        let p = build(500 << 20, 90, Some(120), &[]);
        assert_eq!(p.verdict, ProjectVerdict::Zombie);
    }

    #[test]
    fn missing_activity_markers_falls_back_to_zombie_on_age_only() {
        // 查不到任何标记文件：不因此判「健康」，宁可按依赖年龄判僵尸
        let p = build(500 << 20, 90, None, &[]);
        assert_eq!(p.verdict, ProjectVerdict::Zombie);
        assert_eq!(p.activity_age_days, None);
    }

    #[test]
    fn heavy_beats_healthy_but_not_zombie() {
        let heavy = build(PROJECT_HEAVY_BYTES, 10, Some(1), &[]);
        assert_eq!(heavy.verdict, ProjectVerdict::Heavy);
        // 阈值边界：刚好等于阈值即算偏胖
        let just_under = build(PROJECT_HEAVY_BYTES - 1, 10, Some(1), &[]);
        assert_eq!(just_under.verdict, ProjectVerdict::Healthy);
        // 僵尸优先级高于偏胖
        let zombie = build(PROJECT_HEAVY_BYTES * 2, 90, Some(90), &[]);
        assert_eq!(zombie.verdict, ProjectVerdict::Zombie);
    }

    #[test]
    fn multi_manager_needs_two_lockfiles() {
        let one = build(100 << 20, 1, Some(1), &["npm"]);
        assert!(!one.multi_manager);
        let two = build(100 << 20, 1, Some(1), &["npm", "yarn"]);
        assert!(two.multi_manager);
        assert_eq!(two.lockfiles, vec!["npm", "yarn"]);
    }

    #[test]
    fn unreadable_mtime_never_fakes_huge_age() {
        let p = assemble(
            "p".into(),
            PathBuf::from("/h/p"),
            PathBuf::from("/h/p/node_modules"),
            100,
            0,
            Some(0),
            Vec::new(),
            NOW,
        );
        assert_eq!(p.deps_age_days, 0, "mtime 缺失不能变成 1970 年的年龄");
        assert_eq!(p.verdict, ProjectVerdict::Healthy);
    }

    /// 真实文件系统：两个项目 → 两个账本，按体积降序；锁文件与活动时间真的被读到
    #[test]
    fn build_projects_aggregates_and_sorts() {
        // 活动时间来自真实文件 mtime，所以「现在」必须取真实时钟——
        // 固定常量会和真实 mtime 差出几十天，把刚写的文件算成「久没动」
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
        let home = std::env::temp_dir().join(format!("prune-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let a = home.join("Desktop/big");
        let b = home.join("code/small");
        std::fs::create_dir_all(a.join("node_modules")).unwrap();
        std::fs::create_dir_all(b.join("node_modules")).unwrap();
        std::fs::write(a.join("package.json"), "{}").unwrap();
        std::fs::write(a.join("package-lock.json"), "{}").unwrap();
        std::fs::write(a.join("yarn.lock"), "").unwrap(); // 两个锁文件 → 多管理器

        let mut map = BTreeMap::new();
        map.insert(
            a.join("node_modules").to_string_lossy().to_string(),
            (now - 90 * DAY_NANOS, 1 << 30),
        );
        map.insert(
            b.join("node_modules").to_string_lossy().to_string(),
            (now - DAY_NANOS, 100 << 20),
        );

        let got = build_projects(&map, &home, now);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "Desktop/big", "按体积降序");
        assert_eq!(got[0].deps_bytes, 1 << 30);
        assert_eq!(got[0].lockfiles, vec!["npm", "yarn"]);
        assert!(got[0].multi_manager);
        // package.json 是刚写的 → 活动年龄 0 → 依赖虽旧，项目在动 → Stale
        assert_eq!(got[0].activity_age_days, Some(0));
        assert_eq!(got[0].verdict, ProjectVerdict::Stale);
        assert_eq!(got[1].name, "code/small");
        assert!(got[1].lockfiles.is_empty());
        assert_eq!(got[1].verdict, ProjectVerdict::Healthy);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn project_outside_home_keeps_absolute_name() {
        let home = Path::new("/Users/someone");
        let mut map = BTreeMap::new();
        map.insert("/Volumes/ext/proj/node_modules".to_string(), (NOW, 1 << 30));
        let got = build_projects(&map, home, NOW);
        assert_eq!(got[0].name, "/Volumes/ext/proj");
    }
}
