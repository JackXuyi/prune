//! M30 项目休眠归档（F14）：把「僵尸项目」的依赖目录打包进受管归档区，可完整还原。
//!
//! 语义（docs/f14-m30-archive-plan.md）：
//! - 归档对象**只有依赖目录**（目录名必须是 `node_modules`）——用户代码永不进归档，
//!   这是与「备份工具」的分界线：归档损坏最多丢一次重装，不会丢源码；
//! - 归档区在应用配置目录下（`<config>/com.xuyi.prune/archive`），废纸篓语义之外
//!   （废纸篓会被系统清空），单文件 tar.gz + manifest.json 账本；
//! - 顺序：打包 → 重读校验（条目数 + 非压缩字节数）→ 通过后才删原件 → 写 manifest；
//!   删除失败回滚 manifest 与归档文件，不产生孤儿归档；
//! - 恢复：归档存在 + 目标冲突检测 → 解压（tar 默认拒绝绝对路径与 `..` 逃逸）→ 校验；
//!
//! 本模块不存在对用户路径的 `remove_dir_all`（沿用 reclaim 的 invariant）：
//! 用户路径只走废纸篓，只有「自己刚写的归档半成品」允许直接删文件。

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 归档区账本文件名
pub const MANIFEST_FILE: &str = "manifest.json";
/// 归档区在配置目录下的固定位置（GUI 与未来的 CLI 同口径）
pub const ARCHIVE_SUBDIR: &str = "com.xuyi.prune/archive";

/// 一条归档账本记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// `<unix秒>-<项目名slug>-<路径hash>`：可读且唯一
    pub id: String,
    /// 项目名（依赖目录的父目录名）
    pub project_name: String,
    /// 项目根（恢复后用户定位的落点）
    pub project_root: String,
    /// 依赖目录绝对路径（恢复目标，唯一可信来源）
    pub deps_path: String,
    /// 归档时统计的非压缩字节数（校验基线）
    pub bytes: u64,
    /// 归档时统计的条目数（文件 + 符号链接，不含目录）
    pub files: u64,
    /// 归档时间（Unix 秒）
    pub archived_at: i64,
    /// 归档文件绝对路径
    pub archive_path: String,
}

/// 归档前预览（dry-run）：只读，不写任何文件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePlan {
    pub id: String,
    pub project_name: String,
    pub project_root: String,
    pub deps_path: String,
    pub files: u64,
    pub bytes: u64,
    /// 将写入的归档文件
    pub archive_path: String,
}

/// 恢复前预览（dry-run）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub id: String,
    pub target: String,
    pub files: u64,
    pub bytes: u64,
    /// 目标已存在：未确认覆盖会被拒绝（覆盖也走废纸篓）
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub target: String,
    pub files_restored: u64,
    pub bytes_restored: u64,
}

/// 归档区根。带 `config_dir` 参数而不是直接读系统环境：测试可替换。
pub fn archive_root(config_dir: &Path) -> PathBuf {
    config_dir.join(ARCHIVE_SUBDIR)
}

/// 系统归档区根（GUI / CLI 入口用）
pub fn system_archive_root() -> Option<PathBuf> {
    crate::paths::config_dir().map(|c| archive_root(&c))
}

/// 账本读取。文件缺失 / 损坏一律视为空账本：归档文件本身才是数据，
/// 账本坏了宁可「看不见」也不要崩——孤儿归档可由用户手动处理。
pub fn list(root: &Path) -> Vec<ArchiveEntry> {
    let mut entries = read_manifest(root);
    entries.sort_by_key(|e| std::cmp::Reverse(e.archived_at));
    entries
}

pub fn find(root: &Path, id: &str) -> Option<ArchiveEntry> {
    read_manifest(root).into_iter().find(|e| e.id == id)
}

/// 归档预览：校验依赖目录 + 统计体积，不写任何文件
pub fn plan(deps: &Path, root: &Path, now_secs: i64) -> Result<ArchivePlan, String> {
    validate_deps(deps)?;
    let (files, bytes) = walk_counts(deps)?;
    let id = make_id(deps, now_secs);
    Ok(ArchivePlan {
        project_name: project_name(deps),
        project_root: deps
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        deps_path: deps.display().to_string(),
        files,
        bytes,
        archive_path: root.join(format!("{id}.tar.gz")).display().to_string(),
        id,
    })
}

/// 归档执行：打包 → 校验 → 删原件 → 记账。任一步失败都不丢原件。
pub fn create(deps: &Path, root: &Path, now_secs: i64) -> Result<ArchiveEntry, String> {
    create_with(deps, root, now_secs, &trash_delete)
}

/// 恢复预览：目标冲突检测（只读）
pub fn restore_preview(entry: &ArchiveEntry) -> RestorePreview {
    RestorePreview {
        id: entry.id.clone(),
        target: entry.deps_path.clone(),
        files: entry.files,
        bytes: entry.bytes,
        conflict: Path::new(&entry.deps_path).exists(),
    }
}

/// 恢复执行：冲突检测 → 解压 → 校验。校验失败把解压产物移入废纸篓。
pub fn restore(entry: &ArchiveEntry, force: bool) -> Result<RestoreReport, String> {
    restore_with(entry, force, &trash_delete)
}

/// 删除归档：归档文件走废纸篓 + 移除账本条目（归档区不是只进不出）
pub fn delete(entry: &ArchiveEntry, root: &Path) -> Result<(), String> {
    delete_with(entry, root, &trash_delete)
}

// ---------------------------------------------------------------- 内部实现

/// 删除动作注入：生产走废纸篓，测试传「直接删临时目录」——测试不依赖
/// 系统废纸篓的可用性（CI Linux 容器里没有），生产路径一行都不放宽。
fn trash_delete(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("移入废纸篓失败：{e}"))
}

fn delete_with(
    entry: &ArchiveEntry,
    root: &Path,
    delete: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let archive = PathBuf::from(&entry.archive_path);
    if archive.exists() {
        delete(&archive)?;
    }
    let mut entries = read_manifest(root);
    entries.retain(|e| e.id != entry.id);
    write_manifest(root, &entries)
}

fn create_with(
    deps: &Path,
    root: &Path,
    now_secs: i64,
    delete: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<ArchiveEntry, String> {
    let p = plan(deps, root, now_secs)?;
    let archive = PathBuf::from(&p.archive_path);
    // 同 id 撞名（同秒重复归档）：拒绝而不是覆盖——覆盖会毁掉前一次的归档数据
    if archive.exists() {
        return Err(format!(
            "归档文件已存在（{}），请稍后重试（同秒重复归档）",
            p.archive_path
        ));
    }
    fs::create_dir_all(root).map_err(|e| format!("创建归档区失败：{e}"))?;

    write_archive(deps, &archive)?;

    // 校验：重读归档，条目数与非压缩字节数必须与扫描一致
    let verified = match verify_archive(&archive) {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&archive); // 自己写的半成品，允许直删
            return Err(e);
        }
    };
    if verified != (p.files, p.bytes) {
        let _ = fs::remove_file(&archive);
        return Err(format!(
            "归档校验失败：预期 {} 项 / {} B，实际 {} 项 / {} B。原件未动。",
            p.files, p.bytes, verified.0, verified.1
        ));
    }

    let entry = ArchiveEntry {
        id: p.id,
        project_name: p.project_name,
        project_root: p.project_root,
        deps_path: p.deps_path,
        bytes: p.bytes,
        files: p.files,
        archived_at: now_secs,
        archive_path: p.archive_path,
    };

    // 先写账再删原件：删除失败可回滚，不产生「账上没有、盘上还在」的半状态
    let mut entries = read_manifest(root);
    entries.push(entry.clone());
    write_manifest(root, &entries)?;

    if let Err(e) = delete(deps) {
        entries.retain(|x| x.id != entry.id);
        let _ = write_manifest(root, &entries);
        let _ = fs::remove_file(&archive);
        return Err(format!("删除原件失败，归档已回滚：{e}"));
    }

    Ok(entry)
}

fn restore_with(
    entry: &ArchiveEntry,
    force: bool,
    delete: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<RestoreReport, String> {
    let target = PathBuf::from(&entry.deps_path);
    let archive = PathBuf::from(&entry.archive_path);
    if !archive.is_file() {
        return Err(format!(
            "归档文件不存在：{}（可能已被删除）",
            entry.archive_path
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| "恢复目标路径异常".to_string())?;
    if !parent.is_dir() {
        return Err(format!(
            "项目目录不存在：{}。请先恢复/创建项目目录，再恢复依赖。",
            parent.display()
        ));
    }
    if target.exists() {
        if !force {
            return Err(format!(
                "目标已存在：{}。如确认覆盖请再次确认（覆盖内容会移入废纸篓）。",
                target.display()
            ));
        }
        delete(&target)?;
    }

    // tar crate 默认拒绝绝对路径与 `..` 逃逸条目，无需自己再拦
    let file = File::open(&archive).map_err(|e| format!("打开归档失败：{e}"))?;
    let dec = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut ar = tar::Archive::new(dec);
    ar.unpack(parent)
        .map_err(|e| format!("解压失败（已中止，目标可能部分写入）：{e}"))?;

    let (files, bytes) = walk_counts(&target)?;
    if files != entry.files || bytes != entry.bytes {
        // 解压产物是我们自己刚写的：移入废纸篓比留在项目里更安全（可恢复）
        let _ = delete(&target);
        return Err(format!(
            "恢复校验失败：预期 {} 项 / {} B，实际 {} 项 / {} B；解压产物已移入废纸篓。",
            entry.files, entry.bytes, files, bytes
        ));
    }

    Ok(RestoreReport {
        target: entry.deps_path.clone(),
        files_restored: files,
        bytes_restored: bytes,
    })
}

/// 护栏：只归档依赖目录。GUI / CLI 传任意路径都会在这里被拒。
fn validate_deps(deps: &Path) -> Result<(), String> {
    if !deps.is_absolute() {
        return Err("依赖目录必须是绝对路径".into());
    }
    let md = fs::symlink_metadata(deps)
        .map_err(|_| format!("依赖目录不存在：{}（项目是否已经归档过？）", deps.display()))?;
    if md.file_type().is_symlink() {
        return Err("依赖目录是符号链接，拒绝归档（链接目标不可控）".into());
    }
    if !md.is_dir() {
        return Err("路径不是目录".into());
    }
    if deps.file_name().and_then(|n| n.to_str()) != Some("node_modules") {
        return Err("只归档依赖目录（node_modules）：用户代码不进入归档".into());
    }
    Ok(())
}

/// 统计非目录条目数与非压缩字节数（符号链接按条目计、字节记 0）
fn walk_counts(root: &Path) -> Result<(u64, u64), String> {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
        for ent in rd {
            let ent = ent.map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
            let path = ent.path();
            let md = fs::symlink_metadata(&path)
                .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            if md.file_type().is_symlink() {
                files += 1; // 链接本身不占内容字节
            } else if md.is_dir() {
                stack.push(path);
            } else {
                files += 1;
                bytes = bytes.saturating_add(md.len());
            }
        }
    }
    Ok((files, bytes))
}

/// 流式写 tar.gz：以 `node_modules/` 为根前缀，恢复时解压到项目目录即可
fn write_archive(deps: &Path, target: &Path) -> Result<(), String> {
    let file = File::create(target).map_err(|e| format!("创建归档文件失败：{e}"))?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(enc);
    builder.follow_symlinks(false);
    builder
        .append_dir_all("node_modules", deps)
        .map_err(|e| format!("打包失败：{e}"))?;
    let enc = builder
        .into_inner()
        .map_err(|e| format!("打包收尾失败：{e}"))?;
    enc.finish().map_err(|e| format!("写入归档失败：{e}"))?;
    Ok(())
}

/// 重读归档统计（条目数 / 非压缩字节数）——与 [`walk_counts`] 同口径
fn verify_archive(target: &Path) -> Result<(u64, u64), String> {
    let file = File::open(target).map_err(|e| format!("打开归档失败：{e}"))?;
    let dec = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut ar = tar::Archive::new(dec);
    let mut files = 0u64;
    let mut bytes = 0u64;
    let entries = ar.entries().map_err(|e| format!("读取归档失败：{e}"))?;
    for ent in entries {
        let ent = ent.map_err(|e| format!("读取归档失败：{e}"))?;
        let header = ent.header();
        if header.entry_type().is_dir() {
            continue;
        }
        files += 1;
        if header.entry_type().is_file() {
            bytes = bytes.saturating_add(header.size().unwrap_or(0));
        }
    }
    Ok((files, bytes))
}

fn project_name(deps: &Path) -> String {
    deps.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into())
}

/// id = `<unix秒>-<项目名slug>-<路径hash>`：人可读、可排序、路径唯一
fn make_id(deps: &Path, now_secs: i64) -> String {
    let slug: String = project_name(deps)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut hash = 0xcbf2_9ce4_8422_2325u64; // FNV-1a：稳定、零依赖
    for b in deps.to_string_lossy().as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{now_secs}-{slug}-{:08x}", (hash & 0xffff_ffff) as u32)
}

fn read_manifest(root: &Path) -> Vec<ArchiveEntry> {
    fs::read_to_string(root.join(MANIFEST_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<ArchiveEntry>>(&t).ok())
        .unwrap_or_default()
}

/// 账本原子写：先写 `.tmp` 再 rename，断电不会留半截 JSON
fn write_manifest(root: &Path, entries: &[ArchiveEntry]) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| format!("创建归档区失败：{e}"))?;
    let tmp = root.join(format!("{MANIFEST_FILE}.tmp"));
    let body = serde_json::to_vec_pretty(entries).map_err(|e| format!("序列化账本失败：{e}"))?;
    fs::write(&tmp, body).map_err(|e| format!("写账本失败：{e}"))?;
    fs::rename(&tmp, root.join(MANIFEST_FILE)).map_err(|e| format!("写账本失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用删除器：直接清掉临时目录（生产路径永远走废纸篓）
    fn test_delete(path: &Path) -> Result<(), String> {
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(|e| e.to_string())
        } else {
            fs::remove_file(path).map_err(|e| e.to_string())
        }
    }

    struct Fixture {
        base: PathBuf,
        project: PathBuf,
        deps: PathBuf,
        root: PathBuf,
    }

    fn setup(tag: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!("prune-archive-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let project = base.join("code/my-app");
        let deps = project.join("node_modules");
        fs::create_dir_all(deps.join("some-pkg/dist")).unwrap();
        fs::write(deps.join("some-pkg/dist/index.js"), b"console.log(1)").unwrap();
        fs::write(deps.join("some-pkg/package.json"), b"{}").unwrap();
        fs::write(project.join("package.json"), b"{}").unwrap(); // 用户代码：必须不被归档
        let root = base.join("archive");
        Fixture {
            base,
            project,
            deps,
            root,
        }
    }

    #[test]
    fn rejects_non_node_modules_and_relative_paths() {
        let f = setup("reject");
        let e = plan(&f.project, &f.root, 1).unwrap_err();
        assert!(e.contains("node_modules"), "只允许依赖目录：{e}");
        let e = plan(Path::new("relative/node_modules"), &f.root, 1).unwrap_err();
        assert!(e.contains("绝对路径"), "{e}");
        let missing = f.project.join("nope/node_modules");
        assert!(plan(&missing, &f.root, 1).is_err());
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn archive_roundtrip_restores_same_bytes() {
        let f = setup("roundtrip");
        let plan = plan(&f.deps, &f.root, 1_700_000_000).unwrap();
        assert_eq!(plan.files, 2, "两个文件（不含目录）");
        assert_eq!(
            plan.bytes, 16,
            "console.log(1) 14 字节 + package.json 的 {{}} 2 字节"
        );
        assert!(plan.id.starts_with("1700000000-my-app-"));

        let entry = create_with(&f.deps, &f.root, 1_700_000_000, &test_delete).unwrap();
        assert!(!f.deps.exists(), "原件应已删除");
        assert!(f.project.join("package.json").exists(), "用户代码不许动");
        assert!(Path::new(&entry.archive_path).is_file());
        assert_eq!(list(&f.root).len(), 1);

        let prev = restore_preview(&entry);
        assert!(!prev.conflict);

        let rep = restore_with(&entry, false, &test_delete).unwrap();
        assert_eq!(rep.files_restored, 2);
        assert_eq!(rep.bytes_restored, 16);
        assert_eq!(
            fs::read(f.deps.join("some-pkg/dist/index.js")).unwrap(),
            b"console.log(1)"
        );
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn restore_refuses_conflict_unless_forced() {
        let f = setup("conflict");
        let entry = create_with(&f.deps, &f.root, 1_700_000_001, &test_delete).unwrap();
        fs::create_dir_all(&f.deps).unwrap(); // 用户重建了一个 node_modules
        fs::write(f.deps.join("user-owned.txt"), b"keep").unwrap();

        let prev = restore_preview(&entry);
        assert!(prev.conflict);
        let e = restore_with(&entry, false, &test_delete).unwrap_err();
        assert!(e.contains("已存在"), "{e}");
        assert!(f.deps.join("user-owned.txt").exists(), "拒绝时不得动目标");

        restore_with(&entry, true, &test_delete).unwrap();
        assert!(
            !f.deps.join("user-owned.txt").exists(),
            "确认覆盖后旧内容被替换"
        );
        assert!(restore_preview(&entry).conflict, "恢复后目标已存在");
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn restore_verifies_and_rejects_tampered_manifest() {
        let f = setup("tamper");
        let mut entry = create_with(&f.deps, &f.root, 1_700_000_002, &test_delete).unwrap();
        entry.bytes += 1; // 账本被改：校验必须发现
        let e = restore_with(&entry, false, &test_delete).unwrap_err();
        assert!(e.contains("校验失败"), "{e}");
        assert!(!f.deps.exists(), "解压产物应被移走（测试里直接删）");
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn restore_requires_archive_file() {
        let f = setup("missing");
        let entry = create_with(&f.deps, &f.root, 1_700_000_003, &test_delete).unwrap();
        fs::remove_file(&entry.archive_path).unwrap();
        let e = restore_with(&entry, false, &test_delete).unwrap_err();
        assert!(e.contains("归档文件不存在"), "{e}");
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn delete_removes_entry_and_supports_multi_archive_listing() {
        let f = setup("delete");
        let a = create_with(&f.deps, &f.root, 1_700_000_004, &test_delete).unwrap();
        // 第二个项目
        let deps2 = f.base.join("code/other/node_modules");
        fs::create_dir_all(&deps2).unwrap();
        fs::write(deps2.join("x.js"), b"x").unwrap();
        let b = create_with(&deps2, &f.root, 1_700_000_005, &test_delete).unwrap();

        let all = list(&f.root);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, b.id, "按归档时间倒序");
        assert!(find(&f.root, &a.id).is_some());

        delete_with(&a, &f.root, &test_delete).unwrap();
        assert_eq!(list(&f.root).len(), 1);
        assert!(!Path::new(&a.archive_path).exists());
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn create_refuses_id_collision() {
        let f = setup("collision");
        create_with(&f.deps, &f.root, 1_700_000_006, &test_delete).unwrap();
        fs::create_dir_all(&f.deps).unwrap(); // 重建依赖目录再归档（同秒同路径 → 同 id）
        fs::write(f.deps.join("a.js"), b"a").unwrap();
        let e = create_with(&f.deps, &f.root, 1_700_000_006, &test_delete).unwrap_err();
        assert!(e.contains("已存在"), "撞名必须拒绝而不是覆盖：{e}");
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn manifest_corruption_is_empty_not_crash() {
        let f = setup("corrupt");
        fs::create_dir_all(&f.root).unwrap();
        fs::write(f.root.join(MANIFEST_FILE), b"{not json").unwrap();
        assert!(list(&f.root).is_empty());
        assert!(find(&f.root, "x").is_none());
        let _ = fs::remove_dir_all(&f.base);
    }
}
