//! 顶部图标主题（M11，docs/m11-tray-icon-theme-plan.md）。
//!
//! macOS 菜单栏 / 其他平台任务栏的图标不再写死成剪刀，而是从一组「主题」里选，
//! 选择结果持久化到 app 专属目录 `settings.json` 的 `trayIconTheme` 键
//! （路径与迁移规则见 `crate::settings`）。
//!
//! 现有主题：
//! - [`IconTheme::Scissors`]  Lucide 剪刀，**黑色模板图** —— macOS 由系统按菜单栏
//!   深浅色反色，视觉上与系统图标同质（默认）
//! - [`IconTheme::WhaleLogo`] 蓝鲸线稿，与剪刀**同风格**：Lucide 24 网格描边
//!   字形，macOS 黑色模板图（系统反色）/ Windows 品牌色 #34D399
//!   （M13 起取代鲸鱼娘，持久化 id 保持 `whale` 不变，已选鲸鱼娘的用户升级后
//!   自动落到新皮；M17 去除 DeepSeek 品牌皮并改为线稿风格，
//!   目录名 `whale-logo` 为历史遗留，随 id 一起保留）
//!
//! 再加一套图标只需动本模块 4 个地方（`id` / `label` / `static_icon` / `frames`），
//! 菜单勾选与持久化由 `IconTheme::ALL` 自动跟随，tray.rs 无需改动。

use std::sync::OnceLock;

use tauri::image::Image;

/// 主题持久化键（与 tray.rs 共用同一个 settings.json，只改本键）
const SETTING_TRAY_ICON: &str = "trayIconTheme";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconTheme {
    /// 剪刀（默认）：黑色模板图
    Scissors,
    /// 蓝鲸线稿：与剪刀同风格的描边字形（M17 起去除 DeepSeek 品牌皮，id 不变）
    WhaleLogo,
}

impl IconTheme {
    /// 菜单展示顺序 = 数组顺序
    pub const ALL: [IconTheme; 2] = [Self::Scissors, Self::WhaleLogo];

    /// 持久化值 & 菜单项 ID 后缀（稳定标识，不要跟着 label 改）
    pub fn id(self) -> &'static str {
        match self {
            Self::Scissors => "scissors",
            Self::WhaleLogo => "whale",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Scissors => "剪刀（默认）",
            Self::WhaleLogo => "鲸鱼",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.id() == raw)
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Scissors => 0,
            Self::WhaleLogo => 1,
        }
    }

    /// 未知值一律退回默认主题（settings.json 被手改成乱七八糟时不至于没图标）
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::WhaleLogo,
            _ => Self::Scissors,
        }
    }

    /// macOS 模板图 = 系统只取 alpha 通道上色，彩色图会被反色成剪影。
    /// 两套主题都是单色字形，macOS 上都走模板；Windows 没有模板机制，恒 false。
    pub fn is_template(self) -> bool {
        cfg!(target_os = "macos")
    }

    /// 静态原图：动画关闭、CPU 静止档、以及创建 tray 时用
    pub fn static_icon(self) -> Image<'static> {
        #[cfg(target_os = "macos")]
        let bytes: &'static [u8] = match self {
            Self::Scissors => include_bytes!("../icons/tray.png"),
            Self::WhaleLogo => include_bytes!("../icons/tray-whale-logo/macos.png"),
        };
        #[cfg(not(target_os = "macos"))]
        let bytes: &'static [u8] = match self {
            Self::Scissors => include_bytes!("../icons/32x32.png"),
            Self::WhaleLogo => include_bytes!("../icons/tray-whale-logo/win.png"),
        };
        // 编译期内嵌资源，坏了是构建期问题，直接 panic 比静默没图标好排查
        Image::from_bytes(bytes).expect("内置托盘图标损坏")
    }

    /// CPU 动画的帧（索引 0 与 `static_icon` 同尺寸，作为「静止」姿态）。
    ///
    /// 各主题帧数不同：剪刀 4 帧「开合呼吸」，鲸鱼 8 帧「尾鳍往复」。
    /// 播放顺序由 [`IconTheme::beats`] 给出，不要假设长度。
    pub fn frames(self) -> &'static [Image<'static>] {
        match self {
            Self::Scissors => scissors_frames(),
            Self::WhaleLogo => whale_logo_frames(),
        }
    }

    /// 播放顺序（索引指向 `frames()`）。一轮播完才可能换速 —— 见 tray::run_anim。
    /// - 剪刀：[0,1,2,3,2,1] 往返 6 拍，避免开合动画首尾同帧造成的顿感
    /// - 鲸鱼：0..8 顺序播放 = 一次完整游动往复（尾鳍 ±25° + 身体反向俯仰 ±7°）
    pub fn beats(self) -> &'static [usize] {
        match self {
            Self::Scissors => &SCISSORS_BEATS,
            Self::WhaleLogo => &WHALE_LOGO_BEATS,
        }
    }
}

const SCISSORS_BEATS: [usize; 6] = [0, 1, 2, 3, 2, 1];
const WHALE_LOGO_BEATS: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn scissors_frames() -> &'static [Image<'static>] {
    static FRAMES: OnceLock<[Image<'static>; 4]> = OnceLock::new();
    let arr = FRAMES.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let bytes: [&[u8]; 4] = [
            include_bytes!("../icons/tray-anim/macos/f0.png"),
            include_bytes!("../icons/tray-anim/macos/f1.png"),
            include_bytes!("../icons/tray-anim/macos/f2.png"),
            include_bytes!("../icons/tray-anim/macos/f3.png"),
        ];
        #[cfg(not(target_os = "macos"))]
        let bytes: [&[u8]; 4] = [
            include_bytes!("../icons/tray-anim/win/f0.png"),
            include_bytes!("../icons/tray-anim/win/f1.png"),
            include_bytes!("../icons/tray-anim/win/f2.png"),
            include_bytes!("../icons/tray-anim/win/f3.png"),
        ];
        decode_frames(bytes)
    });
    arr
}

fn whale_logo_frames() -> &'static [Image<'static>] {
    static FRAMES: OnceLock<Vec<Image<'static>>> = OnceLock::new();
    FRAMES.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let bytes: [&'static [u8]; 8] = [
            include_bytes!("../icons/tray-anim/whale-logo/macos/f0.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f1.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f2.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f3.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f4.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f5.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f6.png"),
            include_bytes!("../icons/tray-anim/whale-logo/macos/f7.png"),
        ];
        #[cfg(not(target_os = "macos"))]
        let bytes: [&'static [u8]; 8] = [
            include_bytes!("../icons/tray-anim/whale-logo/win/f0.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f1.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f2.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f3.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f4.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f5.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f6.png"),
            include_bytes!("../icons/tray-anim/whale-logo/win/f7.png"),
        ];
        bytes
            .iter()
            .map(|b| Image::from_bytes(*b).expect("内置托盘动画帧损坏"))
            .collect()
    })
}

fn decode_frames(bytes: [&'static [u8]; 4]) -> [Image<'static>; 4] {
    std::array::from_fn(|i| Image::from_bytes(bytes[i]).expect("内置托盘动画帧损坏"))
}

// ---- 持久化 ----

/// 读主题；文件不存在 / 缺键 / 值不认识 → 默认剪刀
pub fn load(app: &tauri::AppHandle) -> IconTheme {
    crate::settings::get_str(app, SETTING_TRAY_ICON)
        .and_then(|s| IconTheme::parse(&s))
        .unwrap_or(IconTheme::Scissors)
}

/// 只改本键，保留 settings.json 里其它模块的键（CPU 动画开关等）
pub fn save(app: &tauri::AppHandle, theme: IconTheme) -> Result<(), String> {
    crate::settings::set(
        app,
        SETTING_TRAY_ICON,
        serde_json::Value::String(theme.id().to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::IconTheme;

    /// 菜单 ID 后缀、持久化值、from_u8 三者必须一致：
    /// 任一处漂移都会表现为「点了菜单图标不变」或「重启后选择丢失」
    #[test]
    fn theme_ids_round_trip() {
        for theme in IconTheme::ALL {
            assert_eq!(IconTheme::parse(theme.id()), Some(theme));
            assert_eq!(IconTheme::from_u8(theme.as_u8()), theme);
        }
        assert_eq!(IconTheme::parse("no-such-theme"), None);
        // settings.json 被手改成脏值时退回默认主题，而不是没图标
        assert_eq!(IconTheme::from_u8(42), IconTheme::Scissors);
    }

    /// 内嵌 PNG 必须能解码——换皮肤最常见的坑是资源路径写错，
    /// 那样编译期就 panic，这里提前在 CI 里暴露。
    /// 尺寸只要求「高 ≤ 宽」：鲸鱼皮为拉长尾鳍力臂特意用 4:3 宽幅
    /// （64×48 / 44×33），菜单栏按高度适配、宽度自然延展。
    #[test]
    fn every_theme_has_decodable_icons() {
        for theme in IconTheme::ALL {
            let icon = theme.static_icon();
            assert!(icon.width() >= icon.height(), "{} 静态图宽高异常", theme.id());

            let frames = theme.frames();
            assert!(frames.len() >= 2, "{} 帧数不足以构成动画", theme.id());
            for (i, frame) in frames.iter().enumerate() {
                assert_eq!(frame.width(), frames[0].width(), "{} 第 {i} 帧宽度漂移", theme.id());
                assert_eq!(frame.height(), frames[0].height(), "{} 第 {i} 帧高度漂移", theme.id());
                assert!(frame.width() > 0, "{} 第 {i} 帧尺寸为 0", theme.id());
            }
        }
    }

    /// beats 是「播放顺序」，索引越界会在 set_frame 里 panic。
    /// 改帧数（比如鲸鱼 Logo 从 8 帧加半拍）时最容易漏改 beats。
    #[test]
    fn every_beat_points_at_a_real_frame() {
        for theme in IconTheme::ALL {
            let frames = theme.frames();
            let beats = theme.beats();
            assert!(!beats.is_empty(), "{} 播放序列为空", theme.id());
            for &b in beats {
                assert!(b < frames.len(), "{} 播放序列索引 {b} 越界（共 {} 帧）", theme.id(), frames.len());
            }
        }
    }
}
