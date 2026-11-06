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
    pub fn observe(
        &mut self,
        now_ms: u64,
        cpu_pct: f32,
        th: &CpuThresholds,
    ) -> Option<CpuAlert> {
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
        let peak = self
            .samples
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        Some(CpuAlert {
            avg_pct: avg,
            peak_pct: peak,
            ratio_pct: ratio,
            samples: total,
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

    /// 允许则记录并返回 true；拒绝不记录（否则被拒的时刻也会占坑）。
    pub fn allow(&mut self, now_ms: u64) -> bool {
        const HOUR_MS: u64 = 3_600_000;
        while let Some(&t) = self.fired_ms.front() {
            if now_ms.saturating_sub(t) >= HOUR_MS {
                self.fired_ms.pop_front();
            } else {
                break;
            }
        }
        if self.fired_ms.len() as u32 >= self.max_per_hour {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(misfire.is_some(), "旧窗口按新阈值会误触发（这是 reset 存在的理由）");

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
}
