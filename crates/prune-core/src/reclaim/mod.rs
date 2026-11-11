//! 清理层：四条代码层硬护栏（M1/T-305）。
//!
//! 这不是 UI 提示，是**运行时约束**：
//!
//! 1. 路径白名单 —— `canonicalize` 后必须落在 `disk::whitelist` 的根内（拦 `..`、symlink 逃逸）
//! 2. 级别闸门 —— L3 需显式确认，L4 永不执行
//! 3. 总量熔断 —— 单次超过 `Limits::breaker_bytes`（默认 20 GB）需显式确认
//! 4. 废纸篓 —— 只用 `trash`，代码里不存在对用户路径 `remove_dir_all`
//!
//! 另外：体积在**执行时重算**，不信任报告里的旧数字（报告可能已过期或被篡改）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::disk::whitelist::{self, Level};
use crate::disk::{tree_size, DiskReport};
use std::ffi::OsStr;

/// 默认熔断阈值：20 GB
pub const DEFAULT_BREAKER_BYTES: u64 = 20 * 1024 * 1024 * 1024;

/// 可注入的限制（测试用小阈值，产品用默认）
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub breaker_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            breaker_bytes: DEFAULT_BREAKER_BYTES,
        }
    }
}

/// 一条待清理项
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimItem {
    pub path: String,
    pub label: String,
    pub bytes: u64,
    pub level: Level,
}

/// 一级子项（文件或目录）及其占用；由独立命令按需加载（du 慢，不能拖住 preview）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimChild {
    pub name: String,
    pub bytes: u64,
    /// 是否目录（UI 用不同图标；symlink 记 false，与体积口径一致）
    #[serde(default)]
    pub is_dir: bool,
}

/// 枚举 root 的一级文件/目录并统计各自体积（降序，同名次序稳定）。
///
/// symlink 计 0（`tree_size` 自身也跳过）；单文件直接取占用；
/// 上限 500 项防异常巨型目录拖死调用方。
pub fn first_level_children(root: &Path) -> Vec<ReclaimChild> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<ReclaimChild> = rd
        .flatten()
        .take(500)
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            ReclaimChild {
                name: e.file_name().to_string_lossy().into_owned(),
                bytes: tree_size(&e.path()),
                is_dir,
            }
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    out
}

/// dry-run 结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimPreview {
    pub items: Vec<ReclaimItem>,
    pub total_bytes: u64,
    /// 需要二次确认（L3 或超熔断）
    pub needs_confirm: bool,
    /// 被跳过的组（L4 等），给 UI 直接展示原因
    pub notes: Vec<String>,
}

/// 执行结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimReport {
    pub moved: usize,
    pub bytes: u64,
    pub failed: Vec<FailedItem>,
    /// 校验阶段发现已不存在的条目数（报告过期，直接跳过而非整批拒绝）
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReclaimError {
    NotWhitelisted(String),
    NotCleanable(String),
    NeedsExplicitConfirm(u64),
    NothingToDo,
    AllFailed(Vec<FailedItem>),
}

/// 报告派生白名单（A4）：node_modules 明细路径的放行判定。
///
/// 硬护栏（与报告是否被篡改无关，最后一道闸）：
/// 1. canonicalize 成功且落在 home 内（拦 `..`、symlink 逃逸，同静态白名单）
/// 2. **目录名必须是 `node_modules`** —— 即便授权表被伪造，能删的也只有这个名字的目录
/// 3. 必须精确出现在授权表中（canonicalize 后相等，防 `/a/b/../c` 与大小写花招）
fn is_report_detail_path(path: &Path, home: &Path, allowed: &[String]) -> bool {
    let Ok(c) = path.canonicalize() else {
        return false;
    };
    // home 本身也可能经 symlink（macOS：/var → /private/var），同样规范化后比较
    let home_c = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if !c.starts_with(&home_c) {
        return false;
    }
    if c.file_name() != Some(OsStr::new("node_modules")) {
        return false;
    }
    allowed.iter().any(|a| {
        Path::new(a)
            .canonicalize()
            .map(|ac| ac == c)
            .unwrap_or(false)
    })
}

impl std::fmt::Display for ReclaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReclaimError::NotWhitelisted(p) => write!(f, "路径不在白名单内，拒绝清理: {p}"),
            ReclaimError::NotCleanable(p) => write!(f, "该级别只给命令、不执行: {p}"),
            ReclaimError::NeedsExplicitConfirm(n) => {
                write!(f, "单次清理 {n} 字节超过熔断阈值，需二次确认")
            }
            ReclaimError::NothingToDo => write!(f, "没有可清理的内容"),
            ReclaimError::AllFailed(items) => write!(f, "全部清理失败（{} 项）", items.len()),
        }
    }
}

/// 由报告生成 dry-run 列表。
///
/// - A1 组：只取 `Deadweight` 条目（`NeedsConfirm` 的 darwin-x64 不会进默认清单）
/// - **有明细的 L3 组（A4/A5）：逐项生成条目**——整组 root 不再作为待删项
/// - 其余 L3 组：整项一项，并标记需确认
/// - L2 组：整库一项
/// - L4 组：不生成清理项，只留一条说明
pub fn preview(keys: &[String], report: &DiskReport) -> ReclaimPreview {
    let mut items: Vec<ReclaimItem> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut needs_confirm = false;

    for key in keys {
        let Some(group) = report.groups.iter().find(|g| &g.key == key) else {
            notes.push(format!("未找到分组 {key}（报告可能已过期，请先刷新扫描）"));
            continue;
        };

        match group.level {
            Level::L4 => {
                notes.push(format!(
                    "{} 属于 🔴 高风险项，只提供命令，不执行（docker system prune 等请手动执行）",
                    group.label
                ));
                continue;
            }
            _ => {}
        }

        // A4/A5 明细通道：node_modules（cleanable=false）与模拟器/浏览器二进制逐项确认。
        // 整项 root（node-modules 的 root 是 home）绝不能进清单。
        if group.level == Level::L3 && !group.details.is_empty() {
            needs_confirm = true;
            for d in &group.details {
                if d.bytes > 0 {
                    items.push(ReclaimItem {
                        path: d.path.clone(),
                        label: format!("{} · {}", group.label, d.name),
                        bytes: d.bytes,
                        level: Level::L3,
                    });
                }
            }
            continue;
        }

        // 聚合分组（如「55 个项目的 node_modules」）没有单一可删根：
        // 它的 root 是 home，绝不能当成待删项——只说明不提供清理。
        if !group.cleanable {
            notes.push(format!(
                "{} 分散在 {} 个目录，只做统计；请在对应项目内自行删除",
                group.label, group.entry_count
            ));
            continue;
        }

        match group.level {
            Level::L3 => {
                needs_confirm = true;
                if group.total_bytes > 0 {
                    items.push(ReclaimItem {
                        path: group.root.clone(),
                        label: group.label.clone(),
                        bytes: group.total_bytes,
                        level: Level::L3,
                    });
                }
                continue;
            }
            Level::L1 | Level::L2 | Level::L4 => {}
        }

        match &group.deadweight {
            Some(dw) if !dw.items.is_empty() => {
                for it in dw
                    .items
                    .iter()
                    .filter(|i| i.verdict == crate::classify::Verdict::Deadweight)
                {
                    items.push(ReclaimItem {
                        path: it.path.clone(),
                        label: format!("{} · {}", group.label, it.name),
                        bytes: it.bytes,
                        level: Level::L1,
                    });
                }
                let pending = dw
                    .items
                    .iter()
                    .filter(|i| i.verdict == crate::classify::Verdict::NeedsConfirm)
                    .count();
                if pending > 0 {
                    notes.push(format!(
                        "{pending} 个 darwin-x64 条目依赖 Rosetta，已保留在清单之外"
                    ));
                }
            }
            _ => {
                if group.total_bytes > 0 {
                    items.push(ReclaimItem {
                        path: group.root.clone(),
                        label: group.label.clone(),
                        bytes: group.total_bytes,
                        level: group.level,
                    });
                }
            }
        }
    }

    let total_bytes = items.iter().map(|i| i.bytes).sum();
    let over_breaker = total_bytes > DEFAULT_BREAKER_BYTES;
    if over_breaker {
        needs_confirm = true;
    }
    if items.is_empty() && notes.is_empty() {
        notes.push("没有可清理的内容".into());
    }

    ReclaimPreview {
        items,
        total_bytes,
        needs_confirm: needs_confirm || over_breaker,
        notes,
    }
}

/// 执行清理（只走废纸篓）。
///
/// **已不存在的条目直接跳过**（计入 `ReclaimReport::skipped`）：报告是旧快照，
/// 条目可能已被上轮清理或用户手动删除，「不存在」≠「不在白名单」，不得整批拒绝。
/// 全部消失时返回 `NothingToDo`。
///
/// `allowed` 是**报告派生白名单**（A4/A5 明细路径，由调用方从当前报告的
/// `groups[].details[].path` 组装）：node_modules 目录不在静态白名单内，
/// 只能凭「本次报告发现过它」放行，且仍受 `is_report_detail_path` 的硬护栏约束。
pub fn execute(
    items: &[ReclaimItem],
    home: &Path,
    confirmed: bool,
    limits: Limits,
    allowed: &[String],
) -> Result<ReclaimReport, ReclaimError> {
    if items.is_empty() {
        return Err(ReclaimError::NothingToDo);
    }

    // 护栏 1 + 2：先全量校验，有任何一项不合规则整批拒绝（不做「部分执行」）。
    // 「路径已不存在」（报告过期：上轮已清理 / 用户手动删除）不算护栏违规，
    // 直接跳过——它和「在白名单外」是两回事，混在一起会让旧报告整批清不动。
    let mut resolved: Vec<(PathBuf, Level)> = Vec::with_capacity(items.len());
    let mut vanished = 0usize;
    for it in items {
        let path = PathBuf::from(&it.path);
        if !path.exists() {
            vanished += 1;
            continue;
        }
        if !whitelist::is_whitelisted(&path, home) && !is_report_detail_path(&path, home, allowed) {
            return Err(ReclaimError::NotWhitelisted(it.path.clone()));
        }
        if it.level == Level::L4 {
            return Err(ReclaimError::NotCleanable(it.path.clone()));
        }
        if it.level == Level::L3 && !confirmed {
            return Err(ReclaimError::NeedsExplicitConfirm(0));
        }
        resolved.push((path, it.level));
    }

    // 全部条目都已消失：等价于没有可清理的内容
    if resolved.is_empty() {
        return Err(ReclaimError::NothingToDo);
    }

    // 护栏 3：体积执行时重算，不信任报告
    let total: u64 = resolved.iter().map(|(p, _)| tree_size(p)).sum();
    if total > limits.breaker_bytes && !confirmed {
        return Err(ReclaimError::NeedsExplicitConfirm(total));
    }

    // 护栏 4：废纸篓
    let mut moved = 0usize;
    let mut bytes = 0u64;
    let mut failed: Vec<FailedItem> = Vec::new();
    for (path, _) in resolved {
        let size = tree_size(&path);
        match trash::delete(&path) {
            Ok(()) => {
                moved += 1;
                bytes = bytes.saturating_add(size);
            }
            Err(e) => failed.push(FailedItem {
                path: path.to_string_lossy().to_string(),
                reason: e.to_string(),
            }),
        }
    }

    if moved == 0 {
        return Err(ReclaimError::AllFailed(failed));
    }
    Ok(ReclaimReport {
        moved,
        bytes,
        failed,
        skipped: vanished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::{CacheGroup, DeadweightItem, DeadweightStats, Totals};

    fn temp_home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("prune-rc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Library/Caches/Yarn/v6")).unwrap();
        home
    }

    fn group(
        key: &str,
        level: Level,
        root: &Path,
        bytes: u64,
        dw: Option<DeadweightStats>,
    ) -> CacheGroup {
        CacheGroup {
            key: key.into(),
            label: key.into(),
            root: root.to_string_lossy().to_string(),
            level,
            total_bytes: bytes,
            entry_count: 1,
            cleanable: true,
            details: Vec::new(),
            deadweight: dw,
        }
    }

    fn report_with(groups: Vec<CacheGroup>) -> DiskReport {
        DiskReport {
            scanned_at: 0,
            scan_elapsed_ms: 0,
            incremental: false,
            groups,
            dup: None,
            totals: Totals {
                deadweight: 0,
                l2: 0,
                l3: 0,
                l4: 0,
                reclaimable: 0,
                cache_total: 0,
            },
            volume: None,
            projects: Vec::new(),
        }
    }

    #[test]
    fn preview_takes_only_deadweight_and_notes_rosetta_items() {
        let home = temp_home("preview");
        let root = home.join("Library/Caches/Yarn/v6");
        let dw = DeadweightStats {
            bytes: 300,
            native_bytes: 100,
            other_bytes: 0,
            entry_count: 2,
            by_platform: vec![],
            ratio: Some(3.0),
            items: vec![
                DeadweightItem {
                    name: "npm-a-linux-x64".into(),
                    path: root.join("npm-a-linux-x64").to_string_lossy().to_string(),
                    bytes: 200,
                    platform: Some("linux".into()),
                    verdict: crate::classify::Verdict::Deadweight,
                },
                DeadweightItem {
                    name: "npm-b-darwin-x64".into(),
                    path: root.join("npm-b-darwin-x64").to_string_lossy().to_string(),
                    bytes: 100,
                    platform: Some("darwin-x64".into()),
                    verdict: crate::classify::Verdict::NeedsConfirm,
                },
            ],
        };
        let report = report_with(vec![group("yarn", Level::L2, &root, 400, Some(dw))]);
        let pv = preview(&["yarn".into()], &report);
        assert_eq!(pv.items.len(), 1, "NeedsConfirm 不进默认清单");
        assert_eq!(pv.items[0].bytes, 200);
        assert!(pv.notes.iter().any(|n| n.contains("Rosetta")));
        assert!(!pv.needs_confirm);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn preview_skips_l4_and_flags_l3() {
        let home = temp_home("levels");
        let docker = home.join(".colima");
        let sim = home.join("Library/Developer/CoreSimulator/Devices");
        std::fs::create_dir_all(&docker).unwrap();
        std::fs::create_dir_all(&sim).unwrap();
        let report = report_with(vec![
            group("colima", Level::L4, &docker, 900, None),
            group("simulator", Level::L3, &sim, 500, None),
        ]);
        let pv = preview(&["colima".into(), "simulator".into()], &report);
        assert_eq!(pv.items.len(), 1, "L4 永不进清理项");
        assert!(pv.needs_confirm, "L3 需要确认");
        assert!(pv.notes.iter().any(|n| n.contains("只提供命令")));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 聚合分组（root = home）绝不能被当成待删项——那是家目录
    #[test]
    fn preview_refuses_aggregate_group_with_home_root() {
        let home = temp_home("aggregate");
        let mut nm = group("node-modules", Level::L3, &home, 500, None);
        nm.cleanable = false;
        nm.entry_count = 55;
        let report = report_with(vec![nm]);

        let pv = preview(&["node-modules".into()], &report);
        assert!(pv.items.is_empty(), "聚合项不得生成清理项（root 是 home）");
        assert_eq!(pv.total_bytes, 0);
        assert!(
            pv.notes.iter().any(|n| n.contains("55 个目录")),
            "要明确告诉用户只做统计，实际 notes={:?}",
            pv.notes
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 报告过期的条目（已消失）应跳过，不得整批拒绝；其余条目正常清理。
    /// Yarn 白名单条目是 MacOnly——Linux 上该路径本来就不在白名单，
    /// NotWhitelisted 是正确行为，所以本用例只在 macOS 上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn execute_skips_vanished_items() {
        let home = temp_home("vanished");
        let cache = home.join("Library/Caches/Yarn/v6");
        let alive = cache.join("npm-alive-1-integrity");
        std::fs::create_dir_all(alive.join("nodejs")).unwrap();
        std::fs::write(alive.join("nodejs/index.js"), vec![b'x'; 64]).unwrap();
        let gone = cache.join("npm-gone-2-integrity"); // 不创建，模拟报告过期
        let items = vec![
            ReclaimItem {
                path: gone.to_string_lossy().to_string(),
                label: "已消失".into(),
                bytes: 1,
                level: Level::L1,
            },
            ReclaimItem {
                path: alive.to_string_lossy().to_string(),
                label: "存活".into(),
                bytes: 4096,
                level: Level::L1,
            },
        ];
        match execute(&items, &home, true, Limits::default(), &[]) {
            Ok(r) => {
                assert_eq!(r.skipped, 1, "消失的条目应计入 skipped");
                assert_eq!(r.moved, 1);
                assert!(!alive.exists(), "存活条目应已入废纸篓");
            }
            Err(ReclaimError::AllFailed(_)) => {
                // 临时目录（/var/folders）Finder 拒绝入废纸篓（macOS 限制）：
                // 本测试的验证点是幽灵路径没有触发 NotWhitelisted——
                // 错误必须发生在 trash 阶段而不是护栏阶段。
            }
            Err(e) => panic!("消失条目应被跳过而非整批拒绝，实际 {e:?}"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 全部条目都已消失 → NothingToDo，而不是误报
    #[test]
    fn execute_all_vanished_is_nothing_to_do() {
        let home = temp_home("all-vanished");
        let gone = home.join("Library/Caches/Yarn/v6/npm-gone-integrity");
        let items = vec![ReclaimItem {
            path: gone.to_string_lossy().to_string(),
            label: "幽灵".into(),
            bytes: 1,
            level: Level::L1,
        }];
        let err = execute(&items, &home, true, Limits::default(), &[]).unwrap_err();
        assert!(matches!(err, ReclaimError::NothingToDo));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 白名单最后一道闸：家目录本体必须被拒（即使前面环节都漏了）
    #[test]
    fn execute_rejects_home_itself() {
        let home = temp_home("homeguard");
        let items = vec![ReclaimItem {
            path: home.to_string_lossy().to_string(),
            label: "家目录".into(),
            bytes: 1,
            level: Level::L3,
        }];
        let err = execute(&items, &home, true, Limits::default(), &[]).unwrap_err();
        assert!(matches!(err, ReclaimError::NotWhitelisted(_)));
        assert!(home.exists(), "拒绝时不得动目录");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn execute_rejects_outside_whitelist() {
        let home = temp_home("outside");
        let outside = home.join("Documents/report.docx");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, b"important").unwrap();
        let items = vec![ReclaimItem {
            path: outside.to_string_lossy().to_string(),
            label: "用户文档".into(),
            bytes: 9,
            level: Level::L2,
        }];
        let err = execute(&items, &home, true, Limits::default(), &[]).unwrap_err();
        assert!(matches!(err, ReclaimError::NotWhitelisted(_)));
        assert!(outside.exists(), "拒绝时不得动文件");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// L4 / 未确认 L3 拒绝 + 熔断。依赖 Yarn（MacOnly）白名单条目，
    /// Linux 上 NotWhitelisted 先于 level 判定，语义不同——只在 macOS 上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn execute_rejects_l4_and_unconfirmed_l3_and_breaker() {
        let home = temp_home("guards");
        let cache = home.join("Library/Caches/Yarn/v6/npm-x-linux-x64");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("f.bin"), vec![b'x'; 64]).unwrap();

        // L4 直接拒绝
        let l4 = vec![ReclaimItem {
            path: cache.to_string_lossy().to_string(),
            label: "x".into(),
            bytes: 64,
            level: Level::L4,
        }];
        assert!(matches!(
            execute(&l4, &home, true, Limits::default(), &[]).unwrap_err(),
            ReclaimError::NotCleanable(_)
        ));

        // L3 未确认拒绝
        let l3 = vec![ReclaimItem {
            path: cache.to_string_lossy().to_string(),
            label: "x".into(),
            bytes: 64,
            level: Level::L3,
        }];
        assert!(matches!(
            execute(&l3, &home, false, Limits::default(), &[]).unwrap_err(),
            ReclaimError::NeedsExplicitConfirm(_)
        ));

        // 熔断：阈值设成 10 字节（实际按物理块 4096 字节），未确认则拒绝
        let ok = vec![ReclaimItem {
            path: cache.to_string_lossy().to_string(),
            label: "x".into(),
            bytes: 64,
            level: Level::L2,
        }];
        let err = execute(&ok, &home, false, Limits { breaker_bytes: 10 }, &[]).unwrap_err();
        match err {
            ReclaimError::NeedsExplicitConfirm(n) => {
                assert!(n >= 4096, "体积执行时重算（物理块）: {n}")
            }
            other => panic!("应触发熔断，实际 {other:?}"),
        }
        assert!(cache.exists(), "熔断时不得动文件");
        let _ = std::fs::remove_dir_all(&home);
    }

    // ---------------------------------------------------------------- A4/A5

    use crate::disk::DetailItem;

    fn detail(name: &str, path: &Path, bytes: u64, age_days: u64) -> DetailItem {
        DetailItem {
            name: name.into(),
            path: path.to_string_lossy().to_string(),
            bytes,
            age_days,
            note: None,
        }
    }

    /// A4：有明细的 L3 组逐项进 preview（整项 root 是 home，绝不能进清单）
    #[test]
    fn preview_uses_details_for_l3_groups() {
        let home = temp_home("details");
        let nm1 = home.join("Desktop/p1/node_modules");
        let nm2 = home.join("git/p2/node_modules");
        std::fs::create_dir_all(&nm1).unwrap();
        std::fs::create_dir_all(&nm2).unwrap();

        let mut nm = group("node-modules", Level::L3, &home, 500, None);
        nm.cleanable = false;
        nm.details = vec![
            detail("Desktop/p1/node_modules", &nm1, 300, 200),
            detail("git/p2/node_modules", &nm2, 200, 3),
        ];
        let report = report_with(vec![nm]);

        let pv = preview(&["node-modules".into()], &report);
        assert_eq!(pv.items.len(), 2, "逐项生成，不整项");
        assert!(pv.needs_confirm, "L3 明细必须二次确认");
        assert!(pv.items.iter().all(|i| i.level == Level::L3));
        assert!(
            !pv.items.iter().any(|i| i.path == home.to_string_lossy()),
            "home 本体绝不能出现在清单里"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A4 无明细时维持旧行为：只做统计说明
    #[test]
    fn preview_without_details_keeps_statistics_note() {
        let home = temp_home("nodetails");
        let mut nm = group("node-modules", Level::L3, &home, 500, None);
        nm.cleanable = false;
        nm.entry_count = 55;
        let report = report_with(vec![nm]);
        let pv = preview(&["node-modules".into()], &report);
        assert!(pv.items.is_empty());
        assert!(pv.notes.iter().any(|n| n.contains("只做统计")));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 一级子项明细：按体积降序、目录标记正确（供前端按需加载）
    #[test]
    fn first_level_children_sorted_desc_with_dir_flag() {
        let home = temp_home("children");
        let root = home.join(".npm/_cacache");
        let big = root.join("content-v2");
        let small = root.join("index-v5");
        std::fs::create_dir_all(&big).unwrap();
        std::fs::create_dir_all(&small).unwrap();
        std::fs::write(big.join("blob"), vec![0u8; 400]).unwrap();
        std::fs::write(small.join("idx"), vec![0u8; 100]).unwrap();

        let children = first_level_children(&root);
        assert_eq!(children.len(), 2, "两个一级目录都在明细里");
        assert_eq!(children[0].name, "content-v2", "按体积降序");
        assert!(children[0].bytes >= 400 && children[1].bytes >= 100);
        assert!(children.iter().all(|c| c.is_dir), "子目录标记为目录");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 护栏：node_modules 路径没有报告授权 → 拒绝
    #[test]
    fn execute_rejects_node_modules_without_report_allowlist() {
        let home = temp_home("nm-guard");
        let nm = home.join("Desktop/p1/node_modules");
        std::fs::create_dir_all(nm.join("react")).unwrap();
        let items = vec![ReclaimItem {
            path: nm.to_string_lossy().to_string(),
            label: "p1 依赖".into(),
            bytes: 4096,
            level: Level::L3,
        }];
        let err = execute(&items, &home, true, Limits::default(), &[]).unwrap_err();
        assert!(matches!(err, ReclaimError::NotWhitelisted(_)));
        assert!(nm.exists(), "拒绝时不得动目录");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 护栏：授权表里有记录、但目标目录名不是 node_modules → 拒绝（最后一道闸）
    #[test]
    fn execute_rejects_non_node_modules_even_if_allowlisted() {
        let home = temp_home("nm-name-guard");
        let evil = home.join("Desktop/p1/important");
        std::fs::create_dir_all(&evil).unwrap();
        let allowed = vec![evil.to_string_lossy().to_string()];
        let items = vec![ReclaimItem {
            path: evil.to_string_lossy().to_string(),
            label: "伪装路径".into(),
            bytes: 4096,
            level: Level::L3,
        }];
        let err = execute(&items, &home, true, Limits::default(), &allowed).unwrap_err();
        assert!(matches!(err, ReclaimError::NotWhitelisted(_)));
        assert!(evil.exists(), "非 node_modules 名字的目录必须被拦下");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 放行链路：报告授权 + 名字对 + 在 home 内 → 废纸篓成功
    #[test]
    fn execute_allows_reported_node_modules() {
        let home = temp_home("nm-allow");
        let nm = home.join("Desktop/p1/node_modules");
        std::fs::create_dir_all(nm.join("react")).unwrap();
        std::fs::write(nm.join("react/index.js"), vec![b'x'; 64]).unwrap();
        let allowed = vec![nm.to_string_lossy().to_string()];
        let items = vec![ReclaimItem {
            path: nm.to_string_lossy().to_string(),
            label: "p1 依赖".into(),
            bytes: 4096,
            level: Level::L3,
        }];
        let report = execute(&items, &home, true, Limits::default(), &allowed);
        match report {
            Ok(r) => {
                assert_eq!(r.moved, 1);
                assert!(!nm.exists(), "已移入废纸篓");
            }
            Err(ReclaimError::AllFailed(_)) => {
                // 临时目录（/var/folders）Finder 拒绝入废纸篓（macOS 限制）：
                // 本测试的验证点是「报告派生白名单放行了 node_modules」——
                // 错误必须发生在 trash 阶段而不是护栏阶段。真机路径在验收记录覆盖。
            }
            Err(e) => panic!("护栏应放行，实际 {e:?}"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
