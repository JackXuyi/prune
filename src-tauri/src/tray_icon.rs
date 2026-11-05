//! 顶部图标主题（M11，docs/m11-tray-icon-theme-plan.md）。
//!
//! macOS 菜单栏 / 其他平台任务栏的图标不再写死成剪刀，而是从一组「主题」里选，
//! 选择结果持久化到 `config_dir()/settings.json` 的 `trayIconTheme` 键。
//!
//! 现有主题：
//! - [`IconTheme::Scissors`]  Lucide 剪刀，**黑色模板图** —— macOS 由系统按菜单栏
//!   深浅色反色，视觉上与系统图标同质（默认）
//! - [`IconTheme::WhaleGirl`] DeepSeek 鲸鱼娘，**彩色** —— 必须关掉模板机制，
//!   否则会被压成黑白剪影
//!
//! 再加一套图标只需动本模块 4 个地方（`id` / `label` / `static_icon` / `frames`），
//! 菜单勾选与持久化由 `IconTheme::ALL` 自动跟随，tray.rs 无需改动。

use std::sync::OnceLock;

use tauri::image::Image;

/// 设置文件名与键名（与 tray.rs 共用同一个 settings.json，只改本键）
const SETTINGS_FILE: &str = "settings.json";
const SETTING_TRAY_ICON: &str = "trayIconTheme";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconTheme {
    /// 剪刀（默认）：黑色模板图
    Scissors,
    /// DeepSeek 鲸鱼娘：彩色圆形头像
    WhaleGirl,
}

impl IconTheme {
    /// 菜单展示顺序 = 数组顺序
    pub const ALL: [IconTheme; 2] = [Self::Scissors, Self::WhaleGirl];

    /// 持久化值 & 菜单项 ID 后缀（稳定标识，不要跟着 label 改）
    pub fn id(self) -> &'static str {
        match self {
            Self::Scissors => "scissors",
            Self::WhaleGirl => "whale",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Scissors => "剪刀（默认）",
            Self::WhaleGirl => "DeepSeek 鲸鱼娘",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.id() == raw)
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Scissors => 0,
            Self::WhaleGirl => 1,
        }
    }

    /// 未知值一律退回默认主题（settings.json 被手改成乱七八糟时不至于没图标）
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::WhaleGirl,
            _ => Self::Scissors,
        }
    }

    /// macOS 模板图 = 系统只取 alpha 通道上色，彩色图会被反色成剪影。
    /// 所以只有「本来就是单色字形」的主题走模板；Windows 没有模板机制，恒 false。
    pub fn is_template(self) -> bool {
        cfg!(target_os = "macos") && matches!(self, Self::Scissors)
    }

    /// 静态原图：动画关闭、CPU 静止档、以及创建 tray 时用
    pub fn static_icon(self) -> Image<'static> {
        #[cfg(target_os = "macos")]
        let bytes: &'static [u8] = match self {
            Self::Scissors => include_bytes!("../icons/tray.png"),
            Self::WhaleGirl => include_bytes!("../icons/tray-whale/macos.png"),
        };
        #[cfg(not(target_os = "macos"))]
        let bytes: &'static [u8] = match self {
            Self::Scissors => include_bytes!("../icons/32x32.png"),
            Self::WhaleGirl => include_bytes!("../icons/tray-whale/win.png"),
        };
        // 编译期内嵌资源，坏了是构建期问题，直接 panic 比静默没图标好排查
        Image::from_bytes(bytes).expect("内置托盘图标损坏")
    }

    /// CPU 动画的帧（索引 0 与 `static_icon` 同尺寸，作为「静止」姿态）。
    ///
    /// 各主题帧数不同：剪刀 4 帧「开合呼吸」，鲸鱼娘 12 帧「吃完一碗饭」。
    /// 播放顺序由 [`IconTheme::beats`] 给出，不要假设长度。
    pub fn frames(self) -> &'static [Image<'static>] {
        match self {
            Self::Scissors => scissors_frames(),
            Self::WhaleGirl => whale_frames(),
        }
    }

    /// 播放顺序（索引指向 `frames()`）。一轮播完才可能换速 —— 见 tray::run_anim。
    /// - 剪刀：[0,1,2,3,2,1] 往返 6 拍，避免开合动画首尾同帧造成的顿感
    /// - 鲸鱼娘：0..11 顺序播放 = 三口饭（每口 4 拍：舀 → 抬 → 送 → 咽）
    pub fn beats(self) -> &'static [usize] {
        match self {
            Self::Scissors => &SCISSORS_BEATS,
            Self::WhaleGirl => &WHALE_BEATS,
        }
    }
}

const SCISSORS_BEATS: [usize; 6] = [0, 1, 2, 3, 2, 1];
const WHALE_BEATS: [usize; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

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

fn whale_frames() -> &'static [Image<'static>] {
    static FRAMES: OnceLock<Vec<Image<'static>>> = OnceLock::new();
    FRAMES.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let bytes: [&'static [u8]; 12] = [
            include_bytes!("../icons/tray-anim/whale/macos/f0.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f1.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f2.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f3.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f4.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f5.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f6.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f7.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f8.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f9.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f10.png"),
            include_bytes!("../icons/tray-anim/whale/macos/f11.png"),
        ];
        #[cfg(not(target_os = "macos"))]
        let bytes: [&'static [u8]; 12] = [
            include_bytes!("../icons/tray-anim/whale/win/f0.png"),
            include_bytes!("../icons/tray-anim/whale/win/f1.png"),
            include_bytes!("../icons/tray-anim/whale/win/f2.png"),
            include_bytes!("../icons/tray-anim/whale/win/f3.png"),
            include_bytes!("../icons/tray-anim/whale/win/f4.png"),
            include_bytes!("../icons/tray-anim/whale/win/f5.png"),
            include_bytes!("../icons/tray-anim/whale/win/f6.png"),
            include_bytes!("../icons/tray-anim/whale/win/f7.png"),
            include_bytes!("../icons/tray-anim/whale/win/f8.png"),
            include_bytes!("../icons/tray-anim/whale/win/f9.png"),
            include_bytes!("../icons/tray-anim/whale/win/f10.png"),
            include_bytes!("../icons/tray-anim/whale/win/f11.png"),
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

fn settings_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().config_dir().ok().map(|d| d.join(SETTINGS_FILE))
}

/// 读主题；文件不存在 / 缺键 / 值不认识 → 默认剪刀
pub fn load(app: &tauri::AppHandle) -> IconTheme {
    let Some(path) = settings_path(app) else {
        return IconTheme::Scissors;
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.get(SETTING_TRAY_ICON)
                .and_then(|x| x.as_str())
                .and_then(IconTheme::parse)
        })
        .unwrap_or(IconTheme::Scissors)
}

/// 只改本键，保留 settings.json 里其它模块的键（CPU 动画开关等）
pub fn save(app: &tauri::AppHandle, theme: IconTheme) -> Result<(), String> {
    let path = settings_path(app).ok_or_else(|| "config_dir 不可用".to_string())?;
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    root[SETTING_TRAY_ICON] = serde_json::Value::String(theme.id().to_string());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
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

    /// 内嵌 PNG 必须能解码且是正方形——换皮肤最常见的坑是资源路径写错，
    /// 那样编译期就 panic，这里提前在 CI 里暴露
    #[test]
    fn every_theme_has_decodable_square_icons() {
        for theme in IconTheme::ALL {
            let icon = theme.static_icon();
            assert_eq!(icon.width(), icon.height(), "{} 静态图非正方形", theme.id());

            let frames = theme.frames();
            assert!(frames.len() >= 2, "{} 帧数不足以构成动画", theme.id());
            for (i, frame) in frames.iter().enumerate() {
                assert_eq!(frame.width(), frame.height(), "{} 第 {i} 帧非正方形", theme.id());
                assert!(frame.width() > 0, "{} 第 {i} 帧尺寸为 0", theme.id());
            }
        }
    }

    /// beats 是「播放顺序」，索引越界会在 set_frame 里 panic。
    /// 改帧数（比如鲸鱼娘从 12 帧加口数）时最容易漏改 beats。
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
