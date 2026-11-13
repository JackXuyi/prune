//! MCP stdio server（`prune mcp`）：newline-delimited JSON-RPC 2.0。
//!
//! 三个只读工具，面向 AI Agent 查询：
//! - `mem_snapshot`  一次性内存读数（家族聚合 + swap + 压力，Top 20 深探）
//! - `mem_leaks`     B5 泄漏嫌疑判定（读 GUI 已采集的序列库，不自己采——
//!   判定需要 30 min 序列，Agent 的即时查询等不了）
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
pub fn handle_message(
    line: &str,
    tools: &dyn Fn(&str, &Value) -> Result<Value, String>,
) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ));
        }
    };

    // 通知没有 id，不回响应（JSON-RPC 2.0 §4.1）
    let id = msg.get("id").cloned();
    let id = id?;

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
        _ => {
            return Some(error_response(
                id,
                -32601,
                &format!("method not found: {method}"),
            ))
        }
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
        },
        {
            "name": "port_listeners",
            "description": "端口监听反查（只读，约 40-140 ms）：谁占着某个端口，含持有进程的 pid、绑定地址（全部网卡/仅本机）、cwd 归属的项目目录与 ESTABLISHED 连接数。不传 port 时返回关注层（开发/数据库端口 + 有项目归属的端口）。coverage 说明可见性：listenTotal 是内核口径，attributed 是能归属到进程的条数，差额是权限看不到的部分——「没查到」不等于端口空闲。timeWait 是内核残留连接，没有持有进程。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "port": {
                        "type": "number",
                        "description": "只看这个端口（如 3000）。不传则返回关注层全部。"
                    },
                    "all": {
                        "type": "boolean",
                        "description": "是否包含应用内部 IPC / 系统服务的折叠层，默认 false",
                        "default": false
                    }
                }
            }
        },
        {
            "name": "app_resources",
            "description": "单个应用的五域资源画像（只读，约 2-6 秒，不要高频调用）：进程数、内存（RSS 下界）、CPU、监听端口、TCP 连接数、磁盘三层（本体 / 缓存 / 数据，按实占块统计）。family 用大小写不敏感的子串匹配，如 \"cursor\"；匹配到多个家族时会返回 candidates 让你用全名重试。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "family": {
                        "type": "string",
                        "description": "应用家族名（子串匹配即可，如 cursor / chrome / wechat）"
                    }
                },
                "required": ["family"]
            }
        },
        {
            "name": "lan_devices",
            "description": "局域网设备与暴露面（只读缓存，不发包）：同网段设备列表（IP/MAC/厂商/类型/在线状态）+ 本机对它们开放的端口。数据来自最近一轮扫描（GUI 点扫描或 prune lan --scan），本工具只读缓存——若返回 status=unavailable，请先让用户扫一轮。设备厂商为 null 可能是随机化 MAC（正常现象），不代表识别失败；mDNS/SSDP 均空时 notes 会说明可能是防火墙拦截。时间线字段：firstSeen（首次发现，Unix 秒）/ lastSeen（最后活跃）/ isNew（10 分钟内新接入，true 时请向用户提示），0 表示旧版缓存无记录。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "disk_scan",
            "description": "白名单逐条细分视图（只读，秒级）：每条可回收缓存（包管理器 / AI 应用 / Xcode / 模拟器等 84 条）的实时体积与级别（L1/L2 一键清理类、L3 需逐项确认类），children=true 时附一级子目录占比。与 disk_report 的分工：那个回答「全盘浪费结构」（约 15 秒），本工具回答「某条缓存现在多大、什么占的」。未安装的应用（无对应目录）不会出现在结果里。注意：本工具永不执行清理——清理必须由人在 Prune GUI 确认。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "只看这些条目（如 [\"cursor-cache\", \"ollama-models\"]）。缺省返回全部。"
                    },
                    "children": {
                        "type": "boolean",
                        "description": "是否列每条的一级子目录占比（按体积降序），默认 false",
                        "default": false
                    },
                    "childrenTop": {
                        "type": "number",
                        "description": "children 开启时最多列几个子目录，默认 8",
                        "default": 8
                    }
                }
            }
        },
        {
            "name": "net_diag",
            "description": "端口暴露面诊断结论（只读，约 40-140 ms）：直接给结论而不是原始条目——哪些端口绑定全部网卡（同网段可达）、哪些仅本机回环、权限看不到的监听有多少、TIME_WAIT 内核残留有多少。port_listeners 适合「谁占着 3000」这类点查；本工具适合「这台机器对外开着什么」这类体检。绑定具体网卡地址（192.168.x.x）与 0.0.0.0 都算局域网可达，结论里均标 attention。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "port": {
                        "type": "number",
                        "description": "只诊断这个端口。缺省 = 全量体检。"
                    }
                }
            }
        }
    ])
}

/// 工具分发。返回值是放进 content[0].text 的 JSON。
///
/// **底线（M29 信任叙事的代码层）**：本表只允许只读查询。任何执行类请求
/// 在这里被显式拒绝（报「MCP 通道只读」而不是 unknown tool），清理执行
/// 只能由人在 GUI 确认——白名单 / 废纸篓 / 20 GB 熔断三层护栏全在 GUI 侧。
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
        "disk_scan" => Ok(tool_disk_scan(args)),
        "port_listeners" => Ok(tool_port_listeners(args)),
        "net_diag" => Ok(tool_net_diag(args)),
        "app_resources" => Ok(tool_app_resources(args)),
        "lan_devices" => Ok(tool_lan_devices()),
        // 执行类请求显式拒绝（M29）：让 Agent 与读代码的人都直接看到底线，
        // 而不是落进 unknown tool 的笼统报错。此清单只增不减。
        "reclaim" | "clean" | "clean_item" | "execute" | "delete" | "remove" | "kill" | "write"
        | "trash" => Err(
            "MCP 通道只读（代码层底线，测试固化）：清理执行只能由人在 Prune GUI 里确认 \
             （白名单 + 废纸篓 + 20 GB 熔断三层护栏在 GUI 侧）。\
             如需了解可回收体量，请改用只读工具 disk_scan / disk_report。"
                .into(),
        ),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// M18 `lan_devices`（spec §12）：固定读缓存，不触发扫描。
/// MCP server 无 UI 承接 macOS 本地网络授权弹窗，也不能替用户发几百个 SYN。
fn tool_lan_devices() -> Value {
    let loaded = prune_core::paths::lan_cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    match loaded {
        Some(snap) => snap,
        None => json!({
            "status": "unavailable",
            "note": "尚无局域网扫描缓存。请在 Prune GUI 的「局域网」分段点扫描，或运行 prune lan --scan。"
        }),
    }
}

/// M29 `disk_scan`（f13 §2.1）：白名单逐条细分视图，只读，不执行任何清理。
/// 与 disk_report 分工：那边回答「全盘浪费结构」（~15 s 重扫），
/// 这边回答「某条白名单缓存现在多大、里面什么占的」（resolve + 并发测体积，秒级）。
fn tool_disk_scan(args: &Value) -> Value {
    use prune_core::disk::whitelist::Level;

    let home = prune_core::paths::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let keys: Option<Vec<String>> = args.get("keys").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|k| k.as_str().map(String::from))
            .collect()
    });
    let with_children = args
        .get("children")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let children_top = args
        .get("childrenTop")
        .and_then(|v| v.as_u64())
        .unwrap_or(8)
        .clamp(1, 20) as usize;

    let resolved: Vec<_> = prune_core::disk::whitelist::resolve(&home)
        .into_iter()
        .filter(|(e, _)| {
            keys.as_ref()
                .map(|ks| ks.iter().any(|k| k == e.key))
                .unwrap_or(true)
        })
        .collect();

    // 并发测体积：CoreSimulator/Devices 3.5 GB 量级也在秒级（复用磁盘扫描线程池）
    let sized = prune_core::disk::parallel_map(resolved, 8, |(e, path)| {
        let exists = path.is_dir();
        let size = if exists { disk::tree_size(path) } else { 0 };
        let children: Vec<(String, u64)> = if with_children && exists {
            let mut ch = disk::first_level_sizes(path);
            ch.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
            ch.truncate(children_top);
            ch
        } else {
            Vec::new()
        };
        (*e, path.clone(), exists, size, children)
    });

    let mut cleanable_bytes = 0u64;
    let mut confirm_bytes = 0u64;
    let mut present = 0usize;
    let entries: Vec<Value> = sized
        .iter()
        .map(|(e, path, exists, size, children)| {
            if *exists {
                present += 1;
                match e.level {
                    Level::L1 | Level::L2 => cleanable_bytes += size,
                    Level::L3 => confirm_bytes += size,
                    Level::L4 => {} // 容器类只列不计量，本来就不进清理通道
                }
            }
            json!({
                "key": e.key,
                "label": e.label,
                "level": e.level.as_str(),
                "cleanable": e.level.default_cleanable(),
                "path": path.display().to_string(),
                "exists": exists,
                "sizeBytes": size,
                "children": children
                    .iter()
                    .map(|(n, s)| json!({ "name": n, "sizeBytes": s }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    json!({
        "readonly": true,
        "summary": {
            "entries": entries.len(),
            "present": present,
            "cleanableBytes": cleanable_bytes,
            "confirmBytes": confirm_bytes,
        },
        "entries": entries,
        "note": "只读视图：cleanableBytes 是 GUI 里可一键清理的体量，confirmBytes 需逐项确认。\
                 MCP 通道永不执行清理——执行必须由人在 Prune GUI 确认（白名单 + 废纸篓 + 20 GB 熔断）。\
                 children 仅一级子目录，children=true 才计算，未安装的应用 exists=false。",
    })
}

/// M29 `net_diag`（f13 §2.2）：端口暴露面诊断结论。port_listeners 给原始条目，
/// 本工具直接给「这台机器对外开着什么」的结论。scan 是系统调用，结论部分
/// 拆成纯函数 `diag_conclusions` 可单测。
fn tool_net_diag(args: &Value) -> Value {
    let port = args
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|p| *p > 0 && *p <= 65535)
        .map(|p| p as u16);

    let snap = prune_core::net::ports::scan(None);
    let entries: Vec<&prune_core::net::PortEntry> = snap
        .entries
        .iter()
        .filter(|e| port.map(|p| e.port == p).unwrap_or(true))
        .collect();

    let exposed: Vec<Value> = entries
        .iter()
        .filter(|e| {
            matches!(
                e.exposure,
                prune_core::net::BindScope::Any | prune_core::net::BindScope::Interface
            )
        })
        .map(|e| {
            let comms: Vec<String> = {
                let mut v: Vec<String> = e.listeners.iter().map(|b| b.comm.clone()).collect();
                v.sort();
                v.dedup();
                v
            };
            json!({
                "port": e.port,
                "exposure": e.exposure,
                "processes": comms,
                "project": e.listeners.iter().find_map(|b| b.project_label.clone()),
                "conns": e.conns,
                "note": e.note,
            })
        })
        .collect();

    let conclusions = diag_conclusions(&entries, snap.coverage.invisible(), snap.time_wait);

    json!({
        "readonly": true,
        "port": port,
        "exposed": exposed,
        "conclusions": conclusions,
        "coverage": snap.coverage,
        "timeWait": snap.time_wait,
        "scanMs": snap.scan_ms,
        "note": "exposure=any/interface 都是局域网可达（区别：通配绑定 vs 绑定具体网卡地址）。\
                 coverage 的 listenTotal 与 attributed 差额是权限看不到的部分，「没查到」不等于端口空闲。",
    })
}

/// 诊断结论纯函数：入口条目 + 可见性差额 + TIME_WAIT 残留 → 结论列表。
/// 与 BindScope 的「安全语义」同口径，不发明新判定。
fn diag_conclusions(
    entries: &[&prune_core::net::PortEntry],
    invisible: usize,
    time_wait: Option<u32>,
) -> Vec<Value> {
    let mut out = Vec::new();

    let any: Vec<&&prune_core::net::PortEntry> = entries
        .iter()
        .filter(|e| e.exposure == prune_core::net::BindScope::Any)
        .collect();
    for e in &any {
        let comm = e
            .listeners
            .first()
            .map(|b| b.comm.as_str())
            .unwrap_or("未知进程");
        let proj = e
            .listeners
            .iter()
            .find_map(|b| b.project_label.as_deref())
            .map(|p| format!("，项目归属 {p}"))
            .unwrap_or_default();
        out.push(json!({
            "severity": "attention",
            "text": format!("端口 {}（{comm}）绑定全部网卡（0.0.0.0/[::]）：同网段设备可达{proj}。若非有意暴露，建议改绑 127.0.0.1 或在防火墙收口。", e.port),
        }));
    }

    let iface_count = entries
        .iter()
        .filter(|e| e.exposure == prune_core::net::BindScope::Interface)
        .count();
    if iface_count > 0 {
        out.push(json!({
            "severity": "attention",
            "text": format!("另有 {iface_count} 个端口绑定在本机网卡地址（192.168.x.x 等）：局域网同样可达，明细见 exposed。"),
        }));
    }

    if out.is_empty() {
        let loopback = entries
            .iter()
            .all(|e| e.exposure == prune_core::net::BindScope::Loopback);
        let text = if loopback && !entries.is_empty() {
            "所有可见监听都只绑在本机回环（127.0.0.1/::1），无对外暴露面。"
        } else {
            "没有可见的对外监听。"
        };
        out.push(json!({ "severity": "ok", "text": text }));
    }

    if invisible > 0 {
        out.push(json!({
            "severity": "note",
            "text": format!("权限差额：内核共 {invisible} 个监听无法归属到进程（未授权/系统保留）——「没查到」不等于端口空闲。在 Prune GUI 授权后可见性会提升。"),
        }));
    }
    if let Some(tw) = time_wait.filter(|t| *t > 0) {
        out.push(json!({
            "severity": "note",
            "text": format!("{tw} 个 TIME_WAIT 内核残留连接：无持有进程，约 30-120 秒自动消失，不是占用。"),
        }));
    }

    out
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
    let mut procs = scanner.scan().procs;
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

/// 端口反查（只读）。**不暴露任何结束进程的工具**——Agent 拿到 pid 只用于解释与建议。
/// 应用资源画像（只读，M8）。与 CLI 的 `prune app` 共用同一套口径与实现。
fn tool_app_resources(args: &Value) -> Value {
    let Some(family) = args
        .get("family")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return json!({ "error": "需要 family 参数（子串匹配即可，如 \"cursor\"）" });
    };

    match crate::app_resource_report(&family) {
        Ok(r) => json!({
            "family": r.family,
            "processCount": r.process_count,
            "memoryBytes": r.memory_bytes,
            // CLI 不做深探：内存恒为 RSS 下界，别让 Agent 当成精确值
            "memoryExact": r.memory_exact,
            "cpu": r.cpu,
            "cpuCovered": r.cpu_covered,
            "conns": r.conns,
            "ports": r.ports.iter().map(|(p, s)| json!({
                "port": p,
                "exposure": s,
            })).collect::<Vec<_>>(),
            "disk": r.disk,
            "hint": "连接多不等于流量大（本机无公开 API）；磁盘按实占块统计，含 APFS 克隆共享；data 层是用户数据，不要建议删除。",
        }),
        Err(e) => match e {
            crate::AppMatchError::NotFound(candidates) => json!({
                "error": format!("没有匹配 {family:?} 的应用"),
                "candidates": candidates.into_iter().take(30).collect::<Vec<_>>(),
            }),
            crate::AppMatchError::Ambiguous(candidates) => json!({
                "error": format!("{family:?} 同时匹配了多个家族，请用全名重试"),
                "candidates": candidates,
            }),
        },
    }
}

fn tool_port_listeners(args: &Value) -> Value {
    let port = args
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|p| *p > 0 && *p <= 65535)
        .map(|p| p as u16);
    let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

    let snap = prune_core::net::ports::scan(None);
    let entries: Vec<&prune_core::net::PortEntry> = snap
        .entries
        .iter()
        .filter(|e| port.map(|p| e.port == p).unwrap_or(all || e.watched))
        .collect();

    // 单端口查询时只回该端口的 TIME_WAIT 命中：整张 128 条端口列表是纯噪音
    let time_wait_ports: Vec<u16> = match port {
        Some(p) => snap
            .time_wait_ports
            .iter()
            .copied()
            .filter(|x| *x == p)
            .collect(),
        None => snap.time_wait_ports.clone(),
    };

    json!({
        "port": port,
        // 空数组 ≠ 端口空闲：coverage 才是判断依据
        "entries": entries,
        "coverage": snap.coverage,
        "timeWait": snap.time_wait,
        "timeWaitPorts": time_wait_ports,
        "scanMs": snap.scan_ms,
        "hint": if entries.is_empty() {
            "没有可见的监听进程。先看 coverage：listenTotal 与 attributed 的差额是权限看不到的部分；若 timeWaitPorts 含该端口，则是内核保留期内的 TIME_WAIT 残留，不是进程占用。"
        } else {
            "exposure=any 表示绑定在全部网卡，局域网可达；project 来自进程 cwd，是该服务所在的项目目录。"
        },
    })
}

/// L4 Docker 组的命令提示（A6：只给命令，永不执行）
pub fn docker_hints() -> Vec<(&'static str, &'static str)> {
    vec![
        ("docker system prune", "清理悬空镜像、停止的容器、无用网络"),
        (
            "docker system prune --volumes",
            "连同未使用的数据卷一起清理（⚠️ 数据卷删除后不可恢复）",
        ),
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
    let mut procs = scanner.scan().procs;
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
    fn tool_defs_have_readonly_tools() {
        let defs = tool_defs();
        let names: Vec<&str> = defs
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "mem_snapshot",
                "mem_leaks",
                "disk_report",
                "port_listeners",
                "app_resources",
                "lan_devices",
                "disk_scan",
                "net_diag"
            ]
        );
        for t in defs.as_array().unwrap() {
            assert!(t["description"].as_str().unwrap().len() > 10);
            assert!(t["inputSchema"].is_object());
        }
    }

    #[test]
    fn run_tool_rejects_unknown() {
        assert!(run_tool("write_files", &Value::Null).is_err());
    }

    /// M29 只读底线（f13 §3）：工具名不得含执行词根；执行类请求必须显式
    /// 报「只读」错而不是 unknown tool。这是信任叙事的代码层，不许回退。
    #[test]
    fn mcp_channel_is_readonly() {
        // 词根黑名单：出现即说明有人把执行通道接进了 MCP
        const DESTRUCTIVE: [&str; 7] = [
            "clean", "delete", "remove", "execute", "reclaim", "kill", "write",
        ];
        let defs = tool_defs();
        for t in defs.as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            for root in DESTRUCTIVE {
                assert!(
                    !name.to_lowercase().contains(root),
                    "工具 {name} 含执行词根「{root}」，违反 MCP 只读底线"
                );
            }
        }
        // 显式拒绝：错误信息必须让人能看懂是底线而不是没实现
        for name in ["reclaim", "clean", "execute", "delete", "remove", "kill"] {
            let err = run_tool(name, &Value::Null).unwrap_err();
            assert!(
                err.contains("只读"),
                "执行类工具 {name} 应显式拒绝并说明只读底线，实际报错：{err}"
            );
        }
    }

    #[test]
    fn disk_scan_returns_structure_and_respects_keys() {
        // key 必须选跨平台条目（npm 是 Os::All）：原先用 cursor-cache 是 Os::MacOnly，
        // 在 Linux CI 上被 resolve() 过滤成空数组，而断言写死 len == 1 → 本机绿、CI 红。
        // resolve() 不按「路径是否存在」过滤（不存在的条目 size=0 但仍在列表里），
        // 所以这条断言与机器状态无关。
        let v = run_tool("disk_scan", &json!({ "keys": ["npm"], "children": true }))
            .expect("disk_scan 应可调用");
        assert_eq!(v["readonly"], true);
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["key"], "npm");
        assert!(entries[0]["sizeBytes"].is_u64());
        assert!(entries[0]["children"].is_array());
        assert!(v["summary"]["cleanableBytes"].is_u64());
        assert!(v["summary"]["confirmBytes"].is_u64());
        assert!(v["note"].as_str().unwrap().contains("只读"));
    }

    /// 平台专属条目（Os::MacOnly）在异平台必须被 `resolve()` 静默过滤成空，
    /// 既不能报错，也不能冒充「存在且为 0 字节」。v0.9.2 的 CI 红灯就是这个边界
    /// 没被守住——用一条显式用例把它钉住。
    #[test]
    fn disk_scan_filters_platform_specific_keys() {
        let v = run_tool("disk_scan", &json!({ "keys": ["cursor-cache"] }))
            .expect("disk_scan 应可调用");
        let entries = v["entries"].as_array().unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!(entries.len(), 1, "macOS 上 cursor-cache 应可见");
        #[cfg(not(target_os = "macos"))]
        assert!(
            entries.is_empty(),
            "非 macOS 不应返回 mac 专属条目，实际：{entries:?}"
        );
    }

    #[test]
    fn diag_conclusions_flags_exposure_and_invisible() {
        let mk_binding = |addr: &str| prune_core::net::PortBinding {
            port: 3000,
            proto: prune_core::net::Proto::Tcp,
            scope: prune_core::net::BindScope::from_addr(addr),
            addr: addr.into(),
            v6: false,
            pid: Some(42),
            comm: "vite".into(),
            start_time: 1,
            project: None,
            project_label: Some("prune".into()),
        };
        let mk_entry =
            |exposure: prune_core::net::BindScope, addr: &str| prune_core::net::PortEntry {
                port: 3000,
                proto: prune_core::net::Proto::Tcp,
                listeners: vec![mk_binding(addr)],
                conns: Some(0),
                exposure,
                watched: true,
                note: None,
            };
        let loopback = mk_entry(prune_core::net::BindScope::Loopback, "127.0.0.1");
        let wildcard = mk_entry(prune_core::net::BindScope::Any, "0.0.0.0");

        // 全回环 → ok，且不误报暴露
        let ok = diag_conclusions(&[&loopback], 0, Some(0));
        assert_eq!(ok[0]["severity"], "ok");

        // 通配绑定 → attention，含端口、进程、项目与收口建议
        let attn = diag_conclusions(&[&wildcard], 3, Some(7));
        assert_eq!(attn[0]["severity"], "attention");
        let text = attn[0]["text"].as_str().unwrap();
        assert!(text.contains("3000") && text.contains("vite") && text.contains("prune"));
        // 权限差额与 TIME_WAIT 都要提示
        assert!(attn
            .iter()
            .any(|c| c["severity"] == "note" && c["text"].as_str().unwrap().contains("权限差额")));
        assert!(attn
            .iter()
            .any(|c| c["text"].as_str().unwrap().contains("TIME_WAIT")));
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
            cpu: None,
            cpu_time_ms: 0,
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
            cpu: None,
            cpu_time_ms: 0,
        };
        let mut acc = SeriesAccum::new();
        acc.push(&[p], 1000);
        assert!(acc.series().is_empty());
    }
}
