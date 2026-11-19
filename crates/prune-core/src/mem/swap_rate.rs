//! swap 增速量化（F24，docs/f24-swap-rate-plan.md）。
//!
//! 回答三个问题：现在挤了多少（明细归 `swap::collect`，不在本模块）、
//! **谁在加速挤**（按进程增速榜）、系统整体换出净速率（双口径）。
//!
//! 增速全部来自**既有序列差分**（`mem_process` 表的 7 天 `swapped` 序列）：
//! 零新增采集、零新表。窗口内每进程取**首末两个样本**差分 ÷ 实际覆盖时长 = 速率。
//!
//! 口径纪律（f24 §5）：
//! - **不夹 0、不隐藏负值**：负差分 = 发生了 swap-in 或换出被释放，是信息不是脏数据；
//! - **pid 复用剔除**：首末样本 `start_time` 不一致 = 差分跨了两个进程实例，
//!   不进增速也不进聚合（宁缺勿错）；
//! - **样本 < 2 不产增速**：一个点算不出变化，说「数据不足」而不是 0；
//! - **空窗如实标注**：Prune 没跑的时段不能让读者把「没数据」当「没换出」
//!   （复用 F20 的 gap 判据：相邻采样间隔 > 3 × 周期中位数）。

use crate::db::SwapRateRow;
use serde::{Deserialize, Serialize};

const MB: f64 = 1_048_576.0;

/// 按进程的换出增速一行（窗口首末样本差分）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcSwapRate {
    pub pid: i32,
    pub name: String,
    pub family: String,
    /// 窗口内首个样本的换出量（字节）
    pub first_swapped: u64,
    /// 末个样本的换出量（字节）
    pub last_swapped: u64,
    /// 末 − 首。负 = 换回内存 / 退出释放，不夹 0
    pub delta_bytes: i64,
    /// 平均增速（MB/h），按窗口内实际覆盖时长折算（进程中途启动也能比）
    pub rate_mb_h: f64,
    /// 窗口内样本数；< 2 的进程不进本表
    pub samples: u32,
    /// 首末样本 start_time 一致（同一次进程实例）。false 的行不该存在——
    /// 核心层直接丢弃 pid 复用的差分；字段留给序列化形状的诚实性
    pub start_consistent: bool,
    /// 窗口内首个样本时刻（增速的覆盖起点）
    pub first_ts: i64,
    /// 窗口内末个样本时刻（增速的覆盖终点；早于窗口末端 = 进程已退出）
    pub last_ts: i64,
}

/// 系统级净换出速率（f24 DP-1 双口径，近似口径——由进程差分聚合）。
///
/// 偏差来源：进程退出把它名下的 swapped 从下一轮快照清零，表现为负差分。
/// 所以拆两个数字，谁也别冒充谁。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSwapRate {
    /// **含退出释放**：全部可差分进程的 delta 之和
    pub sum_delta_bytes: i64,
    /// **仅存活**：窗口两端都被观测到的进程（同一次实例）的 delta 之和
    pub active_delta_bytes: i64,
    /// sum 口径平均速率（MB/h），按窗口实际覆盖时长折算
    pub rate_mb_h: f64,
    /// active 口径平均速率（MB/h）
    pub active_rate_mb_h: f64,
    /// 计入聚合的进程数（start_consistent 且样本 ≥ 2）
    pub processes: usize,
}

/// 窗口覆盖情况（F20 gap 语义复用）：Prune 没跑的时段如实标注。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCoverage {
    /// 请求窗口起点（now − hours）
    pub requested_from: i64,
    /// 实际最早样本时刻；0 = 窗口内一个样本都没有
    pub actual_from: i64,
    /// 实际最晚样本时刻；0 = 同上
    pub actual_to: i64,
    /// 采样空窗 [(start, end)]：判据与 F20 相同（> 3 × 周期中位数）
    pub gaps: Vec<(i64, i64)>,
    /// 窗口内 pid 被复用而丢弃差分的进程数（这些进程的增速「宁缺勿错」）
    pub pid_reused: usize,
    /// 平台口径缺口的显式说明（Windows 无 per-process swapped → 调用方注入）
    pub note: Option<String>,
}

/// 一次增速报告的完整产出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRateReport {
    pub window_hours: u32,
    /// 按 delta_bytes 降序（负值垫底）。JSON 全量输出，榜单截断由展示层决定
    pub rates: Vec<ProcSwapRate>,
    pub system: SystemSwapRate,
    /// 近 1h 短窗（「当下趋势」）；None = 1h 内凑不出一对样本
    pub recent_1h: Option<SystemSwapRate>,
    pub coverage: SwapCoverage,
}

/// 主入口：原始行 → 增速报告。纯函数。
///
/// `rows` 需来自 `swap_rate_rows`（内部不依赖输入有序性，按 pid 分组后自行
/// 按 ts 排序）。空输入返回全空报告（调用方决定「暂无数据」的呈现，
/// 这里不撒「0 增速」的谎）。
pub fn report(
    rows: &[SwapRateRow],
    now: i64,
    window_hours: u32,
    note: Option<String>,
) -> SwapRateReport {
    let requested_from = now - (window_hours.max(1) as i64) * 3600;
    let agg = aggregate(rows, requested_from);

    let ts: Vec<i64> = distinct_ts(rows, requested_from);
    let (actual_from, actual_to) = match (ts.first(), ts.last()) {
        (Some(&f), Some(&t)) => (f, t),
        _ => (0, 0),
    };
    // 「存活至今」的容差：末样本落在最后一个采样周期内（≈ 1 × 周期中位数）
    // 就算在最后一次观测时还活着。3 × 中位数会把中途退出的进程也吞成存活。
    let grace = median_interval(&ts);

    let system = agg.system(actual_from, actual_to, grace, ts.len());
    let mut rates = agg.rates;
    rates.sort_by_key(|r| std::cmp::Reverse(r.delta_bytes));

    // 近 1h 短窗：同一套聚合，窗口收紧到 3600s。覆盖区间用短窗自己的
    // 实际边界（中途出现的进程也算存活贡献），零样本 → None（不是 0 增速）
    let recent = aggregate(rows, now - 3600);
    let recent_ts = distinct_ts(rows, now - 3600);
    let recent_1h = match (recent_ts.first(), recent_ts.last()) {
        (Some(&f), Some(&t)) if recent.aggregable > 0 => {
            let recent_grace = median_interval(&recent_ts);
            Some(recent.system(f, t, recent_grace, recent_ts.len()))
        }
        _ => None,
    };

    SwapRateReport {
        window_hours,
        rates,
        system,
        recent_1h,
        coverage: SwapCoverage {
            requested_from,
            actual_from,
            actual_to,
            gaps: detect_gaps(&ts),
            pid_reused: agg.pid_reused,
            note,
        },
    }
}

/// 聚合产物（rates + 计数中间态）
struct Agg {
    rates: Vec<ProcSwapRate>,
    pid_reused: usize,
    aggregable: usize,
}

impl Agg {
    fn system(
        &self,
        actual_from: i64,
        actual_to: i64,
        grace: i64,
        ts_count: usize,
    ) -> SystemSwapRate {
        let sum_delta: i64 = self.rates.iter().map(|r| r.delta_bytes).sum();
        let duration_h = duration_hours(actual_from, actual_to, ts_count);
        // active 口径：只算窗口两端都被观测到的存活进程
        let active: Vec<&ProcSwapRate> = self
            .rates
            .iter()
            .filter(|r| is_active(r, actual_from, actual_to, grace))
            .collect();
        let active_delta: i64 = active.iter().map(|r| r.delta_bytes).sum();
        // 速率分母用「存活进程的实际覆盖时长之和」：进程中途启动也能按各自
        // 覆盖期折算，不拿整窗时长稀释中途启动者的速率
        let active_h: f64 = active
            .iter()
            .map(|r| (r.last_ts - r.first_ts).max(1) as f64 / 3600.0)
            .sum();
        SystemSwapRate {
            sum_delta_bytes: sum_delta,
            active_delta_bytes: active_delta,
            rate_mb_h: sum_delta as f64 / MB / duration_h,
            active_rate_mb_h: active_delta as f64 / MB / active_h.max(1e-9),
            processes: self.aggregable,
        }
    }
}

/// 存活判据（f24 DP-1）：窗口两端都被观测到，即「首样本贴近覆盖起点、
/// 末样本贴近覆盖终点」。末样本早于终点 − 一个采样周期的进程视为中途退出，
/// 它的负差分（退出释放）只进 sum 口径。
fn is_active(r: &ProcSwapRate, actual_from: i64, actual_to: i64, grace: i64) -> bool {
    r.first_ts <= actual_from + grace && r.last_ts + grace >= actual_to
}

/// 系统级 sum 口径的分母：按**实际覆盖**时长折算（Prune 刚装/中途没跑时
/// 不拿请求窗口稀释速率；覆盖区间缺口由 coverage 如实标注）
fn duration_hours(actual_from: i64, actual_to: i64, ts_count: usize) -> f64 {
    if ts_count < 2 {
        return 1.0;
    }
    ((actual_to - actual_from).max(1) as f64 / 3600.0).max(1.0 / 3600.0)
}

/// 按 pid 分组 → 首末差分。样本 < 2 或 pid 复用的组不产增速。
fn aggregate(rows: &[SwapRateRow], requested_from: i64) -> Agg {
    // pid → 组内行（先收集再排序，不依赖输入有序性）
    let mut by_pid: std::collections::BTreeMap<i32, Vec<&SwapRateRow>> =
        std::collections::BTreeMap::new();
    for r in rows.iter().filter(|r| r.ts >= requested_from) {
        by_pid.entry(r.pid).or_default().push(r);
    }

    let mut rates = Vec::new();
    let mut pid_reused = 0usize;
    let mut aggregable = 0usize;

    for (_, mut group) in by_pid {
        group.sort_by_key(|r| r.ts);
        let first = group[0];
        let last = *group.last().unwrap();
        // pid 复用：首末是两个不同的进程实例，差分无意义 → 丢弃并计数
        if first.start_time != last.start_time {
            pid_reused += 1;
            continue;
        }
        let samples = group.len() as u32;
        // 一个点算不出变化：宁缺勿错
        if samples < 2 {
            continue;
        }
        let delta = last.swapped as i64 - first.swapped as i64;
        let hours = ((last.ts - first.ts).max(1) as f64) / 3600.0;
        rates.push(ProcSwapRate {
            pid: last.pid,
            name: last.name.clone(),
            family: last.family.clone(),
            first_swapped: first.swapped,
            last_swapped: last.swapped,
            delta_bytes: delta,
            rate_mb_h: delta as f64 / MB / hours,
            samples,
            start_consistent: true,
            first_ts: first.ts,
            last_ts: last.ts,
        });
        aggregable += 1;
    }

    Agg {
        rates,
        pid_reused,
        aggregable,
    }
}

/// 采样空窗检测（F20 同款判据）：相邻采样间隔 > 3 × 周期中位数记一段。
fn detect_gaps(ts: &[i64]) -> Vec<(i64, i64)> {
    if ts.len() < 3 {
        return Vec::new();
    }
    let median = median_interval(ts);
    let threshold = median * 3;
    ts.windows(2)
        .filter(|w| w[1] - w[0] > threshold)
        .map(|w| (w[0], w[1]))
        .collect()
}

/// 周期中位数（相邻采样间隔）。样本 < 3 个时返回 0 → 存活判据退化为
/// 「两端精确贴住」，宁可错杀活跃也不放过退出释放。
fn median_interval(ts: &[i64]) -> i64 {
    if ts.len() < 3 {
        return 0;
    }
    let mut intervals: Vec<i64> = ts.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_unstable();
    intervals[intervals.len() / 2].max(1)
}

/// 过滤窗口后的去重采样时刻（升序）
fn distinct_ts(rows: &[SwapRateRow], requested_from: i64) -> Vec<i64> {
    let mut v: Vec<i64> = rows
        .iter()
        .filter(|r| r.ts >= requested_from)
        .map(|r| r.ts)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts: i64, pid: i32, start: u64, swapped: u64) -> SwapRateRow {
        SwapRateRow {
            ts,
            pid,
            start_time: start,
            name: format!("p{pid}"),
            family: "app".into(),
            swapped,
        }
    }

    /// 一小时翻倍：pid 1 从 100MB 涨到 200MB → Δ +100MB、速率 100 MB/h
    #[test]
    fn delta_and_rate_basic() {
        let rows = vec![
            row(0, 1, 100, 100 * 1_048_576),
            row(3600, 1, 100, 200 * 1_048_576),
        ];
        let r = report(&rows, 7200, 24, None);
        assert_eq!(r.rates.len(), 1);
        let p = &r.rates[0];
        assert_eq!(p.delta_bytes, 100 * 1_048_576);
        assert!((p.rate_mb_h - 100.0).abs() < 1e-6);
        assert_eq!(p.samples, 2);
        assert!(p.start_consistent);
    }

    /// pid 复用：同 pid 两个 start_time → 差分丢弃、coverage 计数，绝不算增速
    #[test]
    fn pid_reuse_is_dropped_and_counted() {
        let rows = vec![
            row(0, 7, 100, 10 * MB as u64),
            row(3600, 7, 100, 20 * MB as u64),
            row(7200, 7, 200, 5 * MB as u64), // 旧进程死了，新进程复用 pid 7
        ];
        let r = report(&rows, 7200, 24, None);
        assert!(r.rates.is_empty(), "跨实例差分必须整组丢弃");
        assert_eq!(r.coverage.pid_reused, 1);
        assert_eq!(r.system.processes, 0);
        assert_eq!(r.system.sum_delta_bytes, 0);
    }

    /// 负差分（swap-in / 释放）如实保留，不夹 0
    #[test]
    fn negative_delta_kept_not_clamped() {
        let rows = vec![
            row(0, 1, 100, 300 * MB as u64),
            row(3600, 1, 100, 100 * MB as u64),
        ];
        let r = report(&rows, 7200, 24, None);
        assert_eq!(r.rates[0].delta_bytes, -200 * 1_048_576);
        assert!(r.rates[0].rate_mb_h < 0.0);
        assert!(r.system.sum_delta_bytes < 0);
    }

    /// 退出释放：中途死掉的进程，负差分进 sum 口径、不进 active 口径
    #[test]
    fn exit_release_shows_in_sum_not_active() {
        let mb = MB as u64;
        let rows = vec![
            // A：窗口起点有样本，600s 后死亡（换出被释放 → 负差分）
            row(0, 1, 100, 200 * mb),
            row(600, 1, 100, 100 * mb),
            // B：活满全窗
            row(0, 2, 100, 10 * mb),
            row(3600, 2, 100, 30 * mb),
            row(7200, 2, 100, 50 * mb),
        ];
        let r = report(&rows, 7200, 24, None);
        // sum 口径含 A 的 −100MB
        assert_eq!(r.system.sum_delta_bytes, (-100 + 40) * mb as i64);
        // active 口径只含 B 的 +40MB
        assert_eq!(r.system.active_delta_bytes, 40 * mb as i64);
    }

    /// 样本 < 2 的进程不产增速（一个点算不出变化）
    #[test]
    fn samples_below_two_yield_no_rate() {
        let rows = vec![
            row(0, 1, 100, 10 * MB as u64),
            row(3600, 1, 100, 20 * MB as u64),
            row(7200, 2, 100, 99 * MB as u64), // 只出现一次
        ];
        let r = report(&rows, 7200, 24, None);
        assert_eq!(r.rates.len(), 1);
        assert_eq!(r.rates[0].pid, 1);
    }

    /// 空窗如实标注：中间停机 > 3 × 周期中位数 → 一段 gap
    #[test]
    fn gaps_detected_like_f20() {
        let mut rows = Vec::new();
        // 周期 100s × 10 个样本
        for i in 0..10i64 {
            rows.push(row(1000 + i * 100, 1, 100, 10));
        }
        // 停机 1000s（> 3 × 100）
        for i in 0..10i64 {
            rows.push(row(5000 + i * 100, 1, 100, 10));
        }
        let r = report(&rows, 10_000, 24, None);
        assert_eq!(r.coverage.gaps.len(), 1);
        assert_eq!(r.coverage.gaps[0], (1900, 5000));
    }

    /// 近 1h 短窗：样本足够才给数，不足是 None 不是 0
    #[test]
    fn recent_1h_none_when_sparse() {
        let mb = MB as u64;
        let rows = vec![row(0, 1, 100, 10 * mb), row(3600, 1, 100, 20 * mb)];
        let r = report(&rows, 7200, 24, None); // 1h 窗（≥3600）内只有 ts 3600 一个样本
        assert_eq!(r.recent_1h, None);

        let rows2 = vec![
            row(0, 1, 100, 10 * mb),
            row(3700, 1, 100, 10 * mb),
            row(6900, 1, 100, 40 * mb),
        ];
        let r2 = report(&rows2, 7200, 24, None);
        let recent = r2.recent_1h.expect("1h 内有两个样本应产出");
        assert_eq!(recent.sum_delta_bytes, 30 * mb as i64);
    }

    /// 榜单按 delta 降序，负值垫底
    #[test]
    fn rates_sorted_desc_negative_last() {
        let mb = MB as u64;
        let rows = vec![
            row(0, 1, 100, 10 * mb),
            row(3600, 1, 100, 50 * mb), // +40
            row(0, 2, 100, 30 * mb),
            row(3600, 2, 100, 40 * mb), // +10
            row(0, 3, 100, 90 * mb),
            row(3600, 3, 100, 70 * mb), // −20
        ];
        let r = report(&rows, 7200, 24, None);
        let deltas: Vec<i64> = r.rates.iter().map(|p| p.delta_bytes).collect();
        assert_eq!(
            deltas,
            vec![40 * mb as i64, 10 * mb as i64, -20 * mb as i64]
        );
    }

    /// 空输入 = 全空报告：不撒「0 增速」的谎
    #[test]
    fn empty_rows_yield_empty_report() {
        let r = report(&[], 7200, 24, None);
        assert!(r.rates.is_empty());
        assert_eq!(r.system.processes, 0);
        assert_eq!(r.coverage.actual_from, 0);
        assert_eq!(r.recent_1h, None);
    }

    /// 窗口过滤：窗口前的旧样本不参与差分
    #[test]
    fn window_filters_old_samples() {
        let mb = MB as u64;
        let rows = vec![
            row(0, 1, 100, 10 * mb),
            row(3500, 1, 100, 20 * mb),
            row(7200, 1, 100, 30 * mb),
        ];
        // 收紧窗口到 1h：只有 ts 7200 一个样本 → 无增速
        let r = report(&rows, 7200, 1, None);
        assert!(r.rates.is_empty());
        let r24 = report(&rows, 7200, 24, None);
        assert_eq!(r24.rates[0].delta_bytes, 20 * mb as i64);
    }
}
