//! 开发探针：在真机上检查工作区归属效果（不参与构建产物）。
//!
//! 用法：`cargo run -p prune-core --example ws-probe`

use std::path::PathBuf;

use prune_core::mem::process::Scanner;
use prune_core::mem::workspace;

fn main() {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let mut scanner = Scanner::new();
    let procs = scanner.scan().procs;

    let known = workspace::known_workspaces(&home);
    println!("== 已知工作区 {} 个（workspaceStorage）==", known.len());
    for k in known.iter().take(6) {
        println!("  {:<22} {:<28} {}", k.label, k.app, k.path);
    }

    let ws = workspace::resolve_many(&procs, &home);
    println!("\n== 归属结果 {}/{} 进程 ==", ws.len(), procs.len());

    let mut rows: Vec<(i32, String, u64, &prune_core::mem::WorkspaceRef)> = procs
        .iter()
        .filter_map(|p| {
            ws.get(&p.pid)
                .map(|w| (p.pid, p.name.clone(), p.rss, w))
        })
        .collect();
    rows.sort_by_key(|(_, _, rss, _)| std::cmp::Reverse(*rss));
    for (pid, name, rss, w) in rows.iter().take(20) {
        println!(
            "  {:<7} {:<46} {:>7} MB  {:?}  {}",
            pid,
            truncate(name, 46),
            rss / 1024 / 1024,
            w.source,
            w.path
        );
    }

    // 按项目汇总
    let mut by_ws: std::collections::HashMap<&str, (usize, u64)> = Default::default();
    for (_, _, rss, w) in &rows {
        let e = by_ws.entry(w.path.as_str()).or_default();
        e.0 += 1;
        e.1 += rss;
    }
    let mut agg: Vec<_> = by_ws.into_iter().collect();
    agg.sort_by_key(|(_, (_, rss))| std::cmp::Reverse(*rss));
    println!("\n== 按项目汇总 ==");
    for (path, (n, rss)) in agg {
        println!("  {:>6} MB · {:>2} 进程  {}", rss / 1024 / 1024, n, path);
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}
