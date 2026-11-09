//! 应用图标提取（D1）：`X.app/Contents/Resources/*.icns` → PNG。
//!
//! 三条纪律：
//! 1. **只读**：不执行 `.app` 内任何二进制，只调用系统 `sips` 转码。
//! 2. **失败即空**：拿不到就返回 `Err`，前端落字母徽标——绝不返回占位灰图。
//! 3. **有缓存**：同一 `exe` 同尺寸只转一次，落自有缓存目录（不写系统目录）。
//!
//! 非 macOS 没有 `.icns` 概念，`locate_icns` 直接返回 `None`；
//! Windows 首版由前端统一走字母徽标（见方案 v1.1 决策 3）。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::appdisk::bundle_of_exe;

/// 卡片用 28、详情页 Hero 用 64；统一转更大的 64，前端按 CSS 缩放（避免两套缓存）。
pub const ICON_SIZE: u32 = 64;
/// 图标缓存有效期：应用升级会换 icns，30 min 足够新鲜
pub const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

/// FNV-1a 64 位：稳定、无依赖，用作缓存文件名
pub fn fnv1a(s: &str) -> u64 {
    // 64 位 FNV 的参数：offset basis 0xcbf29ce484222325、prime 0x100000001b3
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 在 `.app` 里找 icns：优先与 app 同名的（`Cursor.app` → `Cursor.icns`），
/// 否则取第一个 `.icns`（字典序，保证多次调用结果一致）。
pub fn locate_icns(app: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let res = app.join("Contents").join("Resources");
        let stem = app.file_stem()?.to_string_lossy().to_string();
        let preferred = res.join(format!("{stem}.icns"));
        if preferred.is_file() {
            return Some(preferred);
        }

        let mut found: Option<PathBuf> = None;
        let entries = std::fs::read_dir(&res).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e.eq_ignore_ascii_case("icns")) != Some(true) {
                continue;
            }
            // 字典序取最小，避免 read_dir 顺序不稳定导致图标抖动
            if found.as_ref().map(|f| path < *f).unwrap_or(true) {
                found = Some(path);
            }
        }
        found
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        None
    }
}

/// 用系统 `sips` 把 icns 转成指定尺寸的 PNG。
///
/// `sips` 是 macOS 自带工具（同项目里 `footprint` / `lsof` 的「系统命令优先」纪律），
/// 不引 icns 解码 crate。
#[cfg(target_os = "macos")]
pub fn convert(icns: &Path, size: u32, out: &Path) -> Result<(), String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败：{e}"))?;
    }
    let output = std::process::Command::new("sips")
        .args(["-Z", &size.to_string(), "-s", "format", "png"])
        .arg(icns)
        .arg("--out")
        .arg(out)
        .output()
        .map_err(|e| format!("sips 启动失败：{e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("sips 退出码 {:?}：{stderr}", output.status.code()));
    }
    if !out.is_file() {
        return Err("sips 未生成输出文件".into());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn convert(_icns: &Path, _size: u32, _out: &Path) -> Result<(), String> {
    Err("当前平台不支持 icns 转码".into())
}

/// 取（或生成）某进程 exe 对应的 PNG 图标，返回缓存文件路径。
///
/// 失败语义：`Err(原因)` 由命令层吞掉（该家族不进返回 map），前端落徽标。
pub fn icon_png(exe: &str, size: u32, cache_root: &Path) -> Result<PathBuf, String> {
    let app = bundle_of_exe(exe).ok_or("进程不在 .app 内（系统进程 / CLI）")?;
    let icns = locate_icns(&app).ok_or("该应用没有 .icns 资源")?;

    let name = format!("{:016x}-{size}.png", fnv1a(exe));
    let out = cache_root.join(name);

    if let Ok(meta) = std::fs::metadata(&out) {
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .map(|age| age < CACHE_TTL)
            .unwrap_or(false);
        if fresh {
            return Ok(out);
        }
    }

    convert(&icns, size, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_is_stable_and_distinct() {
        assert_eq!(fnv1a("Cursor"), fnv1a("Cursor"));
        assert_ne!(fnv1a("Cursor"), fnv1a("Chrome"));
        // 已知值（FNV-1a 64 的标准向量）
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn missing_app_returns_none() {
        assert!(locate_icns(Path::new("/definitely/not/an.app")).is_none());
    }

    #[test]
    fn non_bundle_exe_has_no_icon() {
        let err = icon_png("/usr/sbin/cupsd", ICON_SIZE, Path::new("/tmp")).unwrap_err();
        assert!(err.contains(".app"), "错误要说明原因：{err}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn extracts_system_app_icon_when_present() {
        // Safari 在所有 macOS 上都有；用真实系统应用验证 sips 链路
        let exe = "/Applications/Safari.app/Contents/MacOS/Safari";
        if bundle_of_exe(exe).map(|p| p.exists()) != Some(true) {
            return; // 精简系统 / CI 上没有 Safari 就跳过
        }
        let dir = std::env::temp_dir().join("prune-icon-test");
        let png = icon_png(exe, ICON_SIZE, &dir).expect("Safari 图标应可提取");
        let meta = std::fs::metadata(&png).expect("应生成 png");
        assert!(meta.len() > 100, "png 太小，可能不是真图标");
    }
}
