//! A7 增量扫描真机探针（验收：docs/a7-incremental-plan.md §4 #4）。
//!
//! 流程：全量（计时）→ 原样增量（计时，应 < 1 s 且数字一致）
//! → 制造变化（向 Yarn v6 写一个临时死重条目 + 触碰 npm cacache 首层）
//! → 增量（应捕获变化）→ 全量（数字应与增量一致）→ 清理临时条目。
//!
//! 只读为主，唯一写入是自带 `_prune_probe_` 前缀的临时条目并在退出时删除。

use std::time::Instant;

use prune_core::disk::{self, ScanProgress};

fn main() {
    let home = dirs_home();
    println!("home = {}", home.display());

    let mut rounds: Vec<(&str, u64, u64, u64, u64)> = Vec::new(); // (tag, ms, reclaimable, deadweight, groups)

    // 1) 全量
    let t = Instant::now();
    let (r1, snap) = disk::scan_with(&home, None, Some(&|p: ScanProgress| progress("全量", &p)));
    let ms = t.elapsed().as_millis() as u64;
    rounds.push((
        "全量 #1",
        ms,
        r1.totals.reclaimable,
        r1.totals.deadweight,
        r1.groups.len() as u64,
    ));

    // 2) 无变化增量：应命中全部指纹，预期亚秒
    let t = Instant::now();
    let (r2, snap) = disk::scan_with(
        &home,
        Some(&snap),
        Some(&|p: ScanProgress| progress("增量", &p)),
    );
    let ms = t.elapsed().as_millis() as u64;
    rounds.push((
        "增量 #1（无变化）",
        ms,
        r2.totals.reclaimable,
        r2.totals.deadweight,
        r2.groups.len() as u64,
    ));
    assert!(r2.incremental);
    assert_eq!(
        r1.totals.reclaimable, r2.totals.reclaimable,
        "无变化增量必须与全量一致"
    );
    assert_eq!(r1.totals.deadweight, r2.totals.deadweight);

    // 3) 制造变化：临时死重条目 + npm 首层直接写入
    let probe = home.join("Library/Caches/Yarn/v6/_prune_probe_linux-x64-0.0.0-tmp");
    std::fs::create_dir_all(&probe).unwrap();
    std::fs::write(probe.join("payload.bin"), vec![0u8; 8 * 1024 * 1024]).unwrap();
    let npm_touch = home.join(".npm/_cacache/_prune_probe_marker");
    if let Ok(rd) = std::fs::read_dir(home.join(".npm/_cacache")) {
        // 往某个既有首层目录直接写一个标记文件，确保该目录 mtime 变化
        if let Some(first) = rd.flatten().next() {
            let _ = std::fs::write(first.path().join("_prune_probe_marker"), b"probe");
            println!("触碰首层目录：{}", first.path().display());
        }
    }
    let _ = npm_touch;

    let t = Instant::now();
    let (r3, snap) = disk::scan_with(
        &home,
        Some(&snap),
        Some(&|p: ScanProgress| progress("增量", &p)),
    );
    let ms = t.elapsed().as_millis() as u64;
    rounds.push((
        "增量 #2（有变化）",
        ms,
        r3.totals.reclaimable,
        r3.totals.deadweight,
        r3.groups.len() as u64,
    ));

    // 4) 全量校准：数字必须与增量一致
    let t = Instant::now();
    let (r4, _snap) = disk::scan_with(&home, None, Some(&|p: ScanProgress| progress("全量", &p)));
    let ms = t.elapsed().as_millis() as u64;
    rounds.push((
        "全量 #2",
        ms,
        r4.totals.reclaimable,
        r4.totals.deadweight,
        r4.groups.len() as u64,
    ));
    assert_eq!(
        r3.totals.reclaimable, r4.totals.reclaimable,
        "增量捕获变化后应与全量一致"
    );
    assert_eq!(r3.totals.deadweight, r4.totals.deadweight);

    // 清理探针产物
    let _ = std::fs::remove_dir_all(&probe);
    if let Some(first) = std::fs::read_dir(home.join(".npm/_cacache"))
        .ok()
        .and_then(|mut rd| rd.flatten().next())
    {
        let _ = std::fs::remove_file(first.path().join("_prune_probe_marker"));
    }

    println!(
        "\n{:<22}{:>10}{:>14}{:>14}{:>8}",
        "轮次", "耗时ms", "可回收GB", "死重GB", "组数"
    );
    for (tag, ms, rec, dw, n) in &rounds {
        println!(
            "{:<22}{:>10}{:>14.2}{:>14.2}{:>8}",
            tag,
            ms,
            *rec as f64 / 1e9,
            *dw as f64 / 1e9,
            n
        );
    }
    println!("\n探针产物已清理");
}

fn progress(tag: &str, p: &ScanProgress) {
    if p.done == p.total {
        println!(
            "[{tag}] {}/{} · {} ms · {:.2} GB",
            p.done,
            p.total,
            p.elapsed_ms,
            p.bytes_so_far as f64 / 1e9
        );
    }
}

fn dirs_home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").expect("HOME"))
}
