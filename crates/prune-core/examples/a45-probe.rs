//! A4/A5 明细真机探针（验收：docs/a4a5-detail-plan.md §4）。
//!
//! 只读 + dry-run：扫一轮 → 打印四组明细样张 → node_modules 逐项 preview 校验。
//! 不执行任何清理。

use prune_core::disk::{self, DetailItem, ScanProgress};
use prune_core::reclaim;

fn main() {
    let home = std::path::PathBuf::from(std::env::var("HOME").expect("HOME"));

    let (report, _snap) = disk::scan_with(&home, None, Some(&|p: ScanProgress| {
        if p.done == p.total {
            println!("[scan] 完成 {} ms · 主数字 {:.2} GB", p.elapsed_ms, p.bytes_so_far as f64 / 1e9);
        }
    }));

    let group = |key: &str| report.groups.iter().find(|g| g.key == key);

    println!("\n===== A4 node_modules 明细 =====");
    if let Some(nm) = group("node-modules") {
        println!("共 {} 项 · {:.2} GB", nm.details.len(), nm.total_bytes as f64 / 1e9);
        for d in nm.details.iter().take(5) {
            print_detail(d);
        }
        let stale = nm.details.iter().filter(|d| d.age_days > disk::STALE_DAYS).count();
        println!(">180 天未改动：{stale} 项");

        // 逐项 dry-run：护栏不走执行，只验证清单形态
        let pv = reclaim::preview(&["node-modules".into()], &report);
        println!(
            "dry-run：{} 项 · 需确认={} · 合计 {:.2} GB · 首项 = {}",
            pv.items.len(),
            pv.needs_confirm,
            pv.total_bytes as f64 / 1e9,
            pv.items.first().map(|i| i.label.as_str()).unwrap_or("—")
        );
    } else {
        println!("未发现 node_modules");
    }

    for key in ["simulator", "playwright", "cypress"] {
        println!("\n===== A5 {key} 明细 =====");
        match group(key) {
            Some(g) if !g.details.is_empty() => {
                println!("共 {} 项 · {:.2} GB", g.details.len(), g.total_bytes as f64 / 1e9);
                for d in g.details.iter().take(6) {
                    print_detail(d);
                }
            }
            Some(_) => println!("（组存在但无明细）"),
            None => println!("未发现"),
        }
    }
}

fn print_detail(d: &DetailItem) {
    println!(
        "  {:>9}  {:>4} 天未改动  {}{}",
        format_bytes(d.bytes),
        d.age_days,
        d.note.as_deref().unwrap_or(&d.name),
        if d.note.is_some() {
            format!("（{}）", d.name)
        } else {
            String::new()
        }
    );
}

fn format_bytes(b: u64) -> String {
    if b >= 1_000_000_000 {
        format!("{:.2} GB", b as f64 / 1e9)
    } else if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1e6)
    } else {
        format!("{:.0} KB", b as f64 / 1e3)
    }
}
