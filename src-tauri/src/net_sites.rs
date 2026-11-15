//! 站点检测 IPC（docs/net-sites-plan.md，F15 / S1）。
//!
//! 与 `net_diag.rs` 同族但是两类任务：
//! - 诊断：固定三个目标（DNS / 网关 / 出口），**串行**，一轮 ≤ 5 s；
//! - 站点：用户自己的站表，**并发 4**，单站最坏 8 s（`--max-time`）。
//!
//! 三条实现约束：
//! 1. **不占主线程**：worker 是 `std::thread::scope` 里的阻塞子进程调用，整体包在
//!    `spawn_blocking` 里（M30 的教训：同步命令跑主线程会把界面钉死）。
//! 2. **单轮互斥**：连点「全部检测」不能变成 4 轮并发把自己带宽打满。
//! 3. **取消只停「还没开始的站」**：在飞的 curl 最多再跑 8 s 自然结束。
//!    杀子进程要共享 `Child` 句柄 + 平台差异，收益不抵复杂度（方案 §4.6）。

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use prune_core::i18n::err::err_text;
use prune_core::i18n::fill;
use prune_core::locale::Locale;
use prune_core::net::sites::{self, Site, SiteList, SiteProbe, SiteRun};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// 每完成一站发一次（前端逐行点亮，不必等整轮）
const EVENT_PROGRESS: &str = "net-sites://progress";
/// 整轮结束（含取消态）
const EVENT_DONE: &str = "net-sites://done";
/// settings.json 的键。沿用 settings 模块（不另建文件）
const SETTING_KEY: &str = "netSites";
/// 明示身份，不伪装浏览器 UA：403 是读数，不是要绕过的障碍
const UA: &str = concat!("PruneSiteProbe/", env!("CARGO_PKG_VERSION"));

pub struct SiteState {
    /// 单轮互斥（只存 bool，不跨 await 持有）
    running: Mutex<bool>,
    cancel: Arc<AtomicBool>,
    /// 最近一轮（报告导出 + 单站重测回填用）。不落库：快照回答「现在」
    last: Mutex<Option<SiteRun>>,
}

impl SiteState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            running: Mutex::new(false),
            cancel: Arc::new(AtomicBool::new(false)),
            last: Mutex::new(None),
        })
    }
}

/// curl 能力（前端据此决定整页可用性）。不硬编码平台：Windows 老版本可能没有 curl
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteCapability {
    pub curl: bool,
    /// `curl --version` 首行；null = 没跑起来
    pub version: Option<String>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 站表读写

/// 读站表。返回 `(sites, customized, recovered)`：
/// - 键缺失 / null → 默认站表，非自定义；
/// - 有数组且校验通过 → 自定义；
/// - 解析或校验失败 → 默认站表 + `recovered = true`（前端必须提示，不静默替换）
fn load_sites(app: &AppHandle, locale: Locale) -> (Vec<Site>, bool, bool) {
    let root = crate::settings::read_root(app);
    match root.get(SETTING_KEY) {
        None | Some(serde_json::Value::Null) => (sites::default_sites(), false, false),
        Some(v) => match serde_json::from_value::<Vec<Site>>(v.clone()) {
            Ok(list) => match sites::validate_sites(&list, locale) {
                Ok(ok) => (ok, true, false),
                Err(e) => {
                    eprintln!("[net-sites] 站表校验失败，回落默认: {e}");
                    (sites::default_sites(), false, true)
                }
            },
            Err(e) => {
                eprintln!("[net-sites] 站表解析失败，回落默认: {e}");
                (sites::default_sites(), false, true)
            }
        },
    }
}

#[tauri::command]
pub fn net_sites_list(app: AppHandle) -> SiteList {
    let locale = crate::i18n::current(&app);
    let (list, customized, recovered) = load_sites(&app, locale);
    SiteList {
        sites: list,
        customized,
        recovered,
    }
}

/// 保存站表：**先校验后写**，失败不写半套配置（方案 §8）
#[tauri::command]
pub fn net_sites_save(app: AppHandle, sites: Vec<Site>) -> Result<SiteList, String> {
    let locale = crate::i18n::current(&app);
    let t = err_text(locale);
    let clean = sites::validate_sites(&sites, locale)?;
    crate::settings::set(&app, SETTING_KEY, serde_json::json!(clean))
        .map_err(|e| fill(t.sites_save_failed, &[("err", &e)]))?;
    Ok(SiteList {
        sites: clean,
        customized: true,
        recovered: false,
    })
}

/// 恢复默认：写 null（不清空成 `[]`——空站表也是合法配置，两者语义不同）
#[tauri::command]
pub fn net_sites_reset(app: AppHandle) -> Result<SiteList, String> {
    let locale = crate::i18n::current(&app);
    let t = err_text(locale);
    crate::settings::set(&app, SETTING_KEY, serde_json::Value::Null)
        .map_err(|e| fill(t.sites_save_failed, &[("err", &e)]))?;
    Ok(SiteList {
        sites: sites::default_sites(),
        customized: false,
        recovered: false,
    })
}

// ---------------------------------------------------------------- 能力探测

#[tauri::command]
pub fn net_sites_capability() -> SiteCapability {
    match Command::new("curl").arg("--version").output() {
        Ok(out) if out.status.success() => SiteCapability {
            curl: true,
            version: String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_string()),
        },
        _ => SiteCapability {
            curl: false,
            version: None,
        },
    }
}

// ---------------------------------------------------------------- 单站探测（薄壳）

/// 跑一次 curl 并判定。任何「机制层」失败都落成 `verdict = Error` + note，
/// 绝不编造读数（方案 §12）。
fn probe_site(site: &Site, at: i64, locale: Locale) -> SiteProbe {
    let t = err_text(locale);
    let args = sites::build_curl_args(site, UA);
    let out = match Command::new("curl")
        .args(&args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return error_probe(
                site,
                at,
                fill(t.sites_curl_spawn, &[("err", &e.to_string())]),
            )
        }
    };
    let exit = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let note = detail_note(exit, &stderr);
    match sites::parse_curl_output(&stdout) {
        Ok(raw) => sites::judge(site, &raw, exit, note, at),
        Err(e) => {
            // -w 行都没拿到：curl 连 URL 都没解析成功（如 exit 3）
            let why = match note {
                Some(n) => format!("{e}; {n}"),
                None => e,
            };
            error_probe(site, at, why)
        }
    }
}

/// curl 的失败原文 → 一行短说明。note 是**技术细节**（英文原样），
/// 用户看到的结论来自 `verdict` + `stage` 的本地化文案
fn detail_note(exit: i32, stderr: &str) -> Option<String> {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(160).collect::<String>());
    match (exit, line) {
        (0, None) => None,
        (0, Some(l)) => Some(l),
        (e, Some(l)) => Some(format!("curl exit {e}: {l}")),
        (e, None) => Some(format!("curl exit {e}")),
    }
}

fn error_probe(site: &Site, at: i64, note: String) -> SiteProbe {
    SiteProbe {
        id: site.id.clone(),
        name: site.name.clone(),
        url: site.url.clone(),
        at,
        dns_ms: None,
        tcp_ms: None,
        tls_ms: None,
        ttfb_ms: None,
        total_ms: None,
        bytes: 0,
        http_code: None,
        remote_ip: None,
        transfer_bps: None,
        verdict: sites::SiteVerdict::Error,
        stage: None,
        note: Some(note),
        curl_exit: None,
    }
}

// ---------------------------------------------------------------- 整轮（并发池）

/// 4 个 worker 抢索引，结果按**配置顺序**回填（UI 不按完成顺序跳动）
fn run_all(app: AppHandle, list: Vec<Site>, cancel: Arc<AtomicBool>, locale: Locale) -> SiteRun {
    let at = now_secs();
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<SiteProbe>>> = Mutex::new(vec![None; list.len()]);

    let workers = sites::SITE_CONCURRENCY.min(list.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let slots = &slots;
            let next = &next;
            let cancel = &cancel;
            let list = &list;
            let app = &app;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= list.len() {
                    return;
                }
                // 取消只拦「还没开始的站」：在飞的那一个最多再跑 8 s
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                let probe = probe_site(&list[i], at, locale);
                if let Ok(mut s) = slots.lock() {
                    s[i] = Some(probe.clone());
                }
                let _ = app.emit(EVENT_PROGRESS, &probe);
            });
        }
    });

    let cancelled = cancel.load(Ordering::SeqCst);
    let results: Vec<SiteProbe> = slots
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect();
    SiteRun {
        at,
        results,
        cancelled,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

/// 一轮全测。busy 时**立即返回**而非排队（排队会让用户以为点了没反应）
#[tauri::command]
pub async fn net_sites_run(
    app: AppHandle,
    state: State<'_, Arc<SiteState>>,
) -> Result<SiteRun, String> {
    let locale = crate::i18n::current(&app);
    let t = err_text(locale);
    let (list, _, _) = load_sites(&app, locale);

    {
        let mut running = state.running.lock().map_err(|_| t.sites_busy.to_string())?;
        if *running {
            return Err(t.sites_busy.into());
        }
        *running = true;
    }
    state.cancel.store(false, Ordering::SeqCst);

    if list.is_empty() {
        let run = SiteRun {
            at: now_secs(),
            results: Vec::new(),
            cancelled: false,
            elapsed_ms: 0.0,
        };
        if let Ok(mut g) = state.running.lock() {
            *g = false;
        }
        return Ok(run);
    }

    let cancel = state.cancel.clone();
    let handle = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || run_all(handle, list, cancel, locale))
        .await
        .map_err(|e| fill(t.sites_dispatch, &[("err", &e.to_string())]));

    if let Ok(mut g) = state.running.lock() {
        *g = false;
    }
    let run = res?;
    if let Ok(mut last) = state.last.lock() {
        *last = Some(run.clone());
    }
    let _ = app.emit(EVENT_DONE, &run);
    Ok(run)
}

/// 单站重测：不跑整轮，也不改别的行。若已有上一轮结果，就地替换该站，
/// 保证「复制结果」与屏幕上看到的一致
#[tauri::command]
pub async fn net_sites_probe_one(
    app: AppHandle,
    state: State<'_, Arc<SiteState>>,
    id: String,
) -> Result<SiteProbe, String> {
    let locale = crate::i18n::current(&app);
    let t = err_text(locale);
    let (list, _, _) = load_sites(&app, locale);
    let site = list
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| fill(t.sites_site_gone, &[("id", &id)]))?;
    let at = now_secs();
    let probe = tauri::async_runtime::spawn_blocking(move || probe_site(&site, at, locale))
        .await
        .map_err(|e| fill(t.sites_dispatch, &[("err", &e.to_string())]))?;
    if let Ok(mut last) = state.last.lock() {
        if let Some(run) = last.as_mut() {
            if let Some(slot) = run.results.iter_mut().find(|p| p.id == probe.id) {
                *slot = probe.clone();
            }
        }
    }
    Ok(probe)
}

/// 取消：只置标志，worker 在取下一个站前检查（在飞的请求自然结束）
#[tauri::command]
pub fn net_sites_cancel(state: State<'_, Arc<SiteState>>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// 生成 Markdown 结果并**直接写入系统剪贴板**（与诊断报告同一条原生通道：
/// Tauri 自定义协议下 `navigator.clipboard` 常被拒，且 IPC 往返后手势已失效）
#[tauri::command]
pub fn net_sites_copy_report(
    app: AppHandle,
    state: State<'_, Arc<SiteState>>,
) -> Result<usize, String> {
    let locale = crate::i18n::current(&app);
    let t = err_text(locale);
    let run = state
        .last
        .lock()
        .map_err(|_| t.sites_busy.to_string())?
        .clone()
        .ok_or_else(|| t.sites_report_none.to_string())?;
    let text = sites::report_markdown(&run, locale);
    crate::net_diag::write_clipboard(&text, locale)?;
    Ok(text.chars().count())
}
