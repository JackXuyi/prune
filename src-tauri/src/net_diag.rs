//! 网络诊断 IPC（docs/net-diag-plan.md §4.2）。
//!
//! 诊断是**按需短任务**，不进常驻采样循环：每轮 ≤ 5 s（探测串行，各 2 s 超时），
//! 用 `spawn_blocking` 包住，不阻塞 Tauri 的 async 运行时。
//!
//! 测速（`networkQuality`）是独立行为、独立耗时（~30 s），与整轮诊断互不阻塞；
//! 仅 macOS 提供系统级等价物，Windows 上 capability 为 false、前端整卡隐藏
//! （不显示「不可用」占位，沿 W2 的拍板）。

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::net::diag::{self, NetDiag, SpeedResult};
use tauri::State;

/// 测速总超时：networkQuality 实测 ~30 s，90 s 是两倍余量
const SPEED_TIMEOUT: Duration = Duration::from_secs(90);

/// 最近一轮结果（报告导出用）。不落库：诊断是快照，回答「现在」
pub struct NetDiagState {
    last: Mutex<Option<NetDiag>>,
    speed: Mutex<Option<SpeedResult>>,
}

impl NetDiagState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last: Mutex::new(None),
            speed: Mutex::new(None),
        })
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 跑一轮分层诊断（DNS / 网关 / 出口 + portal + 结论）
#[tauri::command]
pub async fn net_diag_run(state: State<'_, Arc<NetDiagState>>) -> Result<NetDiag, String> {
    let d = tauri::async_runtime::spawn_blocking(move || diag::run_diag(now_secs()))
        .await
        .unwrap_or_else(|e| NetDiag {
            at: now_secs(),
            status: diag::DiagStatus::Failed,
            note: Some(format!("诊断任务调度失败: {e}")),
            dns: None,
            lan: None,
            wan: None,
            jitter_ms: None,
            gateway: None,
            captive: None,
            verdict: None,
            ssid: None,
        });
    if let Ok(mut last) = state.last.lock() {
        *last = Some(d.clone());
    }
    Ok(d)
}

/// 测速 + bufferbloat（macOS `networkQuality -c`，~30 s）。
/// Windows 无系统等价物 → capability 门控，前端不渲染该卡；误调返回 Err。
#[tauri::command]
pub async fn net_diag_speed_run(state: State<'_, Arc<NetDiagState>>) -> Result<SpeedResult, String> {
    #[cfg(target_os = "macos")]
    {
        let res = tauri::async_runtime::spawn_blocking(run_network_quality)
            .await
            .map_err(|e| format!("测速任务调度失败: {e}"))?;
        if let (Ok(s), Ok(mut slot)) = (&res, state.speed.lock()) {
            *slot = Some(s.clone());
        }
        res
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &state;
        Err("测速仅 macOS 提供（系统 networkQuality）".into())
    }
}

/// 诊断能力（前端渲染测速卡的依据）。不硬编码 navigator.platform，
/// 避免前端判断与后端注册面漂移。
#[tauri::command]
pub fn net_diag_capability() -> serde_json::Value {
    serde_json::json!({ "speedTest": cfg!(target_os = "macos") })
}

/// 生成 Markdown 诊断报告并**直接写入系统剪贴板**（基于最近一轮诊断 + 测速；都没跑过返回 Err）。
///
/// 为什么不在前端写剪贴板：Tauri 自定义协议下 `navigator.clipboard` 常被拒，
/// 兜底的 `document.execCommand("copy")` 又要求同步用户手势——而报告文本要先
/// 经一次 IPC 往返，手势已失效。改由原生命令写剪贴板，绕开 WebView 限制；
/// 沿用项目「系统命令优先、零新依赖」纪律（同 networkQuality / route / netsh）。
#[tauri::command]
pub fn net_diag_copy_report(state: State<'_, Arc<NetDiagState>>) -> Result<usize, String> {
    let text = report_text(&state)?;
    write_clipboard(&text)?;
    Ok(text.chars().count())
}

/// 拼报告文本（copy 与旧 report 命令共用同一口径）
fn report_text(state: &State<'_, Arc<NetDiagState>>) -> Result<String, String> {
    let last = state
        .last
        .lock()
        .map_err(|_| "诊断状态被占用".to_string())?
        .clone();
    let speed = state
        .speed
        .lock()
        .map_err(|_| "诊断状态被占用".to_string())?
        .clone();
    match last {
        Some(d) => Ok(diag::report_markdown(&d, speed.as_ref())),
        None => Err("还没有诊断结果，请先运行诊断".into()),
    }
}

// ---------------------------------------------------------------- 系统剪贴板

/// 把文本写入系统剪贴板。
///
/// - macOS：`pbcopy`（stdin 写入，UTF-8 原样）
/// - Windows：PowerShell `Set-Clipboard`，先显式设 UTF-8 输入编码，
///   否则中文报告会按 OEM 代码页乱码（`clip.exe` 同理，不可用）
#[cfg(target_os = "macos")]
fn write_clipboard(text: &str) -> Result<(), String> {
    let mut cmd = Command::new("pbcopy");
    run_pipe(&mut cmd, text)
}

#[cfg(windows)]
fn write_clipboard(text: &str) -> Result<(), String> {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::InputEncoding=[Text.Encoding]::UTF8; $input | Set-Clipboard",
    ]);
    run_pipe(&mut cmd, text)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn write_clipboard(_text: &str) -> Result<(), String> {
    Err("当前平台未实现剪贴板写入（仅 macOS / Windows）".into())
}

/// 启动子进程 → 全量写 stdin → 等退出。stdin 必须显式 drop，否则子进程等不到 EOF
fn run_pipe(cmd: &mut Command, text: &str) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("剪贴板进程启动失败: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("剪贴板进程无 stdin")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("剪贴板进程等待失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "剪贴板写入被拒绝（{:?}）：{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

// ---------------------------------------------------------------- networkQuality

#[cfg(target_os = "macos")]
fn run_network_quality() -> Result<SpeedResult, String> {
    let mut child = Command::new("networkQuality")
        .arg("-c")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("networkQuality 启动失败: {e}"))?;

    let mut stdout = child.stdout.take().ok_or("networkQuality 无 stdout")?;
    let deadline = Instant::now() + SPEED_TIMEOUT;
    let status = loop {
        match child.try_wait().map_err(|e| format!("networkQuality: {e}"))? {
            Some(st) => break st,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("networkQuality 超时（90 s），已终止".into());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    };
    let mut buf = Vec::new();
    stdout
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取 networkQuality 输出: {e}"))?;
    if !status.success() {
        return Err(format!("networkQuality 异常退出（{:?}）", status.code()));
    }
    parse_speed_json(&String::from_utf8_lossy(&buf), now_secs())
}

/// 容错解析 `-c` JSON：缺字段一律 failed + note，不显示部分数字（§7）。
/// 字段以 macOS 15.7 实跑为准；吞吐单位 bps，直接透传给前端格式化。
#[cfg(target_os = "macos")]
fn parse_speed_json(text: &str, at: i64) -> Result<SpeedResult, String> {
    let json_start = text.find('{').ok_or("networkQuality 没有输出 JSON")?;
    let v: serde_json::Value = serde_json::from_str(&text[json_start..])
        .map_err(|e| format!("networkQuality JSON 解析失败: {e}"))?;
    let num = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_f64())
            .ok_or(format!("networkQuality 缺字段 {k}"))
    };
    Ok(SpeedResult {
        at,
        down_bps: num("dl_throughput")?,
        up_bps: num("ul_throughput")?,
        rpm: num("responsiveness")?,
        rpm_level: v
            .get("responsiveness_level")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        base_rtt_ms: num("base_rtt")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实跑样例字段（2026-11-09，macOS 15.7.7，节选）。缺字段必须报错而不是给 0
    #[cfg(target_os = "macos")]
    #[test]
    fn parse_speed_json_tolerant() {
        let full = r#"{
          "base_rtt" : 518.89,
          "dl_throughput" : 86400000.0,
          "ul_throughput" : 21000000.0,
          "responsiveness" : 320.0,
          "responsiveness_level" : "Medium"
        }"#;
        let s = parse_speed_json(full, 42).unwrap();
        assert_eq!(s.down_bps, 86_400_000.0);
        assert_eq!(s.rpm, 320.0);
        assert_eq!(s.rpm_level, "Medium");
        assert_eq!(s.at, 42);

        // 前导杂音（networkQuality 有时会先打日志）也能解析
        assert!(parse_speed_json(&format!("noise\n{full}"), 42).is_ok());

        // 缺字段 → Err（绝不出现 0 冒充）
        assert!(parse_speed_json(r#"{"base_rtt": 1.0}"#, 42).is_err());
        assert!(parse_speed_json("", 42).is_err());
    }
}
