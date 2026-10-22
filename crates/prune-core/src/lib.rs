//! prune-core：Prune 的核心逻辑层，GUI（src-tauri）与 CLI（prune-cli）共享。
//!
//! 规划中的模块（见 docs/prune-tauri-technical-plan.md）：
//! - 磁盘扫描：`ignore` + `rayon` 并行，探测 node_modules / 包管理器缓存
//! - 跨平台二进制探测：Yarn/npm/pnpm 缓存中本机架构永远加载不了的包
//! - 内存口径：`/usr/bin/footprint -p <pid> -j`（非特权、含 swapped 明细）
//! - 白名单清理：路径白名单 + 废纸篓（`trash` crate）+ dry-run 前置

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
