//! MCP stdio server（`prune mcp`）：newline-delimited JSON-RPC 2.0。
//!
//! 三个只读工具，面向 AI Agent 查询：
//! - `mem_snapshot`  一次性内存读数（家族聚合 + swap + 压力，Top 20 深探）
//! - `mem_leaks`     B5 泄漏嫌疑判定（读 GUI 已采集的序列库，不自己采——
//!                   判定需要 30 min 序列，Agent 的即时查询等不了）
//! - `disk_report`   全盘扫描（~15s，description 已注明成本，防高频轮询）
//!
//! 工具结果统一 `{ content: [{ type: "text", text: <JSON 字符串> }] }`：
//! 结构化数据序列化进 text，Agent 消费无歧义。
//!
//! 编解码与工具分发拆成纯函数（`handle_message` / `run_tool`），可直接单测。

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use prune_core::db::Store;
use prune_core::disk;
use prune_core::leak::{self, LeakReport};
use prune_core::mem::{self, process::Scanner};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "prune";

/// GUI 采样的序列库路径（与 src-tauri/lib.rs 的 db 路径保持一致）
pub fn default_db_path() -> PathBuf {
    prune_core::paths::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.xuyi.prune/prune.db")
}

/// 处理一行 JSON-RPC 消息。通知（无 id）返回 None，其余返回响应行。
pub fn handle_message(line: &str, tools: &dyn Fn(&str, &Value) -> Result<Value, String>) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(Value::Null, -32700, &format!("parse error: {e}")));
        }
    };

    // 通知没有 id，不回响应（JSON-RPC 2.0 §4.1）
    let id = msg.get("id").cloned();
    let id = match id {
        Some(id) => id,
        None => return None,
    };

    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": prune_core::version() },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_defs() })),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match tools(name, &args) {
                Ok(text) => Ok(json!({
                    "content": [{ "type": "text", "text": text.to_string() }]
                })),
                Err(e) => Err(e),
            }
        }
        _ => return Some(error_response(id, -32601, &format!("method not found: {method}"))),
    };

    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }).to_string(),
        Err(e) => error_response(id, -32000, &e),
    })
}

fn error_response(id: Value, code: i32, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

fn tool_defs() -> Value {
    json!([
        {
            "name": "mem_snapshot",
            "description": "一次性内存读数：物理内存/压缩器/swap/压力 + 按应用家族聚合的真实占用（phys_footprint 口径，含被换出部分；未深探的进程为 RSS 估算，字段 hasFootprint=false）。深探 Top 20 进程约需 1-2 秒。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mem_leaks",
            "description": "内存泄漏嫌疑判定（B5）：读 Prune GUI 采集的进程序列库做线性回归。只报有证据的嫌疑，措辞克制；数据不足时显式说明而不是返回空。需要 GUI 已运行至少 30 分钟。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sinceHours": {
                        "type": "number",
                        "description": "回溯多少小时的序列，默认 6",
                        "default": 6
                    }
                }
            }
        },
        {
            "name": "disk_report",
            "description": "全盘磁盘浪费扫描（约 15 秒，不要高频调用）：跨平台二进制死重（A1）、包管理器缓存、项目 node_modules、Docker。返回分层可回收统计。",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

/// 工具分发。返回值是放进 content[0].text 的 JSON。
pub fn run_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "mem_snapshot" => Ok(tool_mem_snapshot()),
        "mem_leaks" => {
            let since_hours = args
                .get("sinceHours")
                .and_then(|v| v.as_f64())
                .unwrap_or(6.0)
                .clamp(0.5, 72.0);
            Ok(tool_mem_leaks(since_hours))
        }
        "disk_report" => Ok(tool_disk_report()),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn probe_top(procs: &mut [prune_core::mem::ProcMem], top: usize) {
    let mut ranked: Vec<i32> = procs
        .iter()
        .filter(|p| p.footprint.is_none())
        .map(|p| p.pid)
        .collect();
    // 深探目标按 RSS 降序取 Top N（RSS 大的进程 footprint 大概率也大）
    ranked.sort_by_key(|pid| {
        std::cmp::Reverse(
            procs
                .iter()
                .find(|p| p.pid == *pid)
                .map(|p| p.rss)
                .unwrap_or(0),
        )
    });
    ranked.truncate(top);
    let samples = prune_core::mem::footprint::probe_many(&ranked, 8);
    for p in procs.iter_mut() {
        if let Some(s) = samples.get(&p.pid) {
            p.footprint = Some(s.footprint);
            p.swapped = s.swapped;
            p.source = prune_core::mem::MemSource::deep();
        }
    }
}

fn tool_mem_snapshot() -> Value {
    let mut scanner = Scanner::new();
    let mut procs = scanner.scan();
    probe_top(&mut procs, 20);

    let swap = prune_core::swap::collect(None);
    let pressure = prune_core::mem::system::read_pressure();
    json!({
        "physicalTotal": prune_core::mem::system::physical_total(),
        "swap": swap,
        "pressure": pressure,
        "families": mem::aggregate(&procs),
        "topSwapped": mem::top_swapped(&procs, 5),
    })
}

fn tool_mem_leaks(since_hours: f64) -> Value {
    let path = default_db_path();
    if !path.exists() {
        return serde_json::to_value(LeakReport::unavailable(format!(
            "序列库不存在（{}）：Prune GUI 尚未运行过",
            path.display()
        )))
        .unwrap_or(Value::Null);
    }
    let store = match Store::open(&path) {
        Ok(s) => s,
        Err(e) => {
            return serde_json::to_value(LeakReport::unavailable(format!("序列库打开失败: {e}")))
                .unwrap_or(Value::Null);
        }
    };

    let series = store.series(0).unwrap_or_default();
    // now 取库内最大 ts：历史库拿当前时间会得出「全部数据不足」的假结论
    let now = series
        .iter()
        .flat_map(|s| s.samples.iter().map(|p| p.ts))
        .max()
        .unwrap_or(0);
    if now == 0 {
        return serde_json::to_value(LeakReport::unavailable("库内没有可用序列"))
            .unwrap_or(Value::Null);
    }
    let since = now - (since_hours * 3600.0) as i64;
    let windowed: Vec<_> = series
        .into_iter()
        .filter(|s| s.samples.last().map(|p| p.ts) >= Some(since))
        .collect();
    let report = leak::detect(&windowed, &[], now);
    serde_json::to_value(report).unwrap_or(Value::Null)
}

fn tool_disk_report() -> Value {
    let home = prune_core::paths::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let report = disk::scan(&home, None);
    serde_json::to_value(report).unwrap_or(Value::Null)
}

/// L4 Docker 组的命令提示（A6：只给命令，永不执行）
pub fn docker_hints() -> Vec<(&'static str, &'static str)> {
    vec![
        ("docker system prune", "清理悬空镜像、停止的容器、无用网络"),
        ("docker system prune --volumes", "连同未使用的数据卷一起清理（⚠️ 数据卷删除后不可恢复）"),
        ("docker builder prune", "清理构建缓存"),
    ]
}

/// L4 分组的 key（disk/whitelist.rs）
pub const L4_KEY: &str = "docker-vm";

/// 供 main.rs 的 watch 模式累积序列用
pub struct SeriesAccum {
    map: HashMap<(i32, u64), leak::ProcSeries>,
}

impl SeriesAccum {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// 把一轮采样并入序列（footprint 为 None 的进程不参与——泄漏判定只认 footprint 口径）
    pub fn push(&mut self, procs: &[prune_core::mem::ProcMem], ts: i64) {
        for p in procs {
            let Some(fp) = p.footprint else { continue };
            self.map
                .entry((p.pid, p.start_time))
                .and_modify(|s| s.samples.push(leak::Sample { ts, footprint: fp }))
                .or_insert_with(|| leak::ProcSeries {
                    pid: p.pid,
                    start_time: p.start_time,
                    family: p.family.clone(),
                    name: p.name.clone(),
                    samples: vec![leak::Sample { ts, footprint: fp }],
                });
        }
    }

    pub fn series(&self) -> Vec<leak::ProcSeries> {
        self.map.values().cloned().collect()
    }
}

impl Default for SeriesAccum {
    fn default() -> Self {
        Self::new()
    }
}

/// watch 一轮采样：scan + 深探 Top N，返回可并入序列的进程
pub fn watch_round(top: usize) -> Vec<prune_core::mem::ProcMem> {
    let mut scanner = Scanner::new();
    let mut procs = scanner.scan();
    probe_top(&mut procs, top);
    procs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_ok(_name: &str, _args: &Value) -> Result<Value, String> {
        Ok(json!({ "ok": true }))
    }

    #[test]
    fn initialize_returns_protocol_version() {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let out = handle_message(&req.to_string(), &init_ok).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn notification_is_silent() {
        let req = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(handle_message(&req.to_string(), &init_ok).is_none());
    }

    #[test]
    fn unknown_method_is_32601() {
        let req = json!({"jsonrpc":"2.0","id":2,"method":"resources/list"});
        let out = handle_message(&req.to_string(), &init_ok).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn malformed_line_is_32700() {
        let out = handle_message("{not json", &init_ok).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["error"]["code"], -32700);
    }

    #[test]
    fn tools_call_success_wraps_text() {
        let req = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"echo","arguments":{"x":1}}
        });
        let out = handle_message(&req.to_string(), &|n, a| {
            assert_eq!(n, "echo");
            assert_eq!(a["x"], 1);
            Ok(json!({"echoed": true}))
        })
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        let inner: Value = serde_json::from_str(text).unwrap();
        assert_eq!(inner["echoed"], true);
    }

    #[test]
    fn tools_call_error_is_32000() {
        let req = json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"nope","arguments":{}}
        });
        let out = handle_message(&req.to_string(), &|_, _| Err("boom".into())).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["error"]["code"], -32000);
        assert_eq!(v["error"]["message"], "boom");
    }

    #[test]
    fn tool_defs_have_three_readonly_tools() {
        let defs = tool_defs();
        let names: Vec<&str> = defs
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["mem_snapshot", "mem_leaks", "disk_report"]);
        for t in defs.as_array().unwrap() {
            assert!(t["description"].as_str().unwrap().len() > 10);
            assert!(t["inputSchema"].is_object());
        }
    }

    #[test]
    fn run_tool_rejects_unknown() {
        assert!(run_tool("write_files", &Value::Null).is_err());
    }

    #[test]
    fn accum_merges_same_process_and_splits_on_restart() {
        let mk = |pid: i32, st: u64, fp: u64| prune_core::mem::ProcMem {
            pid,
            ppid: 1,
            name: "p".into(),
            exe: None,
            family: "F".into(),
            rss: 1,
            footprint: Some(fp),
            swapped: None,
            source: prune_core::mem::MemSource::Footprint,
            start_time: st,
            cmd: Vec::new(),
            workspace: None,
        };
        let mut acc = SeriesAccum::new();
        acc.push(&[mk(1, 100, 10)], 1000);
        acc.push(&[mk(1, 100, 20)], 1010);
        acc.push(&[mk(1, 200, 5)], 1020); // pid 复用：新序列
        let all = acc.series();
        assert_eq!(all.len(), 2);
        let s1 = all.iter().find(|s| s.start_time == 100).unwrap();
        assert_eq!(s1.samples.len(), 2);
        let s2 = all.iter().find(|s| s.start_time == 200).unwrap();
        assert_eq!(s2.samples.len(), 1);
    }

    #[test]
    fn accum_skips_rss_only_processes() {
        let p = prune_core::mem::ProcMem {
            pid: 9,
            ppid: 1,
            name: "p".into(),
            exe: None,
            family: "F".into(),
            rss: 1,
            footprint: None,
            swapped: None,
            source: prune_core::mem::MemSource::Rss,
            start_time: 1,
            cmd: Vec::new(),
            workspace: None,
        };
        let mut acc = SeriesAccum::new();
        acc.push(&[p], 1000);
        assert!(acc.series().is_empty());
    }
}
