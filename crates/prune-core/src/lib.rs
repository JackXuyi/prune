//! prune-core：Prune 的核心逻辑层，GUI（src-tauri）与 CLI（prune-cli）共享。
//!
//! 模块状态（见 docs/prune-tauri-technical-plan.md §2 架构）：
//! - `mem/`     ✅ M2：sysinfo 进程枚举 + `/usr/bin/footprint -p <pid> -j` 深探（非特权、含 swapped）
//! - `swap/`    ✅ M2：`/System/Volumes/VM/` swapfile 监控 + `sysctl vm.swapusage`
//! - `db/`      ✅ M2：rusqlite footprint 时间序列（供 M3 泄漏嫌疑判定回归计算）
//! - `disk/`    ✅ M1：白名单定点扫描 + 条目级体积归因 + A2/A3 汇总（自建线程池，
//!              不用 ignore/rayon：定点目录不需要 gitignore 语义，多一层依赖不划算）
//! - `classify/` ✅ M1：跨平台二进制判定（杀手功能 A1）；进程家族归类在 `mem/process.rs`
//! - `reclaim/`  ✅ M1：安全删除：路径白名单 + 废纸篓（trash）+ dry-run 前置 + >20GB 熔断
//! - `leak/`     ✅ M3：B5 泄漏**嫌疑**判定（单进程序列回归 + 阶梯排除 + 良性名单）
//! - `paths/`    ✅ W1：平台路径口径（home / 配置目录 / 系统根），跨平台唯一出处

pub mod classify;
pub mod db;
pub mod disk;
pub mod leak;
pub mod mem;
pub mod paths;
pub mod reclaim;
pub mod swap;

/// 返回 crate 版本。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_cargo_pkg() {
        assert_eq!(super::version(), "0.1.0");
    }
}
