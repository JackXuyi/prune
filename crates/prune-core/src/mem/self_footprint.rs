//! 自耗透明（F19，docs/f19-self-footprint-plan.md）：Prune 自身的资源读数。
//!
//! 卖点是「口径不撒谎」，对自己的开销同样不撒谎。三条纪律：
//! 1. **口径同源**：内存用 `mem/footprint.rs` 的同一 probe 函数（phys_footprint /
//!    Windows Private Bytes）——设置页与内存页对同一进程的数字必须一致（S2）；
//! 2. **缺数显「暂无」不显 0**：深探失败 → `footprint: None`，不回退 RSS 冒充
//!    同一口径；CPU 采样不足 → `None`；db 路径不可用 → `None`（S3 同一纪律）；
//! 3. **零承诺**：本模块只产出「当下实测 + 口径」，不产出也不允许产生
//!    「永远低于 X」类上限承诺（那是未来的打脸现场）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 一次自耗采样的完整产出。字段 camelCase 直达前端。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfFootprint {
    pub pid: i32,
    /// phys_footprint 字节（macOS phys_footprint / Windows Private Bytes）。
    /// None = 深探失败——**不回退 RSS**：口径必须与内存页一致，宁缺毋假。
    pub footprint: Option<u64>,
    /// 线程数。None = 读不到（不填 0）
    pub threads: Option<u32>,
    /// 自身 CPU 多轮均值（%）。None = 采样不足（fast 轮还没攒出任何有效样本）
    pub cpu_pct_avg: Option<f32>,
    /// 参与均值的样本数（0 = 无样本）
    pub cpu_samples: u32,
    /// 序列库（prune.db 含 -wal / -shm）总字节。None = 路径不可用 / 库不存在
    pub db_bytes: Option<u64>,
    /// 采样时刻（Unix 秒）。UI 显示「打开设置时实测」的依据
    pub sample_at: i64,
}

/// 主入口。`db_path` 是 prune.db 的绝对路径（GUI 传 app_data_dir，
/// CLI / 测试传 None 即跳过）；`cpu_ring` 是壳层 fast 轮维护的自身 CPU 环形缓冲。
pub fn collect(db_path: Option<&Path>, cpu_ring: &[f32]) -> SelfFootprint {
    let pid = std::process::id() as i32;
    let sample_at = crate::swap::now_secs() as i64;

    // 内存：与内存页深探同一个函数（S2 的实现方式——两侧同一数据源）
    let footprint = crate::mem::footprint::probe(pid).ok().map(|s| s.footprint);

    // 线程数：只刷自己的 pid，0.1 ms 级
    let threads = threads_of(pid);

    let (cpu_pct_avg, cpu_samples) = cpu_avg(cpu_ring);
    let db_bytes = db_size(db_path);

    SelfFootprint {
        pid,
        footprint,
        threads,
        cpu_pct_avg,
        cpu_samples,
        db_bytes,
        sample_at,
    }
}

/// 自身 CPU 多轮均值。空环 → (None, 0)：上层显示「暂无数据」，不是 0%。
fn cpu_avg(ring: &[f32]) -> (Option<f32>, u32) {
    if ring.is_empty() {
        return (None, 0);
    }
    let sum: f32 = ring.iter().sum();
    (Some(sum / ring.len() as f32), ring.len() as u32)
}

/// prune.db（含 -wal / -shm）总字节。主库不存在 → None（不是 0）。
fn db_size(path: Option<&Path>) -> Option<u64> {
    let p = path?;
    if !p.exists() {
        return None;
    }
    let mut total = std::fs::metadata(p).ok()?.len();
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{}", p.display(), suffix));
        if let Ok(m) = std::fs::metadata(&side) {
            total += m.len();
        }
    }
    Some(total)
}

/// 只刷自身 pid 拿线程数。sysinfo 读不到（理论不发生）→ None，不填 0。
fn threads_of(pid: i32) -> Option<u32> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let p = Pid::from_u32(pid as u32);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[p]), true);
    let n = sys.process(p)?.tasks()?.len();
    if n > 0 {
        Some(n as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_avg_empty_ring_is_none_not_zero() {
        let (avg, n) = cpu_avg(&[]);
        assert_eq!(avg, None);
        assert_eq!(n, 0);
    }

    #[test]
    fn cpu_avg_single_and_multi_sample() {
        let (avg, n) = cpu_avg(&[2.0]);
        assert_eq!(avg, Some(2.0));
        assert_eq!(n, 1);
        let (avg, n) = cpu_avg(&[1.0, 3.0, 5.0]);
        assert_eq!(avg, Some(3.0));
        assert_eq!(n, 3);
    }

    #[test]
    fn db_size_none_when_missing() {
        assert_eq!(db_size(None), None);
        assert_eq!(db_size(Some(Path::new("/nonexistent-prune-db/prune.db"))), None);
    }

    #[test]
    fn db_size_counts_main_and_wal() {
        let dir = std::env::temp_dir().join(format!("prune-self-fp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("prune.db");
        std::fs::write(&db, vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("prune.db-wal"), vec![0u8; 30]).unwrap();
        let out = db_size(Some(&db));
        assert_eq!(out, Some(130), "主库 + wal 一起算");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collect_carries_camel_case_and_sample_time() {
        let out = collect(None, &[]);
        let doc = serde_json::to_value(&out).unwrap();
        assert!(doc.get("pid").is_some());
        assert!(doc.get("footprint").is_some());
        assert!(doc.get("cpuPctAvg").is_some());
        assert!(doc.get("cpu_pct_avg").is_none(), "契约是 camelCase");
        assert!(doc.get("sampleAt").is_some());
        // 缺数纪律：无 CPU 环 / 无 db → None，绝不出现假 0
        assert_eq!(out.cpu_pct_avg, None);
        assert_eq!(out.cpu_samples, 0);
        assert_eq!(out.db_bytes, None);
    }
}
