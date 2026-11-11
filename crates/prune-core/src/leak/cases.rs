//! 泄漏悬案卷宗（M22）：把逐轮判定的「嫌疑快照」串成有状态的案件。
//!
//! 判定引擎（`super::detect`）每轮只回答「现在谁像泄漏」；本模块在此基础上
//! 追踪跨轮的连续性：谁在立案中、谁已经离场、离场后有没有复发。三条纪律：
//!
//! 1. **结案 ≠ 证明清白**：`Cleared` 只描述「嫌疑人离场且此后同应用未见新嫌疑」，
//!    不给「没泄漏」的结论（M3 §6.3 措辞红线）。
//! 2. **判定单元是单进程序列**：案件键是 `(pid, start_time)`，与判定同口径；
//!    同 family 多个命中进程各立各的案，互不合并。
//! 3. **重启即清零**：案件簿只活在内存里——档案描述的是「这一段试用期」，
//!    持久化收益低，不值得为此加表。
//!
//! 输入全部来自现成的判定结果与进程表，零新权限、零新采样。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::LeakItem;

/// 已结案卷宗的保留上限（超出删最旧）。侦查中的案子不受此限。
pub const ARCHIVE_CAP: usize = 20;

/// 案件状态。命名刻意避开「无罪/有罪」：卷宗只记录观察到的事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaseStatus {
    /// 侦查中：嫌疑人仍在且仍在命中
    Open,
    /// 已归档：嫌疑人离场（进程退出），此后同应用未见新嫌疑
    Cleared { closed_at: i64 },
    /// 疑似复发：嫌疑人离场，但同应用**另立新案**（新的 pid 序列又命中）
    Relapsed { closed_at: i64 },
}

/// 一件悬案的完整档案
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeakCase {
    /// 案件编号 `CASE-{MMDD}-{序号}`，立案时分配、终身不变
    pub id: String,
    pub name: String,
    pub family: String,
    pub pid: i32,
    pub start_time: u64,
    /// 立案时间（Unix 秒）
    pub first_seen: i64,
    /// 最近一次仍在命中的时间
    pub last_seen: i64,
    /// 侦查期最大净增长（字节）
    pub growth_peak: u64,
    /// 最近一轮的净增长
    pub growth_latest: u64,
    pub status: CaseStatus,
}

/// 案件簿。`update` 由深探轮（5 min）驱动，见 [`CaseBook::update`]。
#[derive(Debug, Default)]
pub struct CaseBook {
    /// 侦查中的在前（按立案时间升序），已结案的追加在后（按结案时间）
    cases: Vec<LeakCase>,
    /// 编号序号：全簿递增，保证 id 唯一
    seq: u32,
}

impl CaseBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// 只读快照（前端卷宗页直接消费；侦查中的在前，已归档在后）
    pub fn cases(&self) -> &[LeakCase] {
        &self.cases
    }

    pub fn open_count(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| c.status == CaseStatus::Open)
            .count()
    }

    /// 用一轮判定结果推进案件簿。
    ///
    /// `items`：本轮命中的泄漏嫌疑（已按增长降序）；
    /// `alive`：当前活着的全部 `(pid, start_time)`（供「离场」判定）；
    /// `now`：本轮时间戳。
    pub fn update(&mut self, items: &[LeakItem], alive: &HashSet<(i32, u64)>, now: i64) {
        // 进程表为空说明本轮没采到数据（而不是「全部离场」）——跳过结案，
        // 避免采样故障把所有在办案件错杀成 Cleared
        let closing_allowed = !alive.is_empty();

        // 本轮**之前**就在办的案件键：复发判定要区分「另立新案」和「早就并发在办」
        let open_before: HashSet<(i32, u64)> = self
            .cases
            .iter()
            .filter(|c| c.status == CaseStatus::Open)
            .map(|c| (c.pid, c.start_time))
            .collect();

        // 1) 推进 / 结案 / 复启存量案
        for case in self.cases.iter_mut() {
            let key = (case.pid, case.start_time);
            match items.iter().find(|i| (i.pid, i.start_time) == key) {
                Some(item) => {
                    // 仍在命中：续期 + 更新增长；已结案的键再出现说明之前是采样
                    // 误判离场（同一 start_time 的进程不可能死而复生）——重启侦查
                    case.status = CaseStatus::Open;
                    case.last_seen = now;
                    case.growth_latest = item.growth;
                    case.growth_peak = case.growth_peak.max(item.growth);
                }
                None if case.status == CaseStatus::Open => {
                    if closing_allowed && !alive.contains(&key) {
                        // 嫌疑人离场。复发判定：本轮有同 family 的新命中（既不是本人在办时
                        // 就并发的那件，也不是本人——是新立的案）
                        let relapsed = items.iter().any(|i| {
                            i.family == case.family
                                && (i.pid, i.start_time) != key
                                && !open_before.contains(&(i.pid, i.start_time))
                        });
                        case.status = if relapsed {
                            CaseStatus::Relapsed { closed_at: now }
                        } else {
                            CaseStatus::Cleared { closed_at: now }
                        };
                    }
                    // 命中消失但进程还活着：增长退回门槛之下——侦查未完，保持 Open
                }
                None => {}
            }
        }

        // 2) 新命中立案（已结案的键也不再重开：同序列离场后又回来，视为新案）
        let mut known: HashSet<(i32, u64)> =
            self.cases.iter().map(|c| (c.pid, c.start_time)).collect();
        for item in items {
            let key = (item.pid, item.start_time);
            if known.contains(&key) {
                continue;
            }
            self.seq += 1;
            let stamp = match crate::localtime::day_string(now)
                .split('-')
                .collect::<Vec<_>>()[..]
            {
                [_, mm, dd] => format!("{mm}{dd}"),
                _ => "0000".to_string(),
            };
            self.cases.push(LeakCase {
                id: format!("CASE-{stamp}-{:02}", self.seq),
                name: item.name.clone(),
                family: item.family.clone(),
                pid: item.pid,
                start_time: item.start_time,
                first_seen: now,
                last_seen: now,
                growth_peak: item.growth,
                growth_latest: item.growth,
                status: CaseStatus::Open,
            });
            known.insert(key);
        }

        // 3) 归档上限：结案卷宗只留最近 ARCHIVE_CAP 件（按结案时间新→旧）
        let mut closed_idx: Vec<usize> = self
            .cases
            .iter()
            .enumerate()
            .filter(|(_, c)| c.status != CaseStatus::Open)
            .map(|(i, _)| i)
            .collect();
        if closed_idx.len() > ARCHIVE_CAP {
            closed_idx.sort_by(|&a, &b| {
                let at = match self.cases[a].status {
                    CaseStatus::Open => i64::MAX,
                    CaseStatus::Cleared { closed_at } | CaseStatus::Relapsed { closed_at } => {
                        closed_at
                    }
                };
                let bt = match self.cases[b].status {
                    CaseStatus::Open => i64::MAX,
                    CaseStatus::Cleared { closed_at } | CaseStatus::Relapsed { closed_at } => {
                        closed_at
                    }
                };
                bt.cmp(&at)
            });
            for i in closed_idx.into_iter().skip(ARCHIVE_CAP) {
                self.cases.remove(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(pid: i32, start_time: u64, family: &str, growth: u64) -> LeakItem {
        LeakItem {
            pid,
            start_time,
            family: family.into(),
            name: format!("p{pid}"),
            footprint: growth + 1000,
            growth,
            growth_ratio: 0.5,
            window_secs: 1800,
            points: 7,
            slope_mb_per_hour: 600.0,
            r2: 0.9,
            step_ratio: 0.1,
            gc_runtime: false,
        }
    }

    fn keys(procs: &[(i32, u64)]) -> HashSet<(i32, u64)> {
        procs.iter().copied().collect()
    }

    /// 首次命中立案；再命中续期且编号不变；增长峰值取历史最大
    #[test]
    fn first_hit_opens_and_repeat_updates() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        assert_eq!(book.open_count(), 1);
        let id = book.cases()[0].id.clone();
        assert!(book.cases()[0].id.starts_with("CASE-"));

        book.update(&[item(1, 7, "Cursor", 800)], &keys(&[(1, 7)]), 2000);
        assert_eq!(book.open_count(), 1, "同一序列续期，不另立新案");
        assert_eq!(book.cases()[0].id, id, "编号终身不变");
        assert_eq!(book.cases()[0].growth_peak, 800);
        assert_eq!(book.cases()[0].growth_latest, 800);
        assert_eq!(book.cases()[0].last_seen, 2000);
        // 峰值不回退
        book.update(&[item(1, 7, "Cursor", 300)], &keys(&[(1, 7)]), 3000);
        assert_eq!(book.cases()[0].growth_peak, 800);
        assert_eq!(book.cases()[0].growth_latest, 300);
    }

    /// 命中消失但进程还活着（增长退潮）：保持 Open，不算结案
    #[test]
    fn alive_but_subsided_stays_open() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        book.update(&[], &keys(&[(1, 7)]), 2000);
        assert_eq!(book.open_count(), 1);
        assert_eq!(book.cases()[0].status, CaseStatus::Open);
    }

    /// 嫌疑人离场且无同应用新案：Cleared 归档
    #[test]
    fn departed_without_new_hits_closes_cleared() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        // 嫌疑人退出；机器上还有别的进程（进程表不会是空的）
        book.update(&[], &keys(&[(999, 1)]), 2000);
        assert_eq!(book.open_count(), 0);
        assert_eq!(
            book.cases()[0].status,
            CaseStatus::Cleared { closed_at: 2000 }
        );
    }

    /// 嫌疑人离场但同应用新 pid 命中：Relapsed + 新案同时立案
    #[test]
    fn departed_with_new_family_hit_closes_relapsed() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        // 旧进程退出，新进程（同 family）命中
        book.update(&[item(2, 9, "Cursor", 600)], &keys(&[(2, 9)]), 2000);
        assert_eq!(book.open_count(), 1, "新案在办");
        let closed = book
            .cases()
            .iter()
            .find(|c| c.pid == 1)
            .expect("旧案应有卷宗");
        assert_eq!(closed.status, CaseStatus::Relapsed { closed_at: 2000 });
        let opened = book.cases().iter().find(|c| c.pid == 2).expect("新案已立");
        assert_eq!(opened.status, CaseStatus::Open);
    }

    /// 同 family 不同进程的命中互不合并（判定单元是单进程序列）
    #[test]
    fn same_family_two_pids_are_separate_cases() {
        let mut book = CaseBook::new();
        book.update(
            &[item(1, 7, "Cursor", 500), item(2, 7, "Cursor", 400)],
            &keys(&[(1, 7), (2, 7)]),
            1000,
        );
        assert_eq!(book.open_count(), 2);
        let ids: Vec<&str> = book.cases().iter().map(|c| c.id.as_str()).collect();
        assert_ne!(ids[0], ids[1], "编号必须唯一");
    }

    /// 进程表为空（采样故障）：不结案，全部保持 Open
    #[test]
    fn empty_alive_snapshot_never_closes_cases() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        book.update(&[], &keys(&[]), 2000);
        assert_eq!(book.open_count(), 1);
    }

    /// 已结案的序列再次命中（此前是采样误判离场）：重启侦查，不立新案
    #[test]
    fn closed_key_reopens_investigation() {
        let mut book = CaseBook::new();
        book.update(&[item(1, 7, "Cursor", 500)], &keys(&[(1, 7)]), 1000);
        book.update(&[], &keys(&[(999, 1)]), 2000); // 离场 → Cleared
        book.update(
            &[item(1, 7, "Cursor", 500)],
            &keys(&[(1, 7), (999, 1)]),
            3000,
        );
        assert_eq!(book.cases().len(), 1, "复用原卷宗");
        let case = &book.cases()[0];
        assert_eq!(case.status, CaseStatus::Open, "回到侦查中");
        assert_eq!(case.last_seen, 3000);
        // 再离场仍可正常结案
        book.update(&[], &keys(&[(999, 1)]), 4000);
        assert_eq!(
            book.cases()[0].status,
            CaseStatus::Cleared { closed_at: 4000 }
        );
    }

    /// 归档上限：超过 20 件结案卷宗时删最旧的；在办案件不受影响
    #[test]
    fn archive_cap_drops_oldest_closed() {
        let mut book = CaseBook::new();
        // 立案 + 离场结案 × 25
        for i in 0..25_u64 {
            book.update(
                &[item(i as i32 + 1, 100 + i, "Cursor", 500)],
                &keys(&[(i as i32 + 1, 100 + i)]),
                1000 + i as i64,
            );
            book.update(&[], &keys(&[(999, 1)]), 2000 + i as i64);
        }
        assert_eq!(book.open_count(), 0);
        assert_eq!(
            book.cases().len(),
            ARCHIVE_CAP,
            "只保留最近 ARCHIVE_CAP 件结案卷宗"
        );
        // 最旧的先被删：剩下的最早 first_seen 是第 25 - 20 轮立案的那件
        let oldest_first = book.cases().iter().map(|c| c.first_seen).min().unwrap();
        assert_eq!(oldest_first, 1000 + 5);
    }

    /// 编号序号跨轮递增，同一天内不重复
    #[test]
    fn ids_are_unique_and_incrementing() {
        let mut book = CaseBook::new();
        for i in 0..3_u64 {
            book.update(
                &[item(i as i32 + 1, 100 + i, "Cursor", 500)],
                &keys(&[(i as i32 + 1, 100 + i)]),
                1000 + i as i64,
            );
        }
        let ids: Vec<&str> = book.cases().iter().map(|c| c.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "编号不得重复");
    }
}
