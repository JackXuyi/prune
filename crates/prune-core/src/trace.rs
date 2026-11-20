//! prune trace 任务成本核算（F25，docs/f25-trace-plan.md）。
//!
//! 回答「跑一次 `pnpm install` / `next build` / 起一个 dev server，
//! 到底吃了这台机器多少资源」。trace 的角色是**观测壳**（同 `/usr/bin/time`）：
//! 执行的是用户 `--` 后**显式传入**的命令——Prune 自身不存在任何
//! 「替用户决定执行什么」的清单，这与 D-15 `--emit-script`（打印命令永不执行）
//! 不冲突：那边是"打印"，这边是"观测"，两条路径都没有替用户做决定。
//!
//! 边界纪律（watch 三条 + trace 两条）：
//! - 前台、随子进程生灭：不装后台、不写日志文件；stdout/stderr **原样透传**，
//!   成本单走 stderr——`prune trace -- cmd > build.log` 的重定向习惯不被破坏；
//! - **退出码透传**：`prune trace -- cmd && next` 的 CI 链可直接替换裸命令；
//! - Ctrl-C 中断同样产出成本单（中断前的消耗也是成本）；二次 Ctrl-C 直接退出；
//! - 归属边界诚实：被 launchd 收养的后代不计入且标注，短命进程漏采写明采样粒度。
//!
//! 口径决策（f25 DP-3）：
//! - CPU：`cpu_time_ms`（累计值）末−首，无 sysinfo 差分窗口「首见 None」的坑；
//!   按进程**加入树时刻**起算增量（fork 自长命父进程的「继承前」消耗不计入）；
//! - 峰值 RSS：逐轮树内 RSS **合计**的最大值（并行子进程不吃亏），采样粒度标注；
//! - 换出增量：深探 1-2 s/轮不可逐轮付 → 仅**首末轮**对树内 Top 8 深探，
//!   成对才产 Δ（None = 未采样，如实说明，不冒充 0）；
//! - footprint 字段**不出现在成本单里**——不同口径不并列（f19 同源纪律反向应用）；
//! - 端口轮默认开（dev server 场景正是端口证据），对基线轮 diff，分
//!   「还在监听」与「已释放」两段；`--no-ports` 关闭。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::mem::process::Scanner;
use crate::mem::ProcMem;
use crate::net::{ports, PortSnapshot, Proto};

pub const DEFAULT_INTERVAL_SECS: u64 = 2;
pub const MAX_INTERVAL_SECS: u64 = 60;
/// 转发 SIGTERM 后等待子进程自行退出的上限；超时 SIGKILL——
/// 这是透传用户的中断意图，不是 Prune 主动杀。
const TERMINAL_GRACE: Duration = Duration::from_secs(5);
/// reap 精度：轮询 try_wait 的 tick（100 ms）。采样节奏仍按 interval，
/// tick 只影响墙钟与 reap 的分辨率。
const WAIT_TICK: Duration = Duration::from_millis(100);
/// 换出深探预算：首末轮各一次，树内按 RSS 取 Top N（逐轮深探不可行）。
/// 消费者 probe_swap 仅 macOS 存在——不挂门控会在 Linux CI 被 -D dead_code 打红
#[cfg(target_os = "macos")]
const SWAP_PROBE_TOP: usize = 8;

/// trace 选项（CLI 解析后传入）
#[derive(Debug, Clone)]
pub struct Opts {
    /// `--` 后的命令与参数（非空，parse 层保证）
    pub command: Vec<String>,
    /// 采样间隔（秒），钳制 1-60
    pub interval_secs: u64,
    /// 端口轮开关（默认开）
    pub ports: bool,
}

#[derive(Debug)]
pub enum TraceError {
    /// 命令不在 PATH 里（不经 shell，无注入面；说明后退出 4）
    NotFound(String),
    /// spawn 失败（权限等）
    Spawn(String),
    /// wait 失败
    Wait(String),
}

// ---------------------------------------------------------------- 信号转发（unix）

#[cfg(unix)]
mod sig {
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Ctrl-C / SIGTERM 计数：1 = 转发给子进程组并产出中断单；
    /// 2 = 用户明确不要了，直接退出（成本单丢弃）。
    pub static COUNT: AtomicU32 = AtomicU32::new(0);

    extern "C" fn handler(_: i32) {
        // async-signal-safe：只做一次原子自增
        COUNT.fetch_add(1, Ordering::SeqCst);
    }

    pub fn install() {
        let h = handler as extern "C" fn(i32);
        unsafe {
            libc::signal(libc::SIGINT, h as usize);
            libc::signal(libc::SIGTERM, h as usize);
        }
    }

    pub fn count() -> u32 {
        COUNT.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------- 归属树（纯函数）

/// 归属规则（f25 DP-2）：本轮进程表中 ppid 链可追溯到 `root` 的集合（含 root）。
///
/// - **收养缺口如实缺**：孙进程被 reparent 到 launchd（pid 1）后从链上脱落，
///   自然不在集合内——成本单 coverage 段标注「实际消耗略高于本单」，不补估；
/// - **trace 自身进程树天然不在集合内**：prune 是 root 的父，祖先链不经过 root；
/// - BFS 从 root 沿 children 表下行，父进程先死的分支整枝脱落（同收养语义）。
pub fn tree_of(root: i32, procs: &[ProcMem]) -> HashSet<i32> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let mut out = HashSet::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if out.insert(pid) {
            if let Some(kids) = children.get(&pid) {
                queue.extend(kids.iter().copied());
            }
        }
    }
    out
}

// ---------------------------------------------------------------- 端口段（纯函数）

/// 一条新绑定端口的展示条目
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortBind {
    pub port: u16,
    pub proto: Proto,
    /// 持有进程命令名（首见）
    pub comm: String,
}

/// 从端口快照提取 (port, proto, comm) 集合。
/// `tree` 给定时只收树内 pid 的绑定（归属纪律：不把旁人期间开的新端口算进成本单）。
pub fn binds_of(snap: &PortSnapshot, tree: Option<&HashSet<i32>>) -> HashSet<(u16, Proto, String)> {
    let mut out = HashSet::new();
    for e in &snap.entries {
        for b in &e.listeners {
            if let Some(t) = tree {
                match b.pid {
                    Some(pid) if t.contains(&pid) => {}
                    _ => continue,
                }
            }
            out.insert((e.port, e.proto, b.comm.clone()));
        }
    }
    out
}

/// 端口成本段（纯函数）：期间见过的绑定 − 基线 = 新绑定；
/// 末端轮还在的进 `still_listening`，消失的进 `released`。
pub fn ports_cost(
    baseline: &HashSet<(u16, Proto, String)>,
    seen: &HashSet<(u16, Proto, String)>,
    final_round: &HashSet<(u16, Proto, String)>,
    enabled: bool,
) -> PortsCost {
    if !enabled {
        return PortsCost {
            bound: Vec::new(),
            still_listening: Vec::new(),
            released: Vec::new(),
            enabled: false,
        };
    }
    let mut bound: Vec<PortBind> = seen
        .iter()
        .filter(|k| !baseline.contains(*k))
        .map(|(port, proto, comm)| PortBind {
            port: *port,
            proto: *proto,
            comm: comm.clone(),
        })
        .collect();
    bound.sort_by_key(|b| (b.port, b.proto, b.comm.clone()));
    let mut still: Vec<PortBind> = Vec::new();
    let mut released: Vec<PortBind> = Vec::new();
    for b in &bound {
        let key = (b.port, b.proto, b.comm.clone());
        if final_round.contains(&key) {
            still.push(b.clone());
        } else {
            released.push(b.clone());
        }
    }
    PortsCost {
        bound,
        still_listening: still,
        released,
        enabled: true,
    }
}

// ---------------------------------------------------------------- 逐轮累计器

#[derive(Debug, Default)]
struct PidAcc {
    name: String,
    /// 树内首见时的累计 CPU 时间（加入树时刻起算增量——继承前消耗不计入）
    first_cpu_ms: Option<u64>,
    last_cpu_ms: u64,
    peak_rss: u64,
    /// 深探口径的换出量（仅首末轮 probe 时有值）
    first_swapped: Option<u64>,
    last_swapped: Option<u64>,
}

/// 逐轮累计器。`observe` 每轮喂树内进程切片，`finish` 出成本单。
#[derive(Debug, Default)]
pub struct TraceAccumulator {
    samples: u32,
    per_pid: HashMap<i32, PidAcc>,
    /// 逐轮树内 RSS 合计的最大值（并行子进程不吃亏）
    peak_tree_rss: u64,
}

impl TraceAccumulator {
    pub fn observe(&mut self, in_tree: &[&ProcMem]) {
        self.samples += 1;
        let tree_rss: u64 = in_tree.iter().map(|p| p.rss).sum();
        self.peak_tree_rss = self.peak_tree_rss.max(tree_rss);
        for p in in_tree {
            let a = self.per_pid.entry(p.pid).or_insert_with(|| PidAcc {
                name: p.name.clone(),
                ..PidAcc::default()
            });
            a.last_cpu_ms = p.cpu_time_ms;
            if a.first_cpu_ms.is_none() {
                a.first_cpu_ms = Some(p.cpu_time_ms);
            }
            a.peak_rss = a.peak_rss.max(p.rss);
            if let Some(s) = p.swapped {
                a.last_swapped = Some(s);
                if a.first_swapped.is_none() {
                    a.first_swapped = Some(s);
                }
            }
        }
    }

    /// 换出深探（首末轮各一次；macOS）：按 RSS 降序的树内 (pid, rss) 列表取 Top N。
    /// 死进程探不到 → 首末成对的 pid 才产 Δ，成对才有段。
    #[cfg(target_os = "macos")]
    pub fn probe_swap(&mut self, ranked: &[(i32, u64)]) {
        let pids: Vec<i32> = ranked
            .iter()
            .take(SWAP_PROBE_TOP)
            .map(|(pid, _)| *pid)
            .collect();
        let samples = crate::mem::footprint::probe_many(&pids, 4);
        for (pid, s) in samples {
            let Some(sw) = s.swapped else { continue };
            let a = self.per_pid.entry(pid).or_default();
            a.last_swapped = Some(sw);
            if a.first_swapped.is_none() {
                a.first_swapped = Some(sw);
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn probe_swap(&mut self, _ranked: &[(i32, u64)]) {}

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn process_count(&self) -> usize {
        self.per_pid.len()
    }
}

// ---------------------------------------------------------------- 成本单

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCost {
    /// 树内各进程 cpu_time_ms（末−首）之和（CPU-ms，多线程下可大于墙钟）
    pub time_ms: u64,
    /// 单核口径利用率（time ÷ wall × 100）；>100% 说明并行度
    pub util_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssCost {
    /// 采样粒度下的峰值（逐轮树内 RSS 合计的最大值）
    pub peak_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCost {
    /// 树内 swapped 末−首求和；None = 未采样（首末深探未成对）。
    /// 负值 = 发生 swap-in / 换出被释放，**不夹 0**。
    pub delta_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortsCost {
    /// trace 期间新绑定的 (port, proto, comm)（对基线轮 diff）
    pub bound: Vec<PortBind>,
    /// 子进程退出时还在监听的
    pub still_listening: Vec<PortBind>,
    /// 期间出现过、退出时已释放的
    pub released: Vec<PortBind>,
    /// false = 端口轮已关闭（--no-ports）
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descendant {
    pub pid: i32,
    pub name: String,
    /// 该进程树内增量 CPU（加入时刻起算）
    pub cpu_time_ms: u64,
    pub peak_rss: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCoverage {
    /// 采样轮数（含 spawn 后首采与 reap 后补采）
    pub samples: u32,
    /// 归属到的进程数（含 root）
    pub processes: usize,
    /// 口径注（中文单语，W1-DP-6）：收养缺口 / 采样粒度 / 换出未采样 / 端口关闭
    pub notes: Vec<String>,
}

/// 一张成本单。JSON 走 **stderr**（stdout 属于被观测命令——透传纪律优先），
/// 形状进契约冻结（docs_json::trace_doc 补 schemaVersion 信封）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceReport {
    /// 用户显式传入的命令（展示用空格拼接）
    pub cmd: String,
    /// 子进程 spawn→reap 墙钟（采样粒度见 coverage）
    pub wall_ms: u64,
    /// 子进程退出码（透传）；被信号杀死 = 128 + 信号号（shell 惯例）
    pub exit_code: i32,
    /// Ctrl-C/SIGTERM 转发中断（中断前的消耗也是成本）
    pub interrupted: bool,
    pub cpu: CpuCost,
    pub rss: RssCost,
    pub swap: SwapCost,
    pub ports: PortsCost,
    /// 按树内增量 CPU 降序 top 5
    pub top_descendants: Vec<Descendant>,
    pub coverage: TraceCoverage,
    pub interval_ms: u64,
}

impl TraceAccumulator {
    pub fn finish(
        self,
        cmd: String,
        wall_ms: u64,
        exit_code: i32,
        interrupted: bool,
        ports: PortsCost,
        interval_ms: u64,
    ) -> TraceReport {
        let cpu_time_ms: u64 = self
            .per_pid
            .values()
            .map(|a| {
                a.last_cpu_ms
                    .saturating_sub(a.first_cpu_ms.unwrap_or(a.last_cpu_ms))
            })
            .sum();

        // 换出增量：首末成对的 pid 求和；不成对 = 未采样（None，不冒充 0）
        let paired: Vec<i64> = self
            .per_pid
            .values()
            .filter_map(|a| match (a.first_swapped, a.last_swapped) {
                (Some(f), Some(l)) => Some(l as i64 - f as i64),
                _ => None,
            })
            .collect();
        let swap_delta = (!paired.is_empty()).then(|| paired.iter().sum::<i64>());

        // 归属到的进程总数（含 root；descendants 是截断后的 top 5，别拿它冒充总数）
        let process_count = self.per_pid.len();
        let mut descendants: Vec<Descendant> = self
            .per_pid
            .into_iter()
            .map(|(pid, a)| Descendant {
                pid,
                name: a.name,
                cpu_time_ms: a
                    .last_cpu_ms
                    .saturating_sub(a.first_cpu_ms.unwrap_or(a.last_cpu_ms)),
                peak_rss: a.peak_rss,
            })
            .collect();
        descendants.sort_by_key(|d| std::cmp::Reverse(d.cpu_time_ms));
        descendants.truncate(5);

        // 口径注：诚实标注四类边界
        let mut notes = vec![
            "被系统收养的后代不计入，实际消耗略高于本单".to_string(),
            format!(
                "采样粒度 {:.1}s：存活不足一轮的后代可能整轮漏采",
                interval_ms as f64 / 1000.0
            ),
        ];
        if swap_delta.is_none() {
            notes.push("换出增量未采样：深探 1-2s/轮不可逐轮付，首末轮未成对".to_string());
        }
        if !ports.enabled {
            notes.push("端口轮已关闭（--no-ports）".to_string());
        }

        TraceReport {
            cmd,
            wall_ms,
            exit_code,
            interrupted,
            cpu: CpuCost {
                time_ms: cpu_time_ms,
                util_pct: cpu_time_ms as f64 / wall_ms.max(1) as f64 * 100.0,
            },
            rss: RssCost {
                peak_bytes: self.peak_tree_rss,
            },
            swap: SwapCost {
                delta_bytes: swap_delta,
            },
            ports,
            top_descendants: descendants,
            coverage: TraceCoverage {
                samples: self.samples,
                processes: process_count,
                notes,
            },
            interval_ms,
        }
    }
}

// ---------------------------------------------------------------- 主流程

/// 运行观测壳：spawn 用户显式命令 + 逐轮采样 + reap 后补采 + 成本单。
///
/// stdout/stderr 继承（透传纪律）；信号转发只打子进程的独立进程组
/// （`process_group(0)`），不会波及 prune 自身。
pub fn run(opts: &Opts) -> Result<TraceReport, TraceError> {
    let interval = Duration::from_secs(opts.interval_secs.clamp(1, MAX_INTERVAL_SECS));
    let cmd_display = opts.command.join(" ");

    // 基线轮：全进程快照热身 + 端口绑定基线（DP-2）
    let mut scanner = Scanner::new();
    let _baseline_procs = scanner.scan();
    let baseline_binds = if opts.ports {
        binds_of(&ports::scan(None), None)
    } else {
        HashSet::new()
    };

    let Some(program) = opts.command.first().cloned() else {
        return Err(TraceError::NotFound(String::new()));
    };
    let mut command = std::process::Command::new(&program);
    command.args(&opts.command[1..]);
    // 观测壳不是管道中间件：stdout/stderr 原样透传
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 独立进程组：SIGTERM/SIGKILL 只打用户命令，不打到 prune 自己
        command.process_group(0);
        sig::install();
    }

    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => TraceError::NotFound(program.clone()),
        _ => TraceError::Spawn(e.to_string()),
    })?;
    let root = child.id() as i32;
    let started = Instant::now();

    let mut acc = TraceAccumulator::default();
    let mut seen_binds: HashSet<(u16, Proto, String)> = HashSet::new();
    let mut last_binds: HashSet<(u16, Proto, String)> = HashSet::new();

    /// 采样一轮（进程 + 端口）；返回按 RSS 降序的树内 (pid, rss)，供换出深探取 Top N
    macro_rules! sample {
        () => {{
            let procs = scanner.scan().procs;
            let tree = tree_of(root, &procs);
            let in_tree: Vec<&ProcMem> = procs.iter().filter(|p| tree.contains(&p.pid)).collect();
            acc.observe(&in_tree);
            if opts.ports {
                let cur = binds_of(&ports::scan(None), Some(&tree));
                seen_binds.extend(cur.iter().cloned());
                last_binds = cur;
            }
            let mut ranked: Vec<(i32, u64)> = in_tree.iter().map(|p| (p.pid, p.rss)).collect();
            ranked.sort_by_key(|(_, rss)| std::cmp::Reverse(*rss));
            ranked
        }};
    }

    // spawn 后立即首采（短命命令也能留下至少一轮归属数据）+ 首轮换出深探
    let first_ranked = sample!();
    acc.probe_swap(&first_ranked);

    let mut forwarded_at: Option<Instant> = None;
    let mut killed_after_grace = false;
    let mut interrupted = false;
    let mut since_sample = Duration::ZERO;

    let exit_status: std::process::ExitStatus = loop {
        #[cfg(unix)]
        {
            match sig::count() {
                // 二次 Ctrl-C：用户明确不要成本单了，直接退出（130 = shell SIGINT 惯例）
                n if n >= 2 => {
                    let _ = child.kill();
                    let _ = child.wait();
                    std::process::exit(130);
                }
                // 首次中断：转发 SIGTERM 给子进程组（透传用户意图），照常出成本单
                n if n >= 1 && forwarded_at.is_none() => {
                    forwarded_at = Some(Instant::now());
                    interrupted = true;
                    unsafe {
                        libc::kill(-root, libc::SIGTERM);
                    }
                }
                _ => {}
            }
            if let Some(t) = forwarded_at {
                if !killed_after_grace && t.elapsed() > TERMINAL_GRACE {
                    killed_after_grace = true;
                    unsafe {
                        libc::kill(-root, libc::SIGKILL);
                    }
                }
            }
        }

        if let Some(st) = child
            .try_wait()
            .map_err(|e| TraceError::Wait(e.to_string()))?
        {
            break st;
        }

        if since_sample >= interval {
            let ranked = sample!();
            let _ = ranked;
            since_sample = Duration::ZERO;
        }
        std::thread::sleep(WAIT_TICK);
        since_sample += WAIT_TICK;
    };

    // reap 后补采一轮（DP-7 风险表：缩小短命进程漏窗 + 末端端口快照）
    let final_ranked = sample!();
    acc.probe_swap(&final_ranked);

    let wall_ms = started.elapsed().as_millis() as u64;
    let code = exit_code_of(exit_status);
    let ports = ports_cost(&baseline_binds, &seen_binds, &last_binds, opts.ports);

    Ok(acc.finish(
        cmd_display,
        wall_ms,
        code,
        interrupted,
        ports,
        interval.as_millis() as u64,
    ))
}

/// 退出码透传：正常退出取 code；被信号杀死 = 128 + 信号号（shell 惯例，
/// `prune trace -- cmd && next` 的 CI 链可直接替换裸命令）。
#[cfg(unix)]
fn exit_code_of(st: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    st.code()
        .or_else(|| st.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

#[cfg(not(unix))]
fn exit_code_of(st: std::process::ExitStatus) -> i32 {
    st.code().unwrap_or(1)
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, ppid: i32, rss: u64, cpu_ms: u64) -> ProcMem {
        ProcMem {
            pid,
            ppid,
            name: format!("p{pid}"),
            exe: None,
            family: "app".into(),
            rss,
            footprint: None,
            swapped: None,
            source: crate::mem::MemSource::Rss,
            start_time: 100,
            cmd: Vec::new(),
            workspace: None,
            cpu: None,
            cpu_time_ms: cpu_ms,
        }
    }

    /// 树归属：root + 子 + 孙全收；无关进程、收养孙、trace 自身树都不在集合内
    #[test]
    fn tree_of_follows_ppid_chain() {
        let procs = vec![
            proc(1, 0, 1, 0),      // launchd
            proc(10, 1, 1, 0),     // prune 自身（祖先链不经过 root，天然排除）
            proc(100, 10, 10, 0),  // root（被观测命令）
            proc(200, 100, 10, 0), // root 的子
            proc(300, 200, 10, 0), // 孙
            proc(400, 1, 10, 0),   // 被 launchd 收养的孙（父已死）→ 脱落
            proc(500, 999, 10, 0), // 无关进程
        ];
        let tree = tree_of(100, &procs);
        assert!(tree.contains(&100));
        assert!(tree.contains(&200));
        assert!(tree.contains(&300));
        assert!(
            !tree.contains(&400),
            "收养缺口如实缺：reparent 到 1 的孙不计入"
        );
        assert!(!tree.contains(&500));
        assert!(!tree.contains(&10), "trace 自身进程树不在归属集合内");
    }

    /// CPU 增量：按加入树时刻的首见值起算（继承前消耗不计入）；util 单核口径可 >100%
    #[test]
    fn cpu_delta_counts_from_first_observation() {
        let mut acc = TraceAccumulator::default();
        let mut root = proc(100, 10, 100, 5_000);
        let mut child = proc(200, 100, 50, 0);
        // 首轮：首见值即基线（root 已带 5000ms 累计——增量只算之后的）
        acc.observe(&[&root, &child]);
        root.cpu_time_ms = 6_500;
        child.cpu_time_ms = 1_200;
        acc.observe(&[&root, &child]);
        let r = acc.finish(
            "cmd".into(),
            2_000,
            0,
            false,
            ports_cost(&HashSet::new(), &HashSet::new(), &HashSet::new(), false),
            2_000,
        );
        assert_eq!(r.cpu.time_ms, 1_500 + 1_200);
        assert!(
            (r.cpu.util_pct - (2_700.0 / 2_000.0) * 100.0).abs() < 1e-9,
            "单核口径利用率可 >100%（并行度）"
        );
        assert_eq!(r.top_descendants.len(), 2);
    }

    /// 峰值 RSS：逐轮树内合计的最大值（并行子进程不吃亏）
    #[test]
    fn peak_rss_is_per_round_tree_sum_max() {
        let mut acc = TraceAccumulator::default();
        let a = proc(1, 100, 300, 0);
        let b = proc(2, 100, 200, 0);
        acc.observe(&[&a, &b]); // 树内合计 500
        let a2 = proc(1, 100, 100, 0);
        let c = proc(3, 100, 150, 0);
        acc.observe(&[&a2, &c]); // 树内合计 250
        let r = acc.finish(
            "cmd".into(),
            1_000,
            0,
            false,
            ports_cost(&HashSet::new(), &HashSet::new(), &HashSet::new(), false),
            2_000,
        );
        assert_eq!(r.rss.peak_bytes, 500);
    }

    /// 换出增量：首末不成对必须 None，不冒充 0
    #[test]
    fn swap_delta_none_when_unpaired() {
        let mut acc = TraceAccumulator::default();
        let a = proc(1, 100, 100, 0);
        acc.observe(&[&a]);
        let r = acc.finish(
            "cmd".into(),
            1_000,
            0,
            false,
            ports_cost(&HashSet::new(), &HashSet::new(), &HashSet::new(), false),
            2_000,
        );
        assert_eq!(r.swap.delta_bytes, None);
        assert!(r.coverage.notes.iter().any(|n| n.contains("换出")));
    }

    /// 换出增量负值：swapped 直采路径构造（深探合并后走同一管线），不夹 0
    #[test]
    fn swap_delta_negative_not_clamped() {
        let mut acc = TraceAccumulator::default();
        let mut a = proc(1, 100, 100, 0);
        a.swapped = Some(300 * 1_048_576);
        acc.observe(&[&a]);
        let mut a2 = proc(1, 100, 100, 0);
        a2.swapped = Some(100 * 1_048_576);
        acc.observe(&[&a2]);
        let r = acc.finish(
            "cmd".into(),
            1_000,
            0,
            false,
            ports_cost(&HashSet::new(), &HashSet::new(), &HashSet::new(), false),
            2_000,
        );
        assert_eq!(r.swap.delta_bytes, Some(-200 * 1_048_576));
    }

    /// 端口段：seen − 基线 = 新绑定；末端还在/已释放分两段；关闭时整段禁用
    #[test]
    fn ports_cost_splits_still_and_released() {
        let mut baseline = HashSet::new();
        baseline.insert((80u16, Proto::Tcp, "httpd".to_string()));
        let mut seen = HashSet::new();
        seen.insert((80, Proto::Tcp, "httpd".into())); // 基线已有 → 不是新绑定
        seen.insert((5173, Proto::Tcp, "node".into()));
        seen.insert((9229, Proto::Tcp, "node".into()));
        let mut final_round = HashSet::new();
        final_round.insert((5173, Proto::Tcp, "node".into()));
        let c = ports_cost(&baseline, &seen, &final_round, true);
        assert_eq!(c.bound.len(), 2);
        assert_eq!(c.still_listening.len(), 1);
        assert_eq!(c.still_listening[0].port, 5173);
        assert_eq!(c.released.len(), 1);
        assert_eq!(c.released[0].port, 9229);

        let off = ports_cost(&baseline, &seen, &final_round, false);
        assert!(!off.enabled);
        assert!(off.bound.is_empty());
    }

    /// coverage 口径注：收养缺口与采样粒度永远在；换出未采样与端口关闭按需
    #[test]
    fn coverage_notes_are_honest() {
        let acc = TraceAccumulator::default();
        let r = acc.finish(
            "cmd".into(),
            1_000,
            0,
            false,
            ports_cost(&HashSet::new(), &HashSet::new(), &HashSet::new(), false),
            2_000,
        );
        assert!(r.coverage.notes.iter().any(|n| n.contains("收养")));
        assert!(r.coverage.notes.iter().any(|n| n.contains("采样粒度")));
        assert!(r.coverage.notes.iter().any(|n| n.contains("换出")));
        assert!(r.coverage.notes.iter().any(|n| n.contains("端口轮")));
    }

    /// exit_code_of：信号死亡 = 128 + 信号号（unix）
    #[cfg(unix)]
    #[test]
    fn signal_death_maps_to_128_plus_sig() {
        use std::os::unix::process::ExitStatusExt;
        let st = std::process::ExitStatus::from_raw(0x0009); // SIGKILL
        assert_eq!(exit_code_of(st), 137);
        let st = std::process::ExitStatus::from_raw(0x0002); // SIGINT
        assert_eq!(exit_code_of(st), 130);
    }
}
