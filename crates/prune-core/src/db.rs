//! 内存序列存储（T-205）：只落 **deep 轮** 的 Top N 行，5 min 粒度、保留 7 天。
//!
//! 本期不做泄漏判定（B5），但先把序列打好底——M3 的回归计算直接消费这张表。
//! 存储策略：只写有 footprint 的行（RSS-only 轮次不写），把表控制在
//! 「Top 40 × 5 min × 7 天 ≈ 8 万行」量级。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::leak::{ProcSeries, Sample};
use crate::mem::ProcMem;

/// 单进程单个时间点的序列记录
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub ts: i64,
    pub rss: u64,
    pub footprint: Option<u64>,
    pub swapped: Option<u64>,
}

/// SQLite 存储句柄（内部 Mutex 以支持跨线程共享）
pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS mem_process (
    ts         INTEGER NOT NULL,
    pid        INTEGER NOT NULL,
    start_time INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    family     TEXT    NOT NULL,
    rss        INTEGER NOT NULL,
    footprint  INTEGER,
    swapped    INTEGER,
    PRIMARY KEY (ts, pid)
);
CREATE INDEX IF NOT EXISTS idx_mem_process_pid_ts ON mem_process (pid, ts);
CREATE INDEX IF NOT EXISTS idx_mem_process_ts ON mem_process (ts);
";

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        // WAL 让读写不互相阻塞；内存库会忽略/失败，忽略错误即可
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        Self::from_conn(conn)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 写入一批深探结果，返回实际写入行数（跳过无 footprint 的行）。
    /// 同一 (ts, pid) 重复写入时覆盖，保证幂等。
    pub fn record(&self, ts: i64, procs: &[ProcMem]) -> rusqlite::Result<usize> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO mem_process
             (ts, pid, start_time, name, family, rss, footprint, swapped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut written = 0;
        for p in procs {
            let Some(fp) = p.footprint else { continue };
            stmt.execute(params![
                ts,
                p.pid,
                p.start_time as i64,
                p.name,
                p.family,
                p.rss as i64,
                fp as i64,
                p.swapped.map(|s| s as i64),
            ])?;
            written += 1;
        }
        Ok(written)
    }

    /// 查询某进程自 `since` 起的序列（按时间升序，供 sparkline 直接消费）
    pub fn history(&self, pid: i32, since: i64, limit: usize) -> rusqlite::Result<Vec<Point>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT ts, rss, footprint, swapped FROM (
                 SELECT ts, rss, footprint, swapped FROM mem_process
                 WHERE pid = ?1 AND ts >= ?2
                 ORDER BY ts DESC LIMIT ?3
             ) ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![pid, since, limit as i64], |row| {
            Ok(Point {
                ts: row.get(0)?,
                rss: row.get::<_, i64>(1)? as u64,
                footprint: row.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                swapped: row.get::<_, Option<i64>>(3)?.map(|v| v as u64),
            })
        })?;
        rows.collect()
    }

    /// 删除早于 `cutoff` 的记录（7 天保留期由调用方按 now - 604800 传入）
    pub fn prune_before(&self, cutoff: i64) -> rusqlite::Result<usize> {
        let conn = self.lock();
        conn.execute("DELETE FROM mem_process WHERE ts < ?1", params![cutoff])
    }

    /// 拉取 `since` 之后的全部分程序列，供 B5 泄漏判定消费。
    ///
    /// 切分键是 **`pid + start_time`**：同一个 pid 被复用（旧进程退出、新进程拿到同号）
    /// 时必须拆成两条序列，否则会把两个进程的内存接成一条「暴涨曲线」。
    /// 只取 `footprint IS NOT NULL` 的行——RSS-only 行不进判定，避免口径混用。
    pub fn series(&self, since: i64) -> rusqlite::Result<Vec<ProcSeries>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT pid, start_time, family, name, ts, footprint
             FROM mem_process
             WHERE ts >= ?1 AND footprint IS NOT NULL
             ORDER BY pid, start_time, ts",
        )?;
        let mut rows = stmt.query(params![since])?;

        let mut out: Vec<ProcSeries> = Vec::new();
        while let Some(row) = rows.next()? {
            let pid: i32 = row.get(0)?;
            let start_time: i64 = row.get(1)?;
            let family: String = row.get(2)?;
            let name: String = row.get(3)?;
            let ts: i64 = row.get(4)?;
            // 列上可能有历史负值/脏值，一律夹到 0，避免 as 转换出天文数字
            let footprint = row.get::<_, i64>(5)?.max(0) as u64;
            let sample = Sample { ts, footprint };

            match out.last_mut() {
                Some(last) if last.pid == pid && last.start_time as i64 == start_time => {
                    last.samples.push(sample);
                }
                _ => out.push(ProcSeries {
                    pid,
                    start_time: start_time.max(0) as u64,
                    family,
                    name,
                    samples: vec![sample],
                }),
            }
        }
        Ok(out)
    }

    /// 行数（体积预算校验用）
    pub fn row_count(&self) -> rusqlite::Result<i64> {
        let conn = self.lock();
        conn.query_row("SELECT COUNT(*) FROM mem_process", [], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemSource;

    fn proc(pid: i32, fp: Option<u64>, sw: Option<u64>) -> ProcMem {
        ProcMem {
            pid,
            ppid: 1,
            name: format!("p{pid}"),
            exe: None,
            family: "Test".into(),
            rss: 10,
            footprint: fp,
            swapped: sw,
            source: MemSource::Footprint,
            start_time: 7,
            cmd: Vec::new(),
            workspace: None,
        }
    }

    #[test]
    fn records_only_deep_rows() {
        let store = Store::open_in_memory().unwrap();
        let n = store
            .record(100, &[proc(1, Some(1000), Some(900)), proc(2, None, None)])
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.row_count().unwrap(), 1);
    }

    #[test]
    fn same_timestamp_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc(1, Some(1000), Some(900))]).unwrap();
        store.record(100, &[proc(1, Some(1100), Some(950))]).unwrap();
        assert_eq!(store.row_count().unwrap(), 1);
        let h = store.history(1, 0, 10).unwrap();
        assert_eq!(h[0].footprint, Some(1100));
    }

    #[test]
    fn history_filters_by_pid_and_since_and_orders_asc() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc(1, Some(1000), None), proc(2, Some(50), None)]).unwrap();
        store.record(200, &[proc(1, Some(2000), None)]).unwrap();
        store.record(300, &[proc(1, Some(3000), None)]).unwrap();

        let h = store.history(1, 150, 10).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].ts, 200);
        assert_eq!(h[1].ts, 300);
        assert_eq!(h[1].footprint, Some(3000));
    }

    #[test]
    fn history_respects_limit_keeping_latest() {
        let store = Store::open_in_memory().unwrap();
        for ts in 1..=5 {
            store.record(ts, &[proc(1, Some(ts as u64 * 100), None)]).unwrap();
        }
        let h = store.history(1, 0, 2).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].ts, 4);
        assert_eq!(h[1].ts, 5);
    }

    #[test]
    fn prunes_old_rows() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc(1, Some(1), None)]).unwrap();
        store.record(500, &[proc(1, Some(2), None)]).unwrap();
        assert_eq!(store.prune_before(200).unwrap(), 1);
        assert_eq!(store.row_count().unwrap(), 1);
    }

    fn proc_at(pid: i32, start_time: u64, fp: Option<u64>) -> ProcMem {
        let mut p = proc(pid, fp, None);
        p.start_time = start_time;
        p
    }

    /// pid 复用必须切成两条序列：否则判定会把旧进程的 1000 MB 和新进程的 50 MB
    /// 接成一条「暴跌曲线」，或反过来造出假增长。
    #[test]
    fn series_splits_on_pid_reuse() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc_at(1, 7, Some(1000))]).unwrap();
        store.record(200, &[proc_at(1, 7, Some(1100))]).unwrap();
        // pid 1 被复用（新进程 start_time 不同）
        store.record(300, &[proc_at(1, 9, Some(50))]).unwrap();
        store.record(300, &[proc_at(2, 9, Some(60))]).unwrap();

        let s = store.series(0).unwrap();
        assert_eq!(s.len(), 3, "两条 pid=1 序列 + 一条 pid=2");
        assert_eq!(s[0].pid, 1);
        assert_eq!(s[0].start_time, 7);
        assert_eq!(s[0].samples.len(), 2);
        assert_eq!(s[0].samples[1].footprint, 1100);
        assert_eq!(s[1].start_time, 9, "复用的 pid 必须是独立序列");
        assert_eq!(s[1].samples.len(), 1);
        assert_eq!(s[2].pid, 2);
    }

    /// RSS-only 行不进判定（口径混用会让「换成 footprint 口径」那一跳看起来像暴涨）
    #[test]
    fn series_skips_rows_without_footprint() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc(1, Some(1000), None), proc(2, None, None)]).unwrap();
        let s = store.series(0).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].pid, 1);
    }

    #[test]
    fn series_filters_by_since() {
        let store = Store::open_in_memory().unwrap();
        store.record(100, &[proc(1, Some(1000), None)]).unwrap();
        store.record(500, &[proc(1, Some(1200), None)]).unwrap();
        let s = store.series(200).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].samples.len(), 1);
        assert_eq!(s[0].samples[0].ts, 500);
    }
}
