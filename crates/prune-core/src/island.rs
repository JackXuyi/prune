//! 灵动岛事件判定（M15）：纯逻辑 + 可单测，不碰 Tauri / AppKit。
//!
//! 设计要点（见 docs/m15-dynamic-island-plan.md §3.4 / §5.6）：
//! - **不用均值**：单个 100% 尖峰会拉高均值，误判成「持续暴走」。用**达标率**
//!   （窗口内 ≥ 阈值的采样点占比），既抗毛刺又不漏真持续。
//! - **启动保护**：应用刚起来时窗口里只有 2~3 个点，用它们判会一开就弹。
//! - **滞回**：触发后必须降到 `threshold − 10` 以下才重新武装，否则 CPU 长期
//!   95% 会按频控上限反复弹。
//! - **可配置**：阈值 / 窗口 / 达标率全部来自参数，不在代码里写死。

use std::collections::VecDeque;

/// 采样间隔（秒）。2 s 是「够灵敏」与「够省」的折中：
/// 60 s 窗口有 30 个点，均值稳定；sysinfo `refresh_cpu_usage` < 1 ms/次。
pub const SAMPLE_INTERVAL_SEC: u64 = 2;

/// 滞回带宽（百分点）：触发后要掉到 `threshold − HYSTERESIS_PCT` 才重新武装。
pub const HYSTERESIS_PCT: f32 = 10.0;

/// CPU 暴走阈值组。GUI 侧从 settings.json 读，核心层不认识配置文件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuThresholds {
    /// 触发阈值（%），默认 90
    pub threshold_pct: f32,
    /// 判定窗口（秒），默认 60
    pub sustain_sec: u64,
    /// 窗口内达标率要求（%），默认 90
    pub tolerance_pct: f32,
}

impl Default for CpuThresholds {
    fn default() -> Self {
        Self {
            threshold_pct: 90.0,
            sustain_sec: 60,
            tolerance_pct: 90.0,
        }
    }
}

impl CpuThresholds {
    /// 把配置钳制到安全范围。越界不是「静默接受」——GUI 会把钳制后的值回写，
    /// 用户能看见自己设的值变成了什么（阈值 0 会导致岛每秒弹一次）。
    pub fn clamped(self) -> Self {
        Self {
            threshold_pct: self.threshold_pct.clamp(60.0, 100.0),
            sustain_sec: self.sustain_sec.clamp(10, 600),
            tolerance_pct: self.tolerance_pct.clamp(50.0, 100.0),
        }
    }

    /// 窗口容量：向上取整，至少 3 个点。
    fn capacity(&self) -> usize {
        ((self.sustain_sec / SAMPLE_INTERVAL_SEC) as usize + 1).max(3)
    }
}

/// 一次 CPU 暴走提报。`ratio_pct` 是窗口内达标比例，用于展开态文案（「60 秒里 93% 的时间」）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuAlert {
    pub avg_pct: f32,
    pub peak_pct: f32,
    pub ratio_pct: f32,
    /// 触发时窗口里的采样点数（用于 UI 说明「基于 N 个采样」）
    pub samples: usize,
}

/// 滑动窗口 + 滞回 + 启动保护。`observe` 每 2 s 调一次。
#[derive(Debug)]
pub struct CpuMonitor {
    samples: VecDeque<f32>,
    /// 首个样本的时间戳；None = 还没开始（或刚被 reset）
    started_ms: Option<u64>,
    /// 滞回武装位。true = 可以触发；触发后置 false，掉到阈值下方才恢复。
    armed: bool,
}

impl Default for CpuMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuMonitor {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            started_ms: None,
            armed: true,
        }
    }

    /// 配置变更时调用：清空窗口 + 重新计时 + 重新武装。
    ///
    /// **必须清空**：窗口里的旧样本是按旧阈值判定过的，直接沿用会误触发
    /// （例：90% → 70%，30 个旧点在新阈值下全部达标，立刻误弹一次）。
    pub fn reset(&mut self) {
        self.samples.clear();
        self.started_ms = None;
        self.armed = true;
    }

    /// 投入一个采样，返回是否应当提报。
    pub fn observe(&mut self, now_ms: u64, cpu_pct: f32, th: &CpuThresholds) -> Option<CpuAlert> {
        let th = th.clamped();
        let cap = th.capacity();

        if self.started_ms.is_none() {
            self.started_ms = Some(now_ms);
        }
        self.samples.push_back(cpu_pct);
        while self.samples.len() > cap {
            self.samples.pop_front();
        }

        // 启动保护：攒满一个完整窗口之前不判定，否则 3 个点就能弹
        let elapsed = now_ms.saturating_sub(self.started_ms.unwrap_or(now_ms));
        if elapsed < th.sustain_sec * 1000 {
            return None;
        }

        // 滞回：**当前采样**掉到 threshold − 10 以下才重新武装。
        //
        // 这里必须两个动作一起做，否则会「刚武装就再次触发」：
        // ① 只看当前值，不看窗口里是否曾有低点 —— 窗口里还有 30 个高点时，
        //    出现 1 个低点就武装，下一秒达标率仍然 90%+，立刻又报一次。
        // ② 武装的同时清空窗口并重新计时 —— 旧高点是按上一轮判定的，
        //    留着它们等于让新一轮从半满的窗口起跑。
        if !self.armed {
            let rearm_line = th.threshold_pct - HYSTERESIS_PCT;
            if cpu_pct < rearm_line {
                self.armed = true;
                self.samples.clear();
                self.samples.push_back(cpu_pct);
                self.started_ms = Some(now_ms);
            }
            return None;
        }

        let total = self.samples.len();
        // 样本充足性：至少半个窗口，防止采样线程短暂停摆后误判
        if total < (th.sustain_sec / SAMPLE_INTERVAL_SEC) as usize / 2 {
            return None;
        }

        let hot = self
            .samples
            .iter()
            .filter(|&&v| v >= th.threshold_pct)
            .count();
        let ratio = hot as f32 * 100.0 / total as f32;
        if ratio < th.tolerance_pct {
            return None;
        }

        self.armed = false;
        let avg = self.samples.iter().sum::<f32>() / total as f32;
        let peak = self.samples.iter().copied().fold(0.0f32, f32::max);
        Some(CpuAlert {
            avg_pct: avg,
            peak_pct: peak,
            ratio_pct: ratio,
            samples: total,
        })
    }
}

// ───────────────────────── 内存异常（M15.6） ─────────────────────────

use crate::mem::PressureLevel;

/// 内存阈值组。GUI 侧从 settings.json 读（`island.swap_pct` / `island.mem_sustain_sec`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemThresholds {
    /// swap 占用率触发阈值（%），默认 80。**钳制上限 95 就是关闭位**：
    /// 拉到 95 后通道 C 仍可用但几乎永远不触发，不值得单独加开关键。
    pub swap_pct: f32,
    /// Warning / swap 通道共用的持续窗口（秒），默认 30
    pub sustain_sec: u64,
}

impl Default for MemThresholds {
    fn default() -> Self {
        Self {
            swap_pct: 80.0,
            sustain_sec: 30,
        }
    }
}

impl MemThresholds {
    pub fn clamped(self) -> Self {
        Self {
            swap_pct: self.swap_pct.clamp(50.0, 95.0),
            sustain_sec: self.sustain_sec.clamp(10, 300),
        }
    }
}

/// 一次内存异常提报。GB / 换出速率 / 压缩器占用由 GUI 侧从自己的采样源填
/// （核心层不持有 sysinfo），这里只带判定结论。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemAlert {
    /// 触发时刻的内核压力档
    pub pressure: PressureLevel,
    /// 命中的通道：压力 / swap / 两者同时
    pub trigger: MemTrigger,
    /// 触发时刻 swap 占用率（%）
    pub swap_ratio_pct: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemTrigger {
    /// 压力通道（A 立即 / B 持续）
    Pressure,
    /// swap 占用破阈值且持续
    Swap,
    /// 同一拍两条通道都满足
    Both,
}

/// 内存异常判定：**水平量看状态持续**，与 CPU 的达标率完全不同型。
///
/// 三通道独立滞回（见方案 §12.1）：
/// - A `Critical` 立即触发（内核已在挑杀进程对象，无需再忍），回 Normal 才重新武装；
/// - B `Warning` 持续 `sustain_sec` 才触发（Warning 会长期挂着），回 Normal 重新武装；
/// - C swap ≥ `swap_pct` 持续 `sustain_sec`，掉到 `swap_pct − 10` 才重新武装
///   —— swap 是慢变量，等它归零等于永远不再报。
///
/// Warning / swap 的持续采用**连续计时**：中途出现一拍低于线的样本就清零重来。
/// 偶发抖动本就不该累积成「持续」。
#[derive(Debug)]
pub struct MemMonitor {
    critical_armed: bool,
    warn_armed: bool,
    /// Warning 连续计时起点；None = 当前不在 Warning
    warn_started_ms: Option<u64>,
    swap_armed: bool,
    /// swap 连续计时起点；None = 当前低于阈值
    swap_started_ms: Option<u64>,
}

impl Default for MemMonitor {
    fn default() -> Self {
        // 武装位全部 true：新造出来的监视器必须能触发。派生 Default 会给
        // bool 一个 false，三条通道天生哑火——这个坑单测当场抓出来了。
        Self {
            critical_armed: true,
            warn_armed: true,
            warn_started_ms: None,
            swap_armed: true,
            swap_started_ms: None,
        }
    }
}

impl MemMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 配置变更时调用：三条通道全部重新武装 + 清计时。
    /// 理由同 CpuMonitor::reset——旧计时是按旧阈值走的。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 投入一个采样。`swap_ratio_pct` 为占用率 ×100；swap 读不到时传负值
    /// （如 -1.0），通道 C 会安静跳过而不是拿 0% 误判「充裕」。
    pub fn observe(
        &mut self,
        now_ms: u64,
        pressure: PressureLevel,
        swap_ratio_pct: f32,
        th: &MemThresholds,
    ) -> Option<MemAlert> {
        let th = th.clamped();
        let sustain_ms = th.sustain_sec * 1000;

        // 通道 A：Critical 立即。滞回线是 Normal——Critical 降回 Warning 仍算紧张。
        if pressure == PressureLevel::Critical && self.critical_armed {
            self.critical_armed = false;
            return Some(MemAlert {
                pressure,
                trigger: MemTrigger::Pressure,
                swap_ratio_pct,
            });
        }
        if pressure == PressureLevel::Normal {
            self.critical_armed = true;
            self.warn_armed = true;
            self.warn_started_ms = None;
        }

        // 通道 B：Warning 持续
        let mut warn_fired = false;
        if pressure == PressureLevel::Warning {
            let started = *self.warn_started_ms.get_or_insert(now_ms);
            if self.warn_armed && now_ms.saturating_sub(started) >= sustain_ms {
                self.warn_armed = false;
                self.warn_started_ms = Some(now_ms);
                warn_fired = true;
            }
        } else if pressure == PressureLevel::Critical {
            // Critical 打断 Warning 计时：A 已经把这次提报带走了
            self.warn_started_ms = None;
        }

        // 通道 C：swap 持续。负比率 = 读不到，安静跳过。
        let mut swap_fired = false;
        if swap_ratio_pct >= th.swap_pct {
            let started = *self.swap_started_ms.get_or_insert(now_ms);
            if self.swap_armed && now_ms.saturating_sub(started) >= sustain_ms {
                self.swap_armed = false;
                self.swap_started_ms = Some(now_ms);
                swap_fired = true;
            }
        } else if swap_ratio_pct >= 0.0 {
            self.swap_started_ms = None;
            if swap_ratio_pct < th.swap_pct - HYSTERESIS_PCT {
                self.swap_armed = true;
            }
        }

        let trigger = match (warn_fired, swap_fired) {
            (true, true) => MemTrigger::Both,
            (true, false) => MemTrigger::Pressure,
            (false, true) => MemTrigger::Swap,
            (false, false) => return None,
        };
        Some(MemAlert {
            pressure,
            trigger,
            swap_ratio_pct,
        })
    }
}

/// 全局频控：任意事件每小时不超过 `max_per_hour` 次。
///
/// 同类事件的 10 分钟抑制由事件自身的 cooldown 承担（CPU 侧是滞回 + cooldown），
/// 这里只守「总量」这一道闸，避免多个事件源同时开口子把岛刷成广告位。
#[derive(Debug)]
pub struct RateLimiter {
    max_per_hour: u32,
    fired_ms: VecDeque<u64>,
}

impl RateLimiter {
    pub fn new(max_per_hour: u32) -> Self {
        Self {
            max_per_hour: max_per_hour.clamp(1, 30),
            fired_ms: VecDeque::new(),
        }
    }

    /// 改配置时调用：把上限替换掉并清空历史（旧上限下的计数不该约束新配置）。
    pub fn reconfigure(&mut self, max_per_hour: u32) {
        self.max_per_hour = max_per_hour.clamp(1, 30);
        self.fired_ms.clear();
    }

    /// 清理已过一小时窗口的时间戳。
    fn prune_expired(&mut self, now_ms: u64) {
        const HOUR_MS: u64 = 3_600_000;
        while let Some(&t) = self.fired_ms.front() {
            if now_ms.saturating_sub(t) >= HOUR_MS {
                self.fired_ms.pop_front();
            } else {
                break;
            }
        }
    }

    /// 只查不记：配额还有余量吗。
    ///
    /// 「per-event 闸 + 全局闸」串联判断时必须先查后记：若直接对第一道闸
    /// `allow()`（记录）再查第二道闸，第二道闸拒绝时第一道闸的配额已被
    /// 白白吃掉一次。
    pub fn would_allow(&mut self, now_ms: u64) -> bool {
        self.prune_expired(now_ms);
        self.fired_ms.len() < self.max_per_hour as usize
    }

    /// 允许则记录并返回 true；拒绝不记录（否则被拒的时刻也会占坑）。
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if !self.would_allow(now_ms) {
            return false;
        }
        self.fired_ms.push_back(now_ms);
        true
    }
}

/// CPU 事件的冷却时间：同类事件 10 分钟内只报一次。
///
/// 与滞回是两道独立的闸：滞回防「持续高位的重复提报」，冷却防「降下来又升上去
/// 的反复横跳」——后者在编译 / 渲染场景里很常见。
pub const CPU_COOLDOWN_MS: u64 = 10 * 60 * 1000;

/// 内存事件的冷却时间。与 CPU 相同量级：内存是慢变量，触发后 10 分钟内
/// 形势不太可能从「该报」变成「不该报」。
pub const MEM_COOLDOWN_MS: u64 = CPU_COOLDOWN_MS;

// ───────────────────────── 端口冲突保镖（M19） ─────────────────────────

use crate::net::{BindScope, PortEntry};
use std::collections::{HashMap, HashSet};

/// 端口事件的冷却时间。与 CPU / 内存同量级：dev server「kill → 立刻重启」
/// 是开发日常，没有这个冷却，每次重启都会弹一条「3000 又被占了」。
pub const PORT_COOLDOWN_MS: u64 = CPU_COOLDOWN_MS;

/// 端口事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortGuardKind {
    /// 关注端口从空闲变为被占（dev server 起来了 / 有东西抢了端口）
    NewListen,
    /// 同端口出现多个持有者（SO_REUSEPORT 双绑定 / 真冲突）
    Conflict,
}

/// 一次端口事件提报。字段与 M7 的 `PortEntry` 同源，UI 不需要二次查询就能展开。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortGuardAlert {
    pub port: u16,
    pub kind: PortGuardKind,
    /// 主展示进程（第一个有 pid 的 binding）
    pub comm: String,
    pub pid: Option<i32>,
    /// 项目归属（cwd 推断），None = 无项目线索
    pub project_label: Option<String>,
    /// 聚合暴露面（最宽者）——`Any` 要在 UI 里提示「对外可达」
    pub exposure: BindScope,
    /// 去重后的持有进程数；Conflict 时 >1
    pub owners: usize,
}

/// 端口状态变化判定：**看变化量，不看存量**。
///
/// 与 CPU / 内存的关键差异：端口是离散事件（起来 / 停掉），没有「持续窗口」
/// 的概念，所以不用达标率也不用连续计时，用的是**基线 + 边沿检测**：
/// - 首轮只建基线：Prune 启动时机器上本来就有 20+ 个监听端口，全是存量，
///   一条都不该弹（用户没「丢」任何东西）；
/// - 之后每轮与基线做差：新增占用（且命中关注规则）→ `NewListen`；
///   新增多持有者 → `Conflict`（优先于 NewListen，冲突比「有服务」更紧急）；
/// - 端口转空闲即从集合移除（重新武装）；冷却表按端口隔离，防重启横跳。
#[derive(Debug, Default)]
pub struct PortGuardMonitor {
    baseline_done: bool,
    occupied: HashSet<u16>,
    conflicts: HashSet<u16>,
    cooldown: HashMap<u16, u64>,
}

impl PortGuardMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 配置变更时调用：全部重新武装。基线也会重建——重开开关的第一轮
    /// 当作「新的起点」，不补报中途错过的变化。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn cooled(&self, now_ms: u64, port: u16) -> bool {
        self.cooldown
            .get(&port)
            .map(|&t| now_ms.saturating_sub(t) >= PORT_COOLDOWN_MS)
            .unwrap_or(true)
    }

    /// 投入一轮端口聚合结果，返回本拍要提报的事件。
    ///
    /// 只看 TCP：UDP 没有 LISTEN 语义，mDNS / 随机源端口的绑定抖动不值得报。
    pub fn observe(&mut self, now_ms: u64, entries: &[PortEntry]) -> Vec<PortGuardAlert> {
        // 本轮的占用 / 冲突全景（TCP）
        let mut occupied = HashSet::new();
        let mut conflicts = HashSet::new();
        for e in entries {
            if e.proto != crate::net::Proto::Tcp || e.listeners.is_empty() {
                continue;
            }
            occupied.insert(e.port);
            if e.owner_count() > 1 {
                conflicts.insert(e.port);
            }
        }

        // 首轮：只建基线。存量端口一条不报。
        if !self.baseline_done {
            self.baseline_done = true;
            self.occupied = occupied;
            self.conflicts = conflicts;
            return Vec::new();
        }

        let mut alerts = Vec::new();
        for e in entries {
            if e.proto != crate::net::Proto::Tcp || e.listeners.is_empty() {
                continue;
            }
            let newly_occupied = !self.occupied.contains(&e.port);
            let newly_conflict = conflicts.contains(&e.port) && !self.conflicts.contains(&e.port);
            // 冲突优先：一条端口同拍既是新占用又是冲突时，只报更有信息量的那个
            let kind = if newly_conflict {
                Some(PortGuardKind::Conflict)
            } else if newly_occupied && e.watched {
                Some(PortGuardKind::NewListen)
            } else {
                None
            };
            let Some(kind) = kind else { continue };
            if !self.cooled(now_ms, e.port) {
                continue;
            }
            self.cooldown.insert(e.port, now_ms);
            let primary = e.primary();
            alerts.push(PortGuardAlert {
                port: e.port,
                kind,
                comm: primary.map(|p| p.comm.clone()).unwrap_or_default(),
                pid: primary.and_then(|p| p.pid),
                project_label: primary.and_then(|p| p.project_label.clone()),
                exposure: e.exposure,
                owners: e.owner_count(),
            });
        }

        // 转空闲的端口：移出集合（重新武装）+ 过期冷却表清理
        self.occupied = occupied;
        self.conflicts = conflicts;
        self.cooldown
            .retain(|_, t| now_ms.saturating_sub(*t) < PORT_COOLDOWN_MS);

        alerts
    }
}

// ───────────────────────── 局域网新面孔（M23） ─────────────────────────

use crate::net::lan::LanDevice;

/// 一轮最多提报的新面孔数。超出部分合并进最后一条的 `extra`
/// （绝静默截断——条数对得上用户才敢信）。
pub const LAN_MAX_ALERTS_PER_ROUND: usize = 3;

/// 一次新面孔提报。字段与 `LanDevice` 同源，UI 不需要二次查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanWatchAlert {
    pub ip: String,
    /// 展示名：hostname > vendor > None（UI 兜底显示 IP）
    pub label: Option<String>,
    /// 设备类型（serde 小写串，直接进 payload）
    pub kind: String,
    /// 随机化 MAC → UI 提示「厂商不可识别」
    pub randomized_mac: bool,
    /// 本轮超过 [`LAN_MAX_ALERTS_PER_ROUND`] 的余量，只落在最后一条上
    pub extra: u32,
}

/// 设备类型 → 稳定的展示串（与 serde rename_all = "lowercase" 同口径，
/// 前端 payload 直接消费，不再做一层枚举翻译）。
fn kind_str(k: crate::net::lan::DeviceKind) -> String {
    use crate::net::lan::DeviceKind as K;
    match k {
        K::Router => "router",
        K::Computer => "computer",
        K::Phone => "phone",
        K::Tablet => "tablet",
        K::Tv => "tv",
        K::Speaker => "speaker",
        K::Printer => "printer",
        K::Nas => "nas",
        K::Camera => "camera",
        K::Iot => "iot",
        K::Unknown => "unknown",
    }
    .to_string()
}

/// 新面孔判定：**终身制已知集**。
///
/// 与 M19 端口保镖同构（基线 + 边沿检测），三处差异：
/// 1. 身份键是 IP 而不是端口（M18 spec §9：随机化 MAC 让 MAC 键不可靠；
///    DHCP 换址会误报一次、新机拿旧 IP 会漏报一次——接受，不猜）
/// 2. 已知集**只进不出**：邻居表条目会因 expiry 消失又回来，出集会导致
///    同一台设备反复弹「新面孔」；入集终身制保证同 IP 至多报一次
/// 3. 网段指纹变化即整体重置：换了 Wi-Fi 后「全是新面孔」不是用户想听的
///    故事，重建基线一条不弹
#[derive(Debug, Default)]
pub struct LanWatchMonitor {
    baseline_done: bool,
    /// 网段指纹（CIDR 串）。`None` = 还没见过任何网段
    scope_seen: Option<String>,
    known: HashSet<String>,
}

impl LanWatchMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 配置变更时调用：整体重置（重开开关的第一轮当作新起点，不补报）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 投入一轮邻居表合成结果（`cidr` = 本轮网段指纹，`None` = 读不到网段）。
    ///
    /// 只看 `online` 设备，跳过本机与网关——网关地址变了（换路由器 / 首轮基线时
    /// 网关不在邻居表里）同样会以「新 IP」出现，但它从来不是「新面孔」。
    pub fn observe(&mut self, cidr: Option<&str>, devices: &[LanDevice]) -> Vec<LanWatchAlert> {
        // 网段变了（或首次见到网段）：重建基线，本拍不报
        if cidr != self.scope_seen.as_deref() {
            self.scope_seen = cidr.map(str::to_string);
            self.baseline_done = false;
        }

        // 基线轮：存量设备全部入集，一条不报
        if !self.baseline_done {
            self.baseline_done = true;
            for d in devices {
                self.known.insert(d.ip.clone());
            }
            return Vec::new();
        }

        let mut alerts = Vec::new();
        for d in devices {
            // 网关不是「新面孔」：路由器重启换了地址、或首轮基线时网关恰好不在
            // 邻居表里，都不该弹「路由器加入了局域网」（方案 §2.2）
            if !d.online || d.is_self || d.is_gateway || self.known.contains(&d.ip) {
                continue;
            }
            self.known.insert(d.ip.clone());
            if alerts.len() >= LAN_MAX_ALERTS_PER_ROUND {
                // 超限的新面孔：计入最后一条的 extra，不静默丢弃
                let last: &mut LanWatchAlert = alerts.last_mut().expect("alerts 非空");
                last.extra += 1;
                continue;
            }
            alerts.push(LanWatchAlert {
                ip: d.ip.clone(),
                label: d.hostname.clone().or(d.vendor.clone()),
                kind: kind_str(d.kind),
                randomized_mac: d.randomized_mac,
                extra: 0,
            });
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{PortBinding, Proto};

    // ---- M23 新面孔 ----

    use crate::net::lan::{DeviceKind, NeighborState};

    /// 造一台局域网设备（默认在线、非本机、非网关）
    fn device(ip: &str) -> LanDevice {
        LanDevice {
            ip: ip.to_string(),
            mac: None,
            vendor: None,
            hostname: None,
            kind: DeviceKind::Unknown,
            state: NeighborState::Reachable,
            online: true,
            services: vec![],
            inbound: vec![],
            is_gateway: false,
            is_self: false,
            randomized_mac: false,
            first_seen: 0,
            last_seen: 0,
            is_new: false,
        }
    }

    const CIDR: &str = "192.168.0.0/23";

    #[test]
    fn lan_baseline_records_nothing() {
        let mut m = LanWatchMonitor::new();
        let alerts = m.observe(Some(CIDR), &[device("192.168.0.44"), device("192.168.1.1")]);
        assert!(alerts.is_empty(), "基线轮一条不报");
        // 下一轮同样的设备还是安静
        assert!(m.observe(Some(CIDR), &[device("192.168.0.44")]).is_empty());
    }

    #[test]
    fn lan_new_face_alerts_once() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[device("192.168.0.44")]);
        let alerts = m.observe(Some(CIDR), &[device("192.168.0.77")]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].ip, "192.168.0.77");
        assert_eq!(alerts[0].extra, 0);
        // 终身制：同一 IP 消失再出现也不重报（邻居表 expiry 场景）
        assert!(m.observe(Some(CIDR), &[device("192.168.0.77")]).is_empty());
    }

    #[test]
    fn lan_offline_and_self_are_ignored() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[]);
        let mut offline = device("192.168.0.90");
        offline.online = false;
        let mut me = device("192.168.0.115");
        me.is_self = true;
        let alerts = m.observe(Some(CIDR), &[offline.clone(), me]);
        assert!(alerts.is_empty(), "离线幽灵与本机都不报");
        // 且不入已知集：离线设备真正上线时会报
        offline.online = true;
        let alerts = m.observe(Some(CIDR), &[offline]);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn lan_gateway_joins_baseline_not_alert() {
        let mut m = LanWatchMonitor::new();
        let mut gw = device("192.168.1.1");
        gw.is_gateway = true;
        m.observe(Some(CIDR), &[gw.clone()]);
        // 网关一直在场：不报
        assert!(m.observe(Some(CIDR), &[gw]).is_empty());
    }

    #[test]
    fn lan_gateway_arriving_after_baseline_is_not_a_new_face() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[device("192.168.0.44")]);
        // 路由器重启换了地址 / 首轮基线时网关不在邻居表：新 IP 也不是「新面孔」
        let mut gw = device("192.168.0.1");
        gw.is_gateway = true;
        assert!(
            m.observe(Some(CIDR), &[gw]).is_empty(),
            "网关从不作为新面孔提报"
        );
    }

    #[test]
    fn lan_scope_change_rebaselines() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[device("192.168.0.44")]);
        // 换了 Wi-Fi：新网段里全是「新」设备 → 重置基线，一条不弹
        let alerts = m.observe(Some("10.0.0.0/24"), &[device("10.0.0.5")]);
        assert!(alerts.is_empty(), "网段变化必须重建基线");
        // 之后新网段的新设备正常报
        let alerts = m.observe(Some("10.0.0.0/24"), &[device("10.0.0.9")]);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn lan_round_cap_merges_extra() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[]);
        let flood: Vec<LanDevice> = (1..=5)
            .map(|i| device(&format!("192.168.0.1{i}")))
            .collect();
        let alerts = m.observe(Some(CIDR), &flood);
        assert_eq!(alerts.len(), LAN_MAX_ALERTS_PER_ROUND, "单轮只报 3 条");
        assert_eq!(alerts[2].extra, 2, "余量合并进最后一条");
        assert_eq!(alerts[0].extra, 0);
    }

    #[test]
    fn lan_label_prefers_hostname_then_vendor() {
        let mut m = LanWatchMonitor::new();
        m.observe(Some(CIDR), &[]);
        let mut d = device("192.168.0.77");
        d.vendor = Some("Apple, Inc.".into());
        let alerts = m.observe(Some(CIDR), &[d]);
        assert_eq!(alerts[0].label.as_deref(), Some("Apple, Inc."));
        let mut d2 = device("192.168.0.78");
        d2.hostname = Some("nas.local".into());
        d2.vendor = Some("Synology".into());
        let alerts = m.observe(Some(CIDR), &[d2]);
        assert_eq!(alerts[0].label.as_deref(), Some("nas.local"));
        assert_eq!(alerts[0].kind, "unknown");
    }

    #[test]
    fn lan_kind_string_matches_serde() {
        // kind_str 与 LanDevice 的 serde rename_all = "lowercase" 口径一致
        let k = crate::net::lan::DeviceKind::Nas;
        assert_eq!(kind_str(k), "nas");
        assert_eq!(kind_str(crate::net::lan::DeviceKind::Phone), "phone");
    }

    /// 造一条 TCP 监听 binding
    fn bind(port: u16, pid: i32, comm: &str) -> PortBinding {
        PortBinding {
            port,
            proto: Proto::Tcp,
            scope: BindScope::Loopback,
            addr: "127.0.0.1".into(),
            v6: false,
            pid: Some(pid),
            comm: comm.into(),
            start_time: 0,
            project: None,
            project_label: None,
        }
    }

    /// 造一行 PortEntry
    fn entry(port: u16, watched: bool, listeners: Vec<PortBinding>) -> PortEntry {
        PortEntry {
            port,
            proto: Proto::Tcp,
            exposure: BindScope::Loopback,
            conns: Some(0),
            listeners,
            watched,
            note: None,
            service: None,
        }
    }

    fn observe(m: &mut PortGuardMonitor, now: u64, entries: &[PortEntry]) -> Vec<PortGuardAlert> {
        m.observe(now, entries)
    }

    #[test]
    fn baseline_round_is_silent() {
        // Prune 启动时机器上已有 20+ 监听端口，全是存量：一条不弹
        let mut m = PortGuardMonitor::new();
        let entries = vec![entry(3000, true, vec![bind(3000, 1, "node")])];
        assert!(observe(&mut m, 0, &entries).is_empty(), "首轮基线不应提报");
    }

    #[test]
    fn new_occupied_watched_port_alerts() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let entries = vec![entry(3000, true, vec![bind(3000, 42, "node")])];
        let alerts = observe(&mut m, 1_000, &entries);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, PortGuardKind::NewListen);
        assert_eq!(alerts[0].port, 3000);
        assert_eq!(alerts[0].comm, "node");
    }

    #[test]
    fn unwatched_new_port_is_silent() {
        // 随机高位内部 IPC（Cursor Helper 之类）不该弹
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let entries = vec![entry(50272, false, vec![bind(50272, 7, "Cursor Helper")])];
        assert!(observe(&mut m, 1_000, &entries).is_empty());
    }

    #[test]
    fn occupied_port_staying_up_does_not_repeat() {
        let mut m = PortGuardMonitor::new();
        let entries = vec![entry(3000, true, vec![bind(3000, 42, "node")])];
        observe(&mut m, 0, &entries); // 建基线（已占用）
        assert!(
            observe(&mut m, 30_000, &entries).is_empty(),
            "持续占用不应重复报"
        );
    }

    #[test]
    fn freed_then_occupied_realerts_after_cooldown() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let up = vec![entry(3000, true, vec![bind(3000, 42, "node")])];
        assert!(observe(&mut m, 1_000, &up).len() == 1);
        // 端口释放 → 从集合移除（重新武装）
        observe(&mut m, 2_000, &[]);
        // 冷却期内（< 10 min）再次占用：不弹
        assert!(observe(&mut m, 60_000, &up).is_empty(), "冷却期内不应再报");
        // 冷却过后再占用：弹
        let alerts = observe(&mut m, 2_000, &[]);
        assert!(alerts.is_empty());
        let alerts = observe(&mut m, PORT_COOLDOWN_MS + 60_000, &up);
        assert_eq!(alerts.len(), 1, "冷却过后应能再次触发");
    }

    #[test]
    fn conflict_takes_precedence_and_alerts() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        // 两个进程同时 LISTEN 8080（SO_REUSEPORT / 冲突）
        let entries = vec![entry(
            8080,
            true,
            vec![bind(8080, 100, "node"), bind(8080, 200, "node")],
        )];
        let alerts = observe(&mut m, 1_000, &entries);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, PortGuardKind::Conflict);
        assert_eq!(alerts[0].owners, 2);
    }

    #[test]
    fn conflict_on_prewatched_port_still_alerts() {
        // 端口一直在占用（基线里就有），但**突然**出现第二个持有者 → 冲突必须报
        let mut m = PortGuardMonitor::new();
        let single = vec![entry(8080, true, vec![bind(8080, 100, "node")])];
        observe(&mut m, 0, &single);
        let dual = vec![entry(
            8080,
            true,
            vec![bind(8080, 100, "node"), bind(8080, 200, "node")],
        )];
        let alerts = observe(&mut m, 1_000, &dual);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, PortGuardKind::Conflict);
    }

    #[test]
    fn udp_bindings_are_ignored() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let mut udp = entry(5353, true, vec![bind(5353, 1, "mDNSResponder")]);
        udp.proto = Proto::Udp;
        assert!(
            observe(&mut m, 1_000, &[udp]).is_empty(),
            "UDP 绑定抖动不值得报"
        );
    }

    #[test]
    fn reset_rebuilds_baseline() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let up = vec![entry(3000, true, vec![bind(3000, 42, "node")])];
        assert!(observe(&mut m, 1_000, &up).len() == 1);
        m.reset();
        // reset 后第一轮是新基线：不补报
        assert!(observe(&mut m, 2_000, &up).is_empty());
    }

    #[test]
    fn alert_carries_project_label_and_exposure() {
        let mut m = PortGuardMonitor::new();
        observe(&mut m, 0, &[]);
        let mut b = bind(3118, 99, "node");
        b.project_label = Some("prune".into());
        let mut e = entry(3118, true, vec![b]);
        e.exposure = BindScope::Any;
        let alerts = observe(&mut m, 1_000, &[e]);
        assert_eq!(alerts[0].project_label.as_deref(), Some("prune"));
        assert_eq!(alerts[0].exposure, BindScope::Any);
    }

    fn th() -> CpuThresholds {
        CpuThresholds::default()
    }

    /// 喂一串采样（间隔 2 s），返回**第一次**触发的结果。
    ///
    /// 不能用「最后一次」的返回值判断是否触发过：触发发生后 `armed` 变 false，
    /// 后续 30 次都返回 None，最后一次必然是 None —— 会得出「没触发」的错误结论。
    fn feed(m: &mut CpuMonitor, values: &[f32], start_ms: u64) -> Option<CpuAlert> {
        values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| m.observe(start_ms + i as u64 * 2000, *v, &th()))
            .next()
    }

    #[test]
    fn sustained_hot_triggers() {
        let mut m = CpuMonitor::new();
        let vals = vec![95.0f32; 31];
        assert!(feed(&mut m, &vals, 0).is_some(), "持续 62 s 的 95% 应触发");
    }

    #[test]
    fn single_spike_does_not_trigger() {
        let mut m = CpuMonitor::new();
        let mut vals = vec![20.0f32; 31];
        vals[15] = 100.0;
        assert!(
            feed(&mut m, &vals, 0).is_none(),
            "单点尖峰不应触发（均值会被拉高，达标率不会）"
        );
    }

    #[test]
    fn insufficient_samples_does_not_trigger() {
        let mut m = CpuMonitor::new();
        // 只有 4 个点（8 s），远未攒满 60 s 窗口
        assert!(feed(&mut m, &[99.0; 4], 0).is_none());
    }

    #[test]
    fn startup_guard_blocks_until_full_window() {
        let mut m = CpuMonitor::new();
        // 前 58 s 全 100%，但 elapsed 未到 60 s
        let vals = vec![100.0f32; 29];
        assert!(feed(&mut m, &vals, 0).is_none(), "58 s 时还不该触发");
    }

    #[test]
    fn hysteresis_prevents_repeat_while_still_hot() {
        let mut m = CpuMonitor::new();
        assert!(feed(&mut m, &[95.0; 31], 0).is_some(), "首次应触发");
        // 继续维持 95%：已解除武装，不应再报
        assert!(
            feed(&mut m, &[95.0; 30], 62_000).is_none(),
            "未掉到 80% 以下前不应重复触发"
        );
    }

    #[test]
    fn rearms_after_dropping_below_threshold_minus_hysteresis() {
        let mut m = CpuMonitor::new();
        assert!(feed(&mut m, &[95.0; 31], 0).is_some());
        // 掉到 70%（< 90 − 10）→ 重新武装
        assert!(feed(&mut m, &[70.0; 5], 62_000).is_none());
        // 再回到 95% 并维持一个窗口 → 应重新触发
        let later = 62_000 + 5 * 2000;
        assert!(
            feed(&mut m, &[95.0; 31], later).is_some(),
            "回到高位后应能再次触发"
        );
    }

    #[test]
    fn reset_clears_window_and_rearms() {
        let mut m = CpuMonitor::new();
        assert!(feed(&mut m, &[95.0; 31], 0).is_some());
        m.reset();
        // reset 后窗口为空，立刻喂 31 个点：启动保护会先吃掉一轮
        assert!(feed(&mut m, &[95.0; 5], 100_000).is_none());
    }

    #[test]
    fn changing_threshold_down_without_reset_would_misfire() {
        // 这个测试记录「为什么改配置必须 reset」：
        // 90% 判定下攒了 30 个 85% 的点（不达标），把阈值降到 80% 后，
        // 若沿用旧窗口，这 30 个点会立刻全部达标并触发。
        let mut m = CpuMonitor::new();
        assert!(feed(&mut m, &[85.0; 31], 0).is_none(), "90% 阈值下不该触发");

        let lower = CpuThresholds {
            threshold_pct: 80.0,
            ..Default::default()
        };
        // 不 reset，直接按新阈值判下一个点 → 旧样本全部达标，会触发
        let misfire = m.observe(64_000, 85.0, &lower);
        assert!(
            misfire.is_some(),
            "旧窗口按新阈值会误触发（这是 reset 存在的理由）"
        );

        m.reset();
        assert!(
            m.observe(70_000, 85.0, &lower).is_none(),
            "reset 之后同一条数据不应再误触发"
        );
    }

    #[test]
    fn clamped_rejects_dangerous_values() {
        let bad = CpuThresholds {
            threshold_pct: 0.0,
            sustain_sec: 0,
            tolerance_pct: 0.0,
        }
        .clamped();
        assert_eq!(bad.threshold_pct, 60.0);
        assert_eq!(bad.sustain_sec, 10);
        assert_eq!(bad.tolerance_pct, 50.0);

        let high = CpuThresholds {
            threshold_pct: 999.0,
            sustain_sec: 99_999,
            tolerance_pct: 999.0,
        }
        .clamped();
        assert_eq!(high.threshold_pct, 100.0);
        assert_eq!(high.sustain_sec, 600);
        assert_eq!(high.tolerance_pct, 100.0);
    }

    #[test]
    fn rate_limiter_caps_per_hour() {
        let mut rl = RateLimiter::new(6);
        for i in 0..6 {
            assert!(rl.allow(i * 1000), "第 {} 次应放行", i + 1);
        }
        assert!(!rl.allow(6000), "第 7 次应被拒");
        // 一小时后第一个时间戳过期，重新有额度
        assert!(rl.allow(3_600_000 + 1000));
    }

    #[test]
    fn rejected_attempt_does_not_consume_quota() {
        let mut rl = RateLimiter::new(2);
        assert!(rl.allow(0));
        assert!(rl.allow(1000));
        assert!(!rl.allow(2000));
        // 一小时后第一个时间戳过期，额度回到 1
        assert!(rl.allow(3_600_001));
    }

    // ───────────── MemMonitor ─────────────

    use crate::mem::PressureLevel as PL;

    fn mem_th() -> MemThresholds {
        MemThresholds::default()
    }

    /// 喂一串 (压力档, swap 占用率)，间隔 2 s，返回第一次触发。
    fn feed_mem(
        m: &mut MemMonitor,
        levels: &[PL],
        ratios: &[f32],
        start_ms: u64,
    ) -> Option<MemAlert> {
        levels
            .iter()
            .zip(
                ratios
                    .iter()
                    .chain(std::iter::repeat(&ratios[ratios.len() - 1])),
            )
            .enumerate()
            .filter_map(|(i, (lv, r))| m.observe(start_ms + i as u64 * 2000, *lv, *r, &mem_th()))
            .next()
    }

    #[test]
    fn mem_critical_fires_immediately() {
        let mut m = MemMonitor::new();
        let a = feed_mem(&mut m, &[PL::Critical], &[30.0], 0);
        assert!(a.is_some(), "内核判 Critical 应立即触发，无需持续窗口");
        assert_eq!(a.unwrap().trigger, MemTrigger::Pressure);
    }

    #[test]
    fn mem_warning_needs_sustain_window() {
        let mut m = MemMonitor::new();
        // 28 s（14 个点）不够 30 s
        let got = feed_mem(&mut m, &[PL::Warning; 14], &[30.0], 0);
        assert!(got.is_none(), "28 s 不应触发");
        // 第 15 个点（28 s 起，30 s 处到达）凑满持续窗口
        let got = feed_mem(&mut m, &[PL::Warning; 2], &[30.0], 28_000);
        assert!(got.is_some(), "持续 30 s 的 Warning 应触发");
    }

    #[test]
    fn mem_warning_lapse_resets_sustain_timer() {
        let mut m = MemMonitor::new();
        // Warning 28 s → Normal 一拍 → 再 Warning 28 s：两段都不满 30 s
        let seg = [PL::Warning; 14];
        assert!(feed_mem(&mut m, &seg, &[30.0], 0).is_none());
        assert!(feed_mem(&mut m, &[PL::Normal], &[30.0], 28_000).is_none());
        assert!(
            feed_mem(&mut m, &seg, &[30.0], 30_000).is_none(),
            "中断后计时清零，连续性不足不应触发"
        );
    }

    #[test]
    fn mem_warning_rearms_only_at_normal() {
        let mut m = MemMonitor::new();
        assert!(
            feed_mem(&mut m, &[PL::Warning; 16], &[30.0], 0).is_some(),
            "首次触发"
        );
        // Warning 挂着不放：不应重复报
        assert!(
            feed_mem(&mut m, &[PL::Warning; 60], &[30.0], 34_000).is_none(),
            "未回 Normal 前不应重复触发"
        );
        // 回 Normal → 重新武装 → 再持续 Warning 30 s → 再报
        assert!(feed_mem(&mut m, &[PL::Normal], &[30.0], 154_000).is_none());
        let later = 156_000;
        assert!(
            feed_mem(&mut m, &[PL::Warning; 16], &[30.0], later).is_some(),
            "回到 Normal 重新武装后应能再次触发"
        );
    }

    #[test]
    fn mem_swap_fires_after_sustain() {
        let mut m = MemMonitor::new();
        let got = feed_mem(&mut m, &[PL::Normal; 16], &[94.0], 0);
        assert!(got.is_some(), "swap 94% 持续 30 s 应触发");
        assert_eq!(got.unwrap().trigger, MemTrigger::Swap);
    }

    #[test]
    fn mem_swap_dip_resets_but_short_dip_does_not_rearm() {
        let mut m = MemMonitor::new();
        // 28 s 达标 → 掉到 75%（低于 80 但高于 70 滞回线）→ 计时清零但不重武装
        assert!(feed_mem(&mut m, &[PL::Normal; 14], &[94.0], 0).is_none());
        assert!(feed_mem(&mut m, &[PL::Normal], &[75.0], 28_000).is_none());
        assert!(
            feed_mem(&mut m, &[PL::Normal; 14], &[94.0], 30_000).is_none(),
            "未掉到 70 以下前不重新武装，即便攒满持续窗口也不该再报"
        );
        // 掉到 70 以下 → 重新武装 → 再持续 → 再报
        assert!(feed_mem(&mut m, &[PL::Normal; 3], &[60.0], 58_000).is_none());
        let later = 64_000;
        assert!(
            feed_mem(&mut m, &[PL::Normal; 16], &[94.0], later).is_some(),
            "掉到滞回线以下重新武装后应能再次触发"
        );
    }

    #[test]
    fn mem_both_channels_fire_same_tick() {
        let mut m = MemMonitor::new();
        // Warning 持续 30 s 且 swap 同时 94%：同拍命中两条 → Both
        let got = feed_mem(&mut m, &[PL::Warning; 16], &[94.0], 0);
        let a = got.expect("两通道同拍应触发");
        assert_eq!(a.trigger, MemTrigger::Both);
        assert_eq!(a.pressure, PL::Warning);
    }

    #[test]
    fn mem_critical_preempts_warning_accumulation() {
        let mut m = MemMonitor::new();
        // Warning 攒 14 s（未触发）→ 突然 Critical → 立即报 Critical
        assert!(feed_mem(&mut m, &[PL::Warning; 7], &[30.0], 0).is_none());
        let got = feed_mem(&mut m, &[PL::Critical], &[30.0], 14_000);
        let a = got.expect("Critical 应立即抢占");
        assert_eq!(a.pressure, PL::Critical);
    }

    #[test]
    fn mem_missing_swap_data_does_not_fire_channel_c() {
        let mut m = MemMonitor::new();
        // swap 读不到（负比率），压力一直 Normal → 永不触发
        assert!(feed_mem(&mut m, &[PL::Normal; 30], &[-1.0], 0).is_none());
    }

    #[test]
    fn mem_reset_clears_all_channels() {
        let mut m = MemMonitor::new();
        assert!(feed_mem(&mut m, &[PL::Warning; 16], &[94.0], 0).is_some());
        m.reset();
        // 触发过的通道必须重新武装：再喂同样序列，仍要能走满整个持续窗口
        assert!(feed_mem(&mut m, &[PL::Warning; 3], &[94.0], 100_000).is_none());
    }

    #[test]
    fn mem_thresholds_clamped() {
        let bad = MemThresholds {
            swap_pct: 0.0,
            sustain_sec: 0,
        }
        .clamped();
        assert_eq!(bad.swap_pct, 50.0);
        assert_eq!(bad.sustain_sec, 10);

        let high = MemThresholds {
            swap_pct: 99.0,
            sustain_sec: 99_999,
        }
        .clamped();
        assert_eq!(high.swap_pct, 95.0);
        assert_eq!(high.sustain_sec, 300);
    }

    #[test]
    fn would_allow_does_not_consume_quota() {
        let mut rl = RateLimiter::new(1);
        assert!(rl.would_allow(0), "查一次不消耗配额");
        assert!(rl.allow(0), "查完还能放行");
        assert!(!rl.would_allow(1000), "满额后查询应返回 false");
    }
}
