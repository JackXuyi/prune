//! prune-core：Prune 的核心逻辑层，GUI（src-tauri）与 CLI（prune-cli）共享。
//!
//! 模块状态（见 docs/prune-tauri-technical-plan.md §2 架构）：
//! - `mem/`     ✅ M2：sysinfo 进程枚举 + `/usr/bin/footprint -p <pid> -j` 深探（非特权、含 swapped）
//! - `swap/`    ✅ M2：`/System/Volumes/VM/` swapfile 监控 + `sysctl vm.swapusage`
//! - `db/`      ✅ M2：rusqlite footprint 时间序列（供 M3 泄漏嫌疑判定回归计算）
//! - `disk/`    ⬜ M1：ignore + rayon 并行扫描（6336 个 node_modules 预期 < 15 s）
//! - `classify/` ⬜ M1：跨平台二进制判定（杀手功能 A1）；进程家族归类已落在 `mem/process.rs`
//! - `reclaim/` ⬜ M2+：安全删除：路径白名单 + 废纸篓（trash）+ dry-run 前置 + >20GB 熔断

pub mod db;
pub mod mem;
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
