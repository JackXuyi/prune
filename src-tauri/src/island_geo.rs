//! 灵动岛几何（M15）：刘海测量与窗口落点，**纯函数**，不碰 AppKit。
//!
//! 调用方（`island.rs`）负责从 `NSScreen` 取原始值填进 [`ScreenGeom`]，
//! 本模块只做算术 —— 这样 2x / 1x / 负坐标 / 无刘海四种情况都能在 CI 里跑。
//!
//! 实测基线（MacBook Air M2 13"，2026-11-06）：
//! `frame=(0,0,1710,1107)`、`safeAreaInsets.top=33`、
//! `auxLeft=(0,1074,763,33)`、`auxRight=(948,1074,762,33)` → 刘海 **185 × 33 pt**。

/// 窗口宽：等于展开态宽度，动画期间不 resize（见方案 §5.1）
pub const WINDOW_W: f64 = 380.0;
/// 窗口高：高于展开态 180，留出阴影与 Spring 过冲的余量
pub const WINDOW_H: f64 = 220.0;
/// 折叠态实际形状
pub const COLLAPSED_W: f64 = 229.0;
pub const COLLAPSED_H: f64 = 50.0;
/// 展开态实际形状
pub const EXPANDED_W: f64 = 380.0;
pub const EXPANDED_H: f64 = 180.0;
/// 窗口顶边故意超出屏幕顶边 1~2 pt：顶边与屏幕齐平时两个方角会暴露在画面里，
/// 观感从「刘海长出来的」变成「悬浮黑盒子」（见方案 §5.2）
pub const EDGE_BLEED: f64 = 2.0;

/// 一块屏幕的原始几何，单位全部是 **逻辑 pt**、Cocoa 坐标（左下原点）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenGeom {
    pub frame_x: f64,
    pub frame_y: f64,
    pub frame_w: f64,
    pub frame_h: f64,
    /// `NSScreen.safeAreaInsets.top`；> 0 才意味着有物理刘海
    pub safe_top: f64,
    pub aux_left_w: Option<f64>,
    pub aux_right_w: Option<f64>,
    /// 主屏高度（pt），用于 Cocoa 左下原点 → Tauri 左上原点的 y 翻转
    pub primary_h: f64,
}

/// 窗口落点，单位逻辑 pt、Tauri 左上原点全局坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    /// 物理刘海尺寸，前端用来对齐内容（文字必须从 `notch_h` 之下开始）
    pub notch_w: f64,
    pub notch_h: f64,
}

/// 算出岛窗口该放哪。返回 `None` = 这块屏不该显示岛（无刘海 / 数据异常）。
///
/// 三条判定：
/// 1. `safe_top <= 0` —— 没有物理刘海（外接屏实测为 0），不造假刘海；
/// 2. 辅助区缺失 —— 老系统或异常机型，拿不到就不猜；
/// 3. 反推宽度不在合理区间 —— 防止 aux 数据错乱时算出负数或超屏宽度。
pub fn place(g: &ScreenGeom) -> Option<Placement> {
    if g.safe_top <= 0.0 || g.frame_w <= 0.0 {
        return None;
    }
    let (left, right) = (g.aux_left_w?, g.aux_right_w?);
    let notch_w = g.frame_w - left - right;
    // 合理区间：至少 80 pt（比任何真实刘海都窄），且不超过屏宽
    if !(80.0..=g.frame_w).contains(&notch_w) {
        return None;
    }

    let center_x = g.frame_x + left + notch_w / 2.0;
    // Cocoa（左下原点，y 向上）→ Tauri（主屏左上原点，y 向下）
    let top_in_tauri = g.primary_h - (g.frame_y + g.frame_h);

    Some(Placement {
        x: center_x - WINDOW_W / 2.0,
        y: top_in_tauri - EDGE_BLEED,
        notch_w,
        notch_h: g.safe_top,
    })
}

/// 当前形状在窗口内的矩形（左上原点、窗口本地坐标），供 hover 命中测试用。
///
/// 窗口永远 380×220，形状在内部居中、顶边对齐 —— 所以命中测试不能直接用
/// 窗口矩形，否则鼠标经过岛下方 100 多 pt 的透明区也会被当成「悬停在岛上」。
pub fn shape_rect(expanded: bool) -> (f64, f64, f64, f64) {
    if expanded {
        (0.0, 0.0, EXPANDED_W, EXPANDED_H)
    } else {
        ((WINDOW_W - COLLAPSED_W) / 2.0, 0.0, COLLAPSED_W, COLLAPSED_H)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机内建屏实测值
    fn builtin() -> ScreenGeom {
        ScreenGeom {
            frame_x: 0.0,
            frame_y: 0.0,
            frame_w: 1710.0,
            frame_h: 1107.0,
            safe_top: 33.0,
            aux_left_w: Some(763.0),
            aux_right_w: Some(762.0),
            primary_h: 1107.0,
        }
    }

    #[test]
    fn builtin_screen_matches_measured_values() {
        let p = place(&builtin()).expect("内建屏应能量出刘海");
        assert_eq!(p.notch_w, 185.0);
        assert_eq!(p.notch_h, 33.0);
        // 居中：855.5 − 190 = 665.5
        assert!((p.x - 665.5).abs() < 0.01, "x 应是 {}", p.x);
        // 窗口顶边超出屏幕 2 pt
        assert!((p.y - (-EDGE_BLEED)).abs() < 0.01, "y 应是 {}", p.y);
    }

    #[test]
    fn screen_without_notch_is_skipped() {
        // 外接 LU28R55 实测：safeAreaInsets.top = 0、aux 为 nil
        let ext = ScreenGeom {
            frame_x: -1920.0,
            frame_y: 0.0,
            frame_w: 1920.0,
            frame_h: 1080.0,
            safe_top: 0.0,
            aux_left_w: None,
            aux_right_w: None,
            primary_h: 1107.0,
        };
        assert!(place(&ext).is_none(), "外接屏不该造假刘海");
    }

    #[test]
    fn missing_aux_area_is_skipped() {
        let mut g = builtin();
        g.aux_right_w = None;
        assert!(place(&g).is_none(), "辅助区缺失时不能猜宽度");
    }

    #[test]
    fn secondary_screen_with_negative_origin() {
        // 假设一块带刘海的副屏挂在主屏左侧、比主屏矮 27 pt（底部对齐）
        let g = ScreenGeom {
            frame_x: -1512.0,
            frame_y: 0.0,
            frame_w: 1512.0,
            frame_h: 982.0,
            safe_top: 32.0,
            aux_left_w: Some(663.5),
            aux_right_w: Some(663.5),
            primary_h: 1107.0,
        };
        let p = place(&g).expect("副屏也能量");
        assert_eq!(p.notch_w, 185.0);
        // 副屏顶部在 Tauri 坐标里是 1107 − 982 = 125
        assert!((p.y - (125.0 - EDGE_BLEED)).abs() < 0.01, "y 应是 {}", p.y);
        // 中心：−1512 + 663.5 + 92.5 = −756 → 窗口 x = −946
        assert!((p.x - (-946.0)).abs() < 0.01, "x 应是 {}", p.x);
    }

    #[test]
    fn corrupted_aux_data_does_not_produce_garbage() {
        // aux 之和超过屏宽 → 反推宽度为负
        let mut g = builtin();
        g.aux_left_w = Some(1000.0);
        g.aux_right_w = Some(900.0);
        assert!(place(&g).is_none(), "负宽度必须拒绝，不能把窗口放到屏外");

        // 反推宽度过窄（远小于任何真实刘海）
        let mut narrow = builtin();
        narrow.aux_left_w = Some(850.0);
        narrow.aux_right_w = Some(850.0);
        assert!(place(&narrow).is_none(), "10 pt 宽的『刘海』不可能是真的");
    }

    #[test]
    fn shape_rect_keeps_top_aligned() {
        let (x, y, w, h) = shape_rect(false);
        assert_eq!((x, y, w, h), (75.5, 0.0, COLLAPSED_W, COLLAPSED_H));
        let (ex, ey, ew, eh) = shape_rect(true);
        assert_eq!((ex, ey, ew, eh), (0.0, 0.0, EXPANDED_W, EXPANDED_H));
        // 折叠态必须水平居中于 380 宽窗口
        assert!((x * 2.0 + w - WINDOW_W).abs() < 0.01);
    }

    #[test]
    fn window_taller_than_expanded_shape() {
        // 窗口高度必须留出余量，否则 Spring 过冲会被裁掉
        assert!(WINDOW_H > EXPANDED_H);
        assert!(WINDOW_W >= EXPANDED_W);
    }
}
