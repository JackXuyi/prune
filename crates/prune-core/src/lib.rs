//! prune-core：Prune 的核心逻辑层，GUI（src-tauri）与 CLI（prune-cli）共享。
//!
//! 规划中的模块（见 docs/prune-tauri-technical-plan.md §2 架构）：
//! - mem/      sysinfo 进程枚举 + `/usr/bin/footprint -p <pid> -j` 深探（非特权、含 swapped）
//! - swap/     /System/Volumes/VM/ swapfile 监控 + sysctl vm.swapusage —— 内存与磁盘的桥
//! - disk/     ignore + rayon 并行扫描（6336 个 node_modules 预期 < 15 s）
//! - classify/ 跨平台二进制判定（杀手功能 A1）+ 进程家族归类
//! - reclaim/  安全删除：路径白名单 + 废纸篓（trash）+ dry-run 前置 + >20GB 熔断
//! - db/       rusqlite footprint 时间序列（供泄漏嫌疑判定回归计算）

/// 返回 crate 版本（占位用，M1 起填入真实逻辑）。
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
