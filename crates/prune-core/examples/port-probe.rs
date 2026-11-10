//! 端口扫描探针：直接打印一轮 `net::ports::scan` 的原始结果。
//!
//! 用途（docs/m7-port-plan.md §12 验收）：
//! - 与 `lsof -nP -iTCP -sTCP:LISTEN` 逐条比对；
//! - 确认双栈合并、关注度分层、TIME_WAIT、项目归属是否符合实测基线；
//! - 记录耗时（预算 ≤120 ms）。
//!
//! 运行：`cargo run -p prune-core --example port-probe`

use prune_core::net::ports;

fn main() {
    // 两轮：第一轮冷扫（要跑 20 个 cwd 查询），第二轮吃项目缓存。
    // 验收看的是热扫耗时（GUI 的真实路径）。
    let mut cache = prune_core::net::host::ProjectCache::new();
    let t0 = std::time::Instant::now();
    let _ = ports::scan_cached(None, &mut cache);
    let cold = t0.elapsed();

    let t1 = std::time::Instant::now();
    let snap = ports::scan_cached(None, &mut cache);
    let warm = t1.elapsed();

    println!(
        "冷扫 {cold:?} · 热扫 {warm:?}（内部计时 {} ms）",
        snap.scan_ms
    );
    println!(
        "{} 个端口 · 关注 {} · 折叠 {}",
        snap.entries.len(),
        snap.watched_count(),
        snap.folded_count()
    );
    println!(
        "覆盖：内核 LISTEN {} · 已归属 {} · 不可见 {}",
        snap.coverage.listen_total,
        snap.coverage.attributed,
        snap.coverage.invisible()
    );
    if let Some(note) = &snap.coverage.note {
        println!("⚠️  {note}");
    }
    println!(
        "TIME_WAIT：{} 条 · 涉及端口 {:?}",
        snap.time_wait.unwrap_or(0),
        &snap.time_wait_ports[..snap.time_wait_ports.len().min(12)]
    );
    println!();

    println!(
        "{:<6}{:<6}{:<10}{:<20}{:<30}{:<8}{}",
        "端口", "协议", "暴露面", "进程", "项目", "连接", "分层"
    );
    for e in &snap.entries {
        let b = e.primary();
        println!(
            "{:<6}{:<6}{:<10}{:<20}{:<30}{:<8}{}",
            e.port,
            match e.proto {
                prune_core::net::Proto::Tcp => "tcp",
                prune_core::net::Proto::Udp => "udp",
            },
            format!("{:?}", e.exposure).to_lowercase(),
            b.map(|x| x.comm.as_str()).unwrap_or("—"),
            b.and_then(|x| x.project_label.as_deref()).unwrap_or("—"),
            e.conns.map(|n| n.to_string()).unwrap_or_else(|| "—".into()),
            if e.watched { "关注" } else { "折叠" },
        );
        if let Some(note) = &e.note {
            println!("       ↳ {note}");
        }
        if e.owner_count() > 1 {
            println!("       ↳ {} 个进程共同监听", e.owner_count());
        }
    }
}
