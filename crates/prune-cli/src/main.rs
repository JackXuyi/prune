//! prune-cli：命令行入口（README「CLI（规划中）」一节）。
//!
//! 目标子命令：
//! - prune scan                 全盘扫描，输出可回收清单
//! - prune reclaim --dry-run    预览将删除的内容
//! - prune reclaim --level green
//! - prune mem --swapped / --watch

fn main() {
    println!(
        "prune-cli v{}（CLI 尚未实现，子命令规划见 README）",
        prune_core::version()
    );
}
