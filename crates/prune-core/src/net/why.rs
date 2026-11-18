//! 端口白话解释（F16，docs/f16-port-why-plan.md）：把 `PortSnapshot` 的结构化证据
//! 翻译成一句人话 + 证据行 + 指路。
//!
//! 与 `service.rs` 的分工：那边回答「这是什么类型的服务」（L0 推断，给 UI 标签），
//! 这边回答「这个端口整体是怎么回事」（聚合 service + 系统服务表 + 冲突 +
//! Coverage/TIME_WAIT 口径，给 CLI / MCP 一个可直接转述的结论）。
//!
//! 三条纪律：
//! 1. **(port, comm) 双匹配**：普通用户进程占了 5000 ≠ AirPlay；系统服务表必须
//!    端口与进程名同时命中才生效，单端口命中一律走 Unknown / 常规推断；
//! 2. **不编造用途**：规则不命中时只报观测事实（进程 / 绑定 / 暴露面 / 连接数），
//!    推断不出的不说「未知服务」这种假确定（service.rs 同款纪律）；
//! 3. **零网络 IO**：全部输入来自已采集的 `PortSnapshot`，CLI / MCP / 前端
//!    可以在任意时刻对同一快照重放同一结论（纯函数可单测）。
//!
//! 文案语言：CLI 输出中文单语（决策 W1-DP-6），与本文件 `port_note` 既有惯例一致。

use serde::{Deserialize, Serialize};

use super::BindScope;

/// 解释结论的类别。`KnownService` 是 `service.rs` L0 推断的直接引用——
/// 那是已采集证据上的推断，不是编造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhyKind {
    /// 系统服务（系统服务表命中，杀掉会复活）
    SystemService,
    /// 本地开发服务（进程 cwd 归属到项目）
    DevServer,
    /// 同端口多个持有者冲突
    Conflict,
    /// L0 推断出服务类型（数据库 / 代理 / 调试通道等）
    KnownService,
    /// 内核 TIME_WAIT 保留期内的残留，无持有者
    TimeWait,
    /// 可见范围内确认无监听（Coverage 无差额时才允许这个结论）
    Free,
    /// 只报观测事实，不猜用途
    Unknown,
}

/// 一次白话解释的完整产出。字段是 CLI / MCP / 前端三个消费者的共同契约。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyVerdict {
    pub port: u16,
    pub kind: WhyKind,
    /// 一句话结论（可直接转述给用户 / Agent）
    pub headline: String,
    /// 证据行：进程 / pid / 绑定 / 项目 / 连接 / Coverage 口径
    pub evidence: Vec<String>,
    /// 指路（只指路不执行）：关闭路径、项目目录、自查命令
    pub action_hint: Option<String>,
}

/// 系统服务表（首批 ≤10 条，只收「高频 + 有明确关闭路径」的条目）。
/// 元组：`(端口, 进程名, 服务名, 指路)`。comm 比对大小写不敏感；
/// 端口与 comm 必须同时命中——纪律 1。
const SYSTEM_SERVICES: &[(u16, &str, &str, &str)] = &[
    (
        5000,
        "ControlCenter",
        "AirPlay 接收器",
        "系统设置 → 通用 → 隔空投送与接力 → AirPlay 接收器 可关闭",
    ),
    (
        7000,
        "ControlCenter",
        "AirPlay 接收器",
        "系统设置 → 通用 → 隔空投送与接力 → AirPlay 接收器 可关闭",
    ),
    (22, "sshd", "远程登录（SSH）", "系统设置 → 通用 → 共享 → 远程登录 可关闭"),
    (
        631,
        "cupsd",
        "打印服务（CUPS）",
        "系统设置 → 打印机与扫描仪 管理；不需要打印可移除打印机",
    ),
    (
        5353,
        "mDNSResponder",
        "mDNS / Bonjour 设备发现",
        "系统内置发现服务，不影响开发，无需处理",
    ),
    (
        137,
        "netbiosd",
        "NetBIOS 名称服务（Windows 共享发现）",
        "不需要 Windows 文件共享时可在共享设置里关闭",
    ),
    (
        138,
        "netbiosd",
        "NetBIOS 数据报服务（Windows 共享发现）",
        "不需要 Windows 文件共享时可在共享设置里关闭",
    ),
];

/// 服务类型的中文标签（KnownService 结论的 headline 用）。
fn service_label(kind: super::service::ServiceKind) -> &'static str {
    use super::service::ServiceKind::*;
    match kind {
        Web => "Web 服务",
        DevServer => "开发服务器",
        Database => "数据库 / 中间件",
        Mcp => "网络型 MCP server",
        Proxy => "代理",
        Debug => "调试通道",
        Other => "已认出的服务",
    }
}

/// 传输层的中文短标签（证据行用）。
fn proto_text(p: super::Proto) -> &'static str {
    match p {
        super::Proto::Tcp => "TCP",
        super::Proto::Udp => "UDP",
    }
}

/// 暴露面的中文短标签（证据行用；与 CLI `exposure_cn` 同义，但核心层
/// 自持一份以免 CLI 内部函数反向依赖）。
fn scope_text(scope: BindScope) -> &'static str {
    match scope {
        BindScope::Loopback => "仅本机(127.0.0.1)",
        BindScope::Interface => "绑定具体网卡(局域网可达)",
        BindScope::Any => "全部网卡(0.0.0.0,局域网可达)",
        BindScope::Unknown => "绑定面未知",
    }
}

/// 主入口。`snap` 必须来自一次真实扫描；对同一快照重放得到同一结论。
pub fn why(port: u16, snap: &super::PortSnapshot) -> WhyVerdict {
    let entry = snap.entries.iter().find(|e| e.port == port);
    let Some(e) = entry else {
        return free_or_blocked(port, snap);
    };
    if e.owner_count() > 1 {
        return conflict_verdict(e);
    }
    if let Some(v) = system_service_verdict(e) {
        return v;
    }
    if let Some(b) = e.primary() {
        if let Some(label) = &b.project_label {
            return devserver_verdict(e, b, label);
        }
    }
    if let Some(kind) = e.service {
        return known_service_verdict(e, kind);
    }
    unknown_verdict(e)
}

/// 快照里没有该端口的监听行：三种情况严格区分（S3），不许一律说「空闲」。
fn free_or_blocked(port: u16, snap: &super::PortSnapshot) -> WhyVerdict {
    if snap.time_wait_ports.contains(&port) {
        return WhyVerdict {
            port,
            kind: WhyKind::TimeWait,
            headline: format!("端口 {port} 刚被关闭，处于内核 TIME_WAIT 保留期（约 2 分钟），没有持有进程"),
            evidence: vec![format!(
                "TIME_WAIT 共 {} 条（内核口径）；该端口在其中",
                snap.time_wait.unwrap_or(0)
            )],
            action_hint: Some("保留期内重新绑定会拿到 EADDRINUSE，等约 2 分钟或换端口".into()),
        };
    }
    let invisible = snap.coverage.invisible();
    if invisible > 0 {
        return WhyVerdict {
            port,
            kind: WhyKind::Unknown,
            headline: format!("可见范围内没有进程监听 {port}，但有 {} 个监听无法归属到进程，不能确认空闲", invisible),
            evidence: vec![format!(
                "内核报告 {} 个监听，能归属 {} 个（Coverage 口径）",
                snap.coverage.listen_total, snap.coverage.attributed
            )],
            action_hint: Some(format!("自查：sudo lsof -nP -iTCP:{port} -sTCP:LISTEN")),
        };
    }
    if let Some(note) = &snap.coverage.note {
        return WhyVerdict {
            port,
            kind: WhyKind::Unknown,
            headline: format!("查询受限，无法确认端口 {port} 的状态"),
            evidence: vec![note.clone()],
            action_hint: Some(format!("自查：sudo lsof -nP -iTCP:{port} -sTCP:LISTEN")),
        };
    }
    WhyVerdict {
        port,
        kind: WhyKind::Free,
        headline: format!("端口 {port} 当前没有任何进程监听（可见范围内确认空闲）"),
        evidence: vec![format!(
            "内核口径 {} 个监听全部归属成功，无权限盲区",
            snap.coverage.listen_total
        )],
        action_hint: None,
    }
}

/// 冲突：复用 `owner_count()` 判据（与 F4 端口保镖、`port --conflicts` 同源）。
fn conflict_verdict(e: &super::PortEntry) -> WhyVerdict {
    let owners = e.owner_count();
    let mut evidence = Vec::new();
    for b in &e.listeners {
        evidence.push(format!(
            "{} (pid {})  绑定 {}  {}",
            b.comm,
            b.pid.map(|p| p.to_string()).unwrap_or_else(|| "内核".into()),
            b.addr,
            scope_text(b.scope)
        ));
    }
    WhyVerdict {
        port: e.port,
        kind: WhyKind::Conflict,
        headline: format!(
            "端口 {} 有 {} 个不同进程同时持有（SO_REUSEPORT 或冲突），先分清哪个是真服务",
            e.port, owners
        ),
        evidence,
        action_hint: Some(
            "prune port --conflicts 看冲突详情；停掉多余的那个即可，不要盲杀".to_string(),
        ),
    }
}

/// 系统服务表命中：(port, comm) 双匹配，纪律 1。
fn system_service_verdict(e: &super::PortEntry) -> Option<WhyVerdict> {
    let b = e.primary()?;
    let hit = SYSTEM_SERVICES
        .iter()
        .find(|(p, comm, _, _)| *p == e.port && b.comm.eq_ignore_ascii_case(comm))?;
    Some(WhyVerdict {
        port: e.port,
        kind: WhyKind::SystemService,
        headline: format!(
            "{}（{}）占用端口 {}——这是系统服务，不是你的 dev server，杀掉会复活",
            b.comm, hit.2, e.port
        ),
        evidence: vec![format!(
            "{} (pid {})  绑定 {}  {}  {}/{}",
            b.comm,
            b.pid.map(|p| p.to_string()).unwrap_or_else(|| "内核".into()),
            b.addr,
            scope_text(e.exposure),
            proto_text(e.proto),
            e.port
        )],
        action_hint: Some(hit.3.to_string()),
    })
}

/// 项目归属命中：cwd 就是项目根（M7 口径）。
fn devserver_verdict(
    e: &super::PortEntry,
    b: &super::PortBinding,
    label: &str,
) -> WhyVerdict {
    let mut evidence = vec![format!(
        "{} (pid {})  绑定 {}  {}",
        b.comm,
        b.pid.map(|p| p.to_string()).unwrap_or_default(),
        b.addr,
        scope_text(e.exposure)
    )];
    if let Some(conns) = e.conns {
        evidence.push(format!("ESTABLISHED 连接 {conns} 条"));
    }
    if e.dual_stack() {
        evidence.push("IPv4+IPv6 双栈监听".into());
    }
    WhyVerdict {
        port: e.port,
        kind: WhyKind::DevServer,
        headline: format!(
            "{} 占用端口 {}，属于项目「{}」的本地开发服务，杀掉需要到项目里重启",
            b.comm, e.port, label
        ),
        evidence,
        action_hint: b.project.clone().or_else(|| Some(format!("prune port {}", e.port))),
    }
}

/// L0 推断命中（无项目归属）：引用 service.rs 的结论，不自行加戏。
fn known_service_verdict(
    e: &super::PortEntry,
    kind: super::service::ServiceKind,
) -> WhyVerdict {
    let b = e.primary();
    let (comm, pid) = b
        .map(|b| (b.comm.clone(), b.pid.map(|p| p.to_string()).unwrap_or_default()))
        .unwrap_or_else(|| ("内核".into(), "内核".into()));
    let mut evidence = vec![format!("{} (pid {})  绑定面 {}", comm, pid, scope_text(e.exposure))];
    if let Some(conns) = e.conns {
        evidence.push(format!("ESTABLISHED 连接 {conns} 条"));
    }
    evidence.push("服务类型来自端口/进程名静态推断（L0），未经探针确认".into());
    WhyVerdict {
        port: e.port,
        kind: WhyKind::KnownService,
        headline: format!(
            "{} 占用端口 {}，疑似{}（静态推断，不是确认）",
            comm,
            e.port,
            service_label(kind)
        ),
        evidence,
        action_hint: Some(format!("prune port {} 看完整持有者与绑定明细", e.port)),
    }
}

/// 兜底：只报观测事实，纪律 2。
fn unknown_verdict(e: &super::PortEntry) -> WhyVerdict {
    let b = e.primary();
    let mut evidence = Vec::new();
    if let Some(b) = b {
        evidence.push(format!(
            "{} (pid {})  绑定 {}  {}",
            b.comm,
            b.pid.map(|p| p.to_string()).unwrap_or_else(|| "内核".into()),
            b.addr,
            scope_text(e.exposure)
        ));
    }
    if let Some(conns) = e.conns {
        evidence.push(format!("ESTABLISHED 连接 {conns} 条"));
    }
    WhyVerdict {
        port: e.port,
        kind: WhyKind::Unknown,
        headline: format!(
            "端口 {} 由 {} 监听，用途未知（推断不出就是不知道，不猜）",
            e.port,
            b.map(|b| b.comm.as_str()).unwrap_or("内核进程")
        ),
        evidence,
        action_hint: Some(format!("prune port {} 看完整明细", e.port)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{BindScope, Coverage, PortBinding, PortEntry, PortSnapshot, Proto};

    fn snap(entries: Vec<PortEntry>, coverage: Coverage) -> PortSnapshot {
        PortSnapshot {
            entries,
            coverage,
            time_wait: None,
            time_wait_ports: Vec::new(),
            conns_by_pid: Default::default(),
            configured_watch_ports: Vec::new(),
            at: 0,
            scan_ms: 0,
        }
    }

    fn binding(port: u16, pid: i32, comm: &str, addr: &str) -> PortBinding {
        PortBinding {
            port,
            proto: Proto::Tcp,
            scope: BindScope::from_addr(addr),
            addr: addr.into(),
            v6: false,
            pid: Some(pid),
            comm: comm.into(),
            start_time: 0,
            project: None,
            project_label: None,
        }
    }

    fn empty_coverage() -> Coverage {
        Coverage {
            listen_total: 0,
            attributed: 0,
            note: None,
        }
    }

    #[test]
    fn s1_airplay_system_service_hit() {
        let s = snap(vec![PortEntry {
            port: 5000,
            proto: Proto::Tcp,
            listeners: vec![binding(5000, 291, "ControlCenter", "*")],
            conns: None,
            exposure: BindScope::Any,
            watched: true,
            note: None,
            service: None,
        }], empty_coverage());
        let v = why(5000, &s);
        assert_eq!(v.kind, WhyKind::SystemService);
        assert!(v.headline.contains("系统服务"), "{}", v.headline);
        assert!(v.headline.contains("复活"));
        assert!(v.action_hint.as_deref().unwrap().contains("隔空投送"));
    }

    #[test]
    fn user_process_on_5000_is_not_airplay() {
        // 纪律 1 的负例：用户的 Flask 占了 5000，不能套 AirPlay 结论
        let mut b = binding(5000, 4321, "python", "127.0.0.1");
        b.project = Some("/home/me/flask-app".into());
        b.project_label = Some("flask-app".into());
        let s = snap(vec![PortEntry {
            port: 5000,
            proto: Proto::Tcp,
            listeners: vec![b],
            conns: Some(0),
            exposure: BindScope::Loopback,
            watched: true,
            note: None,
            service: None,
        }], empty_coverage());
        let v = why(5000, &s);
        assert_eq!(v.kind, WhyKind::DevServer);
        assert!(v.headline.contains("flask-app"));
    }

    #[test]
    fn s2_devserver_has_project_and_hint() {
        let mut b = binding(5173, 1000, "node", "127.0.0.1");
        b.project = Some("/Users/me/prune".into());
        b.project_label = Some("prune".into());
        let s = snap(vec![PortEntry {
            port: 5173,
            proto: Proto::Tcp,
            listeners: vec![b],
            conns: Some(2),
            exposure: BindScope::Loopback,
            watched: true,
            note: None,
            service: Some(crate::net::service::ServiceKind::DevServer),
        }], empty_coverage());
        let v = why(5173, &s);
        assert_eq!(v.kind, WhyKind::DevServer);
        assert!(v.headline.contains("prune"));
        assert_eq!(v.action_hint.as_deref(), Some("/Users/me/prune"));
        assert!(v.evidence.iter().any(|l| l.contains("2 条")));
    }

    #[test]
    fn s4_conflict_lists_all_holders() {
        let s = snap(vec![PortEntry {
            port: 3000,
            proto: Proto::Tcp,
            listeners: vec![
                binding(3000, 11, "node", "*"),
                binding(3000, 22, "python", "*"),
            ],
            conns: Some(0),
            exposure: BindScope::Any,
            watched: true,
            note: None,
            service: None,
        }], empty_coverage());
        let v = why(3000, &s);
        assert_eq!(v.kind, WhyKind::Conflict);
        assert!(v.headline.contains("2 个"));
        assert_eq!(v.evidence.len(), 2);
    }

    #[test]
    fn s3_free_requires_no_invisible_gap() {
        let s = snap(Vec::new(), Coverage { listen_total: 5, attributed: 5, note: None });
        let v = why(9999, &s);
        assert_eq!(v.kind, WhyKind::Free);
        assert!(v.headline.contains("空闲"));
    }

    #[test]
    fn s3_invisible_gap_is_unknown_not_free() {
        let s = snap(Vec::new(), Coverage { listen_total: 9, attributed: 5, note: None });
        let v = why(9999, &s);
        assert_eq!(v.kind, WhyKind::Unknown);
        assert!(v.headline.contains("不能确认空闲"));
    }

    #[test]
    fn s3_scan_failure_is_unknown() {
        let s = snap(Vec::new(), Coverage {
            listen_total: 0,
            attributed: 0,
            note: Some("端口采集失败：lsof 无法执行".into()),
        });
        let v = why(3000, &s);
        assert_eq!(v.kind, WhyKind::Unknown);
        assert!(v.headline.contains("查询受限"));
    }

    #[test]
    fn s3_time_wait_distinguished_from_free() {
        let mut s = snap(Vec::new(), empty_coverage());
        s.time_wait = Some(4);
        s.time_wait_ports = vec![3000];
        let v = why(3000, &s);
        assert_eq!(v.kind, WhyKind::TimeWait);
        assert!(v.headline.contains("TIME_WAIT"));
        assert!(v.action_hint.as_deref().unwrap().contains("EADDRINUSE"));
    }

    #[test]
    fn known_service_quotes_static_inference_honestly() {
        let s = snap(vec![PortEntry {
            port: 3306,
            proto: Proto::Tcp,
            listeners: vec![binding(3306, 88, "mysqld", "127.0.0.1")],
            conns: None,
            exposure: BindScope::Loopback,
            watched: true,
            note: None,
            service: Some(crate::net::service::ServiceKind::Database),
        }], empty_coverage());
        let v = why(3306, &s);
        assert_eq!(v.kind, WhyKind::KnownService);
        assert!(v.headline.contains("疑似"));
        assert!(v.headline.contains("数据库"));
        assert!(v.evidence.iter().any(|l| l.contains("静态推断")));
    }

    #[test]
    fn unknown_never_invents_a_use() {
        // 高位随机端口 + 无归属 GUI 进程 + 推断不出：只能报观测事实
        let s = snap(vec![PortEntry {
            port: 58583,
            proto: Proto::Tcp,
            listeners: vec![binding(58583, 77, "Cursor Helper", "127.0.0.1")],
            conns: None,
            exposure: BindScope::Loopback,
            watched: false,
            note: None,
            service: None,
        }], empty_coverage());
        let v = why(58583, &s);
        assert_eq!(v.kind, WhyKind::Unknown);
        assert!(v.headline.contains("未知"));
        // 不出现任何编造的服务语义
        assert!(!v.headline.contains("AirPlay"));
        assert!(!v.headline.contains("dev"));
    }

    #[test]
    fn system_port_wrong_comm_falls_through() {
        // ControlCenter 占了 22？端口+进程双匹配不成立 → 不套系统服务结论
        let s = snap(vec![PortEntry {
            port: 22,
            proto: Proto::Tcp,
            listeners: vec![binding(22, 5, "ControlCenter", "*")],
            conns: None,
            exposure: BindScope::Any,
            watched: true,
            note: None,
            service: None,
        }], empty_coverage());
        let v = why(22, &s);
        assert_eq!(v.kind, WhyKind::Unknown);
    }

    #[test]
    fn verdict_is_serializable_camel_case() {
        let s = snap(Vec::new(), empty_coverage());
        let v = why(80, &s);
        let doc = serde_json::to_value(&v).unwrap();
        assert!(doc.get("port").is_some());
        assert!(doc.get("kind").is_some());
        assert!(doc.get("headline").is_some());
        assert!(doc.get("actionHint").is_some());
        assert!(doc.get("action_hint").is_none(), "契约是 camelCase");
    }
}
