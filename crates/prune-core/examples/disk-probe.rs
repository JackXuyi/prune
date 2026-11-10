//! 开发探针：实测磁盘扫描耗时与 A1 覆盖率（M1/T-301，不进构建产物）。
//!
//! 用法：`cargo run -p prune-core --example disk-probe`
//!
//! 关闭 M0 遗留项：
//! - #2 扫描性能：全量白名单 + node_modules 发现是否 < 20 s
//! - #4 A1 准确率：严格匹配（平台词+架构词）与松散匹配（仅平台词）的条目差，
//!   差值就是「严格规则主动放弃的灰色地带」，用于人工抽样核对。

use std::path::PathBuf;
use std::time::Instant;

use prune_core::classify::{self, has_rosetta, host_arch};
use prune_core::disk;
use prune_core::disk::whitelist::Level;
use prune_core::reclaim;

fn mb(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1e9)
    } else {
        format!("{:.1} MB", bytes as f64 / 1e6)
    }
}

fn main() {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let arch = host_arch();
    let rosetta = has_rosetta();
    println!("home={} arch={:?} rosetta={rosetta}", home.display(), arch);

    let started = Instant::now();
    let last = std::sync::atomic::AtomicU64::new(0);
    let report = disk::scan(
        &home,
        Some(&|p: disk::ScanProgress| {
            let prev = last.swap(p.elapsed_ms, std::sync::atomic::Ordering::Relaxed);
            let delta = p.elapsed_ms.saturating_sub(prev);
            eprintln!(
                "  [{}/{}] {:<14} 累计 {:>9} · Δ{:>6} ms",
                p.done,
                p.total,
                p.phase,
                mb(p.bytes_so_far),
                delta
            );
        }),
    );
    let elapsed = started.elapsed();
    let report = &report;

    println!(
        "\n== 扫描完成 {} ms（in-report {} ms）==",
        elapsed.as_millis(),
        report.scan_elapsed_ms
    );

    println!(
        "\n{:<28} {:>10} {:>8} {:>6}  {}",
        "组", "体积", "条目", "级别", "Deadweight"
    );
    for g in &report.groups {
        let dw = g
            .deadweight
            .as_ref()
            .map(|d| {
                format!(
                    "{} ({} 条, 原生可用 {}, 其余 {}, 比 {})",
                    mb(d.bytes),
                    d.entry_count,
                    mb(d.native_bytes),
                    mb(d.other_bytes),
                    d.ratio
                        .map(|r| format!("{r:.2}:1"))
                        .unwrap_or_else(|| "—".into())
                )
            })
            .unwrap_or_else(|| "—".into());
        println!(
            "{:<28} {:>10} {:>8} {:>6}  {}",
            g.label,
            mb(g.total_bytes),
            g.entry_count,
            match g.level {
                Level::L1 => "L1",
                Level::L2 => "L2",
                Level::L3 => "L3",
                Level::L4 => "L4",
            },
            dw
        );
    }

    let t = &report.totals;
    println!(
        "\n合计：L1 {} · L2 {} · L3 {} · L4(不计入) {} · 主数字 {} · 缓存总计 {}",
        mb(t.deadweight),
        mb(t.l2),
        mb(t.l3),
        mb(t.l4),
        mb(t.reclaimable),
        mb(t.cache_total)
    );
    if let Some(dup) = &report.dup {
        println!("A3: {} — {}", mb(dup.total_bytes), dup.advice);
    }

    // ---- A1 明细与覆盖率 ----
    if let Some(yarn) = report.groups.iter().find(|g| g.key == "yarn") {
        if let Some(dw) = &yarn.deadweight {
            println!("\n== A1 平台分布（严格匹配）==");
            for p in &dw.by_platform {
                println!(
                    "  {:<12} {:>4} 条  {:>10}",
                    p.platform,
                    p.entry_count,
                    mb(p.bytes)
                );
            }
            println!("\n== Top 10 死重条目 ==");
            for it in dw.items.iter().take(10) {
                println!(
                    "  {:>10}  {:<12} {}",
                    mb(it.bytes),
                    it.platform.clone().unwrap_or_default(),
                    it.name
                );
            }
        }
        // 覆盖率对照：松散匹配（任何 linux/win32/android/darwin-x64 字样）
        let sizes = disk::first_level_sizes(std::path::Path::new(&yarn.root));
        let loose = sizes
            .iter()
            .filter(|(n, _)| {
                let l = n.to_ascii_lowercase();
                ["linux", "win32", "android", "darwin-x64"]
                    .iter()
                    .any(|t| l.contains(t))
            })
            .count();
        let strict = sizes
            .iter()
            .filter(|(n, _)| {
                matches!(
                    classify::classify(classify::platform_of(n), arch, rosetta),
                    classify::Verdict::Deadweight
                )
            })
            .count();
        println!(
            "\n覆盖率：条目总数 {} · 松散匹配 {loose} · 严格判死 {strict}（差 {} 条为主动放弃的灰色地带）",
            sizes.len(),
            loose.saturating_sub(strict)
        );

        // 验收标准 #2 的人工抽样清单：等距取 100 条，带判定结果供逐行核对。
        // 死重条目只占全库 4%，纯等距抽样几乎全是「保留」，看不出误判；
        // 因此按「含平台词的」与「不含平台词的」各抽一半——前者查误杀，后者查漏判。
        if let Ok(out) = std::env::var("PRUNE_A1_SAMPLE") {
            let has_token = |n: &str| {
                let l = n.to_ascii_lowercase();
                ["linux", "win32", "android", "darwin"]
                    .iter()
                    .any(|t| l.contains(t))
            };
            let (hits, rest): (Vec<_>, Vec<_>) = sizes.iter().partition(|(n, _)| has_token(n));
            let mut picked: Vec<&(String, u64)> = Vec::new();
            for bucket in [&hits, &rest] {
                let step = (bucket.len() / 50).max(1);
                picked.extend(bucket.iter().step_by(step).take(50));
            }

            let mut lines = vec![
                "# A1 判定抽样（100 条，供人工核对）".to_string(),
                String::new(),
                "> 由 `PRUNE_A1_SAMPLE=<路径> cargo run --release -p prune-core --example disk-probe` 生成。".to_string(),
                "> 抽样口径：含平台词的条目抽 50 条（查误杀），不含平台词的抽 50 条（查漏判）。".to_string(),
                "> 核对方法：目录名里是否真含该平台词、本机（arm64 + Rosetta？）能否加载。判错的行反馈回来改 token 表。".to_string(),
                String::new(),
                "| # | 判定 | 平台 | 目录名（截断 72 字） |".to_string(),
                "|---|---|---|---|".to_string(),
            ];
            for (idx, (name, _)) in picked.iter().enumerate() {
                let p = classify::platform_of(name);
                let v = classify::classify(p, arch, rosetta);
                let vtxt = match v {
                    classify::Verdict::Deadweight => "🟢 死重",
                    classify::Verdict::NeedsConfirm => "🟡 待确认",
                    classify::Verdict::Keep => "✅ 保留",
                };
                let short: String = name.chars().take(72).collect();
                lines.push(format!(
                    "| {} | {} | {} | `{}` |",
                    idx + 1,
                    vtxt,
                    p.map(|x| x.as_str()).unwrap_or("—"),
                    short
                ));
            }
            std::fs::write(&out, lines.join("\n")).unwrap();
            println!(
                "\n抽样清单已写入 {out}（含平台词 {} / 不含 {}）",
                hits.len(),
                rest.len()
            );
        }
    }

    println!("\n== 预算校验 ==");
    println!(
        "扫描 {} ms（预算 20000 ms）→ {}",
        elapsed.as_millis(),
        if elapsed.as_millis() < 20_000 {
            "✅ 通过"
        } else {
            "❌ 超标"
        }
    );

    // ---------------------------------------------------------------- dry-run
    // 只生成清单，不动任何文件。验收 T-309 的「预览可读、L4 拒执行」两项。
    println!("\n== dry-run 预览（不删任何文件）==");
    for key in ["yarn", "npm", "node-modules", "docker-vm"] {
        let p = reclaim::preview(&[key.to_string()], &report);
        let first = p
            .items
            .first()
            .map(|i| i.path.rsplit('/').next().unwrap_or(&i.path).to_string())
            .unwrap_or_else(|| "—".into());
        println!(
            "  {:<13} {:>2} 项 {:>10} · confirm={:<5} · 首项 {}",
            key,
            p.items.len(),
            mb(p.total_bytes),
            p.needs_confirm,
            first
        );
        for n in &p.notes {
            println!("      ↳ {n}");
        }
    }

    // 白名单外路径必须被拒（真删防护的最后一关）
    let evil = vec![prune_core::reclaim::ReclaimItem {
        path: "/etc/hosts".into(),
        label: "越界测试".into(),
        bytes: 1,
        level: Level::L1,
    }];
    match prune_core::reclaim::execute(&evil, &home, true, Default::default(), &[]) {
        Err(e) => println!("\n  白名单外路径 → ✅ 已拒绝（{e}）"),
        Ok(r) => println!("\n  白名单外路径 → ❌ 竟然执行了！moved={}", r.moved),
    }
}
