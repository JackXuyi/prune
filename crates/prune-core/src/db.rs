//! 内存序列存储（T-205）：只落 **deep 轮** 的 Top N 行，5 min 粒度、保留 7 天。
//!
//! 本期不做泄漏判定（B5），但先把序列打好底——M3 的回归计算直接消费这张表。
//! 存储策略：只写有 footprint 的行（RSS-only 轮次不写），把表控制在
//! 「Top 40 × 5 min × 7 天 ≈ 8 万行」量级。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::leak::{ProcSeries, Sample};
use crate::mem::ProcMem;
use crate::net::traffic::DayTraffic;

/// 单进程单个时间点的序列记录
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub ts: i64,
    pub rss: u64,
    pub footprint: Option<u64>,
    pub swapped: Option<u64>,
}

/// 应用小时聚合的**增量**（写入用）。
///
/// 语义与 `net_daily` 一致：`sum` 类字段是「加上去」，`peak` 类字段取最大值。
/// 所以重复提交同一批增量会双计 —— 只允许单一写入者（sampler 的聚合器）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyDelta {
    pub family: String,
    /// 本地时间 `YYYY-MM-DDTHH`
    pub bucket: String,
    pub rss_sum: u64,
    pub rss_n: u32,
    pub rss_peak: u64,
    pub cpu_sum: f32,
    pub cpu_n: u32,
    pub cpu_peak: f32,
    pub net_down: u64,
    pub net_up: u64,
}

/// 读回的一行小时聚合（均值已算好；无样本的 metric 为 `None`，不是 0）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyRow {
    pub bucket: String,
    pub rss_avg: Option<u64>,
    pub rss_peak: Option<u64>,
    pub cpu_avg: Option<f32>,
    pub cpu_peak: Option<f32>,
    pub net_down: u64,
    pub net_up: u64,
    /// RSS 样本数：前端用它判「这一小时 Prune 在不在」
    pub samples: u32,
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
CREATE TABLE IF NOT EXISTS net_daily (
    day        TEXT PRIMARY KEY,
    down_bytes INTEGER NOT NULL,
    up_bytes   INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS app_hourly (
    family     TEXT    NOT NULL,
    bucket     TEXT    NOT NULL,              -- 本地时间 'YYYY-MM-DDTHH'
    rss_sum    INTEGER NOT NULL DEFAULT 0,    -- RSS 累加（字节，读时除以 rss_n 得均值）
    rss_n      INTEGER NOT NULL DEFAULT 0,
    rss_peak   INTEGER NOT NULL DEFAULT 0,
    cpu_sum    REAL    NOT NULL DEFAULT 0,    -- 单核口径 % 累加
    cpu_n      INTEGER NOT NULL DEFAULT 0,
    cpu_peak   REAL    NOT NULL DEFAULT 0,
    net_down   INTEGER NOT NULL DEFAULT 0,
    net_up     INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (family, bucket)
);
CREATE INDEX IF NOT EXISTS idx_app_hourly_bucket ON app_hourly (bucket);
CREATE TABLE IF NOT EXISTS cut_log (
    ts    INTEGER NOT NULL,              -- 秒级本地时间（M21 剪除成绩单）
    label TEXT    NOT NULL,              -- 分组 label（「npm 缓存」「node_modules」…）
    bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cut_log_ts ON cut_log (ts);
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

    // ------------------------------------------------------------ 网络每日流量（M14）

    /// 增量 UPSERT：把「上次落库之后的新增」加到当天行上。
    ///
    /// 语义是 `down_bytes = down_bytes + excluded.down_bytes`，所以重复提交同一批增量
    /// 会双计 —— 只允许**单一采样线程**调用（方案 §3.5）。反过来，它不是「覆盖写」，
    /// 所以重启后接着累加不会把当天已有数字抹掉。
    pub fn net_add(
        &self,
        day: &str,
        down_delta: u64,
        up_delta: u64,
        now: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO net_daily (day, down_bytes, up_bytes, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(day) DO UPDATE SET
                 down_bytes = down_bytes + excluded.down_bytes,
                 up_bytes   = up_bytes   + excluded.up_bytes,
                 updated_at = excluded.updated_at",
            params![day, sat(down_delta), sat(up_delta), now],
        )?;
        Ok(())
    }

    /// 读某一天的累计（启动时读回「今日」基线；`None` = 当天还没落过库）
    pub fn net_day(&self, day: &str) -> rusqlite::Result<Option<DayTraffic>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT day, down_bytes, up_bytes FROM net_daily WHERE day = ?1",
            params![day],
            row_to_day,
        )
        .optional()
    }

    /// 自 `from_day`（含）起的每日流量，按 day 升序。
    ///
    /// 只返回**有记录的日子**：缺失日由 UI 画灰条，不在这里补 0
    /// —— 补 0 等于把「Prune 没运行」说成「那天没流量」。
    pub fn net_recent(&self, from_day: &str) -> rusqlite::Result<Vec<DayTraffic>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT day, down_bytes, up_bytes FROM net_daily
             WHERE day >= ?1 ORDER BY day ASC",
        )?;
        let rows = stmt.query_map(params![from_day], row_to_day)?;
        rows.collect()
    }

    /// 删除早于 `day` 的行（保留期由调用方算好本地日期传进来）
    pub fn net_prune_before(&self, day: &str) -> rusqlite::Result<usize> {
        let conn = self.lock();
        conn.execute("DELETE FROM net_daily WHERE day < ?1", params![day])
    }

    // ------------------------------------------------------------ 应用小时历史（D3）

    /// 批量写入增量：单事务 + 一条预编译语句，避免 348 个家族逐条提交。
    ///
    /// `sum` 字段相加、`peak` 字段取 MAX、`updated_at` 覆盖写 ——
    /// 与 `net_add` 同语义，重启后接着累加不会抹掉当天已有数据。
    pub fn app_hourly_add(&self, rows: &[HourlyDelta], now: i64) -> rusqlite::Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO app_hourly
                     (family, bucket, rss_sum, rss_n, rss_peak, cpu_sum, cpu_n, cpu_peak,
                      net_down, net_up, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(family, bucket) DO UPDATE SET
                     rss_sum    = rss_sum    + excluded.rss_sum,
                     rss_n      = rss_n      + excluded.rss_n,
                     rss_peak   = MAX(rss_peak, excluded.rss_peak),
                     cpu_sum    = cpu_sum    + excluded.cpu_sum,
                     cpu_n      = cpu_n      + excluded.cpu_n,
                     cpu_peak   = MAX(cpu_peak, excluded.cpu_peak),
                     net_down   = net_down   + excluded.net_down,
                     net_up     = net_up     + excluded.net_up,
                     updated_at = excluded.updated_at",
            )?;
            for r in rows {
                stmt.execute(params![
                    r.family,
                    r.bucket,
                    sat(r.rss_sum),
                    r.rss_n as i64,
                    sat(r.rss_peak),
                    r.cpu_sum as f64,
                    r.cpu_n as i64,
                    r.cpu_peak as f64,
                    sat(r.net_down),
                    sat(r.net_up),
                    now,
                ])?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// 读某家族自 `from_bucket`（含）起的小时聚合，按 bucket 升序。
    ///
    /// **只返回有行的小时**：缺失由前端画灰柱，不在这里补 0
    /// —— 补 0 等于把「Prune 没运行」说成「那一小时 CPU 为 0」。
    pub fn app_hourly_get(
        &self,
        family: &str,
        from_bucket: &str,
    ) -> rusqlite::Result<Vec<HourlyRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT bucket, rss_sum, rss_n, rss_peak, cpu_sum, cpu_n, cpu_peak, net_down, net_up
             FROM app_hourly
             WHERE family = ?1 AND bucket >= ?2
             ORDER BY bucket ASC",
        )?;
        let rows = stmt.query_map(params![family, from_bucket], |row| {
            let bucket: String = row.get(0)?;
            let rss_sum = row.get::<_, i64>(1)?.max(0) as u64;
            let rss_n = row.get::<_, i64>(2)?.max(0) as u32;
            let rss_peak = row.get::<_, i64>(3)?.max(0) as u64;
            let cpu_sum = row.get::<_, f64>(4)?.max(0.0) as f32;
            let cpu_n = row.get::<_, i64>(5)?.max(0) as u32;
            let cpu_peak = row.get::<_, f64>(6)?.max(0.0) as f32;
            Ok(HourlyRow {
                bucket,
                rss_avg: (rss_n > 0).then(|| rss_sum / rss_n as u64),
                rss_peak: (rss_n > 0).then_some(rss_peak),
                cpu_avg: (cpu_n > 0).then(|| cpu_sum / cpu_n as f32),
                cpu_peak: (cpu_n > 0).then_some(cpu_peak),
                net_down: row.get::<_, i64>(7)?.max(0) as u64,
                net_up: row.get::<_, i64>(8)?.max(0) as u64,
                samples: rss_n,
            })
        })?;
        rows.collect()
    }

    /// 删除早于 `bucket` 的小时行（保留期由调用方算好本地小时传进来）
    pub fn app_hourly_prune_before(&self, bucket: &str) -> rusqlite::Result<usize> {
        let conn = self.lock();
        conn.execute("DELETE FROM app_hourly WHERE bucket < ?1", params![bucket])
    }

    // ------------------------------------------------------------ 剪除成绩单（M21）

    /// 批量追加剪除记录（单事务）。append-only：清理动作不可撤销，记录也不删改，
    /// 唯一的删除入口是保留期清理（800 天，见 `cut_prune_before`）。
    pub fn cut_add(&self, items: &[(&str, u64)], now: i64) -> rusqlite::Result<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO cut_log (ts, label, bytes) VALUES (?1, ?2, ?3)")?;
            for (label, bytes) in items {
                stmt.execute(params![now, *label, sat(*bytes) as i64])?;
            }
        }
        tx.commit()?;
        Ok(items.len())
    }

    /// 全量聚合：累计字节、次数、首条时间（`None` = 还没剪过，不是 0）。
    /// 记录量级「几百行/年」，全表扫比维护区间参数诚实——不引入「从哪天算起」的口径分歧。
    pub fn cut_summary(&self) -> rusqlite::Result<CutSummary> {
        let conn = self.lock();
        conn.query_row(
            "SELECT COALESCE(SUM(bytes), 0), COUNT(*), MIN(ts) FROM cut_log",
            [],
            |r| {
                Ok(CutSummary {
                    total_bytes: r.get::<_, i64>(0)?.max(0) as u64,
                    total_items: r.get::<_, i64>(1)?.max(0) as u64,
                    first_ts: r.get::<_, Option<i64>>(2)?,
                })
            },
        )
    }

    /// 受害者 Top N：按累计字节降序（并列按 label 字典序，次序稳定）。
    pub fn cut_top(&self, n: u32) -> rusqlite::Result<Vec<CutTop>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT label, SUM(bytes), COUNT(*) FROM cut_log
             GROUP BY label ORDER BY SUM(bytes) DESC, label ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![n], |row| {
            Ok(CutTop {
                label: row.get(0)?,
                bytes: row.get::<_, i64>(1)?.max(0) as u64,
                count: row.get::<_, i64>(2)?.max(0) as u64,
            })
        })?;
        rows.collect()
    }

    /// 删除早于 `cutoff` 的记录（保留期由调用方按秒级时间戳传入）
    pub fn cut_prune_before(&self, cutoff: i64) -> rusqlite::Result<usize> {
        let conn = self.lock();
        conn.execute("DELETE FROM cut_log WHERE ts < ?1", params![cutoff])
    }
}

/// 剪除成绩单聚合（M21）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutSummary {
    pub total_bytes: u64,
    pub total_items: u64,
    /// 首条记录的秒级时间戳；`None` = 还没剪过
    pub first_ts: Option<i64>,
}

/// 按分组聚合的一行 Top N
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutTop {
    pub label: String,
    pub bytes: u64,
    pub count: u64,
}

/// u64 → i64 的饱和转换：日流量不可能到 9.2 EB，但脏数据不能翻转成负数
fn sat(v: u64) -> i64 {
    v.min(i64::MAX as u64) as i64
}

fn row_to_day(row: &rusqlite::Row<'_>) -> rusqlite::Result<DayTraffic> {
    Ok(DayTraffic {
        day: row.get(0)?,
        // 列上若被外部写成负数，夹到 0，避免 as 转换出天文数字
        down_bytes: row.get::<_, i64>(1)?.max(0) as u64,
        up_bytes: row.get::<_, i64>(2)?.max(0) as u64,
    })
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
            cpu: None,
            cpu_time_ms: 0,
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
        store
            .record(100, &[proc(1, Some(1000), Some(900))])
            .unwrap();
        store
            .record(100, &[proc(1, Some(1100), Some(950))])
            .unwrap();
        assert_eq!(store.row_count().unwrap(), 1);
        let h = store.history(1, 0, 10).unwrap();
        assert_eq!(h[0].footprint, Some(1100));
    }

    #[test]
    fn history_filters_by_pid_and_since_and_orders_asc() {
        let store = Store::open_in_memory().unwrap();
        store
            .record(100, &[proc(1, Some(1000), None), proc(2, Some(50), None)])
            .unwrap();
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
            store
                .record(ts, &[proc(1, Some(ts as u64 * 100), None)])
                .unwrap();
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
        store
            .record(100, &[proc(1, Some(1000), None), proc(2, None, None)])
            .unwrap();
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

    // ------------------------------------------------------------ 网络每日流量

    /// 一天内多次 flush：必须是**相加**而不是覆盖，否则 60 s 落库一次只剩最后一次
    #[test]
    fn net_add_accumulates_within_same_day() {
        let store = Store::open_in_memory().unwrap();
        store.net_add("2026-11-05", 1000, 100, 1).unwrap();
        store.net_add("2026-11-05", 500, 50, 2).unwrap();

        let d = store.net_day("2026-11-05").unwrap().expect("必须有一行");
        assert_eq!((d.down_bytes, d.up_bytes), (1500, 150));
    }

    /// 跨天必须是两行：把昨天并进今天，用户会看到「今天突然多用了 10 GB」
    #[test]
    fn net_add_keeps_days_separate() {
        let store = Store::open_in_memory().unwrap();
        store.net_add("2026-11-04", 10, 1, 1).unwrap();
        store.net_add("2026-11-05", 20, 2, 2).unwrap();

        assert_eq!(store.net_day("2026-11-04").unwrap().unwrap().down_bytes, 10);
        assert_eq!(store.net_day("2026-11-05").unwrap().unwrap().down_bytes, 20);
        assert_eq!(store.net_recent("2026-11-01").unwrap().len(), 2);
    }

    /// 「今天还没落过库」= None，不是 0（0 是「今天确实没有流量」）
    #[test]
    fn net_day_missing_is_none() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.net_day("2026-11-05").unwrap().is_none());
    }

    /// 重启后读回同值：走同一份文件再开一次 Store（内存库测不出这件事）
    #[test]
    fn net_daily_survives_reopen() {
        let path = std::env::temp_dir().join(format!("prune-net-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).unwrap();
            store.net_add("2026-11-05", 4_096, 512, 1).unwrap();
        }
        {
            let store = Store::open(&path).unwrap();
            let d = store.net_day("2026-11-05").unwrap().unwrap();
            assert_eq!((d.down_bytes, d.up_bytes), (4_096, 512));
            // 重开后再累加，接着涨（不清零）
            store.net_add("2026-11-05", 1_024, 128, 2).unwrap();
            assert_eq!(
                store.net_day("2026-11-05").unwrap().unwrap().down_bytes,
                5_120
            );
        }
        let _ = std::fs::remove_file(&path);
        // WAL 模式会留下 sidecar 文件
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    /// 7 日柱：缺失日不返回（UI 画灰条），按日期升序
    #[test]
    fn net_recent_skips_missing_days_and_sorts() {
        let store = Store::open_in_memory().unwrap();
        store.net_add("2026-11-03", 30, 3, 1).unwrap();
        store.net_add("2026-11-05", 50, 5, 2).unwrap();

        let recent = store.net_recent("2026-11-01").unwrap();
        let days: Vec<&str> = recent.iter().map(|d| d.day.as_str()).collect();
        assert_eq!(days, vec!["2026-11-03", "2026-11-05"]);
    }

    /// 保留期清理：只删更早的行，边界当天保留
    #[test]
    fn net_prune_before_removes_old_rows_only() {
        let store = Store::open_in_memory().unwrap();
        store.net_add("2026-08-01", 1, 1, 1).unwrap();
        store.net_add("2026-08-08", 2, 2, 2).unwrap();
        store.net_add("2026-11-05", 3, 3, 3).unwrap();

        assert_eq!(store.net_prune_before("2026-08-08").unwrap(), 1);
        let days: Vec<String> = store
            .net_recent("0000-00-00")
            .unwrap()
            .into_iter()
            .map(|d| d.day)
            .collect();
        assert_eq!(days, vec!["2026-08-08", "2026-11-05"]);
    }

    // ---------------------------------------------------------- 应用小时历史

    fn hourly(
        family: &str,
        bucket: &str,
        rss_sum: u64,
        rss_n: u32,
        rss_peak: u64,
        cpu_sum: f32,
        cpu_n: u32,
        cpu_peak: f32,
    ) -> HourlyDelta {
        HourlyDelta {
            family: family.into(),
            bucket: bucket.into(),
            rss_sum,
            rss_n,
            rss_peak,
            cpu_sum,
            cpu_n,
            cpu_peak,
            net_down: 0,
            net_up: 0,
        }
    }

    /// 增量相加 / 峰值取 MAX：两次写入后均值与峰值都要对
    #[test]
    fn app_hourly_accumulates_incrementally() {
        let store = Store::open_in_memory().unwrap();
        store
            .app_hourly_add(
                &[hourly("Cursor", "2026-11-09T10", 100, 1, 100, 4.0, 1, 4.0)],
                1,
            )
            .unwrap();
        store
            .app_hourly_add(
                &[hourly("Cursor", "2026-11-09T10", 300, 2, 200, 8.0, 2, 5.0)],
                2,
            )
            .unwrap();

        let rows = store.app_hourly_get("Cursor", "2026-11-09T00").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rss_avg, Some(133)); // (100 + 300) / 3
        assert_eq!(rows[0].rss_peak, Some(200));
        assert_eq!(rows[0].cpu_avg, Some(4.0)); // (4 + 8) / 3
        assert_eq!(rows[0].cpu_peak, Some(5.0));
        assert_eq!(rows[0].samples, 3);
    }

    /// 缺失小时不返回（前端画灰柱），只有写入过的小时出现
    #[test]
    fn app_hourly_skips_missing_hours() {
        let store = Store::open_in_memory().unwrap();
        store
            .app_hourly_add(
                &[
                    hourly("Cursor", "2026-11-09T10", 1, 1, 1, 0.0, 1, 0.0),
                    hourly("Cursor", "2026-11-09T12", 1, 1, 1, 0.0, 1, 0.0),
                ],
                1,
            )
            .unwrap();

        let buckets: Vec<String> = store
            .app_hourly_get("Cursor", "2026-11-09T00")
            .unwrap()
            .into_iter()
            .map(|r| r.bucket)
            .collect();
        assert_eq!(buckets, vec!["2026-11-09T10", "2026-11-09T12"]);
    }

    /// 无样本的 metric 读回 None（不是 0），且家族之间互不串
    #[test]
    fn app_hourly_zero_samples_read_as_none() {
        let store = Store::open_in_memory().unwrap();
        let mut d = hourly("Node", "2026-11-09T10", 0, 0, 0, 0.0, 0, 0.0);
        d.net_down = 4096;
        store.app_hourly_add(&[d], 1).unwrap();
        store
            .app_hourly_add(
                &[hourly("Cursor", "2026-11-09T10", 1, 1, 1, 1.0, 1, 1.0)],
                1,
            )
            .unwrap();

        let rows = store.app_hourly_get("Node", "2026-11-09T00").unwrap();
        assert_eq!(rows[0].rss_avg, None);
        assert_eq!(rows[0].cpu_avg, None);
        assert_eq!(rows[0].net_down, 4096);

        assert_eq!(
            store
                .app_hourly_get("Cursor", "2026-11-09T00")
                .unwrap()
                .len(),
            1
        );
    }

    /// 保留期清理：只删更早的小时
    #[test]
    fn app_hourly_prune_before_removes_old_buckets() {
        let store = Store::open_in_memory().unwrap();
        store
            .app_hourly_add(
                &[
                    hourly("Cursor", "2026-11-01T00", 1, 1, 1, 0.0, 1, 0.0),
                    hourly("Cursor", "2026-11-09T00", 1, 1, 1, 0.0, 1, 0.0),
                ],
                1,
            )
            .unwrap();

        assert_eq!(store.app_hourly_prune_before("2026-11-02T00").unwrap(), 1);
        assert_eq!(store.app_hourly_get("Cursor", "0000").unwrap().len(), 1);
    }

    // ---------------------------------------------------------- 剪除成绩单（M21）

    /// 空库：summary 全 0 且 first_ts 是 None（「没剪过」≠「剪了 0 字节」）
    #[test]
    fn cut_summary_empty_is_none_first_ts() {
        let store = Store::open_in_memory().unwrap();
        let s = store.cut_summary().unwrap();
        assert_eq!(s.total_bytes, 0);
        assert_eq!(s.total_items, 0);
        assert!(s.first_ts.is_none());
        assert!(store.cut_top(3).unwrap().is_empty());
    }

    /// 多次清理跨 label 累加；Top N 按字节降序并列按字典序
    #[test]
    fn cut_add_accumulates_and_top_sorts_by_bytes() {
        let store = Store::open_in_memory().unwrap();
        store
            .cut_add(&[("npm 缓存", 3_000), ("node_modules", 1_000)], 100)
            .unwrap();
        store
            .cut_add(&[("npm 缓存", 2_000), ("brew", 2_500)], 200)
            .unwrap();

        let s = store.cut_summary().unwrap();
        assert_eq!(s.total_bytes, 8_500);
        assert_eq!(s.total_items, 4);
        assert_eq!(s.first_ts, Some(100));

        let top = store.cut_top(2).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].label, "npm 缓存");
        assert_eq!(top[0].bytes, 5_000);
        assert_eq!(top[0].count, 2);
        assert_eq!(top[1].label, "brew"); // 2_500 > 1_000，node_modules 被挤出
    }

    /// 保留期清理：只删更早的，边界当天保留
    #[test]
    fn cut_prune_before_removes_old_rows_only() {
        let store = Store::open_in_memory().unwrap();
        store.cut_add(&[("a", 1)], 100).unwrap();
        store.cut_add(&[("b", 2)], 300).unwrap();
        assert_eq!(store.cut_prune_before(200).unwrap(), 1);
        let s = store.cut_summary().unwrap();
        assert_eq!(s.total_items, 1);
        assert_eq!(s.total_bytes, 2);
    }

    /// 重启存活：同一文件重开一次 Store，成绩单不丢
    #[test]
    fn cut_log_survives_reopen() {
        let path = std::env::temp_dir().join(format!("prune-cut-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).unwrap();
            store.cut_add(&[("npm 缓存", 700)], 42).unwrap();
        }
        {
            let store = Store::open(&path).unwrap();
            let s = store.cut_summary().unwrap();
            assert_eq!(s.total_bytes, 700);
            assert_eq!(s.first_ts, Some(42));
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
