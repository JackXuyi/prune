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
//! - `net/`      ✅ M7：端口占用反查（lsof 机器格式 + netstat 交叉校验 + 关注度分层）
//!               M8：连接计数按 pid 聚合（应用维度复用同一次 lsof）
//! - `appdisk`   ✅ M8：应用磁盘三层口径（bundle / cache / data），按需测量 + TTL 缓存
//! - `appicon`   ✅ D1：应用图标提取（icns → sips → PNG 缓存），失败由前端落字母徽标
//! - `paths/`    ✅ W1：平台路径口径（home / 配置目录 / 系统根），跨平台唯一出处
//! - `localtime` ✅ M14：本地日期 / 当日 00:00（swap 与 net 共用，不引 chrono）
//! - `island`    ✅ M15：CPU 暴走判定（达标率 + 滞回 + 频控），纯逻辑，不碰 AppKit
//! - `win/`      ✅ W2：Windows 系统调用（GlobalMemoryStatusEx / 进程 Private Bytes）

pub mod appdisk;
pub mod appicon;
pub mod base64;
pub mod classify;
pub mod db;
pub mod disk;
pub mod island;
pub mod leak;
pub mod localtime;
pub mod locale;
pub mod mem;
pub mod net;
pub mod paths;
pub mod pet;
pub mod reclaim;
pub mod swap;
/// Windows 系统调用封装（GlobalMemoryStatusEx / Private Bytes），仅 Windows 编译
#[cfg(windows)]
pub mod win;

/// 返回 crate 版本。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    /// 与仓库根 package.json 交叉校验：防止 bump 脚本漏改某一处后版本漂移。
    /// 此前断言写死 "0.1.0"，把「未发布」固化成了恒等检查（MCP serverInfo
    /// 与 CLI --version 一直对外报旧号）。
    #[test]
    fn version_matches_package_json() {
        let pkg: serde_json::Value = serde_json::from_str(include_str!("../../../package.json"))
            .expect("package.json 应存在且为合法 JSON");
        let expected = pkg["version"]
            .as_str()
            .expect("package.json 应有 version 字段");
        assert_eq!(super::version(), expected);
    }
}
