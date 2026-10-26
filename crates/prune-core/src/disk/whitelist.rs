//! 可回收目录白名单（M1/T-302）。
//!
//! **这份表就是安全护栏的第一层**：`reclaim` 只接受落在这些根之内的路径，
//! 任何不在表内的路径都会被 `ensure_in_whitelist` 拒绝。新增清理能力必须先在此登记。
//!
//! 来源：`docs/research/reclaim-scan.sh`（2026-10-22 实测清单）。
//! 级别语义见 `docs/prune-feature-spec-v2.md` §7.1。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 安全分级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// 🟢 本机永远加载不了 / 包管理器缓存，一键清理
    L1,
    /// 🟢 其余可再生缓存，一键清理
    L2,
    /// 🟡 项目依赖 / 模拟器，需逐个确认
    L3,
    /// 🔴 只输出命令，永不执行
    L4,
}

impl Level {
    /// 是否进入默认清理通道（L1/L2 直接可删，L3 需确认，L4 永不删）
    pub fn default_cleanable(self) -> bool {
        matches!(self, Level::L1 | Level::L2)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::L1 => "l1",
            Level::L2 => "l2",
            Level::L3 => "l3",
            Level::L4 => "l4",
        }
    }
}

/// 一条白名单登记项
#[derive(Debug, Clone, Copy)]
pub struct WhitelistEntry {
    /// 稳定标识（IPC / 前端用于指定清理对象）
    pub key: &'static str,
    /// 展示名
    pub label: &'static str,
    /// 相对 home 的路径（测试时可替换 home，不写死绝对路径）
    pub rel: &'static str,
    pub level: Level,
    /// 是否为 A1 条目级判定对象（只有 Yarn v6 具备「目录名即平台元数据」的性质）
    pub a1_scope: bool,
}

/// 全部登记项（顺序即 UI 展示顺序）
pub const ENTRIES: &[WhitelistEntry] = &[
    // ---------- JS 生态 ----------
    WhitelistEntry {
        key: "yarn",
        label: "Yarn 缓存",
        rel: "Library/Caches/Yarn/v6",
        level: Level::L2,
        a1_scope: true,
    },
    WhitelistEntry {
        key: "npm",
        label: "npm 缓存",
        rel: ".npm/_cacache",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "pnpm",
        label: "pnpm store",
        rel: "Library/pnpm/store",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "pnpm-legacy",
        label: "pnpm store（旧路径）",
        rel: ".local/share/pnpm/store",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "bun",
        label: "Bun 安装缓存",
        rel: ".bun/install/cache",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "playwright",
        label: "Playwright 浏览器",
        rel: "Library/Caches/ms-playwright",
        level: Level::L3,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "cypress",
        label: "Cypress 二进制",
        rel: "Library/Caches/Cypress",
        level: Level::L3,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "electron",
        label: "Electron 下载缓存",
        rel: "Library/Caches/electron",
        level: Level::L2,
        a1_scope: false,
    },
    // ---------- 后端 / 其它包管理器 ----------
    WhitelistEntry {
        key: "cargo",
        label: "Cargo registry",
        rel: ".cargo/registry",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "homebrew",
        label: "Homebrew 下载缓存",
        rel: "Library/Caches/Homebrew",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "cocoapods",
        label: "CocoaPods 缓存",
        rel: "Library/Caches/CocoaPods",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "composer",
        label: "Composer 缓存",
        rel: "Library/Caches/composer",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "gradle",
        label: "Gradle 缓存",
        rel: ".gradle/caches",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "maven",
        label: "Maven 本地仓库",
        rel: ".m2/repository",
        level: Level::L3,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "gomod",
        label: "Go module 缓存",
        rel: "go/pkg/mod",
        level: Level::L2,
        a1_scope: false,
    },
    // ---------- Xcode ----------
    WhitelistEntry {
        key: "deriveddata",
        label: "DerivedData（编译产物）",
        rel: "Library/Developer/Xcode/DerivedData",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "xcode-modulecache",
        label: "ModuleCache（模块缓存）",
        rel: "Library/Developer/Xcode/ModuleCache.noindex",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "xcode-previews",
        label: "SwiftUI Previews 缓存",
        rel: "Library/Developer/Xcode/UserData/Previews",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "xcode-cache",
        label: "Xcode 总缓存",
        rel: "Library/Caches/com.apple.dt.Xcode",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "simulator-caches",
        label: "Simulator 运行时缓存",
        rel: "Library/Developer/CoreSimulator/Caches",
        level: Level::L2,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "simulator",
        label: "Simulator 设备",
        rel: "Library/Developer/CoreSimulator/Devices",
        level: Level::L3,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "xcode-devicesupport",
        label: "iOS DeviceSupport",
        rel: "Library/Developer/Xcode/iOS DeviceSupport",
        level: Level::L3,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "xcode-archives",
        label: "Archives（归档包）",
        rel: "Library/Developer/Xcode/Archives",
        level: Level::L3,
        a1_scope: false,
    },
    // ---------- 容器（L4：只读报告，永不执行） ----------
    WhitelistEntry {
        key: "docker-vm",
        label: "Docker VM 磁盘",
        rel: "Library/Containers/com.docker.docker/Data/vms",
        level: Level::L4,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "orbstack",
        label: "OrbStack 数据",
        rel: "Library/Group Containers/HUAQ24HBR6.dev.orbstack",
        level: Level::L4,
        a1_scope: false,
    },
    WhitelistEntry {
        key: "colima",
        label: "Colima 数据",
        rel: ".colima",
        level: Level::L4,
        a1_scope: false,
    },
];

/// 已存在的登记项（不存在的不进报告，避免 UI 出现一堆 0）
pub fn resolve(home: &Path) -> Vec<(WhitelistEntry, PathBuf)> {
    ENTRIES
        .iter()
        .filter_map(|e| {
            let p = home.join(e.rel);
            if p.is_dir() {
                Some((*e, p))
            } else {
                None
            }
        })
        .collect()
}

/// 按 key 取登记项
pub fn entry(key: &str) -> Option<&'static WhitelistEntry> {
    ENTRIES.iter().find(|e| e.key == key)
}

/// 路径是否落在白名单内（护栏 1）。
///
/// 规则：`canonicalize` 之后必须**等于**某个白名单根，或位于其下。
/// canonicalize 会消掉 `..` 与符号链接，因此 symlink 指向 home 外部也会被拒。
pub fn is_whitelisted(path: &Path, home: &Path) -> bool {
    let Ok(target) = path.canonicalize() else {
        return false; // 不存在的路径不放行（避免 TOCTOU 后门）
    };
    ENTRIES.iter().any(|e| {
        let Ok(root) = home.join(e.rel).canonicalize() else {
            return false;
        };
        target == root || target.starts_with(&root)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_map_to_cleanable_channels() {
        assert!(Level::L1.default_cleanable());
        assert!(Level::L2.default_cleanable());
        assert!(!Level::L3.default_cleanable());
        assert!(!Level::L4.default_cleanable());
    }

    #[test]
    fn whitelist_rejects_traversal_and_symlink_escape() {
        let home = std::env::temp_dir().join(format!("prune-wl-{}", std::process::id()));
        let cache = home.join("Library/Caches/Yarn/v6");
        let outside = home.join("Documents/important");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside = cache.join("npm-@next-swc-linux-x64-1.0.0-x");
        std::fs::create_dir_all(&inside).unwrap();

        assert!(is_whitelisted(&inside, &home), "白名单内应放行");
        assert!(is_whitelisted(&cache, &home), "白名单根本身应放行");
        assert!(!is_whitelisted(&outside, &home), "用户文档目录必须拒绝");

        // ../ 穿越：规范化后落在 Documents，必须拒绝
        let traversal = cache.join("../../../../Documents/important");
        assert!(!is_whitelisted(&traversal, &home));

        // symlink 指向白名单外
        let link = cache.join("escape");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(
                !is_whitelisted(&link, &home),
                "符号链接逃逸必须被 canonicalize 拦下"
            );
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn resolve_skips_missing_paths() {
        let home = std::env::temp_dir().join(format!("prune-wl-miss-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".npm/_cacache")).unwrap();
        let got = resolve(&home);
        assert_eq!(got.len(), 1, "只应返回真实存在的登记项");
        assert_eq!(got[0].0.key, "npm");
        let _ = std::fs::remove_dir_all(&home);
    }
}
