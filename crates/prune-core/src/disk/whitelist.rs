//! 可回收目录白名单（M1/T-302 → W1 平台化）。
//!
//! **这份表就是安全护栏的第一层**：`reclaim` 只接受落在这些根之内的路径，
//! 任何不在表内的路径都会被 `ensure_in_whitelist` 拒绝。新增清理能力必须先在此登记。
//!
//! 平台差异：
//! - 每条登记项带 `os` 标记，`resolve` / `entry` / `is_whitelisted` 只认
//!   「当前平台匹配」的条目 —— macOS 的 `Library/...` 规则不会出现在 Windows 报告里，
//!   反之亦然（否则 Windows 上会白扫一堆不存在的路径，且 UI 口径含混）。
//! - `rel` **一律相对 home**（测试可替换 home）。Windows 的可再生缓存都落在
//!   `%LOCALAPPDATA%`，也就是 `~/AppData/Local/...`，因此同样用 home 相对路径表达；
//!   仅当管理员改过 `LOCALAPPDATA` 时才需要额外解析，暂不覆盖（见 W1 说明）。
//!
//! 来源：`docs/research/reclaim-scan.sh`（macOS 实测清单）、
//! Windows 侧依据各包管理器默认缓存位置（npm / pnpm / Yarn / pip / Cypress / Playwright / Electron）。
//! AI 应用条目（2026-11）：本机实测 + DevCleaner / CacheCleaner / Mole 竞品字段指南对齐——
//! 三家均把 Cursor / Claude / Ollama 缓存列为 Safe 类，但**均按路径扫**；
//! 本表延续同一口径，并显式排除「可再生」与「用户状态」的模糊地带（见各条目注释）。
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

/// 条目适用的操作系统
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// 两端通用（多为 home 下与平台无关的包管理器目录）
    All,
    /// 仅 macOS：`Library/` 布局、Xcode、Homebrew、Apple 容器
    MacOnly,
    /// 仅 Windows：`AppData/` 布局
    WindowsOnly,
}

impl Os {
    /// 当前平台是否适用
    pub fn matches_here(self) -> bool {
        match self {
            Os::All => true,
            Os::MacOnly => cfg!(target_os = "macos"),
            Os::WindowsOnly => cfg!(target_os = "windows"),
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
    /// 适用平台（见 [`Os`]）
    pub os: Os,
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
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "npm",
        label: "npm 缓存",
        rel: ".npm/_cacache",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "pnpm",
        label: "pnpm store",
        rel: "Library/pnpm/store",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "pnpm-legacy",
        label: "pnpm store（旧路径）",
        rel: ".local/share/pnpm/store",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "bun",
        label: "Bun 安装缓存",
        rel: ".bun/install/cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "playwright",
        label: "Playwright 浏览器",
        rel: "Library/Caches/ms-playwright",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cypress",
        label: "Cypress 二进制",
        rel: "Library/Caches/Cypress",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "electron",
        label: "Electron 下载缓存",
        rel: "Library/Caches/electron",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // ---------- 后端 / 其它包管理器 ----------
    WhitelistEntry {
        key: "cargo",
        label: "Cargo registry",
        rel: ".cargo/registry",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "homebrew",
        label: "Homebrew 下载缓存",
        rel: "Library/Caches/Homebrew",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cocoapods",
        label: "CocoaPods 缓存",
        rel: "Library/Caches/CocoaPods",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "composer",
        label: "Composer 缓存",
        rel: "Library/Caches/composer",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "gradle",
        label: "Gradle 缓存",
        rel: ".gradle/caches",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "maven",
        label: "Maven 本地仓库",
        rel: ".m2/repository",
        level: Level::L3,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "gomod",
        label: "Go module 缓存",
        rel: "go/pkg/mod",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    // ---------- Xcode ----------
    WhitelistEntry {
        key: "deriveddata",
        label: "DerivedData（编译产物）",
        rel: "Library/Developer/Xcode/DerivedData",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "xcode-modulecache",
        label: "ModuleCache（模块缓存）",
        rel: "Library/Developer/Xcode/ModuleCache.noindex",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "xcode-previews",
        label: "SwiftUI Previews 缓存",
        rel: "Library/Developer/Xcode/UserData/Previews",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "xcode-cache",
        label: "Xcode 总缓存",
        rel: "Library/Caches/com.apple.dt.Xcode",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "simulator-caches",
        label: "Simulator 运行时缓存",
        rel: "Library/Developer/CoreSimulator/Caches",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "simulator",
        label: "Simulator 设备",
        rel: "Library/Developer/CoreSimulator/Devices",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "xcode-devicesupport",
        label: "iOS DeviceSupport",
        rel: "Library/Developer/Xcode/iOS DeviceSupport",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "xcode-archives",
        label: "Archives（归档包）",
        rel: "Library/Developer/Xcode/Archives",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // ---------- AI 应用（2026-11 扩充；竞品对齐见文件头来源注） ----------
    // Cursor（Electron 系，缓存全部可再生）。
    // ⚠️ `Application Support/Cursor/User/`（含 state.vscdb AI 聊天记录、
    // workspaceStorage 工作区状态）与 `globalStorage/anysphere.cursor-agent-worker`
    // **不是缓存**，永不登记——本机实测 state.vscdb 18 GB，删除即丢失对话与状态。
    WhitelistEntry {
        key: "cursor-cacheddata",
        label: "Cursor V8 代码缓存",
        rel: "Library/Application Support/Cursor/CachedData",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cursor-cache",
        label: "Cursor HTTP 缓存",
        rel: "Library/Application Support/Cursor/Cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cursor-gpu",
        label: "Cursor GPU 缓存",
        rel: "Library/Application Support/Cursor/GPUCache",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cursor-vsix",
        label: "Cursor 扩展安装包缓存",
        rel: "Library/Application Support/Cursor/CachedExtensionVSIXs",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cursor-logs",
        label: "Cursor 日志",
        rel: "Library/Application Support/Cursor/logs",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "cursor-caches",
        label: "Cursor 系统缓存",
        rel: "Library/Caches/com.todesktop.230313mzl4w4u92",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // Claude 桌面版（Electron 系；对话数据在 Application Support/Claude 根下，只登记其缓存子目录）
    WhitelistEntry {
        key: "claude-app-cache",
        label: "Claude HTTP 缓存",
        rel: "Library/Application Support/Claude/Cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "claude-desktop-caches",
        label: "Claude 系统缓存",
        rel: "Library/Caches/com.anthropic.claudefordesktop",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // Claude Code CLI：shell 快照每轮会话自动重建（官方可再生）；
    // `.claude/projects` 会话记录是用户数据，不登记。
    WhitelistEntry {
        key: "claude-shell-snapshots",
        label: "Claude Code shell 快照",
        rel: ".claude/shell-snapshots",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    // Ollama：模型可 `ollama pull` 重下但代价高 → L3 逐项确认；
    // 清理走废纸篓，Ollama 服务重启后自行重建索引。
    WhitelistEntry {
        key: "ollama-models",
        label: "Ollama 模型权重（可重新 pull）",
        rel: ".ollama/models",
        level: Level::L3,
        a1_scope: false,
        os: Os::All,
    },
    WhitelistEntry {
        key: "ollama-logs",
        label: "Ollama 日志",
        rel: ".ollama/logs",
        level: Level::L2,
        a1_scope: false,
        os: Os::All,
    },
    // ---------- Xcode 补扫（2026-11：DerivedData/DeviceSupport/Archives 等既有条目之外的缺口） ----------
    WhitelistEntry {
        key: "xcode-ibsupport",
        label: "Interface Builder 设备支持",
        rel: "Library/Developer/Xcode/UserData/IB Support",
        level: Level::L2,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "simulator-runtimes",
        label: "Simulator 旧平台运行时（可重下）",
        rel: "Library/Developer/CoreSimulator/Profiles/Runtimes",
        level: Level::L3,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // ---------- 容器（L4：只读报告，永不执行） ----------
    WhitelistEntry {
        key: "docker-vm",
        label: "Docker VM 磁盘",
        rel: "Library/Containers/com.docker.docker/Data/vms",
        level: Level::L4,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "orbstack",
        label: "OrbStack 数据",
        rel: "Library/Group Containers/HUAQ24HBR6.dev.orbstack",
        level: Level::L4,
        a1_scope: false,
        os: Os::MacOnly,
    },
    WhitelistEntry {
        key: "colima",
        label: "Colima 数据",
        rel: ".colima",
        level: Level::L4,
        a1_scope: false,
        os: Os::MacOnly,
    },
    // ---------- Windows（%LOCALAPPDATA% = ~/AppData/Local） ----------
    WhitelistEntry {
        key: "win-temp",
        label: "临时文件（Temp）",
        rel: "AppData/Local/Temp",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-npm",
        label: "npm 缓存",
        rel: "AppData/Local/npm-cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-pnpm",
        label: "pnpm store",
        rel: "AppData/Local/pnpm/store",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-yarn",
        label: "Yarn 缓存",
        rel: "AppData/Local/Yarn/Cache/v6",
        level: Level::L2,
        // 保守：Windows 侧尚未实测「目录名即平台元数据」语义，不参与 A1 判死
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-pip",
        label: "pip 缓存",
        rel: "AppData/Local/pip/Cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-nuget",
        label: "NuGet 下载缓存",
        rel: "AppData/Local/NuGet/Cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-electron",
        label: "Electron 下载缓存",
        rel: "AppData/Local/electron/Cache",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-crashdumps",
        label: "崩溃转储",
        rel: "AppData/Local/CrashDumps",
        level: Level::L2,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-playwright",
        label: "Playwright 浏览器",
        rel: "AppData/Local/ms-playwright",
        level: Level::L3,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
    WhitelistEntry {
        key: "win-cypress",
        label: "Cypress 二进制",
        rel: "AppData/Local/Cypress/Cache",
        level: Level::L3,
        a1_scope: false,
        os: Os::WindowsOnly,
    },
];

/// 当前平台适用的登记项
fn here() -> impl Iterator<Item = &'static WhitelistEntry> {
    ENTRIES.iter().filter(|e| e.os.matches_here())
}

/// 已存在的登记项（不存在的不进报告，避免 UI 出现一堆 0）
pub fn resolve(home: &Path) -> Vec<(WhitelistEntry, PathBuf)> {
    here()
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

/// 按 key 取当前平台适用的登记项
pub fn entry(key: &str) -> Option<&'static WhitelistEntry> {
    here().find(|e| e.key == key)
}

/// 路径是否落在白名单内（护栏 1）。
///
/// 规则：`canonicalize` 之后必须**等于**某个白名单根，或位于其下。
/// canonicalize 会消掉 `..` 与符号链接，因此 symlink 指向 home 外部也会被拒。
pub fn is_whitelisted(path: &Path, home: &Path) -> bool {
    let Ok(target) = path.canonicalize() else {
        return false; // 不存在的路径不放行（避免 TOCTOU 后门）
    };
    here().any(|e| {
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
        // 挑一条当前平台适用的条目做样本，避免在 Windows 上测 macOS 路径
        let sample = here().next().expect("至少有一条当前平台条目");
        let cache = home.join(sample.rel);
        let outside = home.join("Documents/important");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside = cache.join("prune-test-child");
        std::fs::create_dir_all(&inside).unwrap();

        assert!(is_whitelisted(&inside, &home), "白名单内应放行");
        assert!(is_whitelisted(&cache, &home), "白名单根本身应放行");
        assert!(!is_whitelisted(&outside, &home), "用户文档目录必须拒绝");

        // ../ 穿越：规范化后落在 Documents，必须拒绝
        let traversal = cache.join("../../../../Documents/important");
        assert!(!is_whitelisted(&traversal, &home));

        // symlink 指向白名单外（Windows 建符号链接需要开发者模式，故仅 unix 验证）
        #[cfg(unix)]
        {
            let link = cache.join("escape");
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

    #[test]
    fn windows_entries_are_home_relative_appdata_paths() {
        let win: Vec<&WhitelistEntry> =
            ENTRIES.iter().filter(|e| e.os == Os::WindowsOnly).collect();
        assert!(
            !win.is_empty(),
            "必须登记 Windows 规则，否则 Windows 上扫不出东西"
        );
        for e in win {
            assert!(
                !Path::new(e.rel).is_absolute(),
                "{} 的 rel 必须是 home 相对路径",
                e.key
            );
            assert!(
                e.rel.starts_with("AppData/"),
                "{} 应落在 %LOCALAPPDATA%（~/AppData/Local）之下，实际 {}",
                e.key,
                e.rel
            );
            assert!(
                !e.rel.contains('\\'),
                "{} 的 rel 用正斜杠分隔，Windows API 同样接受，避免转义问题",
                e.key
            );
        }
    }

    #[test]
    fn macos_entries_are_home_relative() {
        for e in ENTRIES.iter().filter(|e| e.os == Os::MacOnly) {
            assert!(
                !Path::new(e.rel).is_absolute(),
                "{} 的 rel 必须相对 home",
                e.key
            );
            assert!(!e.rel.starts_with("AppData/"), "{} 是 macOS 条目", e.key);
        }
    }

    #[test]
    fn entries_are_filtered_by_platform() {
        let applicable = here().count();
        assert!(applicable > 0);
        assert!(applicable < ENTRIES.len(), "两端条目应有交集也有专属项");

        #[cfg(target_os = "macos")]
        {
            assert_eq!(here().filter(|e| e.os == Os::WindowsOnly).count(), 0);
            assert!(here().any(|e| e.os == Os::MacOnly));
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(here().filter(|e| e.os == Os::MacOnly).count(), 0);
            assert!(here().any(|e| e.os == Os::WindowsOnly));
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = ENTRIES.iter().map(|e| e.key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "key 不能重复（IPC 按 key 定位）");
    }

    #[test]
    fn ai_entries_cover_three_apps_with_safe_levels() {
        let by = |k: &str| ENTRIES.iter().find(|e| e.key == k).expect("登记项应存在");
        // Cursor 六项全部 🟢 可再生缓存
        for k in [
            "cursor-cacheddata",
            "cursor-cache",
            "cursor-gpu",
            "cursor-vsix",
            "cursor-logs",
            "cursor-caches",
        ] {
            assert_eq!(by(k).level, Level::L2, "{k} 必须是 L2");
            assert_eq!(by(k).os, Os::MacOnly, "{k} 路径仅实测过 macOS");
        }
        // Claude：桌面版两条 macOS + CLI 快照跨平台
        assert_eq!(by("claude-app-cache").level, Level::L2);
        assert_eq!(by("claude-desktop-caches").level, Level::L2);
        assert_eq!(by("claude-shell-snapshots").os, Os::All);
        // Ollama：模型是唯一 L3（可重 pull 但代价高），日志 L2
        assert_eq!(by("ollama-models").level, Level::L3);
        assert_eq!(by("ollama-logs").level, Level::L2);
        assert_eq!(by("ollama-models").os, Os::All);
    }

    #[test]
    fn ai_entries_never_touch_user_state() {
        // 红线测试：AI 应用的「用户状态」目录永不入白名单
        // （state.vscdb = AI 聊天记录，workspaceStorage = 工作区状态，projects = 会话记录）
        let banned = [
            "state.vscdb",
            "workspaceStorage",
            "anysphere.cursor-agent-worker",
            "projects",
        ];
        for e in ENTRIES.iter() {
            if e.key.starts_with("cursor-") || e.key.starts_with("claude-") {
                for b in banned {
                    assert!(
                        !e.rel.contains(b),
                        "{} 指向了用户状态路径（含 {b}），AI 聊天记录不可再生",
                        e.key
                    );
                }
            }
        }
    }

    #[test]
    fn xcode_supplement_entries_registered() {
        let ib = ENTRIES
            .iter()
            .find(|e| e.key == "xcode-ibsupport")
            .expect("IB Support 应登记");
        assert_eq!(ib.level, Level::L2);
        let rt = ENTRIES
            .iter()
            .find(|e| e.key == "simulator-runtimes")
            .expect("旧运行时应登记");
        assert_eq!(rt.level, Level::L3, "运行时可重下但代价高，必须逐项确认");
        assert_eq!(ib.os, Os::MacOnly);
        assert_eq!(rt.os, Os::MacOnly);
    }
}
