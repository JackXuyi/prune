//! 公网 IP 查询（f26 G-08，docs/f26-hardware-modules-plan.md D5）。
//!
//! 纪律：
//! - **出站请求最小化**：只取 `/ip` 纯文本端点，不携带任何用户数据；
//!   结果缓存 10 min，`force` 只由用户点刷新按钮触发；
//! - **三级回退**：ipify → ip.sb → ipinfo，单级 10 s 超时；
//! - **开关**：`net.publicIp.enabled`（settings.json），关闭时返回 None。
//!
//! 用 curl 子进程而不是 reqwest：零新依赖（macOS 自带 curl），与
//! nettop/iostat 的子进程范式一致；失败即 None，前端显示「不可用 + 重试」。

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::settings;

/// 缓存时长。方案 §四 D5：10 min 内重复进 tab 不再出站。
const TTL: Duration = Duration::from_secs(600);
/// 单个端点的超时。三级回退最坏 30 s，前端按钮容忍。
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// 回退链：顺序即优先级。都是无 Key、无参数、纯文本响应的端点。
const PROVIDERS: &[&str] = &[
    "https://api.ipify.org",
    "https://api.ip.sb/ip",
    "https://ipinfo.io/ip",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PubIp {
    pub ip: String,
    /// 命中的端点主机（展示「来源」用，也方便排查）
    pub source: String,
    /// 上次成功时间（Unix 秒）
    pub fetched_at: u64,
}

pub struct PubIpState {
    cache: Mutex<Option<(PubIp, Instant)>>,
    in_flight: AtomicBool,
}

impl Default for PubIpState {
    fn default() -> Self {
        Self::new()
    }
}

impl PubIpState {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            in_flight: AtomicBool::new(false),
        }
    }
}

/// 校验响应体：去掉首尾空白后必须能解析成 IP 地址，且长度合理。
/// 防止端点被劫持/返回 HTML 垃圾时把脏数据塞进 UI。
fn sanitize(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.len() > 64 {
        return None;
    }
    s.parse::<IpAddr>().ok().map(|_| s.to_string())
}

async fn fetch_one(url: &str) -> Option<String> {
    let out = tokio::process::Command::new("curl")
        .args([
            "-s",
            "--max-time",
            &FETCH_TIMEOUT.as_secs().to_string(),
            // 纯文本端点不需要 UA，但显式给一个避免个别端点拒绝空 UA
            "-A",
            "prune/1.0",
            url,
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    sanitize(&String::from_utf8_lossy(&out.stdout))
}

async fn fetch_all() -> Option<PubIp> {
    for url in PROVIDERS {
        if let Some(ip) = fetch_one(url).await {
            return Some(PubIp {
                ip,
                source: url
                    .trim_start_matches("https://")
                    .trim_end_matches("/")
                    .split('/')
                    .next()
                    .unwrap_or(url)
                    .to_string(),
                fetched_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
        }
    }
    None
}

/// 取公网 IP。`force` 绕过缓存（仅由用户点「刷新」触发）。
/// 关闭开关时返回 None（前端整卡隐藏）；请求失败时返回 None 并保留旧缓存。
#[tauri::command]
pub async fn pub_ip_get(
    state: State<'_, Arc<PubIpState>>,
    handle: tauri::AppHandle,
    force: bool,
) -> Result<Option<PubIp>, String> {
    if !settings::get_bool(&handle, "net.publicIp.enabled").unwrap_or(true) {
        return Ok(None);
    }

    if !force {
        let cached = state
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|(v, at)| {
                if at.elapsed() < TTL {
                    Some(v.clone())
                } else {
                    None
                }
            });
        if let Some(v) = cached {
            return Ok(Some(v));
        }
    }

    // 单飞：连点刷新/双 tab 同时拉取只发一轮
    if state.in_flight.swap(true, Ordering::Relaxed) {
        // 有人正在拉：有旧缓存就先给旧的，没有就等下一轮
        return Ok(state
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(v, _)| v.clone()));
    }

    let result = fetch_all().await;
    state.in_flight.store(false, Ordering::Relaxed);

    if let Some(v) = &result {
        *state.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((v.clone(), Instant::now()));
    }
    Ok(result)
}
