//! 端口可信基线与漂移（F22，docs/f22-port-baseline-plan.md）。
//!
//! 基线回答「哪些端口是老住户」，漂移回答「谁刚搬进来 / 谁把门开大了」。
//! 三条纪律：
//! 1. **基线键 = (port, proto, comm)**，不含 pid——node 重生 pid 必变，入键则
//!    基线废掉。绑定面变化是独立漂移类别（变宽比新出现更重要），但也不入键；
//! 2. **首扫不报漂移**：基线为空时本轮全部记为基线（`seeded`），与
//!    `lan_history` 的 first_scan_baseline 同款先例；
//! 3. **消失不删行**：只更新 last_seen，30 天 prune 兜底——「服务停了」
//!    的噪音远大于信息，消失只进 CLI 统计不进 UI。
//!
//! 红线：本模块只给「查询态呈现」（CLI / 行内徽标），**不做灵动岛事件**——
//! 路线图已裁决冻结第五类事件，行内徽标不构成打扰型事件（f22 §4.4）。

use serde::{Deserialize, Serialize};

use super::{BindScope, PortSnapshot, Proto};

/// 漂移类别（serde lowercase）。信息量排序：变宽 > 新出现 > 消失（不进 UI）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriftKind {
    /// 基线里没有这个 (port, proto, comm) 的监听，或首次出现不足 24 h
    New,
    /// 暴露面比基线时刻变宽（如 Loopback → Any）。安全相关度最高
    Widened,
}

/// 基线一行（与 db 表 `port_baseline` 一一对应，camelCase 序列化给前端不用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineRow {
    pub port: u16,
    pub proto: Proto,
    /// 进程名。换程序占同端口 = 漂移（新监听）
    pub comm: String,
    /// 首次出现时刻（Unix 秒）。「新」徽标的新鲜度锚点
    pub first_seen: i64,
    /// 末次观测时刻。消失不删行，靠它做 30 天 prune 与重连识别
    pub last_seen: i64,
    /// 基线时刻的暴露面。变宽检测的基准；**只收窄不放宽**——
    /// 变宽后基线保持旧值，徽标持续到用户 rebase 或端口收窄
    pub scope: BindScope,
}

impl BaselineRow {
    /// 基线键（端口 + 协议 + 进程名）。
    pub fn key(&self) -> (u16, Proto, &str) {
        (self.port, self.proto, &self.comm)
    }
}

/// 「新」徽标的新鲜度窗口（24 h）：过点之后端口就是老住户，不再标。
pub const NEW_WINDOW_SECS: i64 = 24 * 3600;

/// 消失行的保留窗口（30 天 prune 兜底，与 mem_process 同模式）。
pub const REMOVED_KEEP_SECS: i64 = 30 * 24 * 3600;

/// 一次基线比对的产出。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftReport {
    /// (port, proto, kind)：对应当前快照 entry 的徽标。首扫时为空
    pub flagged: Vec<(u16, Proto, DriftKind)>,
    /// 基线有、本轮没有（消失）。只进 CLI 统计，不进 UI
    pub removed: Vec<String>,
    /// true = 本轮是首扫（基线原为空）：调用方应落基线，**不标任何漂移**
    pub seeded: bool,
}

/// 暴露面宽度序（变宽检测用）。Unknown 视为最窄（不谎报变宽）。
fn scope_rank(s: BindScope) -> u8 {
    match s {
        BindScope::Loopback => 0,
        BindScope::Unknown => 0,
        BindScope::Interface => 1,
        BindScope::Any => 2,
    }
}

/// 主入口：基线 vs 当前快照。纯函数——对同一输入重放得到同一结论。
///
/// `now` 由调用方传（Unix 秒），便于测试注入时钟。
pub fn diff(baseline: &[BaselineRow], snap: &PortSnapshot, now: i64) -> DriftReport {
    let mut report = DriftReport::default();

    if baseline.is_empty() {
        // 首扫：全部记为基线，不标漂移（避免首装满屏徽标）
        report.seeded = true;
        return report;
    }

    let known: std::collections::HashMap<(u16, Proto, &str), &BaselineRow> =
        baseline.iter().map(|r| (r.key(), r)).collect();
    let live_keys: std::collections::HashSet<(u16, Proto, &str)> = snap
        .entries
        .iter()
        .flat_map(|e| {
            e.listeners
                .iter()
                // 去重：双栈 / 多 fd 的同 pid 同 comm 只算一个基线键
                .map(|b| (e.port, e.proto, b.comm.as_str()))
        })
        .collect();

    for e in &snap.entries {
        // 逐 entry 取主进程 comm（与基线键对齐；多持有者的冲突态不标漂移，
        // 冲突已有 F4 保镖与 port_conflicts 承担，这里不重复吓人）
        let Some(b) = e.primary() else { continue };
        let key = (e.port, e.proto, b.comm.as_str());
        match known.get(&key) {
            None => report.flagged.push((e.port, e.proto, DriftKind::New)),
            Some(row) => {
                // seed 行的 first_seen = now - 24h（整点），这里必须严格小于——
                // 否则 seed 后紧跟的一轮 diff 整表命中「新」
                if now - row.first_seen < NEW_WINDOW_SECS {
                    report.flagged.push((e.port, e.proto, DriftKind::New));
                } else if scope_rank(e.exposure) > scope_rank(row.scope) {
                    report.flagged.push((e.port, e.proto, DriftKind::Widened));
                }
            }
        }
    }

    for row in baseline {
        if !live_keys.contains(&row.key()) {
            report
                .removed
                .push(format!("{}/{} {}", row.port, proto_cn(row.proto), row.comm));
        }
    }
    report.removed.sort();
    report
}

/// 基线合并（比对之后的落库内容）。纯函数：
/// - 本轮在场的行：last_seen 刷新；scope 只收窄不放宽（变宽要一直可见）；
/// - 本轮不在场的行：原样保留（等 30 天 prune）；
/// - 新出现的行：first_seen = now；
/// - **首扫（基线为空）/ rebase 的行：first_seen = now - 24 h（天生老住户）**——
///   否则下一轮 diff 会把整张基线标成「新」，正是首装满屏徽标的老坑。
pub fn merge(baseline: &[BaselineRow], snap: &PortSnapshot, now: i64) -> Vec<BaselineRow> {
    let known: std::collections::HashMap<(u16, Proto, &str), &BaselineRow> =
        baseline.iter().map(|r| (r.key(), r)).collect();
    let cutoff = now - REMOVED_KEEP_SECS;
    // seed/rebase 的行走「天生老住户」；正常新增行走 now
    let new_first_seen = if baseline.is_empty() {
        now - NEW_WINDOW_SECS
    } else {
        now
    };
    let mut out: Vec<BaselineRow> = Vec::new();

    for e in &snap.entries {
        let Some(b) = e.primary() else { continue };
        let key = (e.port, e.proto, b.comm.as_str());
        match known.get(&key) {
            Some(row) => {
                let mut r = (*row).clone();
                r.last_seen = now;
                // 只收窄不放宽：变宽必须持续可见到 rebase
                if scope_rank(e.exposure) < scope_rank(r.scope) {
                    r.scope = e.exposure;
                }
                out.push(r);
            }
            None => out.push(BaselineRow {
                port: e.port,
                proto: e.proto,
                comm: b.comm.clone(),
                first_seen: new_first_seen,
                last_seen: now,
                scope: e.exposure,
            }),
        }
    }
    // 消失的行原样保留（30 天窗口内的），超窗丢弃
    for row in baseline {
        if !out.iter().any(|r| r.key() == row.key()) && row.last_seen >= cutoff {
            out.push(row.clone());
        }
    }
    out.sort_by_key(|r| (r.port, r.proto, r.comm.clone()));
    out
}

fn proto_cn(p: Proto) -> &'static str {
    match p {
        Proto::Tcp => "tcp",
        Proto::Udp => "udp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{BindScope, Coverage, PortBinding, PortEntry};

    fn binding(port: u16, comm: &str) -> PortBinding {
        PortBinding {
            port,
            proto: Proto::Tcp,
            scope: BindScope::Loopback,
            addr: "127.0.0.1".into(),
            v6: false,
            pid: Some(99),
            comm: comm.into(),
            start_time: 0,
            project: None,
            project_label: None,
        }
    }

    fn entry(port: u16, comm: &str, exposure: BindScope) -> PortEntry {
        PortEntry {
            port,
            proto: Proto::Tcp,
            listeners: vec![binding(port, comm)],
            conns: Some(0),
            exposure,
            watched: true,
            note: None,
            service: None,
            inbound: None,
            drift: None,
        }
    }

    fn snap(entries: Vec<PortEntry>) -> PortSnapshot {
        PortSnapshot {
            entries,
            coverage: Coverage::default(),
            time_wait: None,
            time_wait_ports: Vec::new(),
            conns_by_pid: Default::default(),
            configured_watch_ports: Vec::new(),
            at: 0,
            scan_ms: 0,
        }
    }

    fn row(port: u16, comm: &str, first_seen: i64, scope: BindScope) -> BaselineRow {
        BaselineRow {
            port,
            proto: Proto::Tcp,
            comm: comm.into(),
            first_seen,
            last_seen: first_seen,
            scope,
        }
    }

    const T0: i64 = 1_000_000;

    #[test]
    fn first_scan_seeds_without_flags() {
        let s = snap(vec![entry(3000, "node", BindScope::Any)]);
        let r = diff(&[], &s, T0);
        assert!(r.seeded);
        assert!(r.flagged.is_empty());
        assert!(r.removed.is_empty());
    }

    #[test]
    fn new_listen_flagged_then_ages_out() {
        let base = vec![
            // 老住户（早过 24 h 新鲜期）：不标
            row(3000, "node", T0 - NEW_WINDOW_SECS - 100, BindScope::Loopback),
        ];
        let s = snap(vec![
            entry(3000, "node", BindScope::Loopback),
            entry(5173, "vite", BindScope::Loopback),
        ]);
        let r = diff(&base, &s, T0);
        assert_eq!(r.flagged, vec![(5173, Proto::Tcp, DriftKind::New)]);
        assert!(r.removed.is_empty());
        // 采样轮同步基线后（merge），24 h 过点：vite 也成为老住户，不再标「新」
        let base2 = merge(&base, &s, T0);
        let r2 = diff(&base2, &s, T0 + NEW_WINDOW_SECS + 120);
        assert!(r2.flagged.is_empty(), "{:?}", r2.flagged);
    }

    #[test]
    fn pid_rebirth_is_not_drift_but_comm_change_is() {
        // node 重生（pid 变了，comm 没变）：基线键不含 pid → 不是漂移
        let base = vec![row(3000, "node", T0 - NEW_WINDOW_SECS - 100, BindScope::Loopback)];
        let s = snap(vec![entry(3000, "node", BindScope::Loopback)]);
        let r = diff(&base, &s, T0);
        assert!(r.flagged.is_empty());
        // 换程序占同端口：新监听
        let s2 = snap(vec![entry(3000, "python3", BindScope::Loopback)]);
        let r2 = diff(&base, &s2, T0);
        assert_eq!(r2.flagged, vec![(3000, Proto::Tcp, DriftKind::New)]);
        assert_eq!(r2.removed, vec!["3000/tcp node"]);
    }

    #[test]
    fn exposure_widened_flagged() {
        let base = vec![row(3000, "node", T0 - NEW_WINDOW_SECS - 100, BindScope::Loopback)];
        let s = snap(vec![entry(3000, "node", BindScope::Any)]);
        let r = diff(&base, &s, T0);
        assert_eq!(r.flagged, vec![(3000, Proto::Tcp, DriftKind::Widened)]);
    }

    #[test]
    fn merge_updates_last_seen_and_never_widens_scope() {
        let base = vec![
            row(3000, "node", T0 - 60, BindScope::Loopback),
            row(9999, "gone", T0 - 60, BindScope::Loopback), // 本轮消失
        ];
        let s = snap(vec![entry(3000, "node", BindScope::Any)]);
        let merged = merge(&base, &s, T0);
        // 3000 在场：last_seen 刷新；scope 保持 Loopback（放宽不吸收——变宽持续可见）
        let n = merged.iter().find(|r| r.port == 3000).unwrap();
        assert_eq!(n.last_seen, T0);
        assert_eq!(n.scope, BindScope::Loopback);
        assert_eq!(n.first_seen, T0 - 60);
        // 9999 消失但未过 30 天窗口：保留
        assert!(merged.iter().any(|r| r.port == 9999));
        // 新出现的行 first_seen = now；seed（基线空）行走「天生老住户」
        let merged2 = merge(&[], &s, T0);
        assert_eq!(merged2[0].first_seen, T0 - NEW_WINDOW_SECS);
        // seed 之后紧跟一轮 diff：整张基线都不标「新」（首装满屏徽标的老坑）
        let r3 = diff(&merged2, &s, T0);
        assert!(r3.flagged.is_empty(), "{:?}", r3.flagged);
    }
}
