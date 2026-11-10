//! B5 泄漏判定的真机探针：直接读真实序列库跑一遍判定，把「命中」和「被拒」都打出来。
//!
//! 用法：
//! ```text
//! cargo run --release -p prune-core --example leak-probe
//! cargo run --release -p prune-core --example leak-probe -- /path/to/prune.db
//! ```
//!
//! `now` 取库里最后一个时间点（而不是系统当前时间）——历史库是几天前采的，
//! 拿当前时间去算会得出「所有进程都不在窗口内」的假结论。

use prune_core::db::Store;
use prune_core::leak::{self, Outcome};

fn mb(v: u64) -> f64 {
    v as f64 / 1_048_576.0
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/Library/Application Support/com.xuyi.prune/prune.db")
    });

    println!("== 序列库 ==");
    println!("path = {path}");

    let store = match Store::open(std::path::Path::new(&path)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打开失败：{e}");
            std::process::exit(1);
        }
    };

    let rows = store.row_count().unwrap_or(0);
    // 用库内最大 ts 作为「现在」，避免历史库被判成全部数据不足
    let since_wide = 0;
    let series = store.series(since_wide).unwrap_or_default();
    let now = series
        .iter()
        .flat_map(|s| s.samples.iter().map(|p| p.ts))
        .max()
        .unwrap_or(0);
    if now == 0 {
        println!("库内没有可用序列（footprint 行数为 0），无法判定");
        return;
    }
    println!("rows = {rows} · 序列 = {} 条 · now = {now}", series.len());

    // 只保留判定窗口内可能用到的部分（GC 窗口 60 min + 余量）
    let since = now - leak::GC_WINDOW_SECS - 300;
    let windowed: Vec<_> = series
        .iter()
        .map(|s| leak::ProcSeries {
            samples: s
                .samples
                .iter()
                .copied()
                .filter(|p| p.ts >= since)
                .collect(),
            ..s.clone()
        })
        .filter(|s| !s.samples.is_empty())
        .collect();

    // 探针没有实时进程表，exe 拿不到 → 良性名单只能按家族名/进程名匹配，
    // 判定会更严（不放过），人工核对时注意这一点。
    let report = leak::detect(&windowed, &[], now);

    println!("\n== 判定结果 ==");
    println!(
        "参与 {} · 命中 {} · 良性 {} · 数据不足 {} · 未达标 {}{}",
        report.observed,
        report.items.len(),
        report.benign,
        report.insufficient,
        report.below,
        report
            .note
            .as_ref()
            .map(|n| format!(" · 注意：{n}"))
            .unwrap_or_default()
    );

    if report.items.is_empty() {
        println!("→ 0 个疑似内存泄漏（这是正确结果，不是「没在检查」）");
    } else {
        println!("\n== 命中明细 ==");
        for it in &report.items {
            println!(
                "{} [{}] pid {} · 当前 {:.1} MB · {} min 增长 {:.1} MB（+{:.0}%）· 斜率 {:.1} MB/h · R² {:.3} · 阶梯占比 {:.0}%{}",
                it.name,
                it.family,
                it.pid,
                mb(it.footprint),
                it.window_secs / 60,
                mb(it.growth),
                it.growth_ratio * 100.0,
                it.slope_mb_per_hour,
                it.r2,
                it.step_ratio * 100.0,
                if it.gc_runtime { " · GC 语言（门槛翻倍）" } else { "" }
            );
        }
    }

    // 被拒的「像候选」的进程也要打出来：证明规则真的在看数据，
    // 而不是因为没数据才「0 命中」。
    println!("\n== 净增长 > 100 MB 但未命中的进程（逐条给原因）==");
    let mut shown = 0;
    for s in &windowed {
        let (first, last) = match (s.samples.first(), s.samples.last()) {
            (Some(f), Some(l)) => (f, l),
            _ => continue,
        };
        let growth = last.footprint.saturating_sub(first.footprint);
        if growth < 100 * 1024 * 1024 {
            continue;
        }
        let reason = match leak::analyze(s, &[], now) {
            Outcome::Leak(_) => "命中（见上）".to_string(),
            Outcome::Benign => "良性名单".to_string(),
            Outcome::Insufficient => "数据不足（点数/覆盖/断档/存续时长）".to_string(),
            Outcome::Below => {
                let max_step = s
                    .samples
                    .windows(2)
                    .map(|w| w[1].footprint.abs_diff(w[0].footprint))
                    .max()
                    .unwrap_or(0);
                let ratio = max_step as f64 / growth as f64;
                if ratio > leak::STEP_RATIO_MAX {
                    format!("阶梯排除（最大单点跃升占净增长 {:.0}%）", ratio * 100.0)
                } else {
                    "数值条件未达标（占比 / R² / 斜率）".to_string()
                }
            }
        };
        println!(
            "  {} [{}] pid {} · 增长 {:.1} MB（{:.1}→{:.1}）· {}",
            s.name,
            s.family,
            s.pid,
            mb(growth),
            mb(first.footprint),
            mb(last.footprint),
            reason
        );
        shown += 1;
        if shown >= 12 {
            println!("  …（只显示前 12 条）");
            break;
        }
    }
    if shown == 0 {
        println!("  （无）");
    }

    println!("\n== 预算校验 ==");
    println!("判定窗口 30 min（GC 60 min）· 断言：真实库应为 0 命中");
}
