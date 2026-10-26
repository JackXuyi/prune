//! A1 跨平台二进制判定（M1/T-303）。
//!
//! 输入是缓存条目的**目录名**（Yarn v6 形态：`npm-@next-swc-darwin-arm64-13.5.1-<hash>-integrity`），
//! 输出是该条目在本机是否**永远加载不了**。
//!
//! 设计原则：**宁可漏判，不可误杀**。
//! 只有「平台词 + 紧邻架构词」同时出现才判定，单独出现 `linux` 的包名（如 `linux-tools`）一律不判。
//! 这一条把 `reclaim-scan.sh` 里 `grep '*linux*'` 的松散匹配收紧成了保守匹配。
//!
//! | 平台 | 判定 |
//! |---|---|
//! | linux / win32 / android / freebsd | `Deadweight`（macOS 永远加载不了，与 Rosetta 无关） |
//! | darwin-arm64 | `Keep` |
//! | darwin-x64 + arm64 机 + 有 Rosetta | `NeedsConfirm`（x86 进程可能需要） |
//! | darwin-x64 + arm64 机 + 无 Rosetta | `Deadweight` |
//! | 无平台词 / 无法确定架构 | `Keep` |

use serde::{Deserialize, Serialize};

/// 缓存条目承载的二进制平台
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Linux,
    Win32,
    Android,
    Freebsd,
    DarwinX64,
    DarwinArm64,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::Win32 => "win32",
            Platform::Android => "android",
            Platform::Freebsd => "freebsd",
            Platform::DarwinX64 => "darwin-x64",
            Platform::DarwinArm64 => "darwin-arm64",
        }
    }

    /// macOS 上是否可能被加载（不含 Rosetta 场景，Rosetta 在 `classify` 里单独处理）
    pub fn loadable_on_mac(self) -> bool {
        matches!(self, Platform::DarwinX64 | Platform::DarwinArm64)
    }
}

/// 本机架构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X64,
    Unknown,
}

/// 判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// 🟢 本机永远加载不了，可直接清理
    Deadweight,
    /// 🟡 依赖 Rosetta / 架构不明，需人工确认
    NeedsConfirm,
    /// 保留
    Keep,
}

/// 架构词（出现在平台词之后）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchToken {
    X64,
    Arm64,
    X86,
    Arm32,
    Wide64,
}

fn arch_token(tok: &str) -> Option<ArchToken> {
    match tok {
        "x64" | "amd64" | "x86-64" | "x64_64" => Some(ArchToken::X64),
        "arm64" | "aarch64" => Some(ArchToken::Arm64),
        "ia32" | "x86" | "i686" | "i386" => Some(ArchToken::X86),
        "arm" | "armv7" | "armhf" => Some(ArchToken::Arm32),
        "64" => Some(ArchToken::Wide64),
        _ => None,
    }
}

/// 把目录名切成词：`-` 与 `_` 都当分隔符，统一小写。
fn tokens(name: &str) -> Vec<String> {
    name.to_ascii_lowercase()
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// 从条目名推断平台。必须紧邻架构词（`win32-x64` / `darwin-arm64` / `linux-musl-x64`），
/// 单独出现的平台词不算，避免把 `linux-tools` 这类包名误判。
pub fn platform_of(name: &str) -> Option<Platform> {
    let ts = tokens(name);
    let mut i = 0;
    while i < ts.len() {
        let t = ts[i].as_str();
        // linux 允许中间夹一个 musl
        let arch_at = if t == "linux" && ts.get(i + 1).map(String::as_str) == Some("musl") {
            i + 2
        } else {
            i + 1
        };
        let arch = ts.get(arch_at).and_then(|s| arch_token(s));
        let platform = match (t, arch) {
            ("linux", Some(_)) => Some(Platform::Linux),
            ("win32" | "win" | "windows", Some(_)) => Some(Platform::Win32),
            ("android", Some(_)) => Some(Platform::Android),
            ("freebsd", Some(_)) => Some(Platform::Freebsd),
            ("darwin" | "macos" | "mac", Some(ArchToken::Arm64)) => Some(Platform::DarwinArm64),
            (
                "darwin" | "macos" | "mac",
                Some(ArchToken::X64) | Some(ArchToken::X86) | Some(ArchToken::Wide64),
            ) => Some(Platform::DarwinX64),
            _ => None,
        };
        if platform.is_some() {
            return platform;
        }
        i += 1;
    }
    None
}

/// 判定一个条目（纯函数，单测重仓）
pub fn classify(platform: Option<Platform>, arch: Arch, has_rosetta: bool) -> Verdict {
    let Some(p) = platform else {
        return Verdict::Keep; // 认不出来就不动它
    };
    match p {
        // macOS 原生跑不了这些平台的二进制，与 Rosetta 无关
        Platform::Linux | Platform::Win32 | Platform::Android | Platform::Freebsd => Verdict::Deadweight,
        Platform::DarwinArm64 => match arch {
            Arch::Arm64 => Verdict::Keep,
            // 反向场景（intel 机器上的 arm64 包）很少见，交给人工
            Arch::X64 => Verdict::NeedsConfirm,
            Arch::Unknown => Verdict::Keep,
        },
        Platform::DarwinX64 => match arch {
            Arch::Arm64 if has_rosetta => Verdict::NeedsConfirm,
            Arch::Arm64 => Verdict::Deadweight,
            Arch::X64 => Verdict::Keep,
            Arch::Unknown => Verdict::Keep,
        },
    }
}

/// 运行时取本机架构（不用 `cfg!`：交叉编译时编译期常量会说谎）
pub fn host_arch() -> Arch {
    match std::process::Command::new("/usr/bin/uname").arg("-m").output() {
        Ok(out) if out.status.success() => match String::from_utf8_lossy(&out.stdout).trim() {
            "arm64" | "aarch64" => Arch::Arm64,
            "x86_64" | "amd64" => Arch::X64,
            _ => Arch::Unknown,
        },
        _ => Arch::Unknown,
    }
}

/// 本机是否装了 Rosetta 且在运行（`oahd` 存在即说明有 x86 进程在用）。
///
/// 用 sysinfo 直接查进程，不 fork `pgrep`，避免沙箱/权限噪音。
pub fn has_rosetta() -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .any(|p| p.name().to_string_lossy() == "oahd")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_platforms_are_deadweight() {
        // 真实 Yarn 目录名
        let cases = [
            ("npm-@esbuild-android-arm64-0.19.12-b0c2-integrity", Platform::Android),
            ("npm-@next-swc-linux-x64-gnu-14.2.3-abc-integrity", Platform::Linux),
            ("npm-@rollup-rollup-win32-x64-msvc-4.9.6-def-integrity", Platform::Win32),
            ("npm-@esbuild-linux-64-0.21.5-xyz", Platform::Linux),
            ("npm-@esbuild-freebsd-arm64-0.19.0-q", Platform::Freebsd),
            // linux-musl-x64 中间夹 musl
            ("npm-@next-swc-linux-musl-x64-14.0.0-m", Platform::Linux),
        ];
        for (name, want) in cases {
            assert_eq!(platform_of(name), Some(want), "{name}");
            assert_eq!(
                classify(platform_of(name), Arch::Arm64, true),
                Verdict::Deadweight,
                "{name} 与 Rosetta 无关"
            );
        }
    }

    #[test]
    fn darwin_arm64_is_kept_on_arm_host() {
        let name = "npm-@next-swc-darwin-arm64-14.2.3-abc-integrity";
        assert_eq!(platform_of(name), Some(Platform::DarwinArm64));
        assert_eq!(classify(platform_of(name), Arch::Arm64, false), Verdict::Keep);
        assert_eq!(classify(platform_of(name), Arch::Arm64, true), Verdict::Keep);
    }

    #[test]
    fn darwin_x64_depends_on_rosetta() {
        let name = "npm-@next-swc-darwin-x64-14.2.3-abc-integrity";
        assert_eq!(platform_of(name), Some(Platform::DarwinX64));
        assert_eq!(
            classify(platform_of(name), Arch::Arm64, true),
            Verdict::NeedsConfirm,
            "装了 Rosetta：x86 进程可能真的需要，绝不默认清理"
        );
        assert_eq!(
            classify(platform_of(name), Arch::Arm64, false),
            Verdict::Deadweight
        );
        // intel 机器上 darwin-x64 是原生包
        assert_eq!(classify(platform_of(name), Arch::X64, false), Verdict::Keep);
    }

    #[test]
    fn package_names_with_platform_words_are_not_misjudged() {
        // 平台词后面不是架构词 → 不是二进制包
        for name in [
            "npm-linux-tools-2.0.0-abc-integrity",
            "npm-some-win32-thing-1.0.0-abc",
            "npm-darwin-not-a-binary-1.0.0-x",
            "npm-@ahooksjs-use-request-2.8.15-daa3-integrity",
            "npm-window-size-1.1.0-abc",
        ] {
            assert_eq!(platform_of(name), None, "{name} 不该被判为平台包");
            assert_eq!(classify(platform_of(name), Arch::Arm64, false), Verdict::Keep);
        }
    }

    #[test]
    fn unknown_host_arch_keeps_darwin_packages() {
        for p in [Platform::DarwinArm64, Platform::DarwinX64] {
            assert_eq!(
                classify(Some(p), Arch::Unknown, false),
                Verdict::Keep,
                "架构不明时不冒险"
            );
        }
    }

    #[test]
    fn live_probe_matches_host() {
        // 本机架构可读（不假设具体值，只要求不是 Unknown 时逻辑自洽）
        let arch = host_arch();
        let has = has_rosetta();
        let v = classify(Some(Platform::DarwinX64), arch, has);
        if arch == Arch::Arm64 {
            assert_eq!(v, if has { Verdict::NeedsConfirm } else { Verdict::Deadweight });
        }
    }
}
