//! 语言解析与切换（docs/i18n-spec.md §1.3 / §4.2）。
//!
//! 为什么系统语言必须在 Rust 侧读：WKWebView 的 `navigator.language` 取自 App
//! bundle 的 `CFBundleLocalizations` —— 没声明中文本地化时，系统是中文也会返回
//! `en-US`，「默认跟随系统」直接失效。所以前端只问 IPC，不自己推断。
//!
//! 解析优先级：settings 里的手动选择 > 系统首选语言 > En。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use prune_core::locale::{Locale, LocalePref};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// settings.json 里的键名（与 camelCase 习惯一致；`island` 是历史例外）
pub const SETTING_LOCALE: &str = "locale";

/// 语言从哪来：跟随系统 / 用户手动选定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Auto,
    Manual,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

/// 进程内缓存的当前语言。
///
/// 只缓存**解析结果**，不缓存 settings —— 用户切语言后要立刻生效。
/// 1 = Zh，2 = En（0 未初始化，按 En 兜底，与 spec §1.5 的「未知系统语言 → En」一致）。
pub struct AppLocale(AtomicU8);

impl AppLocale {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    /// `None` = 还没解析过（不是「English」）；`Some(En)` 才是用户/系统给出的结论
    pub fn get(&self) -> Option<Locale> {
        match self.0.load(Ordering::Relaxed) {
            1 => Some(Locale::Zh),
            2 => Some(Locale::En),
            _ => None,
        }
    }

    pub fn set(&self, l: Locale) {
        self.0.store(
            match l {
                Locale::Zh => 1,
                Locale::En => 2,
            },
            Ordering::Relaxed,
        );
    }
}

impl Default for AppLocale {
    fn default() -> Self {
        Self::new()
    }
}

/// 返回给前端的语言状态（`locale_get` / `locale_set` / `locale://changed` 共用）
#[derive(Debug, Clone, Serialize)]
pub struct LocalePayload {
    pub current: Locale,
    pub source: &'static str,
}

impl LocalePayload {
    fn new(l: Locale, s: Source) -> Self {
        Self {
            current: l,
            source: s.as_str(),
        }
    }
}

/// settings 覆盖 > 系统首选语言 > En。
///
/// 每次调用都重读 settings（文件很小），只把解析结果缓到 [`AppLocale`]。
pub fn resolve(app: &AppHandle) -> (Locale, Source) {
    let pref = crate::settings::get_str(app, SETTING_LOCALE)
        .map(|raw| Locale::pref_from_str(&raw))
        .unwrap_or(LocalePref::Auto);
    match pref {
        LocalePref::Zh => (Locale::Zh, Source::Manual),
        LocalePref::En => (Locale::En, Source::Manual),
        LocalePref::Auto => (
            system_locale()
                .map(|tag| Locale::from_tag(&tag))
                .unwrap_or(Locale::En),
            Source::Auto,
        ),
    }
}

/// 当前生效语言：优先读缓存，未初始化时现算一次
pub fn current(app: &AppHandle) -> Locale {
    app.try_state::<AppLocale>()
        .and_then(|st| st.get())
        .unwrap_or_else(|| resolve(app).0)
}

/// 系统首选语言（BCP-47 原始串）。失败返回 None —— 调用方兜底 En。
///
/// 实现说明：本机离线环境拉不到 `sys-locale`，因此 macOS 走 `defaults read`
/// （一次子进程，结果进 OnceLock，之后零成本），Windows 直接调
/// `GetUserDefaultUILanguage`（零依赖 FFI）。**接口与语义与 spec 一致**，
/// 换成 sys-locale 只需替换本函数体。
fn system_locale() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos_locale()
    }
    #[cfg(windows)]
    {
        windows_locale()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .ok()
    }
}

/// macOS：`defaults read -g AppleLanguages` 输出形如
/// `( "zh-Hans-CN", "en-US" )`，取第一项（系统首选语言）。
///
/// 缓存进 OnceLock：语言变更本身要重启才全局生效，进程内不需要反复读。
#[cfg(target_os = "macos")]
fn macos_locale() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let out = std::process::Command::new("/usr/bin/defaults")
                .args(["read", "-g", "AppleLanguages"])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let text = String::from_utf8_lossy(&out.stdout);
            text.split('"')
                .nth(1)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .clone()
}

/// Windows：`GetUserDefaultUILanguage` 的低 10 位是主语言 ID，0x04 = 中文。
#[cfg(windows)]
fn windows_locale() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: 无参数、无副作用、线程安全的 Win32 查询
    let langid = unsafe { GetUserDefaultUILanguage() };
    Some(if langid & 0x03FF == 0x0004 {
        "zh".to_string()
    } else {
        "en".to_string()
    })
}

/// 读当前语言（幂等，不失败）。
#[tauri::command]
pub fn locale_get(app: AppHandle) -> LocalePayload {
    let (l, s) = resolve(&app);
    LocalePayload::new(l, s)
}

/// 切换语言：写盘 → 更新缓存 → 重建托盘 → 广播事件。
///
/// 顺序不能改：
/// - 先写盘：写失败要原样回滚（不改内存态、不发事件）
/// - 再重建菜单：事件一到前端就换语言，此时托盘必须是新语言
/// - 最后 emit：三个窗口各自更新 store
#[tauri::command]
pub fn locale_set(app: AppHandle, locale: String) -> Result<LocalePayload, String> {
    let pref = Locale::pref_from_str(&locale);
    let next_raw = pref.as_str();
    let prev_raw = crate::settings::get_str(&app, SETTING_LOCALE).unwrap_or_else(|| "auto".into());

    // 值没变也走「重建 + 广播」：用户选「跟随系统」后可能在系统里换了语言，
    // 重复点一次应当刷新；但不必再写盘
    if next_raw != prev_raw {
        crate::settings::set(&app, SETTING_LOCALE, serde_json::json!(next_raw))
            .map_err(|e| format!("保存语言设置失败: {e}"))?;
    }

    let (resolved, source) = resolve(&app);
    if let Some(st) = app.try_state::<AppLocale>() {
        st.set(resolved);
    }
    let payload = LocalePayload::new(resolved, source);

    // 托盘文案活在 Rust 进程里：不重建就是「主窗口英文、托盘中文」。
    // 菜单必须在主线程建，且 emit 必须排在重建之后 —— 因此放进同一个闭包
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = crate::tray::rebuild_menu(&handle) {
            // 菜单错语言比「语言切换失败」轻得多：只记日志，不回滚
            eprintln!("[i18n] 托盘菜单重建失败（保持旧语言）: {e}");
        }
        if let Err(e) = handle.emit("locale://changed", payload.clone()) {
            eprintln!("[i18n] 语言变更事件广播失败（不影响已切换的界面）: {e}");
        }
    })
    .map_err(|e| format!("调度主线程失败: {e}"))?;

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未解析过必须是 `None`，不能用一个值冒充「English」
    #[test]
    fn app_locale_atomic_round_trip() {
        let state = AppLocale::new();
        assert_eq!(state.get(), None);
        state.set(Locale::Zh);
        assert_eq!(state.get(), Some(Locale::Zh));
        state.set(Locale::En);
        assert_eq!(state.get(), Some(Locale::En));
    }
}
