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
/// - L2 组：整库一项
/// - L3 组：整项一项，并标记需确认
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
            Level::L1 | Level::L2 => {}
        }

        match &group.deadweight {
            Some(dw) if !dw.items.is_empty() => {
                for it in dw.items.iter().filter(|i| i.verdict == crate::classify::Verdict::Deadweight) {
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
pub fn execute(
    items: &[ReclaimItem],
    home: &Path,
    confirmed: bool,
    limits: Limits,
) -> Result<ReclaimReport, ReclaimError> {
    if items.is_empty() {
        return Err(ReclaimError::NothingToDo);
    }

    // 护栏 1 + 2：先全量校验，有任何一项不合规则整批拒绝（不做「部分执行」）
    let mut resolved: Vec<(PathBuf, Level)> = Vec::with_capacity(items.len());
    for it in items {
        let path = PathBuf::from(&it.path);
        if !whitelist::is_whitelisted(&path, home) {
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

    fn group(key: &str, level: Level, root: &Path, bytes: u64, dw: Option<DeadweightStats>) -> CacheGroup {
        CacheGroup {
            key: key.into(),
            label: key.into(),
            root: root.to_string_lossy().to_string(),
            level,
            total_bytes: bytes,
            entry_count: 1,
            deadweight: dw,
        }
    }

    fn report_with(groups: Vec<CacheGroup>) -> DiskReport {
        DiskReport {
            scanned_at: 0,
            scan_elapsed_ms: 0,
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
        std::fs::create_dir_all(&docker).unwrap();
        let report = report_with(vec![
            group("colima", Level::L4, &docker, 900, None),
            group("node-modules", Level::L3, &home, 500, None),
        ]);
        let pv = preview(&["colima".into(), "node-modules".into()], &report);
        assert_eq!(pv.items.len(), 1, "L4 永不进清理项");
        assert!(pv.needs_confirm, "L3 需要确认");
        assert!(pv.notes.iter().any(|n| n.contains("只提供命令")));
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
        let err = execute(&items, &home, true, Limits::default()).unwrap_err();
        assert!(matches!(err, ReclaimError::NotWhitelisted(_)));
        assert!(outside.exists(), "拒绝时不得动文件");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
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
            execute(&l4, &home, true, Limits::default()).unwrap_err(),
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
            execute(&l3, &home, false, Limits::default()).unwrap_err(),
            ReclaimError::NeedsExplicitConfirm(_)
        ));

        // 熔断：阈值设成 10 字节（实际按物理块 4096 字节），未确认则拒绝
        let ok = vec![ReclaimItem {
            path: cache.to_string_lossy().to_string(),
            label: "x".into(),
            bytes: 64,
            level: Level::L2,
        }];
        let err = execute(
            &ok,
            &home,
            false,
            Limits {
                breaker_bytes: 10,
            },
        )
        .unwrap_err();
        match err {
            ReclaimError::NeedsExplicitConfirm(n) => assert!(n >= 4096, "体积执行时重算（物理块）: {n}"),
            other => panic!("应触发熔断，实际 {other:?}"),
        }
        assert!(cache.exists(), "熔断时不得动文件");
        let _ = std::fs::remove_dir_all(&home);
    }
}
