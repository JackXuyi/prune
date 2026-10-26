//! B5 泄漏**嫌疑**判定（M3）。
//!
//! 方案见 `docs/m3-leak-plan.md`。三条硬约束先写在这里，改代码前先读：
//!
//! 1. **判定单元是单进程序列**，不是家族总量。真实库实测：Cursor 家族 6 h 从
//!    6.4 GB 涨到 19.0 GB，同期进程数 11 → 62 —— 涨的是深探覆盖，不是内存。
//! 2. **只看净增长会误报**。真实库里 30 min 增长 633 MB 的进程（pid 50392）是
//!    一次性阶跃（36.7 MB 平台 → 单点 686 MB），不是爬升。阶梯必须排除。
//! 3. **措辞硬约束**（功能方案 §6.3）：本模块只产出「嫌疑」与「依据」，
//!    任何 UI 文案都不允许说「你的 XX 泄漏了」，也不允许出现「释放/加速/优化」。

use serde::{Deserialize, Serialize};

use crate::mem::ProcMem;

/// 默认判定窗口：30 min
pub const WINDOW_SECS: i64 = 1800;
/// GC 语言（Node / Java / Go）窗口翻倍：60 min
pub const GC_WINDOW_SECS: i64 = 3600;
/// 默认净增长门槛：150 MB
pub const GROWTH_BYTES: u64 = 150 * 1024 * 1024;
/// GC 语言净增长门槛：300 MB
pub const GC_GROWTH_BYTES: u64 = 300 * 1024 * 1024;
/// 净增长 / 窗口起点值 的下限
pub const GROWTH_RATIO_MIN: f64 = 0.40;
/// 回归拟合优度下限
pub const R2_MIN: f64 = 0.70;
/// 阶梯排除：最大单点跃升 > 净增长 × 该比例 → 判为阶跃而非泄漏（实测得来，见方案 §3.4）
pub const STEP_RATIO_MAX: f64 = 0.60;
/// 窗口内最少点数（5 min 粒度下 30 min 应有 6~7 点）
pub const MIN_POINTS: usize = 6;
/// 窗口内允许的最大数据断档（超过说明 Prune 没在跑，序列不可信）
pub const MAX_GAP_SECS: i64 = 900;
/// 观测覆盖要求：窗口内首点距窗口起点的最大迟滞比例
pub const COVERAGE_SLACK: f64 = 0.20;

/// 序列里的一个采样点（字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub ts: i64,
    pub footprint: u64,
}

/// 单个进程的时间序列（按 `pid + start_time` 切分，pid 复用不得接续）
#[derive(Debug, Clone, PartialEq)]
pub struct ProcSeries {
    pub pid: i32,
    /// 进程启动时间；0 = 未知（此时退回用序列跨度判断存在时长）
    pub start_time: u64,
    pub family: String,
    pub name: String,
    /// 按 ts 升序
    pub samples: Vec<Sample>,
}

/// 一个命中进程的证据（**只放事实，不放结论性形容词**）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeakItem {
    pub pid: i32,
    pub start_time: u64,
    pub family: String,
    pub name: String,
    /// 窗口末值（footprint 口径）
    pub footprint: u64,
    /// 窗口净增长
    pub growth: u64,
    /// growth / 窗口起点值
    pub growth_ratio: f64,
    pub window_secs: i64,
    pub points: usize,
    /// 回归斜率（MB/小时，展示用）
    pub slope_mb_per_hour: f64,
    pub r2: f64,
    /// 最大单点跃升 / 净增长：证明「不是一次跳变」的透明度指标
    pub step_ratio: f64,
    pub gc_runtime: bool,
}

/// 判定结果汇总
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeakReport {
    /// 按 growth 降序
    pub items: Vec<LeakItem>,
    /// 参与判定的进程数
    pub observed: usize,
    /// 序列不够长（首次运行 / 刚启动的进程）
    pub insufficient: usize,
    /// 良性名单排除（浏览器 / VM / 系统进程 / 自身）
    pub benign: usize,
    /// 数值条件未达标
    pub below: usize,
    pub window_secs: i64,
    pub updated_at: Option<i64>,
    /// 无法判定的原因（如序列库不可用）。None = 判定正常运行
    pub note: Option<String>,
}

impl LeakReport {
    /// 无法判定时的显式状态。**不允许**用空 items 冒充「没有泄漏」。
    pub fn unavailable(note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            window_secs: WINDOW_SECS,
            ..Default::default()
        }
    }

    /// 命中进程涉及的家族数（面板一句话摘要用）
    pub fn family_count(&self) -> usize {
        let mut names: Vec<&str> = self.items.iter().map(|i| i.family.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names.len()
    }

    /// 命中进程的净增长合计（不是家族总量，只是这些进程加起来涨了多少）
    pub fn total_growth(&self) -> u64 {
        self.items.iter().map(|i| i.growth).sum()
    }
}

/// 单个进程的判定结论
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Leak(LeakItem),
    Insufficient,
    Benign,
    Below,
}

/// 最小二乘拟合结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// 字节 / 分钟
    pub slope_per_min: f64,
    pub r2: f64,
}

/// 对 `(x=分钟, y=字节)` 做一元线性回归。
///
/// - 完全平线（`SS_tot == 0`）时 `r2` 记 0：平线不是「拟合完美的泄漏」。
/// - 少于 2 点直接返回 0。
pub fn fit(points: &[(f64, f64)]) -> Fit {
    let n = points.len();
    if n < 2 {
        return Fit {
            slope_per_min: 0.0,
            r2: 0.0,
        };
    }
    let nf = n as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / nf;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / nf;

    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for (x, y) in points {
        sxx += (x - mean_x) * (x - mean_x);
        sxy += (x - mean_x) * (y - mean_y);
    }
    if sxx <= f64::EPSILON {
        return Fit {
            slope_per_min: 0.0,
            r2: 0.0,
        };
    }

    let slope = sxy / sxx;
    let intercept = mean_y - slope * mean_x;

    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (x, y) in points {
        let pred = intercept + slope * x;
        ss_res += (y - pred) * (y - pred);
        ss_tot += (y - mean_y) * (y - mean_y);
    }

    let r2 = if ss_tot <= f64::EPSILON {
        0.0
    } else {
        (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
    };

    Fit {
        slope_per_min: slope,
        r2,
    }
}

/// GC 语言识别（门槛翻倍的依据）。
///
/// 只看运行时进程名 —— Go 二进制编译后名字不可靠，这里尽力而为，
/// 认不出来就按普通进程判（门槛更低，可能多报，但没有危险）。
pub fn is_gc_runtime(name: &str, family: &str) -> bool {
    let hay = format!("{} {}", name.to_lowercase(), family.to_lowercase());
    ["node", "java", "jvm", "openjdk", "golang", "go-build"]
        .iter()
        .any(|k| hay.contains(k))
}

/// 良性名单里的家族（浏览器 / 通用 Electron / 容器 / 自身）。
///
/// 具名 Electron 应用（Cursor / VS Code / Slack）**不在此列**：
/// 它们是用户可行动对象（能重启验证），而 Chrome 的涨落由页面驱动、报了也不可行动。
/// 见方案 D-L2。
const BENIGN_FAMILIES: &[&str] = &[
    "chrome",
    "safari",
    "firefox",
    "arc",
    "electron",
    "docker",
    "prune",
];

/// 良性名单里的名字/路径关键词（虚拟机与容器运行时）
const BENIGN_TOKENS: &[&str] = &[
    "com.apple.webkit",
    "qemu",
    "virtualbox",
    "vmware",
    "parallels",
    "colima",
    "vpnkit",
];

/// 系统进程：这些路径下的进程不归用户处置
const SYSTEM_PREFIXES: &[&str] = &["/System/", "/System/Library/", "/usr/libexec/", "/usr/sbin/"];

/// 是否属于「不该报」的良性对象
pub fn is_benign(family: &str, name: &str, exe: Option<&str>) -> bool {
    let fam = family.to_lowercase();
    if BENIGN_FAMILIES.contains(&fam.as_str()) {
        return true;
    }
    // 家族名也要参与关键词匹配：qemu / colima 这类进程名可能就是家族名本身
    let hay = format!(
        "{} {} {}",
        fam,
        name.to_lowercase(),
        exe.unwrap_or("").to_lowercase()
    );
    if BENIGN_TOKENS.iter().any(|k| hay.contains(k)) {
        return true;
    }
    matches!(exe, Some(e) if SYSTEM_PREFIXES.iter().any(|p| e.starts_with(p)))
}

/// 判定单个进程。
///
/// `now` 为当前时间（Unix 秒）；`procs` 用于补 exe（良性名单需要），可传空切片。
pub fn analyze(series: &ProcSeries, procs: &[ProcMem], now: i64) -> Outcome {
    let exe = procs
        .iter()
        .find(|p| p.pid == series.pid && (series.start_time == 0 || p.start_time == series.start_time))
        .and_then(|p| p.exe.clone());
    let exe = exe.as_deref();

    // 良性判定放在最前面：这些进程即使序列完整也不该出现在结果里
    if is_benign(&series.family, &series.name, exe) {
        return Outcome::Benign;
    }

    let gc = is_gc_runtime(&series.name, &series.family);
    let window = if gc { GC_WINDOW_SECS } else { WINDOW_SECS };
    let window_start = now - window;

    // 条件 1：真实存在时长。start_time 未知时退回序列跨度。
    if series.start_time > 0 {
        if now - (series.start_time as i64) < window {
            return Outcome::Insufficient;
        }
    }

    // 条件 2/3：窗口内覆盖与连续性
    let windowed: Vec<&Sample> = series.samples.iter().filter(|s| s.ts >= window_start).collect();
    if windowed.len() < MIN_POINTS {
        return Outcome::Insufficient;
    }
    let first = windowed[0];
    let last = *windowed[windowed.len() - 1];
    if first.ts - window_start > (window as f64 * COVERAGE_SLACK) as i64 {
        return Outcome::Insufficient;
    }
    if series.start_time == 0 && last.ts - first.ts < (window as f64 * COVERAGE_SLACK) as i64 {
        return Outcome::Insufficient;
    }
    if windowed.windows(2).any(|w| w[1].ts - w[0].ts > MAX_GAP_SECS) {
        return Outcome::Insufficient;
    }

    // 条件 4/5：净增长绝对值与相对占比
    let Some(growth) = last.footprint.checked_sub(first.footprint) else {
        return Outcome::Below;
    };
    let floor = if gc { GC_GROWTH_BYTES } else { GROWTH_BYTES };
    if growth <= floor {
        return Outcome::Below;
    }
    let ratio = if first.footprint == 0 {
        1.0
    } else {
        growth as f64 / first.footprint as f64
    };
    if ratio <= GROWTH_RATIO_MIN {
        return Outcome::Below;
    }

    // 条件 6：持续爬升（斜率 > 0 且拟合优度达标）。x 用真实 ts，不能用点序号——
    // 真实库里同一轮会有 5~21 s 的补采点，按序号算会算歪。
    let pts: Vec<(f64, f64)> = windowed
        .iter()
        .map(|s| ((s.ts - window_start) as f64 / 60.0, s.footprint as f64))
        .collect();
    let fitted = fit(&pts);
    if fitted.slope_per_min <= 0.0 || fitted.r2 < R2_MIN {
        return Outcome::Below;
    }

    // 条件 7：阶梯排除。一次性加载 / 新开窗口是阶跃，不是泄漏。
    let max_step = windowed
        .windows(2)
        .map(|w| w[1].footprint.abs_diff(w[0].footprint))
        .max()
        .unwrap_or(0);
    let step_ratio = max_step as f64 / growth as f64;
    if step_ratio > STEP_RATIO_MAX {
        return Outcome::Below;
    }

    Outcome::Leak(LeakItem {
        pid: series.pid,
        start_time: series.start_time,
        family: series.family.clone(),
        name: series.name.clone(),
        footprint: last.footprint,
        growth,
        growth_ratio: ratio,
        window_secs: window,
        points: windowed.len(),
        slope_mb_per_hour: fitted.slope_per_min * 60.0 / 1_048_576.0,
        r2: fitted.r2,
        step_ratio,
        gc_runtime: gc,
    })
}

/// 批量判定。`series` 应已按 `pid + start_time` 切分好（见 `db::series_since`）。
pub fn detect(series: &[ProcSeries], procs: &[ProcMem], now: i64) -> LeakReport {
    let mut report = LeakReport {
        window_secs: WINDOW_SECS,
        updated_at: Some(now),
        ..Default::default()
    };
    report.observed = series.len();

    for s in series {
        match analyze(s, procs, now) {
            Outcome::Leak(item) => report.items.push(item),
            Outcome::Insufficient => report.insufficient += 1,
            Outcome::Benign => report.benign += 1,
            Outcome::Below => report.below += 1,
        }
    }

    report.items.sort_by(|a, b| b.growth.cmp(&a.growth));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemSource;

    fn mb_u64(v: u64) -> u64 {
        v * 1024 * 1024
    }

    fn proc(pid: i32, family: &str, exe: Option<&str>, start_time: u64) -> ProcMem {
        ProcMem {
            pid,
            ppid: 1,
            name: format!("p{pid}"),
            exe: exe.map(|s| s.to_string()),
            family: family.into(),
            rss: 10,
            footprint: Some(10),
            swapped: None,
            source: MemSource::Footprint,
            start_time,
            cmd: Vec::new(),
            workspace: None,
        }
    }

    /// 构造 series：起点 ts0、每 step_s 一点、共 n 点，footprint 由闭包给
    fn series(
        pid: i32,
        family: &str,
        ts0: i64,
        step_s: i64,
        n: usize,
        f: impl Fn(usize) -> u64,
    ) -> ProcSeries {
        ProcSeries {
            pid,
            start_time: (ts0 - 7200) as u64, // 早就启动了
            family: family.into(),
            name: format!("p{pid}"),
            samples: (0..n)
                .map(|i| Sample {
                    ts: ts0 + step_s * i as i64,
                    footprint: f(i),
                })
                .collect(),
        }
    }

    const NOW: i64 = 1_800_000_000;
    const TS0: i64 = NOW - 1800; // 窗口起点

    /// 教科书式泄漏：每 5 min 稳定 +30 MB，30 min 共 +180 MB
    #[test]
    fn steady_climb_is_flagged() {
        let s = series(1, "Cursor", TS0, 300, 7, |i| mb_u64(400 + 30 * i as u64));
        match analyze(&s, &[], NOW) {
            Outcome::Leak(item) => {
                assert_eq!(item.growth, mb_u64(180));
                assert!((item.growth_ratio - 0.45).abs() < 1e-9);
                assert!(item.r2 > 0.99, "直线拟合优度必须接近 1，实得 {}", item.r2);
                assert!(item.step_ratio < 0.2);
                assert!(!item.gc_runtime);
            }
            o => panic!("稳定爬升必须命中，实得 {o:?}"),
        }
    }

    /// 真实库里的形态：36.7 MB 平台 9 点 → 单点跳到 686 MB → 平台。
    /// 30 min 净增长远超门槛，但必须被拒绝。
    #[test]
    fn step_jump_is_rejected_like_real_db() {
        let mut s = series(50392, "Cursor", TS0, 300, 7, |i| {
            if i < 2 {
                mb_u64(37)
            } else {
                mb_u64(686)
            }
        });
        s.start_time = (TS0 - 100) as u64; // 存在超过窗口
        match analyze(&s, &[], NOW) {
            Outcome::Below => {}
            o => panic!("阶跃不得判为泄漏，实得 {o:?}"),
        }
    }

    /// 阶梯主导增长：R² 仍能过 0.7（因为后续也在缓慢爬），必须靠阶梯规则挡下来。
    /// 这条是规则 7 的独立存在理由——没有它，形态就是「打开一个文件」而不是泄漏。
    #[test]
    fn step_dominant_growth_is_rejected_even_with_good_fit() {
        let s = series(60, "Cursor", TS0, 300, 7, |i| {
            mb_u64(match i {
                0 => 400,
                1 => 1400,
                _ => 1400 + 100 * (i as u64 - 1),
            })
        });
        // 先证明拟合本身「像泄漏」：否则这条测试证明不了阶梯规则的必要性
        let pts: Vec<(f64, f64)> = s
            .samples
            .iter()
            .map(|p| ((p.ts - TS0) as f64 / 60.0, p.footprint as f64))
            .collect();
        assert!(fit(&pts).r2 > R2_MIN, "该形态 R² 应达标，实得 {}", fit(&pts).r2);

        match analyze(&s, &[], NOW) {
            Outcome::Below => {}
            o => panic!("阶梯主导增长必须拒绝，实得 {o:?}"),
        }

        // 同样 1500 MB 的增长，但没有阶跃 → 命中（证明差异只来自阶梯）
        let smooth = series(61, "Cursor", TS0, 300, 7, |i| mb_u64(400 + 250 * i as u64));
        assert!(matches!(analyze(&smooth, &[], NOW), Outcome::Leak(_)));
    }

    /// 锯齿：绝对值与占比都过线，但来回抖 → R² 不达标
    #[test]
    fn sawtooth_is_rejected() {
        // 基准 300 + 60/min 稳定爬升，叠加 ±150 MB 抖动
        let s = series(2, "Cursor", TS0, 300, 7, |i| {
            let base = 300 + 60 * i as u64;
            mb_u64(if i % 2 == 0 { base + 150 } else { base - 100 })
        });
        match analyze(&s, &[], NOW) {
            Outcome::Below => {}
            o => panic!("锯齿不得判为泄漏，实得 {o:?}"),
        }
    }

    /// 平台期：完全不涨
    #[test]
    fn flat_is_rejected() {
        let s = series(3, "Cursor", TS0, 300, 7, |_| mb_u64(800));
        match analyze(&s, &[], NOW) {
            Outcome::Below => {}
            o => panic!("平线不得判为泄漏，实得 {o:?}"),
        }
    }

    /// 大进程慢涨：绝对值够但占比不足（真实库 pid 674 形态）
    #[test]
    fn big_process_slow_growth_is_rejected() {
        // 3 GB 起，30 min 涨 160 MB（5.3%）
        let s = series(4, "Cursor", TS0, 300, 7, |i| {
            mb_u64(3000 + (160 / 6) * i as u64)
        });
        match analyze(&s, &[], NOW) {
            Outcome::Below => {}
            o => panic!("占比不足不得判为泄漏，实得 {o:?}"),
        }
    }

    /// 点数不足 / 刚启动：都归入 insufficient，绝不产出 item
    #[test]
    fn insufficient_series_never_produce_items() {
        // 只有 3 个点
        let short = series(5, "Cursor", NOW - 900, 300, 3, |i| mb_u64(100 + 100 * i as u64));
        assert_eq!(analyze(&short, &[], NOW), Outcome::Insufficient);

        // 序列够长，但进程是 10 min 前才启动的
        let mut young = series(6, "Cursor", TS0, 300, 7, |i| mb_u64(100 + 100 * i as u64));
        young.start_time = (NOW - 600) as u64;
        assert_eq!(analyze(&young, &[], NOW), Outcome::Insufficient);
    }

    /// 窗口内有 > 15 min 断档（Prune 没在跑）→ 不可信
    #[test]
    fn data_gap_invalidates_window() {
        let s = ProcSeries {
            pid: 7,
            start_time: (TS0 - 7200) as u64,
            family: "Cursor".into(),
            name: "p7".into(),
            samples: vec![
                Sample { ts: TS0, footprint: mb_u64(100) },
                Sample { ts: TS0 + 300, footprint: mb_u64(130) },
                Sample { ts: TS0 + 600, footprint: mb_u64(160) },
                // 断档 20 min
                Sample { ts: TS0 + 1800, footprint: mb_u64(400) },
                Sample { ts: TS0 + 1860, footprint: mb_u64(410) },
                Sample { ts: TS0 + 1920, footprint: mb_u64(420) },
            ],
        };
        assert_eq!(analyze(&s, &[], NOW), Outcome::Insufficient);
    }

    /// 良性名单：浏览器 / 通用 Electron / 容器 / 系统进程 / 自身全部零命中
    #[test]
    fn benign_objects_are_never_flagged() {
        let climb = |pid: i32, family: &str, exe: Option<&str>| {
            let s = series(pid, family, TS0, 300, 7, |i| mb_u64(400 + 40 * i as u64));
            let procs = vec![proc(pid, family, exe, s.start_time)];
            analyze(&s, &procs, NOW)
        };
        for (pid, family, exe) in [
            (10, "Chrome", Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")),
            (11, "Electron", None),
            (12, "Docker", None),
            (13, "Prune", Some("/Applications/Prune.app/Contents/MacOS/prune-app")),
            (14, "SomeApp", Some("/System/Library/Frameworks/Foo.framework/foo")),
            (15, "qemu-system-aarch64", None),
        ] {
            assert_eq!(climb(pid, family, exe), Outcome::Benign, "{family} 应被判为良性");
        }
    }

    /// 具名 Electron 应用不放过（D-L2）：Cursor 的进程必须能被判
    #[test]
    fn named_electron_apps_are_still_judged() {
        let s = series(20, "Cursor", TS0, 300, 7, |i| mb_u64(400 + 40 * i as u64));
        let procs = vec![proc(
            20,
            "Cursor",
            Some(
                "/Applications/Cursor.app/Contents/Frameworks/Cursor Helper.app/Contents/MacOS/Cursor Helper",
            ),
            s.start_time,
        )];
        assert!(matches!(analyze(&s, &procs, NOW), Outcome::Leak(_)));
    }

    /// GC 语言门槛翻倍：Node 进程 30 min +200 MB 不命中（需 300 MB / 60 min）
    #[test]
    fn gc_runtime_needs_double_threshold() {
        // 30 min +200 MB，对普通进程够，对 Node 不够
        let node_like = series(30, "Node", TS0, 300, 7, |i| mb_u64(400 + (200 / 6) * i as u64));
        assert_eq!(analyze(&node_like, &[], NOW), Outcome::Insufficient, "GC 窗口是 60 min，30 min 数据不足");

        // 普通进程同样数据 → 命中
        let plain = series(31, "Cursor", TS0, 300, 7, |i| mb_u64(400 + (200 / 6) * i as u64));
        assert!(matches!(analyze(&plain, &[], NOW), Outcome::Leak(_)));

        // Node 在 60 min 窗口 +350 MB → 命中
        let node_long = series(32, "Node", NOW - 3600, 300, 13, |i| mb_u64(400 + 30 * i as u64));
        match analyze(&node_long, &[], NOW) {
            Outcome::Leak(item) => {
                assert!(item.gc_runtime);
                assert_eq!(item.window_secs, GC_WINDOW_SECS);
            }
            o => panic!("GC 语言 60 min +360 MB 应命中，实得 {o:?}"),
        }
    }

    /// pid 复用：同 pid 不同 start_time 由调用方切成两条序列，判定不会把两条接起来
    #[test]
    fn reused_pid_series_are_judged_independently() {
        let old = series(40, "Cursor", TS0, 300, 7, |i| mb_u64(400 + 40 * i as u64));
        let mut new = series(40, "Cursor", NOW - 600, 300, 3, |i| mb_u64(900 + 50 * i as u64));
        new.start_time = (NOW - 600) as u64;
        assert!(matches!(analyze(&old, &[], NOW), Outcome::Leak(_)));
        assert_eq!(analyze(&new, &[], NOW), Outcome::Insufficient);
    }

    /// 汇总：计数分类正确、items 按增长降序
    #[test]
    fn report_counts_and_orders_items() {
        let all = vec![
            series(50, "Cursor", TS0, 300, 7, |i| mb_u64(400 + 40 * i as u64)),
            series(51, "Slack", TS0, 300, 7, |i| mb_u64(200 + 90 * i as u64)),
            series(52, "Chrome", TS0, 300, 7, |i| mb_u64(400 + 40 * i as u64)),
            series(53, "Cursor", NOW - 600, 300, 3, |i| mb_u64(10 * i as u64)),
            series(54, "Cursor", TS0, 300, 7, |_| mb_u64(800)),
        ];
        let report = detect(&all, &[], NOW);
        assert_eq!(report.observed, 5);
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.benign, 1, "Chrome 进良性");
        assert_eq!(report.insufficient, 1, "刚启动的进程数据不足");
        assert_eq!(report.below, 1, "平线未达标");
        assert_eq!(report.items[0].pid, 51, "Slack 增长更多，排第一");
        assert_eq!(report.family_count(), 2);
        assert_eq!(report.total_growth(), mb_u64(540) + mb_u64(240));
    }

    /// 无库时必须是「无法判定」，不能是空 items 冒充「没有泄漏」
    #[test]
    fn unavailable_report_says_why() {
        let r = LeakReport::unavailable("序列库不可用");
        assert!(r.note.is_some());
        assert!(r.items.is_empty());
        assert!(r.updated_at.is_none());
    }

    #[test]
    fn fit_rejects_flat_and_short() {
        assert_eq!(fit(&[(0.0, 100.0)]).r2, 0.0);
        // 平线：SS_tot = 0，r2 记 0 而不是 1
        let flat = fit(&[(0.0, 5.0), (1.0, 5.0), (2.0, 5.0)]);
        assert_eq!(flat.r2, 0.0);
        assert_eq!(flat.slope_per_min, 0.0);
    }
}
