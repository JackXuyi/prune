//! 平台路径口径（W1）：user home / 配置目录的唯一出处。
//!
//! 之前各处直接读 `$HOME`，在 Windows 上会踩两个坑：
//! 1. `HOME` 可能被 Git for Windows / MSYS 改写成 `C:/Users/x` 或干脆不存在；
//! 2. Windows 的用户主目录变量是 `USERPROFILE`。
//!
//! 因此所有需要「主目录」的地方都应走这里，而不是自己去读环境变量。

use std::path::{Path, PathBuf};

/// 用户主目录。
///
/// - Windows：`%USERPROFILE%`（`HOME` 只作兜底，避免被 MSYS 改写后指错）
/// - macOS / Linux：`$HOME`
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// 主目录，取不到时退化为平台根（macOS/Linux `/`，Windows `C:\`）。
///
/// 用于调用方无法接受 `None` 的场景；退化后扫描/归属都会自然变成空结果，
/// 不会因为路径为空而误判。
pub fn home_dir_or_root() -> PathBuf {
    home_dir().unwrap_or_else(|| {
        #[cfg(windows)]
        {
            PathBuf::from("C:\\")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/")
        }
    })
}

/// 给定主目录下的「应用配置目录」（编辑器 workspaceStorage 等）。
///
/// 带上 `home` 参数而不是直接用系统变量，是为了测试可以替换 home。
/// - macOS：`~/Library/Application Support`
/// - Windows：`%APPDATA%` = `~/AppData/Roaming`
/// - 其它 unix：`~/.config`
pub fn config_dir_in(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        home.join("AppData").join("Roaming")
    }
    #[cfg(target_os = "macos")]
    {
        home.join("Library").join("Application Support")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        home.join(".config")
    }
}

/// 与 [`config_dir_in`] 等价，但走系统环境（Windows 优先 `%APPDATA%`）。
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| config_dir_in(&h)))
    }
    #[cfg(not(windows))]
    {
        home_dir().map(|h| config_dir_in(&h))
    }
}

/// 系统配置目录（不随用户变化），用于「系统路径不出现在用户数据里」这类判断。
pub fn system_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut out = vec![PathBuf::from("C:\\Windows")];
        if let Some(pd) = std::env::var_os("ProgramData") {
            out.push(PathBuf::from(pd));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            out.push(PathBuf::from(pf));
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec![
            PathBuf::from("/System"),
            PathBuf::from("/usr"),
            PathBuf::from("/private"),
            PathBuf::from("/Library"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_is_absolute_when_env_present() {
        if let Some(home) = home_dir() {
            assert!(home.is_absolute(), "主目录必须是绝对路径：{}", home.display());
        }
    }

    #[test]
    fn home_dir_or_root_never_empty() {
        let p = home_dir_or_root();
        assert!(p.is_absolute());
    }

    #[test]
    fn config_dir_sits_under_home() {
        let home = std::env::temp_dir().join("prune-paths-home");
        let cfg = config_dir_in(&home);
        assert!(cfg.starts_with(&home), "配置目录必须在 home 之下：{}", cfg.display());
        assert_ne!(cfg, home, "配置目录不能等于 home 本身");
    }

    #[test]
    fn system_roots_are_absolute() {
        for r in system_roots() {
            assert!(r.is_absolute(), "系统根必须是绝对路径：{}", r.display());
        }
    }
}
