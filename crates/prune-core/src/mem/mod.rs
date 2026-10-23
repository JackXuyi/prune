//! 内存口径层（功能方案模块 B1~B4）。
//!
//! 设计要点（见 docs/m2-memory-plan.md）：
//! - **双口径不混用**：`Rss`（廉价筛选）与 `Footprint`（含换出量）必须由 `MemSource`
//!   显式标注，UI 侧据此显示口径徽标，绝不静默混用。
//! - **只做读数**：本模块不提供任何「释放 / 加速 / 优化内存」动作。
//! - 所有金额单位为**字节**，时间戳为 Unix 秒。

pub mod footprint;
pub mod process;
pub mod system;

use serde::{Deserialize, Serialize};

use crate::swap::SwapInfo;

/// 读数来源。降级链命中时整表退回 `Rss`，由 UI 明确标注。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemSource {
    /// `footprint -p <pid> --swapped -j` 的 phys_footprint（含被换出部分）
    Footprint,
    /// sysinfo 的 RSS（不含换出量，仅作筛选与降级口径）
    Rss,
}

/// 内存压力等级（D-M3：语义来自 `kern.memorystatus_vm_pressure_level`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PressureLevel {
    Normal,
    Warning,
    Critical,
}

/// 压力等级 + 原始 sysctl 值（T-210 与活动监视器校准用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pressure {
    pub level: PressureLevel,
    /// 原始 level 值：1=normal / 2=warning / 4=critical（0 或未知按 Normal 处理）
    pub raw: i32,
}

impl Pressure {
    pub fn unknown() -> Self {
        Self {
            level: PressureLevel::Normal,
            raw: 0,
        }
    }
}

/// 换页颠簸速率（每秒），由相邻两次采样的累计值差分得到
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thrash {
    pub swapins_per_sec: f64,
    pub swapouts_per_sec: f64,
}

/// 单个进程的内存读数（B1）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcMem {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    pub exe: Option<String>,
    /// 家族归类结果（B3）
    pub family: String,
    /// RSS：物理常驻集，不含换出量
    pub rss: u64,
    /// phys_footprint；None = 未深探或该进程深探失败
    pub footprint: Option<u64>,
    /// footprint categories[*].swapped 求和；None 同上
    pub swapped: Option<u64>,
    pub source: MemSource,
    /// 进程启动时间（秒），用于 pid 复用校验
    pub start_time: u64,
}

impl ProcMem {
    /// 换出占比 = swapped / footprint（0~1）。无深探数据时返回 None。
    pub fn swapped_ratio(&self) -> Option<f64> {
        match (self.footprint, self.swapped) {
            (Some(fp), Some(sw)) if fp > 0 => Some(sw as f64 / fp as f64),
            _ => None,
        }
    }

    /// 用于排序/展示的总量：有 footprint 用 footprint，否则退回 RSS
    pub fn total(&self) -> u64 {
        self.footprint.unwrap_or(self.rss)
    }
}

/// 家族聚合行（B3：Cursor 67 个进程 → 1 行）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyMem {
    pub name: String,
    pub process_count: usize,
    /// 已完成 footprint 深探的子进程数。`< process_count` 时总量为**下界估算**
    /// （未深探部分按 RSS 计入，RSS ⊆ footprint，所以只会偏低不会偏高）
    pub deep_covered: usize,
    pub rss: u64,
    /// footprint 口径合计：已深探子进程用 footprint，未深探用 RSS
    pub footprint: Option<u64>,
    pub swapped: Option<u64>,
}

impl FamilyMem {
    pub fn total(&self) -> u64 {
        self.footprint.unwrap_or(self.rss)
    }

    /// 是否全部子进程都已深探（总量为精确值）
    pub fn is_exact(&self) -> bool {
        self.footprint.is_some() && self.deep_covered == self.process_count
    }

    pub fn swapped_ratio(&self) -> Option<f64> {
        match (self.footprint, self.swapped) {
            (Some(fp), Some(sw)) if fp > 0 => Some(sw as f64 / fp as f64),
            _ => None,
        }
    }
}

/// 一次采样产出的完整快照
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemSnapshot {
    /// hw.memsize
    pub physical_total: u64,
    /// 压缩内存占用（vm_stat 的 compressor pages × page size）
    pub compressor_bytes: u64,
    pub pages_free_bytes: u64,
    pub swap: SwapInfo,
    pub pressure: Pressure,
    pub thrash: Thrash,
    pub families: Vec<FamilyMem>,
    /// B4 换出受害者榜（按 swapped 降序，最多 5 条）
    pub top_swapped: Vec<ProcMem>,
    /// 本次快照时间（Unix 秒）
    pub fast_at: i64,
    /// 上次 footprint 深探完成时间；None = 尚未深探
    pub deep_updated_at: Option<i64>,
    /// footprint 是否可用。false 时全部读数均为 RSS 口径
    pub deep_available: bool,
}

/// B4：取 swapped 最高的前 N 个进程
pub fn top_swapped(procs: &[ProcMem], n: usize) -> Vec<ProcMem> {
    let mut ranked: Vec<ProcMem> = procs
        .iter()
        .filter(|p| p.swapped.unwrap_or(0) > 0)
        .cloned()
        .collect();
    ranked.sort_by_key(|p| std::cmp::Reverse(p.swapped.unwrap_or(0)));
    ranked.truncate(n);
    ranked
}

/// B3：按家族聚合，按总量降序
///
/// 总量口径：已深探的子进程用 footprint，未深探的用 RSS 补上——
/// 不能只累加「有 footprint 的那部分」，否则大应用的 helper 未深探时
/// 会算出一个明显偏低的假数字。
pub fn aggregate(procs: &[ProcMem]) -> Vec<FamilyMem> {
    #[derive(Default)]
    struct Acc {
        count: usize,
        deep_covered: usize,
        rss: u64,
        /// 已深探子进程的 footprint 之和
        fp_probed: u64,
        /// 未深探子进程的 RSS 之和（footprint 的下界补位）
        rss_unprobed: u64,
        swapped: u64,
        has_footprint: bool,
        has_swapped: bool,
    }

    let mut accs: std::collections::BTreeMap<&str, Acc> = std::collections::BTreeMap::new();
    for p in procs {
        let a = accs.entry(p.family.as_str()).or_default();
        a.count += 1;
        a.rss = a.rss.saturating_add(p.rss);
        match p.footprint {
            Some(fp) => {
                a.fp_probed = a.fp_probed.saturating_add(fp);
                a.deep_covered += 1;
                a.has_footprint = true;
            }
            None => a.rss_unprobed = a.rss_unprobed.saturating_add(p.rss),
        }
        if let Some(sw) = p.swapped {
            a.swapped = a.swapped.saturating_add(sw);
            a.has_swapped = true;
        }
    }

    let mut out: Vec<FamilyMem> = accs
        .into_iter()
        .map(|(name, a)| FamilyMem {
            name: name.to_string(),
            process_count: a.count,
            deep_covered: a.deep_covered,
            rss: a.rss,
            footprint: a
                .has_footprint
                .then_some(a.fp_probed.saturating_add(a.rss_unprobed)),
            swapped: a.has_swapped.then_some(a.swapped),
        })
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.total()));
    out
}

/// 由相邻两次 vm_stat 累计值差分出颠簸速率。
///
/// 计数器可能因重启/回绕而变小 → 此时按 0 处理，避免出现负速率。
pub fn thrash_rate(
    prev: Option<(u64, u64, i64)>,
    swapins_total: u64,
    swapouts_total: u64,
    now: i64,
) -> Thrash {
    let Some((prev_in, prev_out, prev_ts)) = prev else {
        return Thrash::default();
    };
    let dt = (now - prev_ts).max(1) as f64;
    let delta = |cur: u64, prev: u64| cur.saturating_sub(prev) as f64;
    Thrash {
        swapins_per_sec: delta(swapins_total, prev_in) / dt,
        swapouts_per_sec: delta(swapouts_total, prev_out) / dt,
    }
}

/// 装配快照。纯函数，便于单测。
#[allow(clippy::too_many_arguments)]
pub fn build_snapshot(
    physical_total: u64,
    system: &system::SystemMem,
    swap: SwapInfo,
    pressure: Pressure,
    thrash: Thrash,
    procs: &[ProcMem],
    fast_at: i64,
    deep_updated_at: Option<i64>,
    deep_available: bool,
) -> MemSnapshot {
    MemSnapshot {
        physical_total,
        compressor_bytes: system.compressor_bytes(),
        pages_free_bytes: system.pages_free_bytes(),
        swap,
        pressure,
        thrash,
        families: aggregate(procs),
        top_swapped: top_swapped(procs, 5),
        fast_at,
        deep_updated_at,
        deep_available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, family: &str, rss: u64, fp: Option<u64>, sw: Option<u64>) -> ProcMem {
        ProcMem {
            pid,
            ppid: 0,
            name: format!("p{pid}"),
            exe: None,
            family: family.to_string(),
            rss,
            footprint: fp,
            swapped: sw,
            source: if fp.is_some() {
                MemSource::Footprint
            } else {
                MemSource::Rss
            },
            start_time: 0,
        }
    }

    #[test]
    fn aggregate_groups_and_sums_families() {
        let procs = vec![
            proc(1, "Cursor", 100, Some(1000), Some(900)),
            proc(2, "Cursor", 50, Some(500), Some(400)),
            proc(3, "Chrome", 300, None, None),
        ];
        let fams = aggregate(&procs);
        assert_eq!(fams.len(), 2);
        assert_eq!(fams[0].name, "Cursor");
        assert_eq!(fams[0].process_count, 2);
        assert_eq!(fams[0].deep_covered, 2);
        assert!(fams[0].is_exact());
        assert_eq!(fams[0].footprint, Some(1500));
        assert_eq!(fams[0].swapped, Some(1300));
        // Chrome 只有 RSS，footprint 保持 None（不允许用 RSS 冒充 footprint）
        assert_eq!(fams[1].footprint, None);
        assert_eq!(fams[1].total(), 300);
        assert!(!fams[1].is_exact());
    }

    #[test]
    fn aggregate_counts_unprobed_children_as_rss() {
        // 大应用常见形态：主进程已深探，helper 还没探到。
        // 总量必须是 1000(footprint) + 300(rss) = 1300，不能只报 1000。
        let procs = vec![
            proc(1, "Cursor", 100, Some(1000), Some(800)),
            proc(2, "Cursor", 300, None, None),
        ];
        let fams = aggregate(&procs);
        assert_eq!(fams[0].footprint, Some(1300));
        assert_eq!(fams[0].deep_covered, 1);
        assert_eq!(fams[0].process_count, 2);
        assert!(!fams[0].is_exact(), "部分覆盖不能被当成精确值");
    }

    #[test]
    fn top_swapped_sorts_desc_and_skips_zero() {
        let procs = vec![
            proc(1, "A", 10, Some(100), Some(10)),
            proc(2, "B", 10, Some(100), Some(90)),
            proc(3, "C", 10, None, None),
            proc(4, "D", 10, Some(100), Some(50)),
        ];
        let top = top_swapped(&procs, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].pid, 2);
        assert_eq!(top[1].pid, 4);
    }

    #[test]
    fn swapped_ratio_null_safe() {
        let mut p = proc(1, "A", 10, Some(100), Some(97));
        assert!((p.swapped_ratio().unwrap() - 0.97).abs() < 1e-9);
        p.footprint = Some(0);
        assert_eq!(p.swapped_ratio(), None);
        p.footprint = None;
        assert_eq!(p.swapped_ratio(), None);
    }

    #[test]
    fn thrash_rate_handles_first_tick_and_counter_reset() {
        assert_eq!(
            thrash_rate(None, 100, 200, 1000),
            Thrash {
                swapins_per_sec: 0.0,
                swapouts_per_sec: 0.0
            }
        );
        let t = thrash_rate(Some((100, 200, 990)), 300, 500, 1000);
        assert!((t.swapins_per_sec - 20.0).abs() < 1e-9);
        assert!((t.swapouts_per_sec - 30.0).abs() < 1e-9);
        // 计数器回绕/重启：差值压成 0，不出负数
        let t = thrash_rate(Some((9999, 9999, 990)), 10, 10, 1000);
        assert_eq!(t.swapins_per_sec, 0.0);
    }
}
