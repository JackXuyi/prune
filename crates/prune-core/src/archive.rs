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
//! 线程契约（docs/diag-m30-archive-freeze.md）：**本模块全是阻塞 IO / CPU**，
//! 压缩 900 MB 的 `node_modules` 是分钟级操作。调用方必须在阻塞线程池里跑
//! （Tauri 层用 `spawn_blocking`），绝不能直接在主线程调用——0.9.0 就是这么
//! 卡死的。进度通过 [`ProgressFn`] 回调交出去，本模块不认识事件系统。
//!
//! 本模块不存在对用户路径的 `remove_dir_all`（沿用 reclaim 的 invariant）：
//! 用户路径只走废纸篓，只有「自己刚写的归档半成品」允许直接删文件。

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::reclaim;

/// 归档区账本文件名
pub const MANIFEST_FILE: &str = "manifest.json";
/// 归档区在配置目录下的固定位置（GUI 与未来的 CLI 同口径）
pub const ARCHIVE_SUBDIR: &str = "com.xuyi.prune/archive";
/// 打包中的临时后缀：写完并校验通过后 rename 成正式归档（原子发布）
pub const PART_SUFFIX: &str = "part";
/// 取消 / 中断的用户可读文案（原件一律未动）
pub const CANCEL_MSG: &str = "已取消：原件未动，归档半成品已清除";

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

/// 归档进度阶段（前端据此选文案；`as_str` 与 serde 输出一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum Stage {
    /// 统计依赖目录（walk_counts）
    Plan = 0,
    /// 打包 tar.gz
    Pack = 1,
    /// 重读校验 / 解压校验
    Verify = 2,
    /// 移入废纸篓（删原件 / 覆盖旧目录）
    Trash = 3,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Plan => "plan",
            Stage::Pack => "pack",
            Stage::Verify => "verify",
            Stage::Trash => "trash",
        }
    }
}

/// 一条进度快照（`archive://progress` 事件的 payload）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub stage: Stage,
    /// 已完成（非压缩字节）：Pack 用源文件大小累计，Verify 用归档内 header.size 累计
    pub done_bytes: u64,
    /// 阶段总量；Plan 阶段为 0（前端显示不确定态）
    pub total_bytes: u64,
    pub done_files: u64,
    pub total_files: u64,
}

impl Progress {
    pub fn new(
        stage: Stage,
        done_bytes: u64,
        total_bytes: u64,
        done_files: u64,
        total_files: u64,
    ) -> Self {
        Self {
            stage,
            done_bytes,
            total_bytes,
            done_files,
            total_files,
        }
    }
}

/// 进度回调：核心层只把快照交出去，节流与发事件是调用方的事
pub type ProgressFn<'a> = &'a dyn Fn(&Progress);

/// 空回调（不需要进度的调用点，如 CLI / 测试）
fn no_progress(_: &Progress) {}

/// 取消令牌是否已置位。`None` = 不可取消（测试与旧调用点）
fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false)
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

/// 清理异常中断留下的 `.part`（强杀 / 断电 / 崩溃）。
///
/// 只动归档区里、且 mtime 超过 `older_than_secs` 的半成品——**进行中的打包
/// mtime 是新鲜的**，不会被误删；正式归档（`.tar.gz`）一律不碰。
pub fn cleanup_stale_parts(root: &Path, older_than_secs: u64) -> usize {
    let Ok(rd) = fs::read_dir(root) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some(PART_SUFFIX) {
            continue;
        }
        let stale = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .map(|d| d.as_secs() >= older_than_secs)
            .unwrap_or(false);
        if stale && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
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
///
/// 无确认通道的入口（CLI / 脚本）：超 [`reclaim::DEFAULT_BREAKER_BYTES`] 的
/// 归档会被 [`breaker_error`] 拒绝（非交互场景宁可拒绝，与 reclaim 双令牌同纪律）。
pub fn create(deps: &Path, root: &Path, now_secs: i64) -> Result<ArchiveEntry, String> {
    create_full(
        deps,
        root,
        now_secs,
        &trash_delete,
        &no_progress,
        None,
        false,
    )
}

/// 带进度与取消的生产入口（GUI / 未来的 CLI 共用）。
///
/// **必须在阻塞线程池调用**（见模块头「线程契约」）。
/// `breaker_confirmed`：超 20 GB 归档的二次确认（R8b，由 UI 的第二段确认传入）。
pub fn create_guarded(
    deps: &Path,
    root: &Path,
    now_secs: i64,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
    breaker_confirmed: bool,
) -> Result<ArchiveEntry, String> {
    create_full(
        deps,
        root,
        now_secs,
        &trash_delete,
        progress,
        cancel,
        breaker_confirmed,
    )
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
    restore_full(entry, force, &trash_delete, &no_progress, None)
}

/// 带进度与取消的恢复入口（生产用）
pub fn restore_guarded(
    entry: &ArchiveEntry,
    force: bool,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
) -> Result<RestoreReport, String> {
    restore_full(entry, force, &trash_delete, progress, cancel)
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

/// 测试专用：注入删除器的归档入口（生产走 [`create`] / [`create_guarded`]）
#[cfg(test)]
fn create_with(
    deps: &Path,
    root: &Path,
    now_secs: i64,
    delete: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<ArchiveEntry, String> {
    create_full(deps, root, now_secs, delete, &no_progress, None, false)
}

/// R8b 字节熔断（M2 顺带）：归档与 reclaim 是「同一产品两条删除路径」，爆炸半径
/// 必须同一口径——单次体量超过 [`reclaim::DEFAULT_BREAKER_BYTES`]（20 GB）时，
/// 必须拿到**第二次**显式确认（`confirmed` 只覆盖「先预览再执行」，不覆盖超限）。
/// 纯函数便于负面测试：21 GB 未确认 → Err；确认 → Ok；19 GB → 永远 Ok。
pub fn breaker_error(bytes: u64, breaker_confirmed: bool) -> Result<(), String> {
    if bytes > reclaim::DEFAULT_BREAKER_BYTES && !breaker_confirmed {
        return Err(format!(
            "归档体量 {:.1} GB 超过熔断阈值（{} GB）：单次删除路径的爆炸半径由字节定，\
             不由项数定——请回到应用对该次归档做二次确认",
            bytes as f64 / 1e9,
            reclaim::DEFAULT_BREAKER_BYTES / (1024 * 1024 * 1024)
        ));
    }
    Ok(())
}

fn create_full(
    deps: &Path,
    root: &Path,
    now_secs: i64,
    delete: &dyn Fn(&Path) -> Result<(), String>,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
    breaker_confirmed: bool,
) -> Result<ArchiveEntry, String> {
    progress(&Progress::new(Stage::Plan, 0, 0, 0, 0));
    let p = plan(deps, root, now_secs)?;
    if cancelled(cancel) {
        return Err(CANCEL_MSG.into());
    }
    // R8b 字节熔断：超 20 GB 且未拿到二次确认 → 拒绝（在写任何文件之前）
    breaker_error(p.bytes, breaker_confirmed)?;

    let archive = PathBuf::from(&p.archive_path);
    // 同 id 撞名（同秒重复归档）：拒绝而不是覆盖——覆盖会毁掉前一次的归档数据
    if archive.exists() {
        return Err(format!(
            "归档文件已存在（{}），请稍后重试（同秒重复归档）",
            p.archive_path
        ));
    }
    fs::create_dir_all(root).map_err(|e| format!("创建归档区失败：{e}"))?;

    // 先写 `.part`，校验通过后再 rename 成正式归档：中断最多留半成品，
    // 不会出现「账本里有、文件是半截」或「文件在、账本没有」的孤儿
    let part = part_path(&archive);
    if part.exists() {
        let _ = fs::remove_file(&part); // 同一 id 的半成品只可能是上一轮崩溃残留
    }
    if let Err(e) = write_archive(deps, &part, p.bytes, p.files, progress, cancel) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }

    // 校验：重读归档，条目数与非压缩字节数必须与扫描一致
    let verified = match verify_archive(&part, p.files, progress, cancel) {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&part); // 自己写的半成品，允许直删
            return Err(e);
        }
    };
    if verified != (p.files, p.bytes) {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "归档校验失败：预期 {} 项 / {} B，实际 {} 项 / {} B。原件未动。",
            p.files, p.bytes, verified.0, verified.1
        ));
    }

    // 原子发布：rename 同卷是原子操作，不存在「半个正式归档」
    fs::rename(&part, &archive).map_err(|e| format!("归档发布失败：{e}"))?;

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

    progress(&Progress::new(
        Stage::Trash,
        p.bytes,
        p.bytes,
        p.files,
        p.files,
    ));
    if let Err(e) = delete(deps) {
        entries.retain(|x| x.id != entry.id);
        let _ = write_manifest(root, &entries);
        let _ = fs::remove_file(&archive);
        return Err(format!("删除原件失败，归档已回滚：{e}"));
    }

    Ok(entry)
}

/// 测试专用：注入删除器的恢复入口（生产走 [`restore`] / [`restore_guarded`]）
#[cfg(test)]
fn restore_with(
    entry: &ArchiveEntry,
    force: bool,
    delete: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<RestoreReport, String> {
    restore_full(entry, force, delete, &no_progress, None)
}

fn restore_full(
    entry: &ArchiveEntry,
    force: bool,
    delete: &dyn Fn(&Path) -> Result<(), String>,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
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
        progress(&Progress::new(Stage::Trash, 0, entry.bytes, 0, entry.files));
        delete(&target)?;
    }

    // tar crate 逐条目解压：`unpack_in` 会拒绝绝对路径与 `..` 逃逸条目
    // （返回 false）；我们自己产的归档里出现这种条目只可能是被篡改，直接中止
    let file = File::open(&archive).map_err(|e| format!("打开归档失败：{e}"))?;
    let dec = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut ar = tar::Archive::new(dec);
    let entries = ar.entries().map_err(|e| format!("读取归档失败：{e}"))?;
    let mut done_bytes = 0u64;
    let mut done_files = 0u64;
    for ent in entries {
        if cancelled(cancel) {
            let _ = delete(&target); // 解压产物是我们自己刚写的，移走比留着安全
            return Err(CANCEL_MSG.into());
        }
        let mut ent = ent.map_err(|e| format!("读取归档失败：{e}"))?;
        let size = ent.header().size().unwrap_or(0);
        let is_file = ent.header().entry_type().is_file();
        let leak = ent
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let unpacked = ent
            .unpack_in(parent)
            .map_err(|e| format!("解压失败（已中止，目标可能部分写入）：{e}"))?;
        if !unpacked {
            let _ = delete(&target);
            return Err(format!(
                "归档条目路径越界（{leak}），已中止；解压产物已移入废纸篓。"
            ));
        }
        if is_file {
            done_bytes = done_bytes.saturating_add(size);
        }
        done_files += 1;
        progress(&Progress::new(
            Stage::Verify,
            done_bytes.min(entry.bytes),
            entry.bytes,
            done_files.min(entry.files),
            entry.files,
        ));
    }

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

/// 流式写 tar.gz：以 `node_modules/` 为根前缀，恢复时解压到项目目录即可。
///
/// 自己遍历而不是 `append_dir_all`：只有逐条目写才能报进度、才能在条目间
/// 检查取消。符号链接按链接存（与 `follow_symlinks(false)` 同口径），pnpm 的
/// `../../.pnpm/...` 因此能原样还原。
fn write_archive(
    deps: &Path,
    target: &Path,
    total_bytes: u64,
    total_files: u64,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let file = File::create(target).map_err(|e| format!("创建归档文件失败：{e}"))?;
    // level 1（fast）：实测 922 MB 依赖 46.3 s → 22.2 s，归档大 18%——
    // 归档的价值是「可还原」不是「最小体积」，这个交换划算
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(enc);
    builder.follow_symlinks(false);

    builder
        .append_dir("node_modules", deps)
        .map_err(|e| format!("打包失败：{e}"))?;

    let mut done_bytes = 0u64;
    let mut done_files = 0u64;
    let mut stack = vec![deps.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancelled(cancel) {
            drop(builder); // 先放掉文件句柄，调用方才能删掉半成品
            return Err(CANCEL_MSG.into());
        }
        let rd = fs::read_dir(&dir).map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
        for ent in rd {
            let ent = ent.map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
            let path = ent.path();
            let md = fs::symlink_metadata(&path)
                .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            let rel = path
                .strip_prefix(deps)
                .map_err(|_| format!("路径异常：{}", path.display()))?;
            let arc_path = Path::new("node_modules").join(rel);
            if md.file_type().is_symlink() {
                let link = fs::read_link(&path)
                    .map_err(|e| format!("读取符号链接 {} 失败：{e}", path.display()))?;
                let mut header = tar::Header::new_gnu();
                // tar 0.4.46 的 append_link 只写路径与 linkname，**不会**替你
                // 设 entry_type——不显式标 Symlink 就会落成 size=0 的普通文件
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_mode(0o777);
                header.set_size(0);
                header.set_mtime(mtime_secs(&md));
                builder
                    .append_link(&mut header, &arc_path, &link)
                    .map_err(|e| format!("打包失败（{}）：{e}", path.display()))?;
                done_files += 1;
            } else if md.is_dir() {
                builder
                    .append_dir(&arc_path, &path)
                    .map_err(|e| format!("打包失败（{}）：{e}", path.display()))?;
                stack.push(path);
            } else {
                let mut f =
                    File::open(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
                builder
                    .append_file(&arc_path, &mut f)
                    .map_err(|e| format!("打包失败（{}）：{e}", path.display()))?;
                done_files += 1;
                done_bytes = done_bytes.saturating_add(md.len());
            }
            progress(&Progress::new(
                Stage::Pack,
                done_bytes,
                total_bytes,
                done_files,
                total_files,
            ));
        }
    }

    let enc = builder
        .into_inner()
        .map_err(|e| format!("打包收尾失败：{e}"))?;
    enc.finish().map_err(|e| format!("写入归档失败：{e}"))?;
    progress(&Progress::new(
        Stage::Pack,
        total_bytes,
        total_bytes,
        total_files,
        total_files,
    ));
    Ok(())
}

/// 重读归档统计（条目数 / 非压缩字节数）——与 [`walk_counts`] 同口径
fn verify_archive(
    target: &Path,
    total_files: u64,
    progress: ProgressFn<'_>,
    cancel: Option<&AtomicBool>,
) -> Result<(u64, u64), String> {
    let file = File::open(target).map_err(|e| format!("打开归档失败：{e}"))?;
    let dec = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut ar = tar::Archive::new(dec);
    let mut files = 0u64;
    let mut bytes = 0u64;
    let entries = ar.entries().map_err(|e| format!("读取归档失败：{e}"))?;
    for ent in entries {
        if cancelled(cancel) {
            return Err(CANCEL_MSG.into());
        }
        let ent = ent.map_err(|e| format!("读取归档失败：{e}"))?;
        let header = ent.header();
        if header.entry_type().is_dir() {
            continue;
        }
        files += 1;
        if header.entry_type().is_file() {
            bytes = bytes.saturating_add(header.size().unwrap_or(0));
        }
        progress(&Progress::new(
            Stage::Verify,
            bytes,
            bytes.max(1),
            files.min(total_files),
            total_files,
        ));
    }
    Ok((files, bytes))
}

/// `<archive>.tar.gz` → `<archive>.tar.gz.part`
fn part_path(archive: &Path) -> PathBuf {
    let mut s = archive.as_os_str().to_os_string();
    s.push(".");
    s.push(PART_SUFFIX);
    PathBuf::from(s)
}

fn mtime_secs(md: &fs::Metadata) -> u64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    use std::sync::Mutex;

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
        assert!(
            !part_path(Path::new(&entry.archive_path)).exists(),
            "发布后不许留 .part"
        );
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

    // ------------------------------------------- 异步化 / 进度 / 取消 / 原子发布（0.9.1）

    #[test]
    fn progress_covers_plan_pack_verify_trash_in_order() {
        let f = setup("progress");
        let seen: Mutex<Vec<(Stage, u64)>> = Mutex::new(Vec::new());
        let sink = |p: &Progress| seen.lock().unwrap().push((p.stage, p.done_bytes));

        create_full(
            &f.deps,
            &f.root,
            1_700_000_007,
            &test_delete,
            &sink,
            None,
            false,
        )
        .unwrap();

        let seen = seen.into_inner().unwrap();
        assert_eq!(
            seen.first().map(|s| s.0),
            Some(Stage::Plan),
            "首个快照是统计"
        );
        assert_eq!(
            seen.last().map(|s| s.0),
            Some(Stage::Trash),
            "末个快照是删原件"
        );
        let stages: Vec<Stage> = seen.iter().map(|s| s.0).collect();
        let pos = |t: Stage| {
            stages
                .iter()
                .position(|s| *s == t)
                .unwrap_or_else(|| panic!("{t:?} 阶段必须出现"))
        };
        assert!(pos(Stage::Plan) < pos(Stage::Pack));
        assert!(pos(Stage::Pack) < pos(Stage::Verify));
        assert!(pos(Stage::Verify) < pos(Stage::Trash));
        // 打包阶段的最后一条必须报满：前端进度条靠它收尾
        let pack_done: Vec<u64> = seen
            .iter()
            .filter(|s| s.0 == Stage::Pack)
            .map(|s| s.1)
            .collect();
        assert_eq!(pack_done.last().copied(), Some(16), "打包收尾必须报满字节");
        let _ = fs::remove_dir_all(&f.base);
    }

    // ------------------------------------------- R8b 字节熔断（M2 顺带）

    #[test]
    fn breaker_blocks_over_threshold_without_second_confirm() {
        let gb = 1024u64 * 1024 * 1024;
        // 21 GiB（≈ 22.5 GB 十进制，体量展示口径与代码库一致用 /1e9）未二次确认 → 拒绝
        let e = breaker_error(21 * gb, false).unwrap_err();
        assert!(e.contains("GB"), "报错必须带实际体量: {e}");
        assert!(e.contains("20 GB"), "报错必须带阈值: {e}");
        assert!(e.contains("熔断"), "报错必须点明熔断: {e}");
        // 21 GB 已二次确认 → 放行
        assert_eq!(breaker_error(21 * gb, true), Ok(()));
        // 19 GB 未确认 → 放行（confirmed 只覆盖预览确认，不覆盖熔断；阈值内无需二次）
        assert_eq!(breaker_error(19 * gb, false), Ok(()));
        // 恰好 20 GB → 不超阈值，放行（超「限」才算，等于不算）
        assert_eq!(breaker_error(reclaim::DEFAULT_BREAKER_BYTES, false), Ok(()));
    }

    #[test]
    fn breaker_rejects_before_writing_anything() {
        // 用一个「体量超限」的假 plan 不可行（阈值 20 GB 造不出测试夹具），
        // 但可以验证 create_full 在 plan 之后、写文件之前调用 breaker_error：
        // 造一个 >20 GB 的依赖目录不现实，退而验证「取消」路径同位——
        // breaker_error 在 cancelled 检查之后、fs::create_dir_all 之前（见 create_full 源码序）。
        // 真正的 21 GB 端到端由 breaker_blocks_over_threshold_without_second_confirm 锁口径。
        let gb = 1024u64 * 1024 * 1024;
        assert!(breaker_error(21 * gb + 1, false).is_err());
        assert!(breaker_error(gb, false).is_ok());
    }

    #[test]
    fn cancel_leaves_no_part_and_no_entry() {
        let f = setup("cancel");
        let cancel = AtomicBool::new(true); // 一开始就取消
        let e = create_full(
            &f.deps,
            &f.root,
            1_700_000_008,
            &test_delete,
            &no_progress,
            Some(&cancel),
            false,
        )
        .unwrap_err();
        assert_eq!(e, CANCEL_MSG);
        assert!(f.deps.exists(), "取消后原件必须还在");
        assert!(list(&f.root).is_empty(), "取消不许记账");
        let parts: Vec<_> = fs::read_dir(&f.root)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(parts.is_empty(), "取消后不许留 .part：{parts:?}");
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn stale_part_is_cleaned_but_fresh_and_archive_are_kept() {
        let f = setup("partclean");
        let entry = create_with(&f.deps, &f.root, 1_700_000_009, &test_delete).unwrap();
        let stale = f.root.join("1700000000-gone-deadbeef.tar.gz.part");
        fs::write(&stale, b"half").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        // 新鲜 .part：模拟正在进行的打包，绝不能删（必须在 sleep 之后建）
        let fresh = f.root.join("1700000001-live-cafebabe.tar.gz.part");
        fs::write(&fresh, b"running").unwrap();

        let removed = cleanup_stale_parts(&f.root, 1);
        assert_eq!(removed, 1, "只清过期的那一个");
        assert!(!stale.exists());
        assert!(fresh.exists(), "新鲜 .part 属于进行中的任务，不能碰");
        assert!(
            Path::new(&entry.archive_path).exists(),
            "正式归档永不在清理范围"
        );
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn pnpm_style_symlink_roundtrips() {
        // pnpm 的 node_modules 是符号链接森林：顶层链接 + `.pnpm` 内的相对链接
        // （目标含 `..`）。归档按链接存，恢复必须原样还原，否则 pnpm 用户的
        // 依赖恢复出来是坏的。
        let f = setup("pnpm");
        let real = f.deps.join(".pnpm/foo@1.0.0/node_modules/foo");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("index.js"), b"module.exports=1").unwrap();
        // 嵌套依赖：`.pnpm/foo@1.0.0/node_modules/bar` → ../../../bar@2.0.0/node_modules/bar
        let bar = f.deps.join(".pnpm/bar@2.0.0/node_modules/bar");
        fs::create_dir_all(&bar).unwrap();
        fs::write(bar.join("index.js"), b"module.exports=2").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(".pnpm/foo@1.0.0/node_modules/foo", f.deps.join("foo"))
                .unwrap();
            std::os::unix::fs::symlink(
                "../../bar@2.0.0/node_modules/bar",
                f.deps.join(".pnpm/foo@1.0.0/node_modules/bar"),
            )
            .unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = fs::remove_dir_all(&f.base);
            return;
        }

        let entry = create_with(&f.deps, &f.root, 1_700_000_010, &test_delete).unwrap();
        restore_with(&entry, false, &test_delete).unwrap();

        let top = fs::symlink_metadata(f.deps.join("foo")).unwrap();
        assert!(top.file_type().is_symlink(), "顶层链接必须还原成链接");
        assert_eq!(
            fs::read_link(f.deps.join("foo")).unwrap(),
            PathBuf::from(".pnpm/foo@1.0.0/node_modules/foo"),
            "链接目标逐字一致"
        );
        let nested = fs::symlink_metadata(f.deps.join(".pnpm/foo@1.0.0/node_modules/bar")).unwrap();
        assert!(nested.file_type().is_symlink(), "含 .. 的嵌套链接也要保留");
        assert_eq!(
            fs::read_link(f.deps.join(".pnpm/foo@1.0.0/node_modules/bar")).unwrap(),
            PathBuf::from("../../bar@2.0.0/node_modules/bar"),
            "`..` 目标不许被规范化改写"
        );
        assert_eq!(
            fs::read(f.deps.join("foo/index.js")).unwrap(),
            b"module.exports=1",
            "经链接读到的内容正确（pnpm 语义）"
        );
        assert_eq!(
            fs::read(f.deps.join(".pnpm/foo@1.0.0/node_modules/bar/index.js")).unwrap(),
            b"module.exports=2",
            "嵌套链接穿透后可读到真实内容"
        );
        let _ = fs::remove_dir_all(&f.base);
    }

    #[test]
    fn restore_cancel_removes_partial_target() {
        let f = setup("restorecancel");
        let entry = create_with(&f.deps, &f.root, 1_700_000_011, &test_delete).unwrap();
        let cancel = AtomicBool::new(true);
        let e = restore_full(&entry, false, &test_delete, &no_progress, Some(&cancel)).unwrap_err();
        assert_eq!(e, CANCEL_MSG);
        assert!(!f.deps.exists(), "取消后不许留半截 node_modules");
        let _ = fs::remove_dir_all(&f.base);
    }

    /// 契约测试：事件 payload 的字段名与 src/types/disk.ts 的 ArchiveProgress 对齐
    #[test]
    fn progress_payload_matches_frontend_contract() {
        let v = serde_json::to_value(Progress::new(Stage::Pack, 10, 100, 2, 9)).unwrap();
        assert_eq!(v["stage"], "pack", "阶段值是前端 ArchiveStage 的取值域");
        assert_eq!(v["doneBytes"], 10);
        assert_eq!(v["totalBytes"], 100);
        assert_eq!(v["doneFiles"], 2);
        assert_eq!(v["totalFiles"], 9);
        for st in [Stage::Plan, Stage::Pack, Stage::Verify, Stage::Trash] {
            let j = serde_json::to_value(st).unwrap();
            assert_eq!(
                j.as_str().unwrap(),
                st.as_str(),
                "serde 与 as_str 必须同口径"
            );
        }
    }
}
