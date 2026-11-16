//! docs_json：CLI `--json` 输出（契约 v1）的**唯一产出层**。
//!
//! 契约规则（docs/wave1-contract-doctor-plan.md §3.1，W1-DP-1 裁 A 顶层平铺）：
//! 1. 每个 `--json` 输出的顶层必含 `"schemaVersion": 1`；
//! 2. 其余键保持领域形状，**不**做 `{schemaVersion, data}` 双层包装——已发布的
//!    `projects` / `port --conflicts` / `watch` 都是平铺，改成包装等于现在就破坏一次；
//! 3. 新增字段允许；删字段、改类型、改语义 = 破坏性，必须提升 schemaVersion；
//! 4. `--json` 输出永远是一个 JSON 文档（`watch` 事件流除外：JSONL 每行自带）。
//!
//! 为什么单独一个模块：CLI 打印与 `cli_contract_matches_frozen_snapshot` 测试
//! 共用同一批函数——快照必须来自生产路径，不能手写（复用 `mcp-tools.v1.json`
//! 的纪律，见 mcp.rs `tool_defs_match_frozen_snapshot`）。
//!
//! 特例：`lan` 的缓存文件与 CLI 输出**不是**同一份 JSON——缓存继续写裸
//! `LanSnapshot`（GUI/MCP `lan_devices` 读它），只有 CLI 打印走 `lan_doc`
//! 包信封。直接给 `LanSnapshot` 加字段会扩散到两个消费者，比收益大。

use serde_json::{json, Map, Value};

use prune_core::disk::DiskReport;
use prune_core::mem::{self, Pressure};
use prune_core::net::lan::LanSnapshot;
use prune_core::net::ports::PortConflict;
use prune_core::net::{self, PortEntry, PortSnapshot};
use prune_core::swap::SwapInfo;

use crate::{AppResource, DiskTrend};

/// 契约版本。只在破坏性变更（删字段 / 改类型 / 改语义）时提升。
pub const SCHEMA_VERSION: u64 = 1;

/// 给领域 JSON 顶层插入 `schemaVersion`（平铺，不包装）。
fn enveloped(base: Value) -> Value {
    let mut obj = base.as_object().cloned().unwrap_or_else(Map::new);
    obj.insert(
        "schemaVersion".into(),
        Value::Number(serde_json::Number::from(SCHEMA_VERSION)),
    );
    Value::Object(obj)
}

// ---------------------------------------------------------------- 各命令文档

/// `prune scan --json`。**不改 `DiskReport` 结构体**——它同时是 GUI IPC 载荷，
/// 加字段会污染前端类型与旧报告反序列化，所以在序列化产物上补信封。
pub fn scan_doc(report: &DiskReport) -> Value {
    enveloped(serde_json::to_value(report).unwrap_or(Value::Null))
}

/// `prune mem --json`。字段与原 `cmd_mem` 内联 `json!` 逐键一致，只补信封。
pub fn mem_doc(
    swap_info: &SwapInfo,
    pressure: Option<Pressure>,
    families: &[mem::FamilyMem],
    top_swapped: &[mem::ProcMem],
) -> Value {
    enveloped(json!({
        "physicalTotal": mem::system::physical_total(),
        "swap": swap_info,
        "pressure": pressure,
        "families": families,
        "topSwapped": top_swapped,
    }))
}

/// `prune port --json`（默认分支）。`entries` 是调用方按过滤条件筛好的列表
/// （关注层 / 单端口 / --all），本函数不重复筛选逻辑。
pub fn port_doc(only: Option<u16>, entries: &[PortEntry], snap: &PortSnapshot) -> Value {
    enveloped(json!({
        "port": only,
        "entries": entries,
        "coverage": snap.coverage,
        "timeWait": snap.time_wait,
        "timeWaitPorts": snap.time_wait_ports,
        "scanMs": snap.scan_ms,
    }))
}

/// `prune port --conflicts --json`（契约 v1 之前已合规，收进本层统一维护）。
pub fn port_conflicts_doc(at: i64, conflicts: &[PortConflict], coverage: &net::Coverage) -> Value {
    enveloped(json!({
        "at": at,
        "conflicts": conflicts,
        "coverage": coverage,
    }))
}

/// `prune projects --json`。
pub fn projects_doc(from: &str, days: u32, projects: &[DiskTrend]) -> Value {
    enveloped(json!({
        "from": from,
        "days": days,
        "projects": projects,
    }))
}

/// `prune projects --json` 的账本不可用形态（状态 + 原因，projects 恒为空数组）。
pub fn projects_unavailable_doc(days: u32, note: &str) -> Value {
    enveloped(json!({
        "days": days,
        "status": "unavailable",
        "note": note,
        "projects": [],
    }))
}

/// `prune app --json`。
pub fn app_doc(r: &AppResource) -> Value {
    enveloped(json!({
        "family": r.family,
        "processCount": r.process_count,
        "memoryBytes": r.memory_bytes,
        "memoryExact": r.memory_exact,
        "cpu": r.cpu,
        "cpuCovered": r.cpu_covered,
        "conns": r.conns,
        "ports": r.ports.iter().map(|(p, s)| json!({
            "port": p, "exposure": crate::exposure_cn(*s),
        })).collect::<Vec<_>>(),
        "disk": r.disk,
    }))
}

/// `prune lan --json`。缓存文件**不含**这个信封（见模块注释）。
pub fn lan_doc(snap: &LanSnapshot) -> Value {
    enveloped(serde_json::to_value(snap).unwrap_or(Value::Null))
}

/// `prune doctor --json`：findings + coverage（wave1 方案 §4.3）。
pub fn doctor_doc(
    at: i64,
    elapsed_ms: u64,
    findings: &[crate::doctor::Finding],
    coverage: &std::collections::BTreeMap<&'static str, crate::doctor::DomainCoverage>,
) -> Value {
    enveloped(json!({
        "at": at,
        "elapsedMs": elapsed_ms,
        "findings": findings,
        "coverage": coverage,
    }))
}

// ---------------------------------------------------------------- 契约快照

/// 契约快照（`cli-contract.v1.json`）的活体来源：用最小 fixture 逐命令构造
/// 结构、收集顶层键。改任何 doc 函数的键集合都会让快照测试红——
/// 「改契约」因此成为一个显式动作。仅测试路径消费（快照测试 + dump 导出）。
#[cfg(test)]
pub fn contract_shape() -> Value {
    let keys_of = |v: &Value| -> Vec<String> {
        let mut keys: Vec<String> = v
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        keys.sort();
        keys
    };

    // scan：最小 DiskReport（不需要真实扫描数据，只要键集合与生产一致）
    let report = DiskReport {
        scanned_at: 0,
        scan_elapsed_ms: 0,
        incremental: false,
        groups: Vec::new(),
        dup: None,
        totals: prune_core::disk::Totals {
            deadweight: 0,
            l2: 0,
            l3: 0,
            l4: 0,
            reclaimable: 0,
            cache_total: 0,
        },
        volume: None,
        projects: Vec::new(),
        config_notes: None,
    };

    let swap_info = SwapInfo {
        total: 0,
        used: 0,
        free: 0,
        file_count: None,
        new_today: None,
    };
    let mem_doc = mem_doc(&swap_info, None, &[], &[]);

    let port_snap = PortSnapshot {
        entries: Vec::new(),
        coverage: net::Coverage {
            listen_total: 0,
            attributed: 0,
            note: None,
        },
        time_wait: None,
        time_wait_ports: Vec::new(),
        conns_by_pid: Default::default(),
        configured_watch_ports: Vec::new(),
        at: 0,
        scan_ms: 0,
    };
    let port_doc = port_doc(None, &[], &port_snap);
    let conflicts_doc = port_conflicts_doc(0, &[], &port_snap.coverage);

    let app = AppResource {
        family: String::new(),
        process_count: 0,
        memory_bytes: 0,
        memory_exact: false,
        cpu: None,
        cpu_covered: 0,
        conns: net::ConnStats {
            established: 0,
            close_wait: 0,
        },
        ports: Vec::new(),
        disk: prune_core::appdisk::AppDisk {
            family: String::new(),
            bundle: None,
            cache: None,
            data: None,
            at: 0,
            elapsed_ms: 0,
            note: None,
        },
    };
    let app_doc = app_doc(&app);

    let lan = LanSnapshot {
        at: 0,
        iface: None,
        cidr: None,
        scanned: 0,
        total: 0,
        devices: Vec::new(),
        exposed_ports: Vec::new(),
        notes: Vec::new(),
        scan_ms: 0,
    };
    let lan_doc = lan_doc(&lan);

    let projects_doc = projects_doc("1970-01-01", 1, &[]);

    let doctor_cov: std::collections::BTreeMap<&'static str, crate::doctor::DomainCoverage> =
        crate::doctor::Domain::ALL
            .iter()
            .map(|d| (d.name(), crate::doctor::DomainCoverage::new("ok")))
            .collect();
    let doctor_doc = doctor_doc(0, 0, &[], &doctor_cov);

    json!({
        "schemaVersion": SCHEMA_VERSION,
        "commands": {
            "scan":           { "keys": keys_of(&scan_doc(&report)) },
            "mem":            { "keys": keys_of(&mem_doc) },
            "port":           { "keys": keys_of(&port_doc) },
            "port_conflicts": { "keys": keys_of(&conflicts_doc) },
            "app":            { "keys": keys_of(&app_doc) },
            "lan":            { "keys": keys_of(&lan_doc) },
            "projects":       { "keys": keys_of(&projects_doc) },
            "doctor":         { "keys": keys_of(&doctor_doc) },
            "watch_once":     { "keys": crate::watch::ONCE_TOP_KEYS },
        },
        "exitCodes": crate::exit_code_map(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_is_flat_not_wrapped() {
        // W1-DP-1：顶层平铺。如果哪天有人把领域键塞进 "data" 里，这里先红。
        let lan = LanSnapshot {
            at: 0,
            iface: None,
            cidr: None,
            scanned: 0,
            total: 0,
            devices: Vec::new(),
            exposed_ports: Vec::new(),
            notes: Vec::new(),
            scan_ms: 0,
        };
        let doc = lan_doc(&lan);
        assert_eq!(doc["schemaVersion"], SCHEMA_VERSION);
        assert!(
            doc.get("data").is_none(),
            "契约 v1 是平铺形状，不引入 data 包装"
        );
        assert!(doc.get("devices").is_some());
    }
}
