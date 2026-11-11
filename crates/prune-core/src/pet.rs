//! 机器宠物状态机（M20，docs/f1-pet-mood-plan.md §3.1）：纯逻辑 + 可单测。
//!
//! 小猫托盘皮的「心情」由机器健康决定：悠闲 / 狂奔 / 喘气 / 炸毛四态。
//! 设计要点与 [`crate::island`] 的监视器同族：
//! - **水平量看状态持续**：swap / 磁盘是慢变量，用连续计时而不是达标率
//!   （CPU 已由 EMA 平滑，进入/退出各配滞回线防临界抖动）；
//! - **读不到不冒充**：任一数据源传 `None`，对应通道安静跳过——
//!   不会拿 0 误判「充裕」导致状态卡死；
//! - **优先级每拍重算**：喘气 > 狂奔 > 炸毛 > 悠闲。内存危机最紧急；
//!   CPU 高是「正在卖力」（狂奔），比慢性的磁盘将满（炸毛）更有即时信息量；
//! - **退出滞回与状态绑定**：只有当前正处于该状态时才查它的退出计时，
//!   低优先级通道在高位时持续累计进入计时，高优先级一解除立即按优先级接管。

use crate::mem::PressureLevel;

/// 宠物心情。Leisure / Sprint 播同一套前跃帧（Sprint 只是更快），
/// Pant / Alert 有独立帧集。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PetMood {
    /// 悠闲：默认态，前跃循环按 CPU 快慢
    #[default]
    Leisure,
    /// 狂奔：CPU 持续高位，连续扑跳
    Sprint,
    /// 喘气：内存紧张（swap 高危 / 内核压力 Warning+），趴下喘气
    Pant,
    /// 炸毛：磁盘将满，弓背警戒
    Alert,
}

impl PetMood {
    /// 托盘 tooltip 的心情词；悠闲不标注（正常态不值得占字符）。
    pub fn label(self) -> Option<&'static str> {
        match self {
            PetMood::Leisure => None,
            PetMood::Sprint => Some("狂奔"),
            PetMood::Pant => Some("喘气"),
            PetMood::Alert => Some("炸毛"),
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            PetMood::Leisure => 0,
            PetMood::Sprint => 1,
            PetMood::Pant => 2,
            PetMood::Alert => 3,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => PetMood::Sprint,
            2 => PetMood::Pant,
            3 => PetMood::Alert,
            _ => PetMood::Leisure,
        }
    }
}

/// 喘气通道：swap 占用率进入线（%）。
pub const PANT_ENTER_SWAP_PCT: f32 = 80.0;
/// 喘气通道：swap 退出滞回线（%）。
pub const PANT_EXIT_SWAP_PCT: f32 = 70.0;
/// 喘气进入需要连续紧张的时长（毫秒）。
pub const PANT_SUSTAIN_MS: u64 = 30_000;
/// 喘气退出需要连续平静的时长（毫秒）。
pub const PANT_CALM_MS: u64 = 15_000;

/// 狂奔通道：CPU EMA 进入线（%）。
pub const SPRINT_ENTER_CPU: f32 = 85.0;
/// 狂奔通道：CPU 退出滞回线（%）。
pub const SPRINT_EXIT_CPU: f32 = 75.0;
/// 狂奔进入 / 退出的连续计时（毫秒）。
pub const SPRINT_SUSTAIN_MS: u64 = 10_000;
pub const SPRINT_CALM_MS: u64 = 10_000;

/// 炸毛通道：home 卷剩余占比进入线。
pub const ALERT_ENTER_FREE_RATIO: f32 = 0.10;
/// 炸毛通道：剩余占比退出滞回线。
pub const ALERT_EXIT_FREE_RATIO: f32 = 0.12;
/// 炸毛进入 / 退出的连续计时（毫秒）。磁盘是慢性信号，进慢出慢防闪切。
pub const ALERT_SUSTAIN_MS: u64 = 60_000;
pub const ALERT_CALM_MS: u64 = 30_000;

/// 单通道连续计时器：条件成立累计时长，掉线清零。
#[derive(Debug, Default)]
struct SustainTimer {
    started_ms: Option<u64>,
}

impl SustainTimer {
    fn tick(&mut self, now_ms: u64, cond: bool) {
        if cond {
            self.started_ms.get_or_insert(now_ms);
        } else {
            self.started_ms = None;
        }
    }

    fn sustained(&self, now_ms: u64, target_ms: u64) -> bool {
        self.started_ms
            .map(|t| now_ms.saturating_sub(t) >= target_ms)
            .unwrap_or(false)
    }
}

/// 宠物状态机。`observe` 每 1 s 喂一次（与 tray 的 CPU watcher 同拍）。
#[derive(Debug, Default)]
pub struct PetMoodMachine {
    current: PetMood,
    pant_enter: SustainTimer,
    pant_exit: SustainTimer,
    sprint_enter: SustainTimer,
    sprint_exit: SustainTimer,
    alert_enter: SustainTimer,
    alert_exit: SustainTimer,
}

impl PetMoodMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 投入一拍读数，返回当前心情。
    ///
    /// - `cpu`：EMA 平滑后的整机 CPU%（tray 侧 1 Hz watcher）
    /// - `swap_ratio_pct`：swap 占用率 ×100；None = 平台无此口径，通道跳过
    /// - `pressure`：内核压力档；None = 读不到，通道跳过
    /// - `disk_free_ratio`：home 卷剩余占比 0–1；None = 读不到，通道跳过
    pub fn observe(
        &mut self,
        now_ms: u64,
        cpu: Option<f32>,
        swap_ratio_pct: Option<f32>,
        pressure: Option<PressureLevel>,
        disk_free_ratio: Option<f32>,
    ) -> PetMood {
        // ── 喘气通道 ──
        let pant_stressed = match (swap_ratio_pct, pressure) {
            (Some(r), Some(p)) => r >= PANT_ENTER_SWAP_PCT || p != PressureLevel::Normal,
            (Some(r), None) => r >= PANT_ENTER_SWAP_PCT,
            (None, Some(p)) => p != PressureLevel::Normal,
            // 两个源都读不到：通道保持安静，也不能冒充「平静」
            (None, None) => false,
        };
        let pant_calm = match (swap_ratio_pct, pressure) {
            (Some(r), Some(p)) => r < PANT_EXIT_SWAP_PCT && p == PressureLevel::Normal,
            (Some(r), None) => r < PANT_EXIT_SWAP_PCT,
            (None, Some(p)) => p == PressureLevel::Normal,
            (None, None) => false,
        };
        self.pant_enter.tick(now_ms, pant_stressed);
        self.pant_exit.tick(now_ms, pant_calm);

        // ── 狂奔通道 ──
        let cpu_hot = cpu.map(|c| c >= SPRINT_ENTER_CPU).unwrap_or(false);
        let cpu_cool = cpu.map(|c| c < SPRINT_EXIT_CPU).unwrap_or(false);
        self.sprint_enter.tick(now_ms, cpu_hot);
        self.sprint_exit.tick(now_ms, cpu_cool);

        // ── 炸毛通道 ──
        let disk_full = disk_free_ratio
            .map(|r| r <= ALERT_ENTER_FREE_RATIO)
            .unwrap_or(false);
        let disk_ok = disk_free_ratio
            .map(|r| r > ALERT_EXIT_FREE_RATIO)
            .unwrap_or(false);
        self.alert_enter.tick(now_ms, disk_full);
        self.alert_exit.tick(now_ms, disk_ok);

        // ── 优先级裁决。各通道「活跃」的定义按当前状态二选一：
        // 不在该状态 → 看进入计时（要连续保持 SUSTAIN 才进）；
        // 已在该状态 → 看退出计时（要连续平静 CALM 才出，滞回防抖）。
        let pant_active = match self.current {
            PetMood::Pant => !self.pant_exit.sustained(now_ms, PANT_CALM_MS),
            _ => self.pant_enter.sustained(now_ms, PANT_SUSTAIN_MS),
        };
        let sprint_active = match self.current {
            PetMood::Sprint => !self.sprint_exit.sustained(now_ms, SPRINT_CALM_MS),
            _ => self.sprint_enter.sustained(now_ms, SPRINT_SUSTAIN_MS),
        };
        let alert_active = match self.current {
            PetMood::Alert => !self.alert_exit.sustained(now_ms, ALERT_CALM_MS),
            _ => self.alert_enter.sustained(now_ms, ALERT_SUSTAIN_MS),
        };

        let mood = if pant_active {
            PetMood::Pant
        } else if sprint_active {
            PetMood::Sprint
        } else if alert_active {
            PetMood::Alert
        } else {
            PetMood::Leisure
        };
        self.current = mood;
        mood
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 快捷喂入（全部读数）
    #[allow(clippy::too_many_arguments)]
    fn feed_with(
        m: &mut PetMoodMachine,
        now: u64,
        cpu: Option<f32>,
        swap_ratio_pct: Option<f32>,
        pressure: Option<PressureLevel>,
        disk_free_ratio: Option<f32>,
    ) -> PetMood {
        m.observe(now, cpu, swap_ratio_pct, pressure, disk_free_ratio)
    }

    fn feed(m: &mut PetMoodMachine, now: u64, cpu: Option<f32>) -> PetMood {
        m.observe(now, cpu, None, None, None)
    }

    #[test]
    fn leisure_by_default() {
        let mut m = PetMoodMachine::new();
        assert_eq!(feed(&mut m, 0, Some(10.0)), PetMood::Leisure);
    }

    #[test]
    fn sprint_needs_sustained_hot_cpu() {
        let mut m = PetMoodMachine::new();
        // 9 s 的 90%：不够 10 s
        for i in 0..9 {
            assert_eq!(feed(&mut m, i * 1000, Some(90.0)), PetMood::Leisure);
        }
        assert_eq!(
            feed(&mut m, 10_000, Some(90.0)),
            PetMood::Sprint,
            "持续 10 s 应进入狂奔"
        );
    }

    #[test]
    fn sprint_lapse_resets_timer() {
        let mut m = PetMoodMachine::new();
        for i in 0..9 {
            feed(&mut m, i * 1000, Some(90.0));
        }
        // 中间一拍掉到 20%：计时清零
        feed(&mut m, 9_000, Some(20.0));
        assert_eq!(feed(&mut m, 10_000, Some(90.0)), PetMood::Leisure);
    }

    #[test]
    fn sprint_exits_only_below_hysteresis_line() {
        let mut m = PetMoodMachine::new();
        for i in 0..=10 {
            feed(&mut m, i * 1000, Some(90.0));
        }
        // 80%：高于退出线 75%，不累计退出
        assert_eq!(feed(&mut m, 11_000, Some(80.0)), PetMood::Sprint);
        for i in 0..10 {
            feed(&mut m, 12_000 + i * 1000, Some(70.0));
        }
        assert_eq!(
            feed(&mut m, 22_000, Some(70.0)),
            PetMood::Leisure,
            "持续低于滞回线应退出"
        );
    }

    #[test]
    fn pant_triggers_on_sustained_high_swap() {
        let mut m = PetMoodMachine::new();
        for i in 0..30 {
            assert_eq!(
                feed_with(
                    &mut m,
                    i * 1000,
                    Some(10.0),
                    Some(94.0),
                    Some(PressureLevel::Normal),
                    None
                ),
                PetMood::Leisure
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                30_000,
                Some(10.0),
                Some(94.0),
                Some(PressureLevel::Normal),
                None
            ),
            PetMood::Pant,
            "swap 94% 持续 30 s 应喘气"
        );
    }

    #[test]
    fn pant_triggers_on_kernel_pressure() {
        let mut m = PetMoodMachine::new();
        for i in 0..30 {
            feed_with(
                &mut m,
                i * 1000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Warning),
                None,
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                30_000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Warning),
                None
            ),
            PetMood::Pant,
            "压力 Warning 持续 30 s 应喘气"
        );
    }

    #[test]
    fn pant_exits_after_sustained_calm() {
        let mut m = PetMoodMachine::new();
        for i in 0..=30 {
            feed_with(
                &mut m,
                i * 1000,
                Some(10.0),
                Some(94.0),
                Some(PressureLevel::Normal),
                None,
            );
        }
        // 回到 swap 60%：低于退出线 70%，持续 15 s 才恢复
        assert_eq!(
            feed_with(
                &mut m,
                31_000,
                Some(10.0),
                Some(60.0),
                Some(PressureLevel::Normal),
                None
            ),
            PetMood::Pant
        );
        for i in 0..15 {
            feed_with(
                &mut m,
                32_000 + i * 1000,
                Some(10.0),
                Some(60.0),
                Some(PressureLevel::Normal),
                None,
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                47_000,
                Some(10.0),
                Some(60.0),
                Some(PressureLevel::Normal),
                None
            ),
            PetMood::Leisure
        );
    }

    #[test]
    fn pant_stays_while_swap_between_lines() {
        let mut m = PetMoodMachine::new();
        for i in 0..=30 {
            feed_with(
                &mut m,
                i * 1000,
                Some(10.0),
                Some(94.0),
                Some(PressureLevel::Normal),
                None,
            );
        }
        // 75%：高于退出线 70%——保持喘气（退出计时被清零）
        assert_eq!(
            feed_with(
                &mut m,
                31_000,
                Some(10.0),
                Some(75.0),
                Some(PressureLevel::Normal),
                None
            ),
            PetMood::Pant
        );
    }

    #[test]
    fn alert_triggers_on_low_disk() {
        let mut m = PetMoodMachine::new();
        for i in 0..60 {
            assert_eq!(
                feed_with(
                    &mut m,
                    i * 1000,
                    Some(10.0),
                    Some(40.0),
                    Some(PressureLevel::Normal),
                    Some(0.08)
                ),
                PetMood::Leisure
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                60_000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08)
            ),
            PetMood::Alert,
            "磁盘剩余 8% 持续 60 s 应炸毛"
        );
    }

    #[test]
    fn alert_exits_after_sustained_recovery() {
        let mut m = PetMoodMachine::new();
        for i in 0..=60 {
            feed_with(
                &mut m,
                i * 1000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                61_000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.20)
            ),
            PetMood::Alert
        );
        for i in 0..30 {
            feed_with(
                &mut m,
                62_000 + i * 1000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.20),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                92_000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.20)
            ),
            PetMood::Leisure
        );
    }

    #[test]
    fn alert_reclaims_priority_after_pant_ends() {
        // 磁盘满 60 s → 炸毛；随后 swap 也爆了 → 喘气接管；
        // swap 回落平静 15 s → 退出喘气，磁盘仍满 → 应回到炸毛
        let mut m = PetMoodMachine::new();
        for i in 0..=60 {
            feed_with(
                &mut m,
                i * 1000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                61_000,
                Some(10.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08)
            ),
            PetMood::Alert
        );
        for i in 0..=30 {
            feed_with(
                &mut m,
                62_000 + i * 1000,
                Some(10.0),
                Some(94.0),
                Some(PressureLevel::Normal),
                Some(0.08),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                93_000,
                Some(10.0),
                Some(94.0),
                Some(PressureLevel::Normal),
                Some(0.08)
            ),
            PetMood::Pant,
            "喘气优先级高于炸毛"
        );
        for i in 0..15 {
            feed_with(
                &mut m,
                94_000 + i * 1000,
                Some(10.0),
                Some(60.0),
                Some(PressureLevel::Normal),
                Some(0.08),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                109_000,
                Some(10.0),
                Some(60.0),
                Some(PressureLevel::Normal),
                Some(0.08)
            ),
            PetMood::Alert,
            "喘气解除后应回到炸毛（磁盘仍满）"
        );
    }

    #[test]
    fn pant_beats_sprint_beats_alert() {
        let mut m = PetMoodMachine::new();
        // 三通道全部持续达标：喘气最优先
        for i in 0..=60 {
            let mood = m.observe(
                i * 1000,
                Some(95.0),
                Some(94.0),
                Some(PressureLevel::Critical),
                Some(0.05),
            );
            if i >= 60 {
                assert_eq!(mood, PetMood::Pant);
            }
        }
    }

    #[test]
    fn sprint_beats_alert() {
        let mut m = PetMoodMachine::new();
        // CPU 高 + 磁盘满同时喂入：CPU 先到 10 s 时间线，狂奔应先出现
        for i in 0..=10 {
            feed_with(
                &mut m,
                i * 1000,
                Some(95.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08),
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                11_000,
                Some(95.0),
                Some(40.0),
                Some(PressureLevel::Normal),
                Some(0.08)
            ),
            PetMood::Sprint,
            "狂奔优先于炸毛"
        );
    }

    #[test]
    fn missing_data_channels_stay_silent() {
        let mut m = PetMoodMachine::new();
        // 全部读不到：不能拿 0 误判成紧张或平静
        for i in 0..120 {
            assert_eq!(
                m.observe(i * 1000, None, None, None, None),
                PetMood::Leisure,
                "全 None 不应触发任何状态"
            );
        }
    }

    #[test]
    fn missing_swap_does_not_block_pant_via_pressure() {
        let mut m = PetMoodMachine::new();
        // swap 读不到但压力 Critical：喘气通道应靠压力工作
        for i in 0..30 {
            assert_eq!(
                feed_with(
                    &mut m,
                    i * 1000,
                    Some(10.0),
                    None,
                    Some(PressureLevel::Critical),
                    None
                ),
                PetMood::Leisure
            );
        }
        assert_eq!(
            feed_with(
                &mut m,
                30_000,
                Some(10.0),
                None,
                Some(PressureLevel::Critical),
                None
            ),
            PetMood::Pant
        );
    }

    #[test]
    fn mood_labels_and_round_trip() {
        assert_eq!(PetMood::Leisure.label(), None);
        assert_eq!(PetMood::Sprint.label(), Some("狂奔"));
        for m in [
            PetMood::Leisure,
            PetMood::Sprint,
            PetMood::Pant,
            PetMood::Alert,
        ] {
            assert_eq!(PetMood::from_u8(m.as_u8()), m);
        }
        assert_eq!(PetMood::from_u8(42), PetMood::Leisure);
    }
}
