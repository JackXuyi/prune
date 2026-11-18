//! Open Files 占用反查（F23，docs/f23-open-files-plan.md）：谁占着这块盘。
//!
//! 外置盘弹出失败时 macOS 只说「有文件正在使用」，不说是谁。本模块把
//! `lsof` 的结构化证据翻译成占用者列表。三条纪律：
//! 1. **只读**：只查不解——解除占用（关 App / 强制弹出）是用户的决定；
//! 2. **Coverage 纪律**：以当前用户权限运行，其他用户的进程句柄不可见，
//!    `coverage_note` 永远随结果输出，不谎报「无人占用」；
//! 3. **VREG 过滤**：目录 / socket / pipe 不算「占用着文件」——只有普通
//!    文件句柄才会阻止弹出。
//!
//! 为什么全量列举 + 前缀过滤而不是 `+D`（M-A 实测后定案）：`+D` 会递归
//! stat 整棵目录树（Time Machine 级目录十几秒起步），全量 `lsof -F ptcn0`
//! 是一次系统调用式枚举（< 1 s），代价可预期且与目标目录大小无关。
//!
//! `-F ptcn0`：t = 文件类型（VREG 才留），n = 路径，0 = NUL 分隔——
//! 空格 / 中文路径天然安全（f23 方案 §六）。

use serde::{Deserialize, Serialize};
use std::process::Command;

/// 一个占用者：按占用文件数降序输出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    pub pid: i32,
    pub comm: String,
    /// 该进程在此路径下打开的普通文件数
    pub files: u32,
    /// 代表文件（≤3 条，给人看；路径可能是长串，UI hover 全文）
    pub sample_files: Vec<String>,
}

/// 一次占用反查的完整产出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFilesReport {
    /// 归一化后的查询路径（去尾斜杠）
    pub path: String,
    /// 占用者列表（files 降序）
    pub holders: Vec<Holder>,
    pub scanned_ms: u128,
    /// 权限盲区说明（**永远有值**——「没有可见的占用者」≠「没人占用」）
    pub coverage_note: Option<String>,
}

/// lsof 可执行文件路径（与 `net::ports` 同一探测纪律：写死单一路径会让
/// Linux 上整个功能静默失效）。
fn lsof_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "/usr/sbin/lsof"
    }
    #[cfg(not(target_os = "macos"))]
    {
        for p in ["/usr/bin/lsof", "/usr/sbin/lsof", "/bin/lsof"] {
            if std::path::Path::new(p).exists() {
                return p;
            }
        }
        "lsof"
    }
}

/// 主入口：反查 `path` 下被打开的普通文件。`path` 不存在/不可读由调用方
/// 先行校验（本函数只对 lsof 的成败负责）。
pub fn scan(path: &str) -> Result<OpenFilesReport, String> {
    let started = std::time::Instant::now();
    let out = Command::new(lsof_path())
        .args(["-w", "-n", "-F", "ptcn0"])
        .output()
        .map_err(|e| format!("lsof 无法执行：{e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!(
            "lsof 失败（{}）：{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // lsof 输出的是**解析后的真实路径**（/tmp → /private/tmp），查询路径必须
    // 先 canonicalize 到同一坐标系，否则前缀匹配永远落空（真机踩坑，2026-11-18）
    let real = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string());
    let holders = parse(&text, &real);
    Ok(OpenFilesReport {
        path: normalize(path),
        holders,
        scanned_ms: started.elapsed().as_millis(),
        coverage_note: Some(
            "以当前用户权限运行：其他用户的进程句柄不可见。「没有可见的占用者」不等于「没人占用」，可用 sudo lsof 复查".into(),
        ),
    })
}

/// 路径归一化：去尾斜杠（`/Volumes/X/` → `/Volumes/X`）；根路径保持 `/`。
pub fn normalize(path: &str) -> String {
    let t = path.trim_end_matches('/');
    if t.is_empty() {
        "/".into()
    } else {
        t.to_string()
    }
}

/// 解析 `lsof -F ptcn0` 输出并按路径前缀过滤。纯函数（好测试）。
///
/// lsof 的字段流：`p<pid>` 开新进程段，`c<comm>` 进程名，随后每个 fd 一组
/// `f<fd>` / `t<type>` / `n<name>`。t 在 n 之前——按「当前 fd 的类型」过滤。
pub fn parse(text: &str, path: &str) -> Vec<Holder> {
    let prefix = normalize(path);
    // 前缀匹配规则：name == prefix，或 name 以 prefix + "/" 开头。
    // 根路径（prefix = "/"）匹配一切绝对路径
    let dir_prefix = format!("{prefix}/");
    let is_match = |name: &str| name == prefix || name.starts_with(&dir_prefix);

    let mut by_pid: std::collections::HashMap<i32, (String, Vec<String>)> =
        std::collections::HashMap::new();
    let mut order: Vec<i32> = Vec::new();

    let mut cur_pid: Option<i32> = None;
    let mut cur_comm = String::new();
    // 当前 fd 的文件类型：t 行先于 n 行出现，n 行消费后即失效（防串 fd）
    let mut cur_type: Option<String> = None;

    for tok in text.split('\0') {
        if tok.is_empty() {
            continue;
        }
        let (tag, rest) = tok.split_at(1);
        let rest = rest.trim_start_matches('\n');
        match tag {
            "p" => {
                cur_pid = rest.trim().parse::<i32>().ok();
                cur_comm.clear();
                cur_type = None;
                if let Some(pid) = cur_pid {
                    if !by_pid.contains_key(&pid) {
                        order.push(pid);
                    }
                    by_pid
                        .entry(pid)
                        .or_insert_with(|| (String::new(), Vec::new()));
                }
            }
            "c" => {
                cur_comm = rest.to_string();
                if let Some(pid) = cur_pid {
                    if let Some(slot) = by_pid.get_mut(&pid) {
                        slot.0 = cur_comm.clone();
                    }
                }
            }
            "t" => cur_type = Some(rest.to_string()),
            "n" => {
                let Some(t) = cur_type.as_deref() else {
                    continue;
                };
                // macOS lsof 报 `REG`，GNU lsof 报 `VREG`——两者都是普通文件
                let is_vreg = t == "VREG" || t == "REG";
                cur_type = None;
                if !is_vreg || !is_match(rest) {
                    continue;
                }
                if let Some(pid) = cur_pid {
                    if let Some(slot) = by_pid.get_mut(&pid) {
                        slot.1.push(rest.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    let mut holders: Vec<Holder> = order
        .into_iter()
        .filter_map(|pid| {
            let (comm, files) = by_pid.remove(&pid)?;
            if files.is_empty() {
                return None;
            }
            let n = files.len() as u32;
            Some(Holder {
                pid,
                comm,
                files: n,
                sample_files: files.into_iter().take(3).collect(),
            })
        })
        .collect();
    holders.sort_by(|a, b| b.files.cmp(&a.files).then(a.pid.cmp(&b.pid)));
    holders
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工构造的 lsof -F ptcn0 输出（NUL 分隔；含空格与中文路径）
    const SAMPLE: &str = "p123\0cDisk Helper\0f5\0tVREG\0n/Volumes/Backup/我的 文件.txt\0f6\0tDIR\0n/Volumes/Backup/subdir\0f7\0tVREG\0n/Users/me/other.txt\0\0p456\0cnode\0f3\0tVREG\0n/Volumes/Backup/log.txt\0f4\0tVREG\0n/Volumes/Backup/data/a.db\0f8\0tVREG\0n/Volumes/Backup/data/b.db\0f9\0tCHR\0n/dev/null\0\0";

    #[test]
    fn parse_keeps_vreg_under_prefix_only() {
        let holders = parse(SAMPLE, "/Volumes/Backup");
        // Disk Helper：1 个 VREG 在盘上（DIR 与盘外文件不算）；node：3 个
        assert_eq!(holders.len(), 2, "{holders:?}");
        assert_eq!(holders[0].comm, "node");
        assert_eq!(holders[0].files, 3);
        assert_eq!(holders[0].pid, 456);
        assert_eq!(holders[1].comm, "Disk Helper", "空格路径不破列");
        assert_eq!(holders[1].files, 1);
        assert_eq!(holders[1].sample_files[0], "/Volumes/Backup/我的 文件.txt");
    }

    #[test]
    fn parse_accepts_macos_reg_type() {
        // macOS lsof 报 tREG（GNU 报 tVREG）——两者都必须命中
        let holders = parse(
            "p9\0csleep\0f1\0tREG\0n/private/tmp/x.txt\0\0",
            "/private/tmp",
        );
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].comm, "sleep");
    }

    #[test]
    fn parse_honors_prefix_boundary() {
        // /Volumes/Backup2 不能命中 /Volumes/Backup 的前缀（目录边界，不是字符串前缀）
        let holders = parse(
            "p1\0capp\0f1\0tVREG\0n/Volumes/Backup2/x\0\0",
            "/Volumes/Backup",
        );
        assert!(holders.is_empty());
        // 精确等于前缀本身（打开的是挂载点目录本体 → 不是 VREG，也不该命中）
        let holders2 = parse(
            "p1\0capp\0f1\0tVREG\0n/Volumes/Backup\0\0",
            "/Volumes/Backup",
        );
        assert_eq!(holders2.len(), 1);
    }

    #[test]
    fn normalize_strips_trailing_slash() {
        assert_eq!(normalize("/Volumes/X/"), "/Volumes/X");
        assert_eq!(normalize("/"), "/");
        assert_eq!(normalize("/Volumes/X"), "/Volumes/X");
    }
}
